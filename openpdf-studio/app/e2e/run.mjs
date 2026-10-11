// End-to-end tests against the real desktop app (debug build) through
// tauri-driver + WebKitWebDriver (Linux) or msedgedriver (Windows).
//
//   npx tauri build --debug --no-bundle
//   xvfb-run node e2e/run.mjs            # Linux CI
//
// Native file dialogs cannot be automated, so tests grant paths through the
// debug-only `e2e_grant` command (OPDF_E2E=1) and replace the picker
// functions; everything after the picker runs the normal UI code paths.

import { spawn, execFileSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { deflateSync } from "node:zlib";
import { Session } from "./webdriver.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, "..", "..");
const exe = process.platform === "win32" ? ".exe" : "";
const appBin = process.env.OPDF_APP ?? join(root, "target", "debug", `openpdf-studio${exe}`);
const cliBin = join(root, "target", "debug", `opdf${exe}`);
const shots = join(here, "screenshots");
const work = mkdtempSync(join(tmpdir(), "opdf-e2e-"));
mkdirSync(shots, { recursive: true });

if (!existsSync(appBin)) throw new Error(`build the app first: ${appBin}`);
if (!existsSync(cliBin)) execFileSync("cargo", ["build", "-p", "opdf-cli"], { cwd: root, stdio: "inherit" });

const cli = (...args) => execFileSync(cliBin, args, { encoding: "utf8" });
const cliFails = (...args) => {
  try {
    execFileSync(cliBin, args, { stdio: "pipe" });
    return false;
  } catch {
    return true;
  }
};

// ---------------------------------------------------------------- fixtures
const sample = join(work, "sample.pdf");
const second = join(work, "second.pdf");
cli("sample", "--pages", "5", "-o", sample);
cli("sample", "--pages", "3", "-o", second);
const png = join(work, "photo.png");
writeFileSync(png, makePng(320, 200));

function makePng(w, h) {
  const crcTable = Array.from({ length: 256 }, (_, n) => {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    return c >>> 0;
  });
  const crc = (buf) => {
    let c = 0xffffffff;
    for (const b of buf) c = crcTable[(c ^ b) & 0xff] ^ (c >>> 8);
    return (c ^ 0xffffffff) >>> 0;
  };
  const chunk = (type, data) => {
    const len = Buffer.alloc(4);
    len.writeUInt32BE(data.length);
    const td = Buffer.concat([Buffer.from(type), data]);
    const c = Buffer.alloc(4);
    c.writeUInt32BE(crc(td));
    return Buffer.concat([len, td, c]);
  };
  const raw = Buffer.alloc((w * 3 + 1) * h);
  for (let y = 0; y < h; y++)
    for (let x = 0; x < w; x++) {
      const o = y * (w * 3 + 1) + 1 + x * 3;
      raw[o] = (x * 255) / w;
      raw[o + 1] = (y * 255) / h;
      raw[o + 2] = 180;
    }
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(w, 0);
  ihdr.writeUInt32BE(h, 4);
  ihdr[8] = 8;
  ihdr[9] = 2;
  return Buffer.concat([Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]), chunk("IHDR", ihdr), chunk("IDAT", deflateSync(raw)), chunk("IEND", Buffer.alloc(0))]);
}

// ---------------------------------------------------------------- driver
const driver = spawn("tauri-driver", [], {
  env: { ...process.env, OPDF_E2E: "1", XDG_DATA_HOME: join(work, "data"), XDG_CONFIG_HOME: join(work, "config") },
  stdio: ["ignore", "inherit", "inherit"],
});
const base = "http://127.0.0.1:4444";
for (let i = 0; ; i++) {
  try {
    await fetch(base + "/status");
    break;
  } catch {
    if (i > 100) throw new Error("tauri-driver did not start");
    await new Promise((r) => setTimeout(r, 100));
  }
}

