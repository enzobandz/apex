import { useMemo, useState } from "react";
import { FileInput, FileOutput, FilePlus, FileText, Scissors } from "lucide-react";
import { Button, Checkbox, Field, Modal, Segmented, Spinner } from "../components/ui";
import { api } from "../lib/ipc";
import type { FileRef, SplitPlan } from "../lib/types";
import { formatRanges, parseRanges } from "../lib/ranges";
import { adoptDoc, applyOp, commitAnnotations, confirmSignedEdit, describe, setDoc } from "../state/actions";
import { closeDialog, toast, useActiveTab, type Tab } from "../state/store";
import { A4_OR_LETTER } from "../components/pageMenu";
import { canvasToBytes, releaseCanvas, renderPageToCanvas } from "../lib/pdfjs";
import { peekPdf } from "../viewer/pdfRegistry";
import { stem } from "../lib/format";

function useRange(tab: Tab | null, initial: string) {
  const [text, setText] = useState(initial);
  const parsed = useMemo(() => {
    if (!tab) return { pages: [] as number[], error: null as string | null };
    try {
      return { pages: parseRanges(text, tab.pageCount), error: null };
    } catch (e) {
      return { pages: [] as number[], error: (e as Error).message };
    }
  }, [text, tab]);
  return { text, setText, ...parsed };
}

function defaultRange(tab: Tab | null) {
  if (!tab) return "";
  return tab.selection.length ? formatRanges(tab.selection) : String(tab.page);
}

export function ExtractDialog() {
  const tab = useActiveTab();
  const r = useRange(tab, defaultRange(tab));
  const [target, setTarget] = useState<"file" | "tab">("file");
  const [andDelete, setAndDelete] = useState(false);
  const [working, setWorking] = useState(false);
  if (!tab) return null;
  const run = async () => {
    setWorking(true);
    try {
      if (!(await commitAnnotations(tab))) return;
      if (target === "file") {
        const dest = await api.pickSave(`${stem(tab.doc.name)} (pages ${formatRanges([...r.pages].sort((a, b) => a - b))}).pdf`);
        if (!dest) return;
        await api.extract(tab.id, r.pages, dest.id);
        toast("success", `Extracted ${r.pages.length} page${r.pages.length > 1 ? "s" : ""}`, dest.path);
      } else {
        adoptDoc(await api.extractToTab(tab.id, r.pages));
      }
      if (andDelete) {
        const unique = [...new Set(r.pages)];
        if (unique.length < tab.pageCount) await applyOp({ kind: "delete", pages: unique }, tab);
      }
      closeDialog();
    } catch (e) {
      toast("error", "Extraction failed", describe(e));
    } finally {
      setWorking(false);
    }
  };
  return (
    <Modal
      title="Extract pages"
      icon={<FileOutput size={18} />}
      onClose={() => closeDialog()}
      footer={
        <>
          <Button onClick={() => closeDialog()}>Cancel</Button>
          <Button variant="primary" disabled={!!r.error || working} onClick={() => void run()} icon={working ? <Spinner size={14} /> : undefined}>
            Extract
          </Button>
        </>
      }
    >
      <Field label="Pages" error={r.error} hint={`e.g. 1-3, 5, 8-  ·  “odd”, “even”  ·  document has ${tab.pageCount} pages`}>
        <input className="input" value={r.text} data-autofocus onChange={(e) => r.setText(e.target.value)} />
      </Field>
      <Field label="Destination">
        <Segmented
          label="Destination"
          value={target}
          onChange={setTarget}
          options={[
            { value: "file", label: "Save as a new file" },
            { value: "tab", label: "Open in a new tab" },
          ]}
        />
      </Field>
      <Checkbox checked={andDelete} onChange={setAndDelete} label="Delete the extracted pages from this document" />
    </Modal>
  );
}

