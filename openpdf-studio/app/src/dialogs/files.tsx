import { useState } from "react";
import { ArrowDown, ArrowUp, Combine, FileImage, FileText, Plus, X } from "lucide-react";
import { Button, Checkbox, Field, IconButton, Modal, Segmented, Spinner } from "../components/ui";
import { api, AppError } from "../lib/ipc";
import type { FileRef, ImagePageOptions } from "../lib/types";
import { adoptDoc, describe } from "../state/actions";
import { closeDialog, toast } from "../state/store";
import { useDropHandler } from "../state/drop";
import { formatBytes } from "../lib/format";

interface MergeItem {
  file: FileRef;
  pages: string;
  password: string;
  needsPassword: boolean;
}

function move<T>(list: T[], i: number, d: number): T[] {
  const j = i + d;
  if (j < 0 || j >= list.length) return list;
  const out = [...list];
  [out[i], out[j]] = [out[j]!, out[i]!];
  return out;
}

export function MergeDialog() {
  const [items, setItems] = useState<MergeItem[]>([]);
  const [bookmarks, setBookmarks] = useState(true);
  const [working, setWorking] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const add = (files: FileRef[]) => {
    const pdfs = files.filter((f) => f.kind === "pdf");
    if (pdfs.length < files.length) toast("info", "Only PDF files can be merged", "Use “Images to PDF” for images.");
    setItems((prev) => [...prev, ...pdfs.map((file) => ({ file, pages: "", password: "", needsPassword: false }))]);
  };
  useDropHandler(add);

  const pick = async () => add(await api.pickOpen("pdf", true));

  const merge = async () => {
    setWorking(true);
    setError(null);
    try {
      const doc = await api.merge(
        items.map((i) => ({ fileId: i.file.id, pages: i.pages.trim() || null, password: i.password || null })),
        bookmarks,
      );
      closeDialog();
      adoptDoc(doc);
      toast("success", `Merged ${items.length} files`, "The result is open as a new, unsaved document.");
    } catch (e) {
      if (e instanceof AppError && (e.code === "password_required" || e.code === "wrong_password")) {
        const name = e.message.split(":")[0];
        setItems((prev) => prev.map((i) => (i.file.name.replace(/\.pdf$/i, "") === name ? { ...i, needsPassword: true } : i)));
      }
      setError(describe(e));
    } finally {
      setWorking(false);
    }
  };

  return (
    <Modal
      title="Merge files"
      icon={<Combine size={18} />}
      width={620}
      onClose={() => closeDialog()}
      footer={
        <>
          <Checkbox checked={bookmarks} onChange={setBookmarks} label="Add a bookmark for each file" />
          <span className="grow" />
          <Button onClick={() => closeDialog()}>Cancel</Button>
          <Button variant="primary" disabled={items.length < 2 || working} onClick={() => void merge()} icon={working ? <Spinner size={14} /> : undefined}>
            Merge {items.length > 1 ? `${items.length} files` : ""}
          </Button>
        </>
      }
    >
      <p>Files are combined in this order. Leave “Pages” empty to include every page, or enter ranges like 1-3, 7.</p>
      <div className="file-list">
        {items.map((it, i) => (
          <div key={`${it.file.id}-${i}`} className="file-row">
            <FileText size={18} color="var(--accent)" />
            <div className="meta">
              <div className="name" title={it.file.path}>
                {it.file.name}
              </div>
              <div className="sub">{formatBytes(it.file.size)}</div>
            </div>
            {it.needsPassword && (
              <input
                className="input input-sm"
                type="password"
                placeholder="Password"
                aria-label={`Password for ${it.file.name}`}
                value={it.password}
                onChange={(e) => setItems((p) => p.map((x, j) => (j === i ? { ...x, password: e.target.value } : x)))}
              />
            )}
            <input
              className="input input-sm"
              placeholder="All pages"
              aria-label={`Pages of ${it.file.name}`}
              value={it.pages}
              onChange={(e) => setItems((p) => p.map((x, j) => (j === i ? { ...x, pages: e.target.value } : x)))}
            />
            <IconButton label="Move up" disabled={i === 0} onClick={() => setItems((p) => move(p, i, -1))}>
              <ArrowUp size={15} />
            </IconButton>
            <IconButton label="Move down" disabled={i === items.length - 1} onClick={() => setItems((p) => move(p, i, 1))}>
              <ArrowDown size={15} />
            </IconButton>
            <IconButton label={`Remove ${it.file.name}`} onClick={() => setItems((p) => p.filter((_, j) => j !== i))}>
              <X size={15} />
            </IconButton>
          </div>
        ))}
        {items.length === 0 && <div className="callout">Add at least two PDF files — use the button below or drop files onto this window.</div>}
      </div>
      <div>
        <Button icon={<Plus size={16} />} onClick={() => void pick()} data-autofocus>
          Add files…
        </Button>
      </div>
      {error && <div className="callout danger">{error}</div>}
    </Modal>
  );
}

