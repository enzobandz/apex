import { useEffect, useRef, useState } from "react";
import { ArchiveRestore, Gauge, Info, Lock, PenLine, Trash, Upload } from "lucide-react";
import { Button, Checkbox, Field, Modal, Segmented, Spinner } from "../components/ui";
import { api, AppError } from "../lib/ipc";
import type { AllowedActions, MetadataUpdate, Recoverable } from "../lib/types";
import { adoptDoc, applyOp, describe } from "../state/actions";
import { ask, closeDialog, set, toast, useActiveTab, useStore } from "../state/store";
import { formatBytes, formatDate, percentDiff } from "../lib/format";
import { placeImage } from "../state/commands";
import { saveSettings, savedSignature } from "../state/settings";

function strength(pw: string): { score: number; label: string; color: string } {
  let s = 0;
  if (pw.length >= 8) s++;
  if (pw.length >= 12) s++;
  if (/[a-z]/.test(pw) && /[A-Z]/.test(pw)) s++;
  if (/\d/.test(pw)) s++;
  if (/[^A-Za-z0-9]/.test(pw)) s++;
  if (pw.length >= 16) s++;
  const levels = [
    { label: "Very weak", color: "var(--danger)" },
    { label: "Weak", color: "var(--danger)" },
    { label: "Fair", color: "var(--warning)" },
    { label: "Good", color: "var(--warning)" },
    { label: "Strong", color: "var(--success)" },
    { label: "Very strong", color: "var(--success)" },
  ];
  const l = levels[Math.min(5, s)]!;
  return { score: Math.min(5, s), ...l };
}

export function ProtectDialog() {
  const tab = useActiveTab();
  const [user, setUser] = useState("");
  const [user2, setUser2] = useState("");
  const [restrict, setRestrict] = useState(false);
  const [owner, setOwner] = useState("");
  const [allowed, setAllowed] = useState<AllowedActions>({
    print: true,
    printHighQuality: true,
    modify: false,
    copy: true,
    annotate: true,
    fillForms: true,
    assemble: false,
  });
  if (!tab) return null;
  const st = strength(user);
  const mismatch = user !== user2;
  const restricted = restrict && Object.values(allowed).some((v) => !v);
  let error: string | null = null;
  if (!user && !restricted) error = "Set an open password, restrict actions, or both.";
  else if (mismatch) error = "The passwords do not match.";
  else if (restricted && !owner) error = "Restrictions need a permissions password.";
  else if (restricted && owner === user) error = "The permissions password must differ from the open password.";
  const flag = (k: keyof AllowedActions, label: string) => (
    <Checkbox checked={allowed[k]} onChange={(v) => setAllowed({ ...allowed, [k]: v })} label={label} disabled={!restrict} />
  );
  const run = async () => {
    const res = await applyOp({
      kind: "protect",
      options: {
        userPassword: user,
        ownerPassword: restricted ? owner : "",
        allowed: restricted ? allowed : { print: true, printHighQuality: true, modify: true, copy: true, annotate: true, fillForms: true, assemble: true },
      },
    });
    if (res) {
      closeDialog();
      toast("success", "Protection added", "Save the document to apply it to the file. Keep your password safe — it cannot be recovered.");
    }
  };
  return (
    <Modal
      title="Password protect"
      icon={<Lock size={18} />}
      width={540}
      onClose={() => closeDialog()}
      footer={
        <>
          <span className="grow field-hint">{error}</span>
          <Button onClick={() => closeDialog()}>Cancel</Button>
          <Button variant="primary" disabled={!!error} onClick={() => void run()}>
            Protect
          </Button>
        </>
      }
    >
      <p>Uses AES-256 encryption (PDF 2.0). Without the password the document cannot be opened or recovered by anyone — including you.</p>
      <div className="grid-2">
        <Field label="Open password">
          <input className="input" type="password" autoComplete="new-password" value={user} onChange={(e) => setUser(e.target.value)} data-autofocus />
        </Field>
        <Field label="Confirm" error={user2 && mismatch ? "Does not match" : null}>
          <input className="input" type="password" autoComplete="new-password" value={user2} onChange={(e) => setUser2(e.target.value)} />
        </Field>
      </div>
      {user && (
        <div className="stack" style={{ gap: 4 }}>
          <div className="strength">
            <div style={{ width: `${((st.score + 1) / 6) * 100}%`, background: st.color }} />
          </div>
          <span className="field-hint">{st.label}</span>
        </div>
      )}
      <Checkbox checked={restrict} onChange={setRestrict} label="Restrict what others can do (requires a permissions password)" />
      {restrict && (
        <>
          <Field label="Permissions password" hint="Needed to change these restrictions later.">
            <input className="input" type="password" autoComplete="new-password" value={owner} onChange={(e) => setOwner(e.target.value)} />
          </Field>
          <div className="grid-2">
            {flag("print", "Allow printing")}
            {flag("copy", "Allow copying text and images")}
            {flag("annotate", "Allow comments")}
            {flag("fillForms", "Allow filling forms")}
            {flag("assemble", "Allow page changes")}
            {flag("modify", "Allow other editing")}
          </div>
          <div className="callout warn">
            Permission restrictions are honoured by standard PDF software but are not a substitute for encryption: someone who can open the document
            could still use tools that ignore them.
          </div>
        </>
      )}
    </Modal>
  );
}