export function SplitDialog() {
  const tab = useActiveTab();
  const [mode, setMode] = useState<"every" | "single" | "ranges">("every");
  const [n, setN] = useState(1);
  const [ranges, setRanges] = useState("1-2\n3-");
  const [working, setWorking] = useState(false);
  if (!tab) return null;
  const plan: SplitPlan =
    mode === "every" ? { kind: "everyN", n } : mode === "single" ? { kind: "singlePages" } : { kind: "ranges", ranges: ranges.split(/\n/).filter((x) => x.trim()) };
  let error: string | null = null;
  let parts = 0;
  try {
    if (plan.kind === "everyN") {
      if (!(n >= 1)) throw new Error("Enter 1 or more pages per file");
      parts = Math.ceil(tab.pageCount / n);
    } else if (plan.kind === "singlePages") parts = tab.pageCount;
    else {
      plan.ranges.forEach((x) => parseRanges(x, tab.pageCount));
      parts = plan.ranges.length;
      if (!parts) throw new Error("Enter at least one range");
    }
  } catch (e) {
    error = (e as Error).message;
  }
  const run = async () => {
    const folder = await api.pickFolder();
    if (!folder) return;
    setWorking(true);
    try {
      if (!(await commitAnnotations(tab))) return;
      const out = await api.split(tab.id, plan, folder.id);
      closeDialog();
      toast("success", `Created ${out.length} files`, folder.path);
    } catch (e) {
      toast("error", "Split failed", describe(e));
    } finally {
      setWorking(false);
    }
  };
  return (
    <Modal
      title="Split document"
      icon={<Scissors size={18} />}
      onClose={() => closeDialog()}
      footer={
        <>
          <span className="grow field-hint">{error ? "" : `${parts} file${parts === 1 ? "" : "s"} will be created`}</span>
          <Button onClick={() => closeDialog()}>Cancel</Button>
          <Button variant="primary" disabled={!!error || working} onClick={() => void run()} icon={working ? <Spinner size={14} /> : undefined}>
            Choose folder & split
          </Button>
        </>
      }
    >
      <Segmented
        label="Split mode"
        value={mode}
        onChange={setMode}
        options={[
          { value: "every", label: "Every N pages" },
          { value: "single", label: "Each page" },
          { value: "ranges", label: "Custom ranges" },
        ]}
      />
      {mode === "every" && (
        <Field label="Pages per file" error={error}>
          <input className="input" type="number" min={1} max={tab.pageCount} value={n} onChange={(e) => setN(Number(e.target.value))} data-autofocus />
        </Field>
      )}
      {mode === "ranges" && (
        <Field label="One file per line" error={error} hint="Each line is a page range, e.g. 1-3 or 4, 6, 9-">
          <textarea className="input" rows={5} value={ranges} onChange={(e) => setRanges(e.target.value)} />
        </Field>
      )}
      <p className="field-hint">The original document is not changed. Password protection is kept in the new files.</p>
    </Modal>
  );
}

