import { useState } from "react";
import { Copy, FolderSearch, HardDrive, Trash2 } from "lucide-react";
import { api, type DuplicateGroup, type LargeFileScan, type PlanReport } from "../api";
import { Badge, Button, Card, ErrorBox, Loading, Meter, Notice, PlanReportView, Segmented, useAsync, useConfirm, useToast } from "../components/ui";
import { bytes } from "../lib/format";

type Tab = "drives" | "temp" | "space" | "dupes";

function Drives() {
  const snap = useAsync(() => api.snapshot(false));
  if (snap.error) return <ErrorBox error={snap.error} />;
  if (!snap.data) return <Loading label="Reading drives…" />;
  return (
    <div className="grid g2">
      {snap.data.disks.filter((d) => d.totalBytes > 0).map((d) => {
        const used = d.totalBytes - d.availableBytes;
        return (
          <Card key={d.mountPoint} title={<>{d.mountPoint} {d.isSystem && <Badge tone="accent">System</Badge>}</>} icon={<HardDrive size={18} />}>
            <div className="row"><span className="metric-value" style={{ fontSize: 22 }}>{bytes(d.availableBytes)}</span><span className="muted">free of {bytes(d.totalBytes)}</span></div>
            <Meter value={used} max={d.totalBytes} warnAt={85} badAt={92} label={`${d.mountPoint} usage`} />
            <div className="faint" style={{ marginTop: 8 }}>{d.kind === "unknown" ? "Drive type not reported" : d.kind.toUpperCase()} · {d.fileSystem}{d.removable ? " · Removable" : ""}</div>
          </Card>
        );
      })}
      <Notice>SSD health (SMART / wear level) requires vendor or administrator-level access and is not shown in this version. Windows' own Optimize Drives (TRIM/defrag) runs on a schedule; APEX does not replace it.</Notice>
    </div>
  );
}

function Temp() {
  const [days, setDays] = useState(7);
  const a = useAsync(() => api.tempAnalysis(days), [days]);
  const [report, setReport] = useState<PlanReport | null>(null);
  const confirm = useConfirm();
  const toast = useToast();
  const clean = async () => {
    if (!a.data) return;
    const ok = await confirm({
      title: `Delete ${bytes(a.data.eligibleBytes)} of temporary files?`,
      danger: true,
      confirmLabel: "Delete",
      body: <>This deletes {a.data.eligibleFiles.toLocaleString()} file(s) in your user Temp folder that have not changed for {days}+ days. Deleted files <strong>cannot be restored</strong>. Files in use are skipped. Nothing outside the Temp folder is touched, and links out of it are never followed.</>,
    });
    if (!ok) return;
    try {
      const r = await api.cleanTemp(days);
      setReport(r);
      toast(r.status === "applied" ? "good" : "bad", r.summary);
      void a.reload();
    } catch (e) {
      toast("bad", e instanceof Error ? e.message : String(e));
    }
  };
  return (
    <Card title="Temporary files" icon={<Trash2 size={18} />} sub="Only your user Temp folder. Windows' own temp folder and update caches need administrator rights and are left to Storage Sense.">
      <div className="row" style={{ marginBottom: 12 }}>
        <label htmlFor="days">Older than</label>
        <select id="days" className="input" value={days} onChange={(e) => setDays(Number(e.target.value))}>
          {[1, 3, 7, 14, 30, 90].map((d) => <option key={d} value={d}>{d} day{d > 1 ? "s" : ""}</option>)}
        </select>
        <span className="spacer" />
        <Button kind="danger" disabled={!a.data || a.data.eligibleFiles === 0} onClick={clean}><Trash2 size={16} />Delete eligible files</Button>
      </div>
      {a.loading && !a.data ? <Loading label="Scanning Temp…" /> : a.error ? <ErrorBox error={a.error} /> : a.data && (
        <dl className="kv">
          <dt>Folder</dt><dd className="mono break">{a.data.roots.join(", ")}</dd>
          <dt>Total</dt><dd>{a.data.totalFiles.toLocaleString()} files · {bytes(a.data.totalBytes)}</dd>
          <dt>Eligible</dt><dd><strong>{a.data.eligibleFiles.toLocaleString()} files · {bytes(a.data.eligibleBytes)}</strong></dd>
          <dt>Skipped</dt><dd>{a.data.skippedSymlinks} links (never followed), {a.data.unreadableEntries} unreadable</dd>
        </dl>
      )}
      {report && <div style={{ marginTop: 12 }}><PlanReportView report={report} /></div>}
    </Card>
  );
}

function FolderPicker({ onPick, busy }: { onPick: (p: string) => void; busy: boolean }) {
  const folders = useAsync(() => api.knownFolders());
  const [path, setPath] = useState("");
  const quick = folders.data ? [["Downloads", folders.data.downloads], ["Documents", folders.data.documents], ["Desktop", folders.data.desktop], ["Home", folders.data.home], ["System drive", folders.data.systemDrive]] as const : [];
  return (
    <div className="stack" style={{ gap: 8, marginBottom: 12 }}>
      <form className="row" onSubmit={(e) => { e.preventDefault(); if (path.trim()) onPick(path.trim()); }}>
        <input className="input" style={{ flex: 1 }} placeholder="Folder path, e.g. C:\Users\you\Videos" value={path} onChange={(e) => setPath(e.target.value)} aria-label="Folder to scan" />
        <Button type="submit" kind="primary" busy={busy} disabled={!path.trim()}><FolderSearch size={16} />Scan</Button>
      </form>
      <div className="row" style={{ flexWrap: "wrap", gap: 6 }}>
        {quick.filter(([, p]) => p).map(([label, p]) => <Button key={label} small onClick={() => { setPath(p!); onPick(p!); }}>{label}</Button>)}
      </div>
    </div>
  );
}

