import { useEffect, useMemo, useState } from "react";
import { Search, ShieldAlert, XOctagon } from "lucide-react";
import { api, type PriorityClass, type ProcessInfo } from "../api";
import { Badge, ErrorBox, Loading, useConfirm, useToast } from "../components/ui";
import { bytes, duration, pct, rate } from "../lib/format";

type SortKey = "cpuPercent" | "memoryBytes" | "diskIo" | "name" | "pid";

export function Processes() {
  const [procs, setProcs] = useState<ProcessInfo[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [q, setQ] = useState("");
  const [sort, setSort] = useState<{ key: SortKey; desc: boolean }>({ key: "cpuPercent", desc: true });
  const [paused, setPaused] = useState(false);
  const [sel, setSel] = useState<ProcessInfo | null>(null);
  const confirm = useConfirm();
  const toast = useToast();

  useEffect(() => {
    let alive = true;
    const load = async () => {
      if (paused || document.visibilityState === "hidden") return;
      try {
        const p = await api.processes();
        if (alive) { setProcs(p); setError(null); }
      } catch (e) {
        if (alive) setError(e instanceof Error ? e.message : String(e));
      }
    };
    void load();
    const id = setInterval(load, 2000);
    return () => { alive = false; clearInterval(id); };
  }, [paused]);

  const rows = useMemo(() => {
    const t = q.trim().toLowerCase();
    const list = (procs ?? []).filter((p) => !t || p.name.toLowerCase().includes(t) || String(p.pid) === t || (p.exe ?? "").toLowerCase().includes(t));
    const val = (p: ProcessInfo): number | string => sort.key === "diskIo" ? p.diskReadBytesPerSec + p.diskWriteBytesPerSec : sort.key === "name" ? p.name.toLowerCase() : p[sort.key];
    return [...list].sort((a, b) => {
      const va = val(a), vb = val(b);
      const c = va < vb ? -1 : va > vb ? 1 : 0;
      return sort.desc ? -c : c;
    });
  }, [procs, q, sort]);

  const th = (key: SortKey, label: string, num = false) => (
    <th className={`sortable ${num ? "num" : ""}`} aria-sort={sort.key === key ? (sort.desc ? "descending" : "ascending") : "none"}>
      <button className="btn subtle sm" style={{ padding: 0, height: "auto", fontWeight: 600 }} onClick={() => setSort((s) => ({ key, desc: s.key === key ? !s.desc : key !== "name" }))}>
        {label}{sort.key === key ? (sort.desc ? " ↓" : " ↑") : ""}
      </button>
    </th>
  );

  const terminate = async (p: ProcessInfo) => {
    const ok = await confirm({
      title: `End ${p.name}?`,
      danger: true,
      confirmLabel: "End process",
      body: <>Unsaved work in this app will be lost, and any app that depends on it may stop working. This cannot be undone. PID {p.pid}{p.exe ? <><br /><span className="mono break">{p.exe}</span></> : null}</>,
    });
    if (!ok) return;
    try {
      toast("good", await api.terminate(p.pid));
      setSel(null);
    } catch (e) {
      toast("bad", e instanceof Error ? e.message : String(e));
    }
  };

  const priority = async (p: ProcessInfo, pr: PriorityClass) => {
    try {
      const r = await api.setPriority(p.pid, pr);
      toast(r.status === "applied" ? "good" : "bad", r.status === "applied" ? `${p.name}: priority set to ${pr}. Undo from History; it also resets when the app restarts.` : r.summary);
    } catch (e) {
      toast("bad", e instanceof Error ? e.message : String(e));
    }
  };

  return (
    <div className="page">
      <header className="page-head">
        <div>
          <h1>Process Explorer</h1>
          <p>Every running process, refreshed every 2 seconds. CPU % is per core (100% = one full core). Protected Windows and security processes can't be ended or re-prioritized here.</p>
        </div>
        <div className="actions-row">
          <button className="btn" aria-pressed={paused} onClick={() => setPaused(!paused)}>{paused ? "Resume" : "Pause"}</button>
        </div>
      </header>

      <div className="row" style={{ marginBottom: 12 }}>
        <Search size={16} className="muted" />
        <input className="input" style={{ width: 320 }} placeholder="Filter by name, PID or path" value={q} onChange={(e) => setQ(e.target.value)} aria-label="Filter processes" />
        <span className="spacer" />
        <span className="faint">{procs?.length ?? 0} processes</span>
      </div>

      {sel && (
        <div className="card" style={{ marginBottom: 12 }}>
          <div className="row" style={{ flexWrap: "wrap" }}>
            <strong>{sel.name}</strong><span className="faint">PID {sel.pid}{sel.parentPid ? ` · parent ${sel.parentPid}` : ""} · running {duration(sel.runTimeSecs)}</span>
            {sel.critical && <Badge tone="accent"><ShieldAlert size={12} />Protected</Badge>}
            <span className="spacer" />
            {!sel.critical && (
              <>
                <label className="faint" htmlFor="prio">Priority</label>
                <select id="prio" className="input" defaultValue="" onChange={(e) => e.target.value && void priority(sel, e.target.value as PriorityClass)}>
                  <option value="" disabled>Change…</option>
                  <option value="idle">Low</option>
                  <option value="belowNormal">Below normal</option>
                  <option value="normal">Normal</option>
                  <option value="aboveNormal">Above normal</option>
                  <option value="high">High</option>
                </select>
                <button className="btn danger" onClick={() => void terminate(sel)}><XOctagon size={16} />End process</button>
              </>
            )}
            <button className="btn subtle" onClick={() => setSel(null)}>Close</button>
          </div>
          {sel.exe && <div className="mono break faint" style={{ marginTop: 6 }}>{sel.exe}</div>}
          {!sel.critical && <div className="faint" style={{ marginTop: 6 }}>Raising priority can make other apps less responsive. Realtime priority is never offered.</div>}
        </div>
      )}

      {error && <ErrorBox error={error} />}
      {!procs ? <Loading label="Sampling processes…" /> : (
        <div className="table-wrap" style={{ maxHeight: "calc(100vh - 260px)" }}>
          <table className="data">
            <thead><tr>{th("name", "Name")}{th("pid", "PID", true)}{th("cpuPercent", "CPU", true)}{th("memoryBytes", "Memory", true)}{th("diskIo", "Disk I/O", true)}<th>Status</th></tr></thead>
            <tbody>
              {rows.map((p) => (
                <tr key={p.pid} onClick={() => setSel(p)} style={{ cursor: "default" }} aria-selected={sel?.pid === p.pid}>
                  <td><span className="row" style={{ gap: 6 }}>{p.name}{p.critical && <ShieldAlert size={12} color="var(--text-3)" aria-label="Protected" />}</span></td>
                  <td className="num">{p.pid}</td>
                  <td className="num">{pct(p.cpuPercent, 1)}</td>
                  <td className="num">{bytes(p.memoryBytes)}</td>
                  <td className="num">{rate(p.diskReadBytesPerSec + p.diskWriteBytesPerSec)}</td>
                  <td className="faint">{p.status}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}