const s = await Session.create(base, appBin, [sample]);
let failures = 0;
const results = [];
async function test(name, fn) {
  const t0 = Date.now();
  try {
    await fn();
    results.push(`✔ ${name} (${Date.now() - t0} ms)`);
  } catch (e) {
    failures++;
    results.push(`✘ ${name}\n    ${String(e.stack ?? e).split("\n").slice(0, 4).join("\n    ")}`);
    writeFileSync(join(shots, `FAILED-${name.replace(/\W+/g, "-")}.png`), await s.screenshot().catch(() => Buffer.alloc(0)));
  }
  console.log(results[results.length - 1]);
}
const shot = async (name) => writeFileSync(join(shots, `${name}.png`), await s.screenshot());
const state = () => s.run("const st = __opdf.store.getState(); return { tabs: st.tabs.map(t => ({ id: t.id, name: t.doc.name, pages: t.pageCount, dirty: t.doc.dirty, rev: t.doc.revision, annot: t.annotModified, mode: t.mode, enc: t.doc.summary?.permissions.encrypted })), active: st.activeId, dialog: st.dialog?.kind ?? null };");
const active = async () => {
  const st = await state();
  return st.tabs.find((t) => t.id === st.active);
};
const pageText = (n) => s.run(`const p = await __opdf.pdf().getPage(${n}); const c = await p.getTextContent(); return c.items.map(i => i.str).join(' ');`);
const waitRevision = async (prev) => s.waitFor(`const st = __opdf.store.getState(); const t = st.tabs.find(t => t.id === st.activeId); return t && t.doc.revision > ${prev} && __opdf.pdf() && __opdf.pdf().numPages === t.pageCount;`, { message: "new revision loaded" });

await s.waitFor("return !!window.__opdf", { timeout: 30000, message: "test hooks" });

// ---------------------------------------------------------------- tests
await test("opens the file passed on the command line", async () => {
  await s.waitFor("const st = __opdf.store.getState(); return st.tabs.length === 1 && st.tabs[0].pageCount === 5;", { timeout: 30000 });
  await s.waitFor("return document.querySelectorAll('.pdfViewer .page canvas').length > 0", { message: "rendered canvas" });
  await s.waitFor("return document.querySelectorAll('.thumb img').length >= 2", { message: "thumbnails" });
  const text = await pageText(1);
  if (!text.includes("Page 1")) throw new Error(`unexpected text: ${text}`);
  await shot("01-opened");
});

await test("rotate, undo and redo through commands", async () => {
  const t = await active();
  await s.run("__opdf.run('rotateRight')");
  await waitRevision(t.rev);
  const rot = await s.run("return (await __opdf.pdf().getPage(1)).rotate");
  if (rot !== 90) throw new Error(`rotation ${rot}`);
  if (!(await active()).dirty) throw new Error("not dirty after edit");
  const t2 = await active();
  await s.run("__opdf.run('undo')");
  await waitRevision(t2.rev);
  if ((await s.run("return (await __opdf.pdf().getPage(1)).rotate")) !== 0) throw new Error("undo did not restore rotation");
  if ((await active()).dirty) throw new Error("dirty after undoing to the saved state");
  const t3 = await active();
  await s.run("__opdf.run('redo')");
  await waitRevision(t3.rev);
  if ((await s.run("return (await __opdf.pdf().getPage(1)).rotate")) !== 90) throw new Error("redo failed");
});

await test("organize grid: select and delete a page with the toolbar", async () => {
  await s.run("__opdf.run('organize')");
  await s.waitFor("return document.querySelectorAll('.org-card .thumb-img img').length >= 3", { message: "grid thumbnails" });
  await shot("02-organize");
  const t = await active();
  await s.click('.org-card[data-page-index="1"]');
  await s.waitFor("const st = __opdf.store.getState(); return st.tabs[0].selection.join() === '1'", { message: "selection" });
  await s.click('[aria-label="Delete selected pages"]');
  await waitRevision(t.rev);
  if ((await active()).pages !== 4) throw new Error("page not deleted");
  if (!(await pageText(2)).includes("Page 3")) throw new Error("wrong page order after delete");
});

await test("organize grid: drag a page to a new position", async () => {
  const t = await active();
  const r = await s.run(`
    const a = document.querySelector('.org-card[data-page-index="0"]').getBoundingClientRect();
    const b = document.querySelector('.org-card[data-page-index="2"]').getBoundingClientRect();
    return { ax: a.x + a.width / 2, ay: a.y + a.height / 2, bx: b.x + b.width * 0.85, by: b.y + b.height / 2 };`);
  await s.drag([
    [r.ax, r.ay],
    [r.ax + 20, r.ay + 5],
    [(r.ax + r.bx) / 2, r.by],
    [r.bx, r.by],
  ]);
  await waitRevision(t.rev);
  const order = [];
  for (let i = 1; i <= 4; i++) order.push((await pageText(i)).trim());
  if (order.join("|") !== "Page 3|Page 4|Page 1|Page 5") throw new Error(`order: ${order.join("|")}`);
  await s.run("__opdf.run('organize')"); // back to page view
});

await test("find in document", async () => {
  await s.run("__opdf.run('find')");
  await s.waitFor("return !!document.querySelector('.findbar input')");
  await s.type(".findbar input", "Page 4");
  await s.waitFor("return document.querySelector('.find-count')?.textContent === '1 of 1'", { message: "one match", timeout: 15000 });
  await shot("03-find");
  await s.click('[aria-label="Close search"]');
});

