import { ClipboardList, Download, RefreshCw } from "lucide-react";
import { api } from "../api";
import { Badge, Button, Card, ErrorBox, Loading, Notice, useAsync, useToast } from "../components/ui";
import { bytes, when } from "../lib/format";

export function Diagnostics() {
  const snap = useAsync(() => api.snapshot(false));
  const toast = useToast();
  const s = snap.data;
  return (
    <div className="page">
      <header className="page-head">
        <div>
          <h1>System Diagnostics</h1>
          <p>Hardware and configuration as measured by APEX. Data stays on this PC unless you export it.</p>
        </div>
        <div className="actions-row">
          <Button onClick={async () => { try { snap.setData(await api.snapshot(true)); } catch (e) { toast("bad", String(e instanceof Error ? e.message : e)); } }}><RefreshCw size={16} />Re-measure</Button>
          <Button onClick={async () => { try { toast("good", `Report saved to ${await api.exportReport()}`); } catch (e) { toast("bad", String(e instanceof Error ? e.message : e)); } }}><Download size={16} />Export report</Button>
        </div>
      </header>
      {snap.loading && !s ? <Loading label="Measuring…" /> : snap.error ? <ErrorBox error={snap.error} retry={snap.reload} /> : s && (
        <div className="stack">
          {s.limitations.length > 0 && (
            <Card title="What APEX could not measure" icon={<ClipboardList size={18} />}>
              {s.limitations.map((l) => <Notice key={l}>{l}</Notice>)}
            </Card>
          )}
          <div className="grid g2">
            <Card title="Operating system">
              <dl className="kv">
                <dt>Edition</dt><dd>{s.os.name}</dd>
                <dt>Version</dt><dd>{s.os.version ?? "—"}</dd>
                <dt>Build</dt><dd>{s.os.build ?? "—"}</dd>
                <dt>Kernel</dt><dd>{s.os.kernel ?? "—"}</dd>
                <dt>Computer name</dt><dd>{s.os.hostname ?? "—"}</dd>
                <dt>Measured</dt><dd>{when(s.capturedAt)}</dd>
              </dl>
            </Card>
            <Card title="Processor">
              <dl className="kv">
                <dt>Model</dt><dd>{s.cpu.brand || "—"}</dd>
                <dt>Vendor</dt><dd>{s.cpu.vendor || "—"}</dd>
                <dt>Architecture</dt><dd>{s.cpu.arch}</dd>
                <dt>Cores</dt><dd>{s.cpu.physicalCores ?? "?"} physical · {s.cpu.logicalCores} logical</dd>
                <dt>Reported clock</dt><dd>{s.cpu.frequencyMhz ? `${s.cpu.frequencyMhz} MHz` : "—"}</dd>
              </dl>
            </Card>
            <Card title="Memory">
              <dl className="kv">
                <dt>Installed (usable)</dt><dd>{bytes(s.memory.totalBytes)}</dd>
                <dt>Available</dt><dd>{bytes(s.memory.availableBytes)}</dd>
                <dt>Page file</dt><dd>{bytes(s.memory.swapUsedBytes)} used of {bytes(s.memory.swapTotalBytes)}</dd>
              </dl>
            </Card>
            <Card title="Graphics">
              {s.gpus.length === 0 ? <div className="muted">Not detected.</div> : s.gpus.map((g) => (
                <dl key={g.name} className="kv" style={{ marginBottom: 8 }}>
                  <dt>Adapter</dt><dd>{g.name}</dd>
                  <dt>Driver</dt><dd>{g.driverVersion ?? "—"}</dd>
                  <dt>Dedicated memory</dt><dd>{g.dedicatedMemoryBytes ? bytes(g.dedicatedMemoryBytes) : "Not reported"}</dd>
                </dl>
              ))}
            </Card>
            <Card title="Storage">
              {s.disks.map((d) => (
                <div key={d.mountPoint} className="row" style={{ padding: "3px 0" }}>
                  <strong>{d.mountPoint}</strong>{d.isSystem && <Badge tone="accent">System</Badge>}
                  <span className="faint">{d.kind.toUpperCase()} · {d.fileSystem}</span><span className="spacer" />
                  <span>{bytes(d.availableBytes)} free / {bytes(d.totalBytes)}</span>
                </div>
              ))}
            </Card>
            <Card title="Power">
              {s.power ? (
                <dl className="kv">
                  <dt>Active plan</dt><dd>{s.power.active.name}</dd>
                  <dt>Installed plans</dt><dd>{s.power.available.map((p) => p.name).join(", ")}</dd>
                  <dt>Battery</dt><dd>{s.power.batteryPresent == null ? "Unknown" : s.power.batteryPresent ? (s.power.onBattery ? "Present · on battery" : "Present · plugged in") : "None"}</dd>
                  <dt>Game Mode</dt><dd>{s.gameModeEnabled == null ? "—" : s.gameModeEnabled ? "On" : "Off"}</dd>
                </dl>
              ) : <div className="muted">Not available.</div>}
            </Card>
          </div>
        </div>
      )}
    </div>
  );
}