export function PropertiesDialog() {
  const tab = useActiveTab();
  const m = tab?.doc.summary?.metadata;
  const [f, setF] = useState<Required<MetadataUpdate>>({
    title: m?.title ?? "",
    author: m?.author ?? "",
    subject: m?.subject ?? "",
    keywords: m?.keywords ?? "",
    creator: m?.creator ?? "",
  });
  if (!tab) return null;
  const s = tab.doc.summary;
  const canEdit = !tab.doc.viewOnlyReason && (s?.permissions.modify ?? false);
  const changed = (Object.keys(f) as (keyof MetadataUpdate)[]).filter((k) => (f[k] ?? "") !== (m?.[k] ?? ""));
  const save = async () => {
    const update: MetadataUpdate = {};
    for (const k of changed) update[k] = f[k];
    const res = await applyOp({ kind: "setMetadata", update });
    if (res) closeDialog();
  };
  const input = (k: keyof MetadataUpdate, label: string, multiline = false) => (
    <Field label={label}>
      {multiline ? (
        <textarea className="input" rows={2} value={f[k]} disabled={!canEdit} onChange={(e) => setF({ ...f, [k]: e.target.value })} />
      ) : (
        <input className="input" value={f[k]} disabled={!canEdit} onChange={(e) => setF({ ...f, [k]: e.target.value })} data-autofocus={k === "title" ? true : undefined} />
      )}
    </Field>
  );
  return (
    <Modal
      title="Document properties"
      icon={<Info size={18} />}
      width={560}
      onClose={() => closeDialog()}
      footer={
        <>
          <Button onClick={() => closeDialog()}>Cancel</Button>
          <Button variant="primary" disabled={!canEdit || changed.length === 0} onClick={() => void save()}>
            Apply
          </Button>
        </>
      }
    >
      {!canEdit && <div className="callout warn">This document’s security settings do not allow editing its properties.</div>}
      {input("title", "Title")}
      <div className="grid-2">
        {input("author", "Author")}
        {input("creator", "Application")}
      </div>
      {input("subject", "Subject", true)}
      {input("keywords", "Keywords")}
      <dl className="info-grid" style={{ padding: 0 }}>
        <dt>Producer</dt>
        <dd>{m?.producer ?? "—"}</dd>
        <dt>Created</dt>
        <dd>{formatDate(m?.creationDate)}</dd>
        <dt>Modified</dt>
        <dd>{formatDate(m?.modificationDate)}</dd>
        <dt>PDF version</dt>
        <dd>{s?.pdfVersion ?? "—"}</dd>
        <dt>File size</dt>
        <dd>{formatBytes(tab.doc.size)}</dd>
      </dl>
    </Modal>
  );
}

const PRESETS = {
  lossless: { label: "Lossless", desc: "Removes unused data, merges duplicates and compresses structure. Content is unchanged.", images: null },
  balanced: { label: "Balanced", desc: "Also downsamples images above 150 dpi (JPEG quality 75). Good for sharing.", images: { maxDpi: 150, jpegQuality: 75 } },
  strong: { label: "Smallest", desc: "Downsamples images above 96 dpi (JPEG quality 60). Best for e-mail; images look softer.", images: { maxDpi: 96, jpegQuality: 60 } },
} as const;

