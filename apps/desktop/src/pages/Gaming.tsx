import { useState } from "react";
import { Gamepad2, Monitor, Play, Plus, Save, Square, Trash2, Zap } from "lucide-react";
import { api, type GameProfile, type PlanReport } from "../api";
import { Badge, Button, Card, ErrorBox, Loading, Notice, PlanReportView, useAsync, useToast } from "../components/ui";
import { bytes, pct } from "../lib/format";

function newId() {
  return `p-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 7)}`;
}

export function Gaming() {
  const status = useAsync(() => api.gaming());
  const profiles = useAsync(() => api.gameProfiles());
  const procs = useAsync(() => api.processes());
  const [draft, setDraft] = useState<GameProfile[] | null>(null);
  const [session, setSession] = useState<{ profile: string; report: PlanReport } | null>(null);
  const toast = useToast();

  const list = draft ?? profiles.data ?? [];
  const power = status.data?.power;
  const edit = (id: string, patch: Partial<GameProfile>) => setDraft(list.map((p) => (p.id === id ? { ...p, ...patch } : p)));

  const save = async () => {
    try {
      await api.saveGameProfiles(list);
      setDraft(null);
      await profiles.reload();
      toast("good", "Profiles saved.");
    } catch (e) {
      toast("bad", String(e instanceof Error ? e.message : e));
    }
  };

  const start = async (p: GameProfile) => {
    try {
      const report = await api.startSession(p.id);
      setSession({ profile: p.name, report });
      toast(report.status === "applied" ? "good" : "bad", report.summary);
      void status.reload();
    } catch (e) {
      toast("bad", String(e instanceof Error ? e.message : e));
    }
  };

  const end = async () => {
    if (!session) return;
    try {
      const r = await api.undoBatch(session.report.batchId);
      const failed = r.filter((x) => !x.ok);
      toast(failed.length ? "bad" : "good", failed.length ? failed.map((f) => f.message).join("; ") : "Session ended — settings restored.");
      setSession(null);
      void status.reload();
    } catch (e) {
      toast("bad", String(e instanceof Error ? e.message : e));
    }
  };

  const background = (procs.data ?? []).filter((p) => !p.critical).slice(0, 8);

  return (
    <div className="page">
      <header className="page-head">
        <div>
          <h1>Gaming Center</h1>
          <p>Check game-relevant settings, find background load, and run reversible game sessions.</p>
        </div>
        <Button onClick={() => { void status.reload(); void procs.reload(); }}>Refresh</Button>
      </header>

      {status.data?.limitations.map((l) => <Notice key={l}>{l}</Notice>)}

      <div className="grid g3 mt">
        <Card title="Game Mode" icon={<Gamepad2 size={18} />}>
          {status.loading && !status.data ? <Loading label="Checking…" /> : status.error ? <ErrorBox error={status.error} /> : (
            <>
              <div className="metric-value">{status.data?.gameModeEnabled == null ? "Unknown" : status.data.gameModeEnabled ? "On" : "Off"}</div>
              <p className="muted" style={{ fontSize: 13 }}>Windows default is on. Game profiles can turn it on for a session; the Smart Optimizer (Gaming mode) suggests it when it's off.</p>
            </>
          )}
        </Card>
        <Card title="GPU scheduling" icon={<Zap size={18} />}>
          <div className="metric-value">{status.data?.hardwareGpuScheduling == null ? "Not reported" : status.data.hardwareGpuScheduling ? "On" : "Off"}</div>
          <p className="muted" style={{ fontSize: 13 }}>Hardware-accelerated GPU scheduling. Shown for information; effects vary by game and driver, so APEX doesn't change it. Settings › Display › Graphics.</p>
        </Card>
        <Card title="Power plan" icon={<Zap size={18} />}>
          <div className="metric-value" style={{ fontSize: 20 }}>{power?.active.name ?? "—"}</div>
          <p className="muted" style={{ fontSize: 13 }}>{power?.onBattery ? "Running on battery — performance plans will drain it faster." : `${power?.available.length ?? 0} plan(s) installed.`}</p>
        </Card>
      </div>

      <Card title="Displays" icon={<Monitor size={18} />} className="mt" sub="Refresh rate in use vs. the highest your display driver offers at this resolution. To change it: Settings › System › Display › Advanced display.">
        {(status.data?.displays ?? []).length === 0 ? <div className="muted">No display information available.</div> : (
          <div className="table-wrap">
            <table className="data">
              <thead><tr><th>Display</th><th>Adapter</th><th>Resolution</th><th className="num">Current</th><th className="num">Max available</th><th /></tr></thead>
              <tbody>
                {status.data!.displays.map((d) => (
                  <tr key={d.device}>
                    <td>{d.device}{d.primary && <> <Badge>Primary</Badge></>}</td>
                    <td className="ellipsis" style={{ maxWidth: 260 }}>{d.adapter}</td>
                    <td>{d.width}×{d.height}</td>
                    <td className="num">{d.currentHz ? `${d.currentHz} Hz` : "Default"}</td>
                    <td className="num">{d.maxHzAtCurrentResolution ? `${d.maxHzAtCurrentResolution} Hz` : "—"}</td>
                    <td>{d.maxHzAtCurrentResolution > d.currentHz && d.currentHz > 0 ? <span title="Change in Settings › System › Display › Advanced display › Choose a refresh rate"><Badge tone="warn">Below maximum</Badge></span> : <Badge tone="good">OK</Badge>}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </Card>

      <Card title="Background load right now" className="mt" sub="Non-system processes using the most CPU. Close what you don't need before playing; APEX never closes apps automatically.">
        {procs.error ? <ErrorBox error={procs.error} /> : (
          <div className="table-wrap">
            <table className="data">
              <thead><tr><th>Process</th><th className="num">CPU</th><th className="num">Memory</th></tr></thead>
              <tbody>{background.map((p) => <tr key={p.pid}><td>{p.name}</td><td className="num">{pct(p.cpuPercent, 1)}</td><td className="num">{bytes(p.memoryBytes)}</td></tr>)}</tbody>
            </table>
          </div>
        )}
        <div className="faint" style={{ marginTop: 6 }}>CPU % is relative to one core (100% = one full core), as in Windows Resource Monitor.</div>
      </Card>

      <Card
        title="Game profiles"
        className="mt"
        sub="A profile applies a power plan and/or Game Mode as one reversible batch. End the session to restore your previous settings exactly."
        actions={
          <div className="actions-row">
            <Button small onClick={() => setDraft([...list, { id: newId(), name: "New game", powerSchemeGuid: null, enableGameMode: true, notes: "" }])}><Plus size={14} />Add</Button>
            <Button small kind="primary" disabled={!draft} onClick={save}><Save size={14} />Save</Button>
          </div>
        }
      >
        {session && (
          <div className="stack" style={{ marginBottom: 12 }}>
            <Notice tone="good">Session running for “{session.profile}”. You can also end it later from Optimization History.</Notice>
            <PlanReportView report={session.report} />
            <div><Button kind="primary" onClick={end}><Square size={14} />End session & restore</Button></div>
          </div>
        )}
        {profiles.error && <ErrorBox error={profiles.error} />}
        {list.length === 0 ? <div className="muted">No profiles yet.</div> : (
          <div className="table-wrap">
            <table className="data">
              <thead><tr><th>Name</th><th>Power plan during session</th><th>Game Mode</th><th>Notes</th><th /></tr></thead>
              <tbody>
                {list.map((p) => (
                  <tr key={p.id}>
                    <td><input className="input" value={p.name} maxLength={60} onChange={(e) => edit(p.id, { name: e.target.value })} aria-label="Profile name" /></td>
                    <td>
                      <select className="input" value={p.powerSchemeGuid ?? ""} onChange={(e) => edit(p.id, { powerSchemeGuid: e.target.value || null })} aria-label="Power plan">
                        <option value="">Leave unchanged</option>
                        {(power?.available ?? []).map((s) => <option key={s.guid} value={s.guid}>{s.name}</option>)}
                      </select>
                    </td>
                    <td><input type="checkbox" className="switch" checked={p.enableGameMode} onChange={(e) => edit(p.id, { enableGameMode: e.target.checked })} aria-label="Turn on Game Mode" /></td>
                    <td><input className="input" value={p.notes} maxLength={120} onChange={(e) => edit(p.id, { notes: e.target.value })} aria-label="Notes" /></td>
                    <td>
                      <div className="actions-row">
                        <Button small disabled={!!draft || !!session} title={draft ? "Save first" : undefined} onClick={() => start(p)}><Play size={14} />Start</Button>
                        <Button small kind="subtle" onClick={() => setDraft(list.filter((x) => x.id !== p.id))} title="Delete profile"><Trash2 size={14} /></Button>
                      </div>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </Card>
    </div>
  );
}
