// Copies pdf.js runtime assets (CMaps, standard fonts, WASM decoders, ICC
// profiles, annotation icons) into public/pdfjs so they ship with the app and
// are served from the app origin — no network access at runtime.
import { cpSync, existsSync, mkdirSync, rmSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const src = join(root, "node_modules", "pdfjs-dist");
const dest = join(root, "public", "pdfjs");
if (!existsSync(src)) {
  console.error("pdfjs-dist is not installed; run npm ci first");
  process.exit(1);
}
rmSync(dest, { recursive: true, force: true });
mkdirSync(dest, { recursive: true });
for (const dir of ["cmaps", "standard_fonts", "wasm", "iccs"]) {
  if (existsSync(join(src, dir))) cpSync(join(src, dir), join(dest, dir), { recursive: true });
}
cpSync(join(src, "web", "images"), join(dest, "images"), { recursive: true });
console.log("pdf.js assets copied to public/pdfjs");
