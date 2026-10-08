import { Activity, ArrowRight, Cpu, Gauge, HardDrive, History, MemoryStick, Network, Sparkles } from "lucide-react";
import { api } from "../api";
import { Badge, Button, Card, ErrorBox, LineChart, Loading, Meter, Notice, useAsync } from "../components/ui";
import { batchStatusLabel, bytes, categoryLabel, pct, rate, when } from "../lib/format";
import { useLive } from "../lib/live";
import type { PageId } from "../App";

export function Dashboard({ go }: { go: (p: PageId) => void }) {
  const live = useLive();
  const snap = useAsync(() => api.snapshot(false));
  const recs = useAsync(() => api.recommendations("balanced", false));
  const hist = useAsync(() => api.history(4));
  const bench = useAsync(() => api.benchmarks());

  const l = live.latest;
  const cpuSeries = live.samples.map((s) => s.cpuPercent);
  const memSeries = live.samples.map((s) => (s.memoryTotalBytes ? (s.memoryUsedBytes / s.memoryTotalBytes) * 100 : 0));
  const sysDisk = snap.data?.disks.find((d) => d.isSystem);
  const actionable = (recs.data ?? []).filter((r) => r.action);
  const latestBench = Object.values(
    (bench.data ?? []).reduce<Record<string, NonNullable<typeof bench.data>[number]>>((acc, b) => {
      if (!acc[b.kind]) acc[b.kind] = b;
      return acc;
    }, {}),
  );

  return (
    <div className="page">
      <header className="page-head">
        <div>
          <h1>Dashboard</h1>
          <p>Live, measured view of this PC. Nothing here changes your system.</p>
        </div>
        <Button kind="primary" onClick={() => go("optimizer")}><Sparkles size={16} />Optimize my PC</Button>
      </header>

      {live.error && <Notice tone="warn">{live.error}</Notice>}

      <div className="grid g4">
        <Card>
          <div className="metric-label"><Cpu size={14} />CPU</div>
          <div className="metric-value">{pct(l?.cpuPercent)}</div>
          <LineChart series={[{ values: cpuSeries, color: "var(--chart-1)", label: "CPU" }]} height={48} max={100} />
        </Card>
        <Card>
          <div className="metric-label"><MemoryStick size={14} />Memory</div>
          <div className="metric-value">{bytes(l?.memoryUsedBytes)}<small>of {bytes(l?.memoryTotalBytes, 0)}</small></div>
          <LineChart series={[{ values: memSeries, color: "var(--chart-2)", label: "Memory" }]} height={48} max={100} />
        </Card>
        <Card>
          <div className="metric-label"><HardDrive size={14} />Disk activity</div>
          <div className="metric-value" style={{ fontSize: 20 }}>{rate((l?.diskReadBytesPerSec ?? 0) + (l?.diskWriteBytesPerSec ?? 0))}</div>
          <LineChart series={[{ values: live.samples.map((s) => s.diskReadBytesPerSec + s.diskWriteBytesPerSec), color: "var(--chart-3)", label: "Disk" }]} height={48} />
        </Card>
        <Card>
          <div className="metric-label"><Network size={14} />Network</div>
          <div className="metric-value" style={{ fontSize: 20 }}>↓ {rate(l?.netRxBytesPerSec)}</div>
          <LineChart series={[{ values: live.samples.map((s) => s.netRxBytesPerSec + s.netTxBytesPerSec), color: "var(--chart-4)", label: "Network" }]} height={48} />
        </Card>
      </div>

      <div className="grid g2 mt">
        <Card title="Health" icon={<Activity size={18} />} actions={<Button small kind="subtle" onClick={() => go("diagnostics")}>Details<ArrowRight size={14} /></Button>}>
          {snap.loading && !snap.data ? <Loading label="Measuring system…" /> : snap.error ? <ErrorBox error={snap.error} retry={snap.reload} /> : snap.data && (
            <div className="stack" style={{ gap: 12 }}>
              {sysDisk && (
                <div>
                  <div className="row"><span>System drive {sysDisk.mountPoint}</span><span className="spacer" /><span className="muted">{bytes(sysDisk.availableBytes)} free · {sysDisk.kind.toUpperCase()}</span></div>
                  <Meter value={sysDisk.totalBytes - sysDisk.availableBytes} max={sysDisk.totalBytes} warnAt={85} badAt={92} label="System drive usage" />
                </div>
              )}
              <div>
                <div className="row"><span>Memory in use</span><span className="spacer" /><span className="muted">{bytes(snap.data.memory.totalBytes - snap.data.memory.availableBytes)} of {bytes(snap.data.memory.totalBytes)}</span></div>
                <Meter value={snap.data.memory.totalBytes - snap.data.memory.availableBytes} max={snap.data.memory.totalBytes} label="Memory usage" />
              </div>
              <dl className="kv">
                <dt>Processor</dt><dd className="ellipsis">{snap.data.cpu.brand || "—"}</dd>
                <dt>Graphics</dt><dd className="ellipsis">{snap.data.gpus.map((g) => g.name).join(", ") || "Not detected"}</dd>
                <dt>Power plan</dt><dd>{snap.data.power?.active.name ?? "—"}</dd>
                <dt>Startup apps</dt><dd>{snap.data.startup.filter((s) => s.enabled).length} enabled</dd>
              </dl>
            </div>
          )}
        </Card>

        <Card title="Opportunities" icon={<Gauge size={18} />} sub="From measured facts on this PC (Balanced mode)." actions={<Button small kind="subtle" onClick={() => go("optimizer")}>Review all<ArrowRight size={14} /></Button>}>
          {recs.loading && !recs.data ? <Loading label="Analyzing…" /> : recs.error ? <ErrorBox error={recs.error} retry={recs.reload} /> : (recs.data ?? []).length === 0 ? (
            <Notice tone="good">Nothing measured on this PC needs attention right now.</Notice>
          ) : (
            <div className="stack" style={{ gap: 8 }}>
              {(recs.data ?? []).slice(0, 5).map((r) => (
                <div key={r.id} className="row" style={{ alignItems: "flex-start" }}>
                  <Badge tone={r.category === "verifiedProblem" ? "bad" : r.category === "likelyBottleneck" ? "warn" : undefined}>{categoryLabel[r.category]}</Badge>
                  <span style={{ flex: 1 }}>{r.title}</span>
                </div>
              ))}
              <div className="faint">{actionable.length} can be applied automatically and undone at any time.</div>
            </div>
          )}
        </Card>

        <Card title="Recent changes" icon={<History size={18} />} actions={<Button small kind="subtle" onClick={() => go("history")}>History<ArrowRight size={14} /></Button>}>
          {hist.error ? <ErrorBox error={hist.error} /> : (hist.data ?? []).length === 0 ? <div className="muted">APEX hasn't changed anything on this PC.</div> : (
            <div className="stack" style={{ gap: 8 }}>
              {(hist.data ?? []).map((b) => (
                <div key={b.id} className="row">
                  <span style={{ flex: 1 }} className="ellipsis">{b.label}</span>
                  <Badge tone={b.status === "applied" ? "good" : b.status === "failed" ? "bad" : undefined}>{batchStatusLabel[b.status]}</Badge>
                  <span className="faint">{when(b.createdAt)}</span>
                </div>
              ))}
            </div>
          )}
        </Card>

        <Card title="Latest benchmarks" icon={<Gauge size={18} />} actions={<Button small kind="subtle" onClick={() => go("bench")}>Benchmark Lab<ArrowRight size={14} /></Button>}>
          {latestBench.length === 0 ? <div className="muted">No measurements yet. Run a baseline before optimizing so you can see whether changes help.</div> : (
            <div className="stack" style={{ gap: 8 }}>
              {latestBench.map((b) => {
                const mean = b.samples.reduce((a, c) => a + c, 0) / b.samples.length;
                return (
                  <div key={b.id} className="row">
                    <span style={{ flex: 1 }}>{String(b.context["benchmark"] ?? b.kind)}</span>
                    <strong>{mean.toFixed(1)} {b.unit}</strong>
                    <span className="faint">{when(b.createdAt)}</span>
                  </div>
                );
              })}
            </div>
          )}
        </Card>
      </div>
    </div>
  );
}