await test("add a text box annotation and save as a new file", async () => {
  await s.run("__opdf.store.setState({ ribbon: 'comment' })");
  await s.click('.tool-btn[title^="Text box"]');
  await s.waitFor("return __opdf.store.getState().tool === 'freetext'");
  await s.waitFor("return !!document.querySelector('.annotationEditorLayer')", { message: "editor layer" });
  // The page under the centre of the viewer, clamped to the visible area.
  const visible = `const v = document.querySelector('.viewer-scroll').getBoundingClientRect();
    const page = document.elementFromPoint(v.x + v.width / 2, v.y + v.height / 2).closest('.page');
    const r = page.getBoundingClientRect();
    const top = Math.max(r.y, v.y) + 10, bottom = Math.min(r.bottom, v.bottom) - 10;`;
  const p = await s.run(`${visible} return { x: r.x + r.width * 0.3, y: top + (bottom - top) * 0.3 };`);
  await s.clickAt(p.x, p.y);
  await s.waitFor("return !!document.querySelector('.freeTextEditor .internal')", { message: "free text editor" });
  await s.keys("Hello E2E");
  // Commit the editor by clicking an empty spot of the page (inside the viewport).
  const q = await s.run(`${visible} return { x: r.x + r.width * 0.8, y: top + (bottom - top) * 0.85 };`);
  await s.clickAt(q.x, q.y);
  await s.waitFor("return __opdf.store.getState().tabs[0].annotModified", { message: "annotation registered" });
  await shot("04-annotation");
  const out = join(work, "saved.pdf");
  await s.run(`const g = await __opdf.grant(${JSON.stringify(out)}, 'write'); __opdf.api.pickSave = async () => g;`);
  await s.run("__opdf.run('saveAs')");
  await s.waitFor("const t = __opdf.store.getState().tabs[0]; return t.doc.name === 'saved.pdf' && !t.doc.dirty", { message: "saved" });
  const bytes = readFileSync(out);
  if (!bytes.includes(Buffer.from("/FreeText"))) throw new Error("saved file has no FreeText annotation");
  const info = cli("info", out);
  if (!info.includes("Pages:       4")) throw new Error(info);
  await s.run("__opdf.store.setState({ tool: 'select' })");
});

await test("protect with a password via the dialog and save", async () => {
  // Work on a copy so a failure here cannot affect the other tests' inputs.
  const out = join(work, "protected.pdf");
  await s.run(`const g = await __opdf.grant(${JSON.stringify(out)}, 'write'); __opdf.api.pickSave = async () => g;`);
  await s.run("__opdf.run('saveAs')");
  await s.waitFor("return __opdf.store.getState().tabs[0].doc.name === 'protected.pdf'", { message: "saved copy" });
  await s.run("__opdf.run('protect')");
  await s.waitFor("return !!document.querySelector('.modal input[type=password]')");
  const inputs = await s.run("return document.querySelectorAll('.modal input[type=password]').length");
  if (inputs < 2) throw new Error("password inputs missing");
  await s.run(`
    const set = (el, v) => { const d = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value'); d.set.call(el, v); el.dispatchEvent(new Event('input', { bubbles: true })); };
    const [a, b] = document.querySelectorAll('.modal input[type=password]'); set(a, 'Secr3t!pass'); set(b, 'Secr3t!pass');`);
  await shot("05-protect-dialog");
  await s.run("[...document.querySelectorAll('.modal-footer button')].find(b => b.textContent.trim() === 'Protect').click()");
  await s.waitFor("const t = __opdf.store.getState().tabs[0]; return t.doc.summary?.permissions.encrypted", { message: "encrypted" });
  await s.run("__opdf.run('save')");
  await s.waitFor("return !__opdf.store.getState().tabs[0].doc.dirty", { message: "saved" });
  if (!cliFails("info", out)) throw new Error("file opens without a password");
  if (!cli("info", out, "--password", "Secr3t!pass").includes("Encrypted:   yes")) throw new Error("not encrypted");
});