export function InsertBlankDialog() {
  const tab = useActiveTab();
  const [count, setCount] = useState(1);
  const [where, setWhere] = useState<"after" | "before" | "end">("after");
  const [size, setSize] = useState<"match" | "a4" | "letter">("match");
  if (!tab) return null;
  const ref = tab.selection.length ? Math.max(...tab.selection) : tab.page - 1;
  const first = tab.selection.length ? Math.min(...tab.selection) : tab.page - 1;
  const at = where === "after" ? ref + 1 : where === "before" ? first : tab.pageCount;
  const run = async () => {
    let dims: [number, number] = A4_OR_LETTER();
    if (size === "a4") dims = [595.276, 841.89];
    else if (size === "letter") dims = [612, 792];
    else {
      const pdf = peekPdf(tab.id, tab.doc.revision);
      if (pdf) {
        // Match the page as the user sees it (including its rotation).
        const page = await pdf.getPage(Math.min(tab.pageCount, ref + 1));
        const vp = page.getViewport({ scale: 1 });
        dims = [vp.width, vp.height];
      }
    }
    closeDialog();
    await applyOp({ kind: "insertBlank", at, width: dims[0], height: dims[1], count }, tab);
  };
  return (
    <Modal
      title="Insert blank pages"
      icon={<FilePlus size={18} />}
      onClose={() => closeDialog()}
      footer={
        <>
          <Button onClick={() => closeDialog()}>Cancel</Button>
          <Button variant="primary" disabled={!(count >= 1 && count <= 1000)} onClick={() => void run()}>
            Insert
          </Button>
        </>
      }
    >
      <div className="grid-2">
        <Field label="Number of pages">
          <input className="input" type="number" min={1} max={1000} value={count} onChange={(e) => setCount(Number(e.target.value))} data-autofocus />
        </Field>
        <Field label="Page size">
          <select className="select" value={size} onChange={(e) => setSize(e.target.value as typeof size)}>
            <option value="match">Same as page {ref + 1}</option>
            <option value="a4">A4</option>
            <option value="letter">US Letter</option>
          </select>
        </Field>
      </div>
      <Field label="Position">
        <Segmented
          label="Position"
          value={where}
          onChange={setWhere}
          options={[
            { value: "before", label: `Before page ${first + 1}` },
            { value: "after", label: `After page ${ref + 1}` },
            { value: "end", label: "At the end" },
          ]}
        />
      </Field>
    </Modal>
  );
}

export function InsertPagesDialog() {
  const tab = useActiveTab();
  const [file, setFile] = useState<FileRef | null>(null);
  const [pages, setPages] = useState("");
  const [password, setPassword] = useState("");
  const [needsPw, setNeedsPw] = useState(false);
  const [where, setWhere] = useState<"after" | "before" | "end">("after");
  const [working, setWorking] = useState(false);
  if (!tab) return null;
  const ref = tab.selection.length ? Math.max(...tab.selection) : tab.page - 1;
  const at = where === "after" ? ref + 1 : where === "before" ? (tab.selection.length ? Math.min(...tab.selection) : tab.page - 1) : tab.pageCount;
  const run = async () => {
    if (!file) return;
    if (!(await confirmSignedEdit(tab))) return;
    setWorking(true);
    try {
      if (!(await commitAnnotations(tab))) return;
      const doc = await api.insertFrom(tab.id, file.id, at, pages.trim() || null, password || undefined);
      setDoc(doc);
      closeDialog();
      toast("success", `Inserted pages from ${file.name}`);
    } catch (e) {
      const msg = describe(e);
      if (/password/i.test(msg)) setNeedsPw(true);
      toast("error", "Could not insert pages", msg);
    } finally {
      setWorking(false);
    }
  };
  return (
    <Modal
      title="Insert pages from a file"
      icon={<FileInput size={18} />}
      onClose={() => closeDialog()}
      footer={
        <>
          <Button onClick={() => closeDialog()}>Cancel</Button>
          <Button variant="primary" disabled={!file || working} onClick={() => void run()} icon={working ? <Spinner size={14} /> : undefined}>
            Insert
          </Button>
        </>
      }
    >
      <div className="row">
        <Button
          data-autofocus
          onClick={async () => {
            const f = (await api.pickOpen("pdf", false))[0];
            if (f) setFile(f);
          }}
        >
          Choose PDF…
        </Button>
        {file && (
          <span className="row">
            <FileText size={16} /> {file.name}
          </span>
        )}
      </div>
      <Field label="Pages to insert" hint="Leave empty to insert every page, or enter ranges like 1-3, 7">
        <input className="input" value={pages} placeholder="All pages" onChange={(e) => setPages(e.target.value)} />
      </Field>
      {needsPw && (
        <Field label="Password of the inserted file">
          <input className="input" type="password" value={password} onChange={(e) => setPassword(e.target.value)} />
        </Field>
      )}
      <Field label="Position">
        <Segmented
          label="Position"
          value={where}
          onChange={setWhere}
          options={[
            { value: "before", label: `Before page ${(tab.selection.length ? Math.min(...tab.selection) : tab.page - 1) + 1}` },
            { value: "after", label: `After page ${ref + 1}` },
            { value: "end", label: "At the end" },
          ]}
        />
      </Field>
    </Modal>
  );
}