function Space() {
  const [scan, setScan] = useState<LargeFileScan | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const toast = useToast();
  const run = async (p: string) => {
    setBusy(true); setError(null);
    try { setScan(await api.largeFiles(p)); } catch (e) { setError(e instanceof Error ? e.message : String(e)); } finally { setBusy(false); }
  };
  const top = scan?.byChild.slice(0, 12) ?? [];
  const max = top[0]?.[1] ?? 1;
  const now = Date.now() / 1000;
  return (
    <Card title="Space explorer" icon={<FolderSearch size={18} />} sub="Read-only. APEX shows where space goes; you decide what to delete in Explorer.">
      <FolderPicker onPick={run} busy={busy} />
      {busy && <div className="progress-indeterminate" />}
      {error && <ErrorBox error={error} />}
      {scan && (
        <div className="stack">
          <div className="faint">{scan.filesScanned.toLocaleString()} files · {bytes(scan.bytesScanned)}{scan.truncated ? " · stopped early (very large folder)" : ""}{scan.unreadableEntries ? ` · ${scan.unreadableEntries} unreadable` : ""}</div>
          <div>
            <h3 style={{ marginBottom: 6 }}>By folder</h3>
            {top.map(([name, size]) => (
              <div key={name} className="bar-row"><span className="ellipsis" title={name}>{name}</span><Meter value={size} max={max} warnAt={101} badAt={101} label={name} /><span className="faint" style={{ textAlign: "right" }}>{bytes(size)}</span></div>
            ))}
          </div>
          <div className="table-wrap" style={{ maxHeight: 420 }}>
            <table className="data">
              <thead><tr><th>Largest files</th><th className="num">Size</th><th className="num">Last changed</th><th /></tr></thead>
              <tbody>
                {scan.largest.map((f) => (
                  <tr key={f.path}>
                    <td className="mono break">{f.path}</td>
                    <td className="num">{bytes(f.sizeBytes)}</td>
                    <td className="num faint">{f.modifiedUnix ? `${Math.max(0, Math.round((now - f.modifiedUnix) / 86400))} days ago` : "—"}</td>
                    <td><button className="btn sm" onClick={() => api.reveal(f.path).catch((e) => toast("bad", String(e instanceof Error ? e.message : e)))}>Show in Explorer</button></td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      )}
    </Card>
  );
}

function Dupes() {
  const [groups, setGroups] = useState<DuplicateGroup[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [minMb, setMinMb] = useState(1);
  const [error, setError] = useState<string | null>(null);
  const toast = useToast();
  const run = async (p: string) => {
    setBusy(true); setError(null);
    try { setGroups(await api.duplicates(p, minMb)); } catch (e) { setError(e instanceof Error ? e.message : String(e)); } finally { setBusy(false); }
  };
  const total = (groups ?? []).reduce((a, g) => a + g.reclaimableBytes, 0);
  return (
    <Card title="Duplicate files" icon={<Copy size={18} />} sub="Files are only reported as duplicates when every byte matches. Report only — nothing is deleted.">
      <div className="row" style={{ marginBottom: 8 }}>
        <label htmlFor="min">Minimum size</label>
        <select id="min" className="input" value={minMb} onChange={(e) => setMinMb(Number(e.target.value))}>{[1, 10, 50, 100].map((m) => <option key={m} value={m}>{m} MB</option>)}</select>
      </div>
      <FolderPicker onPick={run} busy={busy} />
      {busy && <div className="progress-indeterminate" />}
      {error && <ErrorBox error={error} />}
      {groups && (groups.length === 0 ? <Notice tone="good">No duplicates found.</Notice> : (
        <div className="stack" style={{ gap: 10 }}>
          <div className="muted">{groups.length} sets · up to {bytes(total)} reclaimable by keeping one copy of each</div>
          {groups.slice(0, 100).map((g, i) => (
            <div key={i} className="card" style={{ padding: 12 }}>
              <div className="row"><strong>{bytes(g.sizeBytes)} × {g.paths.length}</strong><span className="spacer" /><span className="faint">{bytes(g.reclaimableBytes)} reclaimable</span></div>
              {g.paths.map((p) => <div key={p} className="row"><span className="mono break" style={{ flex: 1 }}>{p}</span><button className="btn sm" onClick={() => api.reveal(p).catch((e) => toast("bad", String(e instanceof Error ? e.message : e)))}>Show</button></div>)}
            </div>
          ))}
        </div>
      ))}
    </Card>
  );
}

export function Storage() {
  const [tab, setTab] = useState<Tab>("drives");
  return (
    <div className="page">
      <header className="page-head">
        <div>
          <h1>Storage Cleaner</h1>
          <p>Only temporary files are ever deleted, and only after you confirm. Personal files are never deleted automatically.</p>
        </div>
      </header>
      <div style={{ marginBottom: 16 }}>
        <Segmented label="Storage tools" value={tab} onChange={setTab} options={[{ value: "drives", label: "Drives" }, { value: "temp", label: "Temporary files" }, { value: "space", label: "Space explorer" }, { value: "dupes", label: "Duplicates" }]} />
      </div>
      {tab === "drives" && <Drives />}
      {tab === "temp" && <Temp />}
      {tab === "space" && <Space />}
      {tab === "dupes" && <Dupes />}
    </div>
  );
}