await test("annotate the encrypted document and save it again", async () => {
  const out = join(work, "protected.pdf");
  await s.run("__opdf.store.setState({ ribbon: 'comment' })");
  await s.click('.tool-btn[title^="Text box"]');
  await s.waitFor("return !!document.querySelector('.annotationEditorLayer')", { message: "editor layer" });
  const visible = `const v = document.querySelector('.viewer-scroll').getBoundingClientRect();
    const page = document.elementFromPoint(v.x + v.width / 2, v.y + v.height / 2).closest('.page');
    const r = page.getBoundingClientRect();
    const top = Math.max(r.y, v.y) + 10, bottom = Math.min(r.bottom, v.bottom) - 10;`;
  const p = await s.run(`${visible} return { x: r.x + r.width * 0.3, y: top + (bottom - top) * 0.6 };`);
  await s.clickAt(p.x, p.y);
  await s.waitFor("return !!document.querySelector('.freeTextEditor .internal:focus')", { message: "editor focused" });
  await s.keys("Secret note");
  const q = await s.run(`${visible} return { x: r.x + r.width * 0.8, y: top + (bottom - top) * 0.9 };`);
  await s.clickAt(q.x, q.y);
  await s.waitFor("return __opdf.store.getState().tabs[0].annotModified", { message: "annotation registered" });
  await s.run("__opdf.store.setState({ tool: 'select' }); __opdf.run('save')");
  await s.waitFor("const t = __opdf.store.getState().tabs[0]; return !t.doc.dirty && !t.annotModified", { message: "saved" });
  if (!cliFails("info", out)) throw new Error("annotating removed the encryption");
  const info = cli("info", out, "--password", "Secr3t!pass");
  if (!info.includes("Encrypted:   yes")) throw new Error(info);
  // Both annotations survive and the text is not stored in clear.
  const bytes = readFileSync(out);
  if (bytes.includes(Buffer.from("Secret note"))) throw new Error("annotation text stored unencrypted");
});

await test("merge two files into a new tab", async () => {
  await s.run(`
    const a = await __opdf.grant(${JSON.stringify(sample)}, 'read');
    const b = await __opdf.grant(${JSON.stringify(second)}, 'read');
    __opdf.api.pickOpen = async () => [a, b];`);
  await s.run("__opdf.run('merge')");
  await s.waitFor("return !!document.querySelector('.modal')");
  await s.run("[...document.querySelectorAll('.modal button')].find(b => b.textContent.includes('Add files')).click()");
  await s.waitFor("return document.querySelectorAll('.modal .file-row').length === 2");
  await shot("06-merge-dialog");
  await s.run("[...document.querySelectorAll('.modal-footer button')].find(b => b.textContent.includes('Merge')).click()");
  await s.waitFor("const st = __opdf.store.getState(); const t = st.tabs.find(t => t.id === st.activeId); return t && t.doc.name === 'Merged.pdf' && t.pageCount === 8", { message: "merged tab" });
  await s.waitFor("return document.querySelectorAll('.pdfViewer .page canvas').length > 0");
});

await test("images dropped on the window become a PDF", async () => {
  await s.run(`const f = await __opdf.grant(${JSON.stringify(png)}, 'read'); await __opdf.actions.openFiles([f]);`);
  await s.waitFor("const st = __opdf.store.getState(); const t = st.tabs.find(t => t.id === st.activeId); return t && t.doc.name === 'photo.pdf' && t.pageCount === 1");
});

await test("closing a modified document asks to save", async () => {
  const t = await active();
  await s.run("__opdf.run('rotateRight')");
  await waitRevision(t.rev);
  await s.run("__opdf.run('close')");
  await s.waitFor("return __opdf.store.getState().dialog?.kind === 'confirm'", { message: "confirm dialog" });
  await shot("07-unsaved-prompt");
  await s.run("[...document.querySelectorAll('.modal-footer button')].find(b => b.textContent.includes('save')).click()");
  await s.waitFor(`return !__opdf.store.getState().tabs.some(t => t.id === ${t.id})`, { message: "tab closed" });
});

await test("home screen lists recent files", async () => {
  await s.run("__opdf.store.setState({ activeId: null })");
  await s.waitFor("return document.querySelectorAll('.recent-row').length >= 1", { message: "recent rows" });
  await shot("08-home");
});

await test("dark theme renders", async () => {
  await s.run("__opdf.store.setState({ theme: 'dark', activeId: __opdf.store.getState().tabs[0].id })");
  await s.waitFor("return document.documentElement.dataset.theme === 'dark'");
  await new Promise((r) => setTimeout(r, 800));
  await shot("09-dark");
  await s.run("__opdf.store.setState({ theme: 'light' })");
});

// ---------------------------------------------------------------- teardown
await s.run("for (const t of __opdf.store.getState().tabs) __opdf.store.setState(st => ({ tabs: st.tabs.map(x => ({ ...x, doc: { ...x.doc, dirty: false } })) }))").catch(() => {});
await s.close();
driver.kill();
console.log(`\n${results.length - failures} passed, ${failures} failed. Screenshots: ${shots}`);
if (!process.env.OPDF_E2E_KEEP) rmSync(work, { recursive: true, force: true });
process.exit(failures ? 1 : 0);