export function ExportImagesDialog() {
  const tab = useActiveTab();
  const r = useRange(tab, "all");
  const [format, setFormat] = useState<"png" | "jpeg">("png");
  const [dpi, setDpi] = useState(150);
  const [progress, setProgress] = useState<{ done: number; total: number } | null>(null);
  const [cancel, setCancel] = useState<{ cancelled: boolean } | null>(null);
  if (!tab) return null;
  const run = async () => {
    const folder = await api.pickFolder();
    if (!folder) return;
    const pdf = peekPdf(tab.id, tab.doc.revision);
    if (!pdf) return;
    const token = { cancelled: false };
    setCancel(token);
    const pages = [...new Set(r.pages)];
    const base = stem(tab.doc.name);
    const digits = String(tab.pageCount).length;
    let written = 0;
    try {
      for (const i of pages) {
        if (token.cancelled) break;
        setProgress({ done: written, total: pages.length });
        const page = await pdf.getPage(i + 1);
        const canvas = await renderPageToCanvas(page, { scale: dpi / 72, forPrint: true });
        const bytes = await canvasToBytes(canvas, format === "png" ? "image/png" : "image/jpeg", 0.92);
        releaseCanvas(canvas);
        await api.writeExport(folder.id, `${base} - page ${String(i + 1).padStart(digits, "0")}.${format === "png" ? "png" : "jpg"}`, bytes);
        written++;
      }
      closeDialog();
      toast("success", `Exported ${written} page${written === 1 ? "" : "s"}`, folder.path);
    } catch (e) {
      toast("error", "Export failed", describe(e));
    } finally {
      setProgress(null);
      setCancel(null);
    }
  };
  const megapixels = (dpi / 72) ** 2 * 612 * 792 / 1e6;
  return (
    <Modal
      title="Export pages as images"
      icon={<FileOutput size={18} />}
      onClose={() => {
        if (cancel) cancel.cancelled = true;
        closeDialog();
      }}
      footer={
        <>
          {progress && (
            <span className="grow row">
              <Spinner size={14} /> {progress.done} / {progress.total}
            </span>
          )}
          <Button
            onClick={() => {
              if (cancel) cancel.cancelled = true;
              else closeDialog();
            }}
          >
            {cancel ? "Stop" : "Cancel"}
          </Button>
          <Button variant="primary" disabled={!!r.error || !!progress} onClick={() => void run()}>
            Choose folder & export
          </Button>
        </>
      }
    >
      <Field label="Pages" error={r.error} hint={`${tab.pageCount} pages in this document`}>
        <input className="input" value={r.text} onChange={(e) => r.setText(e.target.value)} data-autofocus />
      </Field>
      <div className="grid-2">
        <Field label="Format">
          <Segmented
            label="Format"
            value={format}
            onChange={setFormat}
            options={[
              { value: "png", label: "PNG" },
              { value: "jpeg", label: "JPEG" },
            ]}
          />
        </Field>
        <Field label="Resolution" hint={`≈ ${megapixels.toFixed(1)} MP for a Letter page`}>
          <select className="select" value={dpi} onChange={(e) => setDpi(Number(e.target.value))}>
            <option value={72}>72 dpi (screen)</option>
            <option value={150}>150 dpi</option>
            <option value={200}>200 dpi</option>
            <option value={300}>300 dpi (print)</option>
            <option value={600}>600 dpi</option>
          </select>
        </Field>
      </div>
    </Modal>
  );
}
