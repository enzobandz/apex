import { useEffect, useMemo, useState } from "react";
import { RefreshCw, ShieldCheck, Sparkles } from "lucide-react";
import { api, type Mode, type PlanReport, type Recommendation } from "../api";
import { Badge, Button, Card, ErrorBox, Loading, Notice, PlanReportView, Segmented, useAsync, useConfirm, useToast } from "../components/ui";
import { categoryLabel, describeAction, riskLabel } from "../lib/format";
import type { PageId } from "../App";

const MODES: { value: Mode; label: string }[] = [
  { value: "balanced", label: "Balanced" },
  { value: "gaming", label: "Gaming" },
  { value: "maximumPerformance", label: "Maximum performance" },
];

const MODE_TEXT: Record<Mode, string> = {
  balanced: "Conservative, reversible changes that keep everyday usability and battery life.",
  gaming: "Adds game-related settings and power options. Nothing is pre-selected that costs battery or adds heat.",
  maximumPerformance: "Also shows experimental options with thermal, power and noise trade-offs. Each needs explicit confirmation.",
};

function RecCard({ r, selected, onToggle }: { r: Recommendation; selected: boolean; onToggle: () => void }) {
  const [open, setOpen] = useState(false);
  const toneFor = r.category === "verifiedProblem" ? "bad" : r.category === "likelyBottleneck" ? "warn" : r.category === "experimental" ? "accent" : undefined;
  const blocked = r.requiresAdmin;
  return (
    <div className={`rec ${selected ? "selected" : ""}`}>
      <div style={{ paddingTop: 2 }}>
        {r.action ? (
          <input type="checkbox" className="check" checked={selected} disabled={blocked} onChange={onToggle} aria-label={`Select: ${r.title}`} />
        ) : (
          <span className="faint" title="Advice only — APEX changes nothing for this item" style={{ display: "inline-block", width: 18 }}>ⓘ</span>
        )}
      </div>
      <div style={{ minWidth: 0 }}>
        <div className="row" style={{ flexWrap: "wrap", gap: 6 }}>
          <span className="rec-title">{r.title}</span>
          <Badge tone={toneFor}>{categoryLabel[r.category]}</Badge>
          <Badge tone={r.risk === "low" ? "good" : r.risk === "medium" ? "warn" : "bad"}>{riskLabel[r.risk]}</Badge>
          {!r.reversible && <Badge tone="warn">Not reversible</Badge>}
          {r.requiresAdmin && <Badge>Needs administrator</Badge>}
          {r.requiresConfirmation && <Badge tone="accent">Confirmation required</Badge>}
        </div>
        <div className="rec-body">{r.expectedBenefit}</div>
        {open && (
          <dl>
            <dt>Evidence</dt><dd>{r.evidence.map((e, i) => <div key={i}>{e}</div>)}</dd>
            <dt>Downsides</dt><dd>{r.downsides}</dd>
            <dt>Change</dt><dd>{r.action ? describeAction(r.action) : "None — advice only"}</dd>
            <dt>Rollback</dt><dd>{r.rollback}</dd>
            {r.requiresAdmin && <><dt>Note</dt><dd>This entry is machine-wide. This version of APEX does not elevate; use Task Manager › Startup apps as administrator.</dd></>}
          </dl>
        )}
        <button className="btn subtle sm" style={{ marginTop: 6, marginLeft: -10 }} onClick={() => setOpen(!open)} aria-expanded={open}>
          {open ? "Hide details" : "Evidence, downsides & rollback"}
        </button>
      </div>
      <div className="confidence" title="How confident APEX is that this helps this PC">
        <div style={{ fontWeight: 600, color: "var(--text)" }}>{r.confidence}%</div>
        confidence
      </div>
    </div>
  );
}

