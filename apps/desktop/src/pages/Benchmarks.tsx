import { useEffect, useState } from "react";
import { FlaskConical, Play, Scale } from "lucide-react";
import { api, type BenchKind, type BenchmarkRecord, type Comparison } from "../api";
import { Badge, Button, Card, ErrorBox, Notice, RunsPlot, useAsync, useToast } from "../components/ui";
import { verdictLabel, when } from "../lib/format";

const KINDS: { kind: BenchKind; label: string; what: string }[] = [
  { kind: "cpuSingle", label: "CPU single-thread", what: "Integer and branch-heavy work on one core (responsiveness, many games)." },
  { kind: "cpuMulti", label: "CPU multi-thread", what: "Same work on every logical core at once (compiling, rendering)." },
  { kind: "memoryBandwidth", label: "Memory bandwidth", what: "Copies a 256 MB buffer to measure RAM throughput." },
  { kind: "storageSequential", label: "Storage write", what: "Writes 512 MB to your Temp folder and flushes it to the drive. File is removed afterwards." },
];

const mean = (xs: number[]) => xs.reduce((a, b) => a + b, 0) / xs.length;

export function Benchmarks() {
  const [kind, setKind] = useState<BenchKind>("cpuSingle");
  const [runs, setRuns] = useState(5);
  const [label, setLabel] = useState("Baseline");
  const [progress, setProgress] = useState<{ done: number; total: number } | null>(null);
  const list = useAsync(() => api.benchmarks(kind), [kind]);
  const [before, setBefore] = useState("");
  const [after, setAfter] = useState("");
  const [cmp, setCmp] = useState<Comparison | null>(null);
  const toast = useToast();

  useEffect(() => {
    let un: (() => void) | undefined;
    void api.onBenchProgress((p) => setProgress({ done: p.done, total: p.total })).then((f) => { un = f; });
    return () => un?.();
  }, []);
  useEffect(() => { setCmp(null); setBefore(""); setAfter(""); }, [kind]);

  const run = async () => {
    setProgress({ done: 0, total: runs });
    try {
      const r = await api.runBenchmark(kind, runs, label);
      toast("good", `${KINDS.find((k) => k.kind === kind)?.label}: ${mean(r.samples).toFixed(1)} ${r.unit}`);
      await list.reload();
    } catch (e) {
      toast("bad", e instanceof Error ? e.message : String(e));
    } finally {
      setProgress(null);
    }
  };

  const compare = async () => {
    try { setCmp(await api.compare(before, after)); } catch (e) { toast("bad", e instanceof Error ? e.message : String(e)); }
  };

  const records = list.data ?? [];
  const byId = (id: string) => records.find((r) => r.id === id);
  const b = byId(before), a = byId(after);
  const verdictTone = cmp?.verdict === "improved" ? "good" : cmp?.verdict === "regressed" ? "bad" : undefined;

  return (
    <div className="page">
      <header className="page-head">
        <div>
          <h1>Benchmark Lab</h1>
          <p>Measure before and after a change. APEX only calls something an improvement when the difference is larger than run-to-run noise (95% confidence) and at least 2%.</p>
        </div>
      </header>

      <div className="grid g4">
        {KINDS.map((k) => (
          <button key={k.kind} className="card pick" style={{ cursor: "default", outline: kind === k.kind ? "2px solid var(--accent)" : undefined, outlineOffset: -1 }} aria-pressed={kind === k.kind} onClick={() => setKind(k.kind)}>
            <h3>{k.label}</h3>
            <div className="faint" style={{ marginTop: 4 }}>{k.what}</div>
          </button>
        ))}
      </div>

      <Card title="Run" icon={<FlaskConical size={18} />} className="mt" sub="Close other apps and plug in laptops for consistent results. One warm-up run is discarded.">
        <div className="row" style={{ flexWrap: "wrap" }}>
          <input className="input" value={label} maxLength={80} onChange={(e) => setLabel(e.target.value)} aria-label="Result label" placeholder="Label, e.g. Before startup cleanup" />
          <label htmlFor="runs" className="faint">Runs</label>
          <select id="runs" className="input" value={runs} onChange={(e) => setRuns(Number(e.target.value))}>{[3, 5, 7, 10].map((n) => <option key={n}>{n}</option>)}</select>
          <Button kind="primary" busy={!!progress} onClick={run}><Play size={16} />Run {KINDS.find((k) => k.kind === kind)?.label}</Button>
          {progress && <span className="muted">Run {progress.done} of {progress.total}…</span>}
        </div>
        {progress && <div className="progress-indeterminate" style={{ marginTop: 12 }} />}
      </Card>

      <Card title="Compare" icon={<Scale size={18} />} className="mt">
        <div className="row" style={{ flexWrap: "wrap" }}>
          <select className="input" value={before} onChange={(e) => setBefore(e.target.value)} aria-label="Before result"><option value="">Before…</option>{records.map((r) => <option key={r.id} value={r.id}>{r.label} — {when(r.createdAt)}</option>)}</select>
          <span>→</span>
          <select className="input" value={after} onChange={(e) => setAfter(e.target.value)} aria-label="After result"><option value="">After…</option>{records.map((r) => <option key={r.id} value={r.id}>{r.label} — {when(r.createdAt)}</option>)}</select>
          <Button disabled={!before || !after || before === after} onClick={compare}>Compare</Button>
        </div>
        {cmp && b && a && (
          <div className="stack mt">
            <div className="row"><Badge tone={verdictTone}>{verdictLabel[cmp.verdict]}</Badge><span>{cmp.explanation}</span></div>
            <RunsPlot unit={b.unit} groups={[{ label: b.label, values: b.samples, color: "var(--chart-2)" }, { label: a.label, values: a.samples, color: "var(--chart-1)" }]} />
            <dl className="kv">
              <dt>Before</dt><dd>{cmp.before.mean.toFixed(2)} {b.unit} ± {cmp.before.ci95.toFixed(2)} (95% CI), CV {cmp.before.cvPercent.toFixed(1)}%, n={cmp.before.n}</dd>
              <dt>After</dt><dd>{cmp.after.mean.toFixed(2)} {a.unit} ± {cmp.after.ci95.toFixed(2)} (95% CI), CV {cmp.after.cvPercent.toFixed(1)}%, n={cmp.after.n}</dd>
              <dt>Welch t-test</dt><dd>t = {Number.isFinite(cmp.tStatistic) ? cmp.tStatistic.toFixed(2) : "∞"}, df ≈ {cmp.degreesOfFreedom.toFixed(1)}{cmp.significant ? " · statistically significant" : ""}</dd>
            </dl>
          </div>
        )}
      </Card>

      <Card title="History" className="mt">
        {list.error ? <ErrorBox error={list.error} /> : records.length === 0 ? <div className="muted">No results for this benchmark yet.</div> : (
          <div className="table-wrap">
            <table className="data">
              <thead><tr><th>Label</th><th>When</th><th className="num">Mean</th><th className="num">Spread (CV)</th><th className="num">Runs</th><th>Conditions</th></tr></thead>
              <tbody>
                {records.map((r: BenchmarkRecord) => {
                  const m = mean(r.samples);
                  const sd = Math.sqrt(r.samples.reduce((s, x) => s + (x - m) ** 2, 0) / Math.max(1, r.samples.length - 1));
                  const warnings = r.context.warnings ?? [];
                  return (
                    <tr key={r.id}>
                      <td>{r.label}</td>
                      <td className="faint">{when(r.createdAt)}</td>
                      <td className="num"><strong>{m.toFixed(1)}</strong> {r.unit}</td>
                      <td className="num">{((sd / m) * 100).toFixed(1)}%</td>
                      <td className="num">{r.samples.length}</td>
                      <td>{warnings.length ? <span className="faint">{warnings.join(" ")}</span> : <Badge tone="good">Clean</Badge>}</td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        )}
        <Notice>Boot-time, app-launch and in-game benchmarks are not available yet. Temperatures are recorded with each run when Windows exposes them.</Notice>
      </Card>
    </div>
  );
}