export function ImagesDialog() {
  const [files, setFiles] = useState<FileRef[]>([]);
  const [opts, setOpts] = useState<ImagePageOptions>({ pageSize: "a4", orientation: "auto", margin: 24 });
  const [working, setWorking] = useState(false);
  const add = (f: FileRef[]) => {
    const imgs = f.filter((x) => x.kind === "image");
    if (imgs.length < f.length) toast("info", "Only images can be added here");
    setFiles((p) => [...p, ...imgs]);
  };
  useDropHandler(add);
  const convert = async () => {
    setWorking(true);
    try {
      const doc = await api.imagesToPdf(
        files.map((f) => f.id),
        opts,
      );
      closeDialog();
      adoptDoc(doc);
      toast("success", `Created a ${files.length}-page PDF`, "Save it to keep it.");
    } catch (e) {
      toast("error", "Conversion failed", describe(e));
    } finally {
      setWorking(false);
    }
  };
  return (
    <Modal
      title="Images to PDF"
      icon={<FileImage size={18} />}
      width={620}
      onClose={() => closeDialog()}
      footer={
        <>
          <Button onClick={() => closeDialog()}>Cancel</Button>
          <Button variant="primary" disabled={!files.length || working} onClick={() => void convert()} icon={working ? <Spinner size={14} /> : undefined}>
            Create PDF
          </Button>
        </>
      }
    >
      <p>One page per image, in this order. JPEG photos are embedded without re-compression; other formats are stored losslessly.</p>
      <div className="file-list">
        {files.map((f, i) => (
          <div key={`${f.id}-${i}`} className="file-row">
            <FileImage size={18} color="var(--accent)" />
            <div className="meta">
              <div className="name">{f.name}</div>
              <div className="sub">{formatBytes(f.size)}</div>
            </div>
            <IconButton label="Move up" disabled={i === 0} onClick={() => setFiles((p) => move(p, i, -1))}>
              <ArrowUp size={15} />
            </IconButton>
            <IconButton label="Move down" disabled={i === files.length - 1} onClick={() => setFiles((p) => move(p, i, 1))}>
              <ArrowDown size={15} />
            </IconButton>
            <IconButton label={`Remove ${f.name}`} onClick={() => setFiles((p) => p.filter((_, j) => j !== i))}>
              <X size={15} />
            </IconButton>
          </div>
        ))}
      </div>
      <div>
        <Button icon={<Plus size={16} />} data-autofocus onClick={async () => add(await api.pickOpen("images", true))}>
          Add images…
        </Button>
      </div>
      <div className="grid-2">
        <Field label="Page size">
          <select className="select" value={opts.pageSize} onChange={(e) => setOpts({ ...opts, pageSize: e.target.value as ImagePageOptions["pageSize"] })}>
            <option value="a4">A4</option>
            <option value="letter">US Letter</option>
            <option value="fitImage">Same as image</option>
          </select>
        </Field>
        <Field label="Margin">
          <select
            className="select"
            disabled={opts.pageSize === "fitImage"}
            value={opts.margin}
            onChange={(e) => setOpts({ ...opts, margin: Number(e.target.value) })}
          >
            <option value={0}>None</option>
            <option value={12}>Small</option>
            <option value={24}>Medium</option>
            <option value={48}>Large</option>
          </select>
        </Field>
      </div>
      {opts.pageSize !== "fitImage" && (
        <Field label="Orientation">
          <Segmented
            label="Orientation"
            value={opts.orientation}
            onChange={(o) => setOpts({ ...opts, orientation: o })}
            options={[
              { value: "auto", label: "Automatic" },
              { value: "portrait", label: "Portrait" },
              { value: "landscape", label: "Landscape" },
            ]}
          />
        </Field>
      )}
    </Modal>
  );
}