export function Optimizer({ go, initialMode }: { go: (p: PageId) => void; initialMode: Mode }) {
  const [mode, setMode] = useState<Mode>(initialMode);
  const [refresh, setRefresh] = useState(0);
  const recs = useAsync(() => api.recommendations(mode, refresh > 0), [mode, refresh]);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [report, setReport] = useState<PlanReport | null>(null);
  const confirm = useConfirm();
  const toast = useToast();

  useEffect(() => {
    setSelected(new Set((recs.data ?? []).filter((r) => r.action && r.selectedByDefault && !r.requiresAdmin).map((r) => r.id)));
  }, [recs.data]);

  const chosen = useMemo(() => (recs.data ?? []).filter((r) => selected.has(r.id)), [recs.data, selected]);
  const toggle = (id: string) => setSelected((s) => { const n = new Set(s); if (n.has(id)) n.delete(id); else n.add(id); return n; });

  const apply = async () => {
    const needsConfirm = chosen.filter((r) => r.requiresConfirmation || !r.reversible);
    const ok = await confirm({
      title: `Apply ${chosen.length} change${chosen.length === 1 ? "" : "s"}?`,
      confirmLabel: "Apply",
      body: (
        <div className="stack" style={{ gap: 10 }}>
          <ul style={{ margin: 0, paddingLeft: 18 }}>{chosen.map((r) => <li key={r.id}>{r.action ? describeAction(r.action) : r.title}</li>)}</ul>
          <div>Each change is verified after it is made. If any step fails, the changes already made in this batch are restored automatically.</div>
          {needsConfirm.length > 0 && <Notice tone="warn">{needsConfirm.map((r) => r.title).join("; ")} — {needsConfirm.some((r) => !r.reversible) ? "includes a step that cannot be undone." : "has trade-offs described in its details."}</Notice>}
          <div className="faint">Tip: run a Benchmark Lab baseline first to measure the effect.</div>
        </div>
      ),
    });
    if (!ok) return;
    try {
      const rep = await api.applyPlan(mode, chosen.map((r) => r.id), needsConfirm.map((r) => r.id));
      setReport(rep);
      toast(rep.status === "applied" ? "good" : "bad", rep.summary);
      setRefresh((n) => n + 1);
    } catch (e) {
      toast("bad", e instanceof Error ? e.message : String(e));
    }
  };

  const groups: { title: string; items: Recommendation[] }[] = [
    { title: "Can be applied", items: (recs.data ?? []).filter((r) => r.action) },
    { title: "Advice (APEX won't change anything)", items: (recs.data ?? []).filter((r) => !r.action) },
  ];

  return (
    <div className="page">
      <header className="page-head">
        <div>
          <h1>Smart Optimizer</h1>
          <p>{MODE_TEXT[mode]}</p>
        </div>
        <div className="actions-row">
          <Button onClick={() => setRefresh((n) => n + 1)}><RefreshCw size={16} />Re-analyze</Button>
          <Button kind="primary" disabled={chosen.length === 0} onClick={apply}><Sparkles size={16} />Apply {chosen.length || ""} selected</Button>
        </div>
      </header>

      <div className="row" style={{ marginBottom: 16 }}>
        <Segmented label="Optimization mode" value={mode} options={MODES} onChange={(m) => { setMode(m); setReport(null); }} />
        <span className="spacer" />
        <span className="faint row" style={{ gap: 6 }}><ShieldCheck size={14} />Never disables Defender, Firewall, Windows Update or services.</span>
      </div>

      {report && (
        <Card title="Result" className="" actions={<Button small onClick={() => go("history")}>Open history (undo)</Button>}>
          <PlanReportView report={report} />
        </Card>
      )}

      <div className="stack mt">
        {recs.loading && !recs.data ? <Loading label="Measuring this PC and building a plan…" /> : recs.error ? <ErrorBox error={recs.error} retry={recs.reload} /> : (recs.data ?? []).length === 0 ? (
          <Card><Notice tone="good">No changes recommended for this mode. Based on what APEX measured, this PC is already configured sensibly — so APEX won't change things just to look busy.</Notice></Card>
        ) : groups.filter((g) => g.items.length).map((g) => (
          <section key={g.title}>
            <h3 style={{ margin: "0 0 8px", fontSize: 14, color: "var(--text-2)" }}>{g.title}</h3>
            {g.items.map((r) => <RecCard key={r.id} r={r} selected={selected.has(r.id)} onToggle={() => toggle(r.id)} />)}
          </section>
        ))}
      </div>
    </div>
  );
}