export function OptimizeDialog() {
  const tab = useActiveTab();
  const [preset, setPreset] = useState<keyof typeof PRESETS>("balanced");
  const [result, setResult] = useState<{ before: number; after: number; text: string } | null>(null);
  const busy = useStore((s) => s.busy);
  if (!tab) return null;
  const run = async () => {
    const before = tab.doc.size;
    const res = await applyOp({ kind: "optimize", options: { images: PRESETS[preset].images } });
    if (!res) return;
    const r = res.report;
    const parts = r
      ? [
          r.imagesDownsampled && `${r.imagesDownsampled} image${r.imagesDownsampled === 1 ? "" : "s"} downsampled`,
          r.duplicateStreamsMerged && `${r.duplicateStreamsMerged} duplicate${r.duplicateStreamsMerged === 1 ? "" : "s"} merged`,
          r.streamsCompressed && `${r.streamsCompressed} stream${r.streamsCompressed === 1 ? "" : "s"} compressed`,
          r.unusedObjectsRemoved && `${r.unusedObjectsRemoved} unused object${r.unusedObjectsRemoved === 1 ? "" : "s"} removed`,
        ].filter(Boolean)
      : [];
    setResult({ before, after: res.state.size, text: parts.join(" · ") || "No further savings were possible." });
  };
  return (
    <Modal
      title="Reduce file size"
      icon={<Gauge size={18} />}
      width={540}
      onClose={() => closeDialog()}
      footer={
        result ? (
          <Button variant="primary" onClick={() => closeDialog()}>
            Done
          </Button>
        ) : (
          <>
            <Button onClick={() => closeDialog()}>Cancel</Button>
            <Button variant="primary" disabled={!!busy} onClick={() => void run()} icon={busy ? <Spinner size={14} /> : undefined}>
              Optimize
            </Button>
          </>
        )
      }
    >
      {result ? (
        <div className={`callout ${result.after < result.before ? "success" : ""}`}>
          <div>
            <strong>
              {formatBytes(result.before)} → {formatBytes(result.after)}
            </strong>{" "}
            ({percentDiff(result.before, result.after)})
            <br />
            {result.text}
            <br />
            Save the document to keep the smaller version, or Undo to revert.
          </div>
        </div>
      ) : (
        <>
          <Segmented
            label="Level"
            value={preset}
            onChange={setPreset}
            options={(Object.keys(PRESETS) as (keyof typeof PRESETS)[]).map((k) => ({ value: k, label: PRESETS[k].label }))}
          />
          <p>{PRESETS[preset].desc}</p>
          <p className="field-hint">Current size: {formatBytes(tab.doc.size)}. Only images whose encoding is fully understood are recompressed; others are kept as they are.</p>
        </>
      )}
    </Modal>
  );
}

export function RecoveryDialog() {
  const items = useStore((s) => s.recoverable);
  const [working, setWorking] = useState<string | null>(null);
  const refresh = async () => set({ recoverable: await api.recoveryList().catch(() => []) });
  const restore = async (r: Recoverable) => {
    setWorking(r.id);
    let password: string | undefined;
    try {
      for (;;) {
        try {
          const doc = await api.recoveryRestore(r.id, password);
          adoptDoc(doc);
          toast("success", `Recovered ${r.name}`, "Save it to keep the recovered version.");
          break;
        } catch (e) {
          if (e instanceof AppError && (e.code === "password_required" || e.code === "wrong_password")) {
            const pw = await ask<string>({ kind: "password", fileName: r.name, retry: password !== undefined });
            if (pw == null) return;
            password = pw;
            continue;
          }
          toast("error", "Recovery failed", describe(e));
          return;
        }
      }
    } finally {
      setWorking(null);
      await refresh();
      if (!useStore.getState().recoverable.length && !useStore.getState().dialog) closeDialog();
    }
  };
  return (
    <Modal title="Recover unsaved work" icon={<ArchiveRestore size={18} />} width={600} onClose={() => closeDialog()} footer={<Button onClick={() => closeDialog()}>Close</Button>}>
      <p>OpenPDF Studio did not close normally last time. These documents had unsaved changes:</p>
      <div className="file-list">
        {items.map((r) => (
          <div className="file-row" key={r.id}>
            <ArchiveRestore size={18} color="var(--accent)" />
            <div className="meta">
              <div className="name">{r.name}</div>
              <div className="sub">
                Last change: {r.lastChange} · {formatDate(r.updatedAt)} · {formatBytes(r.size)}
                {r.encrypted ? " · password protected" : ""}
              </div>
            </div>
            <Button size="sm" variant="primary" disabled={!!working} onClick={() => void restore(r)} icon={working === r.id ? <Spinner size={12} /> : undefined}>
              Restore
            </Button>
            <Button
              size="sm"
              variant="danger"
              disabled={!!working}
              icon={<Trash size={14} />}
              onClick={async () => {
                await api.recoveryDiscard(r.id).catch((e) => toast("error", describe(e)));
                await refresh();
              }}
            >
              Discard
            </Button>
          </div>
        ))}
        {items.length === 0 && <div className="callout">Nothing left to recover.</div>}
      </div>
    </Modal>
  );
}

