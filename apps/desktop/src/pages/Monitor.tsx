import { Cpu, HardDrive, MemoryStick, Network, Thermometer } from "lucide-react";
import { api } from "../api";
import { Card, LineChart, Notice, useAsync } from "../components/ui";
import { bytes, pct, rate } from "../lib/format";
import { useLive } from "../lib/live";

export function PerfMonitor() {
  const live = useLive();
  const temps = useAsync(() => api.temperatures());
  const l = live.latest;
  const s = live.samples;
  return (
    <div className="page">
      <header className="page-head">
        <div>
          <h1>Performance Monitor</h1>
          <p>Sampled at the interval set in Settings while this window is visible. Sampling pauses when APEX is minimized.</p>
        </div>
      </header>
      {live.error && <Notice tone="warn">{live.error}</Notice>}

      <div className="grid g2">
        <Card title="Processor" icon={<Cpu size={18} />} actions={<strong>{pct(l?.cpuPercent, 1)}</strong>}>
          <LineChart series={[{ values: s.map((x) => x.cpuPercent), color: "var(--chart-1)", label: "CPU" }]} max={100} height={140} />
          <div className="faint" style={{ margin: "12px 0 6px" }}>Per logical processor</div>
          <div className="cores">
            {(l?.perCorePercent ?? []).map((c, i) => (
              <div key={i} className="core" title={`CPU ${i}: ${c.toFixed(0)}%`}><span style={{ height: `${Math.min(100, c)}%` }} /></div>
            ))}
          </div>
        </Card>
        <Card title="Memory" icon={<MemoryStick size={18} />} actions={<strong>{bytes(l?.memoryUsedBytes)} / {bytes(l?.memoryTotalBytes, 0)}</strong>}>
          <LineChart series={[{ values: s.map((x) => (x.memoryTotalBytes ? (x.memoryUsedBytes / x.memoryTotalBytes) * 100 : 0)), color: "var(--chart-2)", label: "Memory" }]} max={100} height={140} />
          <dl className="kv" style={{ marginTop: 12 }}>
            <dt>In use</dt><dd>{bytes(l?.memoryUsedBytes)} ({pct(l && l.memoryTotalBytes ? (l.memoryUsedBytes / l.memoryTotalBytes) * 100 : null)})</dd>
            <dt>Page file in use</dt><dd>{bytes(l?.swapUsedBytes)}</dd>
          </dl>
          <div className="faint" style={{ marginTop: 8 }}>“In use” excludes standby/cache memory, which Windows frees instantly when needed. APEX never force-empties it.</div>
        </Card>
        <Card title="Disk" icon={<HardDrive size={18} />} actions={<span className="muted">R {rate(l?.diskReadBytesPerSec)} · W {rate(l?.diskWriteBytesPerSec)}</span>}>
          <LineChart series={[
            { values: s.map((x) => x.diskReadBytesPerSec), color: "var(--chart-3)", label: "Read" },
            { values: s.map((x) => x.diskWriteBytesPerSec), color: "var(--chart-4)", label: "Write" },
          ]} height={140} format={rate} />
        </Card>
        <Card title="Network" icon={<Network size={18} />} actions={<span className="muted">↓ {rate(l?.netRxBytesPerSec)} · ↑ {rate(l?.netTxBytesPerSec)}</span>}>
          <LineChart series={[
            { values: s.map((x) => x.netRxBytesPerSec), color: "var(--chart-1)", label: "Download" },
            { values: s.map((x) => x.netTxBytesPerSec), color: "var(--chart-2)", label: "Upload" },
          ]} height={140} format={rate} />
        </Card>
      </div>

      <Card title="Temperatures" icon={<Thermometer size={18} />} className="mt" actions={<button className="btn sm" onClick={() => void temps.reload()}>Refresh</button>}>
        {(temps.data ?? []).length === 0 ? (
          <div className="muted">Windows didn't report any temperature sensors to APEX. Many PCs only expose them through vendor drivers or tools; APEX shows nothing rather than guessing.</div>
        ) : (
          <div className="grid g4">
            {temps.data!.map(([label, t]) => (
              <div key={label}><div className="metric-label">{label}</div><div className="metric-value" style={{ fontSize: 22 }}>{t.toFixed(0)}°C</div></div>
            ))}
          </div>
        )}
      </Card>
    </div>
  );
}
