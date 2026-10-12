import { useEffect, useState } from "react";
import { ArchiveRestore, Combine, FileImage, FileText, FolderOpen, Gauge, Lock, PenLine, Scissors, ShieldCheck, X } from "lucide-react";
import { api } from "../lib/ipc";
import type { RecentFile } from "../lib/types";
import { openFile, pickAndOpen } from "../state/actions";
import { openDialog, toast, useStore, type Dialog } from "../state/store";
import { formatBytes, relativeTime } from "../lib/format";
import { Button, IconButton } from "./ui";
import iconUrl from "../../app-icon.svg?url";
import { isMac } from "../lib/keys";

async function withPickedPdf(next: Dialog["kind"] | null) {
  const files = await api.pickOpen("pdf", false).catch(() => []);
  const f = files[0];
  if (!f) return;
  if ((await openFile(f)) && next) openDialog({ kind: next } as Dialog);
}

const TOOLS: { title: string; desc: string; icon: React.ReactNode; color: string; run: () => void }[] = [
  { title: "Open a PDF", desc: "View, search, comment and fill forms", icon: <FolderOpen size={20} />, color: "#2563EB", run: () => void pickAndOpen() },
  { title: "Merge files", desc: "Combine PDFs into one document", icon: <Combine size={20} />, color: "#7C3AED", run: () => openDialog({ kind: "merge" }) },
  { title: "Images to PDF", desc: "JPG, PNG, TIFF, WebP… to a PDF", icon: <FileImage size={20} />, color: "#0EA5E9", run: () => openDialog({ kind: "images" }) },
  { title: "Split a PDF", desc: "By ranges, every N pages or per page", icon: <Scissors size={20} />, color: "#F59E0B", run: () => void withPickedPdf("split") },
  { title: "Reduce file size", desc: "Lossless cleanup or image downsampling", icon: <Gauge size={20} />, color: "#10B981", run: () => void withPickedPdf("optimize") },
  { title: "Protect a PDF", desc: "AES-256 password and permissions", icon: <Lock size={20} />, color: "#EF4444", run: () => void withPickedPdf("protect") },
  { title: "Sign a document", desc: "Draw, type or upload your signature", icon: <PenLine size={20} />, color: "#EC4899", run: () => void withPickedPdf("signature") },
  { title: "Export as images", desc: "Save pages as PNG or JPEG", icon: <FileText size={20} />, color: "#64748B", run: () => void withPickedPdf("exportImages") },
];

export function HomeScreen() {
  const [recent, setRecent] = useState<RecentFile[] | null>(null);
  const recoverable = useStore((s) => s.recoverable);
  const tabs = useStore((s) => s.tabs.length);

  const load = () =>
    api
      .recentFiles()
      .then(setRecent)
      .catch(() => setRecent([]));
  useEffect(() => {
    void load();
  }, [tabs]);

  return (
    <div className="home">
      <div className="home-inner">
        {recoverable.length > 0 && (
          <div className="banner info" style={{ borderRadius: 12, border: "1px solid var(--border)" }}>
            <ArchiveRestore size={18} />
            <span className="grow">
              {recoverable.length === 1 ? "A document" : `${recoverable.length} documents`} with unsaved changes can be recovered from the last session.
            </span>
            <Button size="sm" variant="primary" onClick={() => openDialog({ kind: "recovery" })}>
              Review
            </Button>
          </div>
        )}
        <section className="hero">
          <img src={iconUrl} alt="" />
          <div className="hero-text">
            <h1>OpenPDF Studio</h1>
            <p>Free, open-source PDF tools that work entirely on your computer.</p>
            <div className="actions">
              <Button variant="primary" icon={<FolderOpen size={16} />} onClick={() => void pickAndOpen()}>
                Open file…
              </Button>
              <Button icon={<Combine size={16} />} onClick={() => openDialog({ kind: "merge" })}>
                Merge files
              </Button>
            </div>
          </div>
          <div className="dropzone" aria-hidden>
            Drop PDFs or images anywhere
            <br />
            <span style={{ fontSize: 12, color: "var(--text-3)" }}>or press {isMac ? "⌘" : "Ctrl+"}O</span>
          </div>
        </section>

        <section>
          <h2 className="section-title">Tools</h2>
          <div className="tool-grid">
            {TOOLS.map((t) => (
              <button key={t.title} type="button" className="tool-card" onClick={t.run}>
                <span className="tool-card-icon" style={{ background: t.color }}>
                  {t.icon}
                </span>
                <span>
                  <div className="tool-card-title">{t.title}</div>
                  <div className="tool-card-desc">{t.desc}</div>
                </span>
              </button>
            ))}
          </div>
        </section>

        <section>
          <h2 className="section-title">
            <span>Recent</span>
            {recent && recent.length > 0 && (
              <Button
                size="sm"
                variant="subtle"
                onClick={async () => {
                  await api.forgetRecent(null);
                  void load();
                }}
              >
                Clear list
              </Button>
            )}
          </h2>
          {recent && recent.length === 0 && <div className="field-hint">Documents you open will appear here.</div>}
          {recent && recent.length > 0 && (
            <div className="recent-list">
              {recent.map((r) => (
                <div key={r.file.id} className="recent-row" role="button" tabIndex={0} onClick={() => void openFile(r.file)} onKeyDown={(e) => e.key === "Enter" && void openFile(r.file)}>
                  <FileText size={20} color="var(--accent)" />
                  <div className="meta">
                    <div className="name">{r.file.name}</div>
                    <div className="path">{r.file.path}</div>
                  </div>
                  <span className="when">
                    {formatBytes(r.file.size)} · {relativeTime(r.openedAt)}
                  </span>
                  <IconButton
                    label={`Remove ${r.file.name} from the list`}
                    onClick={async (e) => {
                      e.stopPropagation();
                      await api.forgetRecent(r.file.id).catch((err) => toast("error", String(err)));
                      void load();
                    }}
                  >
                    <X size={14} />
                  </IconButton>
                </div>
              ))}
            </div>
          )}
        </section>
        <div className="privacy-note">
          <ShieldCheck size={16} />
          Your files never leave this device. No account, no ads, no telemetry. Licensed under MPL-2.0.
        </div>
      </div>
    </div>
  );
}