const SIG_FONTS = [
  { label: "Script", css: '"Segoe Script", "Snell Roundhand", "Brush Script MT", "URW Chancery L", cursive' },
  { label: "Handwriting", css: '"Ink Free", "Bradley Hand", "Comic Neue", "Comic Sans MS", cursive' },
  { label: "Formal", css: '"Lucida Calligraphy", "Apple Chancery", "Zapf Chancery", serif' },
];

/** Draw, type or upload a signature; placed as a movable image annotation. */
export function SignatureDialog() {
  const [mode, setMode] = useState<"draw" | "type" | "image">(savedSignature() ? "draw" : "draw");
  const [name, setName] = useState("");
  const [font, setFont] = useState(0);
  const [color, setColor] = useState("#1E3A8A");
  const [remember, setRemember] = useState(false);
  const [file, setFile] = useState<File | null>(null);
  const [hasInk, setHasInk] = useState(false);
  const canvas = useRef<HTMLCanvasElement>(null);
  const saved = savedSignature();

  useEffect(() => {
    const c = canvas.current;
    if (!c || mode !== "draw") return;
    const dpr = window.devicePixelRatio || 1;
    const r = c.getBoundingClientRect();
    c.width = r.width * dpr;
    c.height = r.height * dpr;
    const ctx = c.getContext("2d")!;
    ctx.scale(dpr, dpr);
    ctx.lineCap = "round";
    ctx.lineJoin = "round";
    let last: { x: number; y: number; t: number } | null = null;
    let width = 2.5;
    const pos = (e: PointerEvent) => {
      const b = c.getBoundingClientRect();
      return { x: e.clientX - b.left, y: e.clientY - b.top, t: performance.now() };
    };
    const down = (e: PointerEvent) => {
      c.setPointerCapture(e.pointerId);
      last = pos(e);
      width = 2.5;
    };
    const moveH = (e: PointerEvent) => {
      if (!last) return;
      const p = pos(e);
      const dist = Math.hypot(p.x - last.x, p.y - last.y);
      const speed = dist / Math.max(1, p.t - last.t);
      // Faster strokes are thinner, like ink.
      const target = Math.max(1.2, Math.min(3.6, 3.6 - speed * 1.4)) * (e.pressure > 0 && e.pointerType === "pen" ? e.pressure * 1.6 : 1);
      width = width * 0.7 + target * 0.3;
      ctx.strokeStyle = color;
      ctx.lineWidth = width;
      ctx.beginPath();
      ctx.moveTo(last.x, last.y);
      ctx.lineTo(p.x, p.y);
      ctx.stroke();
      last = p;
      setHasInk(true);
    };
    const up = () => (last = null);
    c.addEventListener("pointerdown", down);
    c.addEventListener("pointermove", moveH);
    c.addEventListener("pointerup", up);
    c.addEventListener("pointercancel", up);
    return () => {
      c.removeEventListener("pointerdown", down);
      c.removeEventListener("pointermove", moveH);
      c.removeEventListener("pointerup", up);
      c.removeEventListener("pointercancel", up);
    };
  }, [mode, color]);

  const clear = () => {
    const c = canvas.current;
    c?.getContext("2d")?.clearRect(0, 0, c.width, c.height);
    setHasInk(false);
  };

  /** Crop transparent margins so the placed image hugs the signature. */
  const trimmed = (src: HTMLCanvasElement): HTMLCanvasElement => {
    const ctx = src.getContext("2d")!;
    const { width: w, height: h } = src;
    const data = ctx.getImageData(0, 0, w, h).data;
    let x0 = w,
      y0 = h,
      x1 = 0,
      y1 = 0;
    for (let y = 0; y < h; y++)
      for (let x = 0; x < w; x++)
        if (data[(y * w + x) * 4 + 3]! > 8) {
          x0 = Math.min(x0, x);
          y0 = Math.min(y0, y);
          x1 = Math.max(x1, x);
          y1 = Math.max(y1, y);
        }
    if (x1 < x0) return src;
    const pad = 8;
    const out = document.createElement("canvas");
    out.width = x1 - x0 + 1 + pad * 2;
    out.height = y1 - y0 + 1 + pad * 2;
    out.getContext("2d")!.drawImage(src, x0, y0, x1 - x0 + 1, y1 - y0 + 1, pad, pad, x1 - x0 + 1, y1 - y0 + 1);
    return out;
  };

  const typedCanvas = (): HTMLCanvasElement => {
    const c = document.createElement("canvas");
    c.width = 1200;
    c.height = 300;
    const ctx = c.getContext("2d")!;
    ctx.fillStyle = color;
    ctx.font = `120px ${SIG_FONTS[font]!.css}`;
    ctx.textBaseline = "middle";
    ctx.fillText(name, 20, 150, 1160);
    return c;
  };

  const toFile = (c: HTMLCanvasElement) =>
    new Promise<File>((resolve, reject) =>
      c.toBlob((b) => (b ? resolve(new File([b], "signature.png", { type: "image/png" })) : reject(new Error("encode failed"))), "image/png"),
    );

  const place = async (f: File) => {
    if (remember && f.size < 400_000) {
      const dataUrl = await new Promise<string>((resolve) => {
        const r = new FileReader();
        r.onload = () => resolve(String(r.result));
        r.readAsDataURL(f);
      });
      void saveSettings({ savedSignature: dataUrl });
    }
    closeDialog();
    placeImage(f);
    toast("info", "Signature placed", "Drag to position it and use the corner handles to resize. It is saved as an image annotation.");
  };

  const ok = async () => {
    if (mode === "draw" && canvas.current) await place(await toFile(trimmed(canvas.current)));
    else if (mode === "type") await place(await toFile(trimmed(typedCanvas())));
    else if (mode === "image" && file) await place(file);
  };

  const useSaved = async () => {
    if (!saved) return;
    const blob = await (await fetch(saved)).blob();
    closeDialog();
    placeImage(new File([blob], "signature.png", { type: "image/png" }));
  };

  const ready = (mode === "draw" && hasInk) || (mode === "type" && name.trim()) || (mode === "image" && file);
  return (
    <Modal
      title="Add signature"
      icon={<PenLine size={18} />}
      width={600}
      onClose={() => closeDialog()}
      footer={
        <>
          <Checkbox checked={remember} onChange={setRemember} label="Remember on this device" />
          <span className="grow" />
          {saved && (
            <Button onClick={() => void useSaved()} variant="subtle">
              Use saved signature
            </Button>
          )}
          <Button onClick={() => closeDialog()}>Cancel</Button>
          <Button variant="primary" disabled={!ready} onClick={() => void ok()}>
            Place signature
          </Button>
        </>
      }
    >
      <Segmented
        label="Signature type"
        value={mode}
        onChange={setMode}
        options={[
          { value: "draw", label: "Draw" },
          { value: "type", label: "Type" },
          { value: "image", label: "Image" },
        ]}
      />
      {mode !== "image" && (
        <div className="swatches" role="radiogroup" aria-label="Ink color">
          {["#111827", "#1E3A8A", "#1D4ED8"].map((c) => (
            <button key={c} type="button" role="radio" aria-checked={color === c} aria-label={c} className={`color-swatch ${color === c ? "is-active" : ""}`} style={{ background: c }} onClick={() => setColor(c)} />
          ))}
        </div>
      )}
      {mode === "draw" && (
        <>
          <canvas ref={canvas} className="sig-pad" aria-label="Signature drawing area" />
          <div className="row">
            <span className="field-hint grow">Sign with your mouse, trackpad, touch screen or pen.</span>
            <Button size="sm" variant="subtle" onClick={clear}>
              Clear
            </Button>
          </div>
        </>
      )}
      {mode === "type" && (
        <>
          <Field label="Your name">
            <input className="input" value={name} onChange={(e) => setName(e.target.value)} data-autofocus />
          </Field>
          <div className="sig-typed" style={{ fontFamily: SIG_FONTS[font]!.css, color }}>
            {name || "Your name"}
          </div>
          <Segmented label="Style" value={String(font)} onChange={(v) => setFont(Number(v))} options={SIG_FONTS.map((f, i) => ({ value: String(i), label: f.label }))} />
        </>
      )}
      {mode === "image" && (
        <div className="stack">
          <label className="btn" style={{ alignSelf: "flex-start" }}>
            <Upload size={16} />
            <span>Choose image…</span>
            <input type="file" accept="image/png,image/jpeg,image/webp" hidden onChange={(e) => setFile(e.target.files?.[0] ?? null)} />
          </label>
          {file && <span className="field-hint">{file.name}</span>}
          <span className="field-hint">A PNG with a transparent background looks best.</span>
        </div>
      )}
      <div className="callout">
        This adds a visual signature (an image). Cryptographic digital signatures (certificate-based) are planned; see the roadmap.
      </div>
    </Modal>
  );
}
