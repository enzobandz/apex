import { Info, Moon, RotateCcw, Shield } from "lucide-react";
import { api, type AppInfo, type Mode } from "../api";
import { Button, Card, Segmented, useAsync, useConfirm, useToast } from "../components/ui";

export type Theme = "system" | "light" | "dark";

export function Settings({ theme, setTheme, interval, setInterval: setIv, defaultMode, setDefaultMode }: { theme: Theme; setTheme: (t: Theme) => void; interval: number; setInterval: (n: number) => void; defaultMode: Mode; setDefaultMode: (m: Mode) => void }) {
  const info = useAsync<AppInfo>(() => api.appInfo());
  const confirm = useConfirm();
  const toast = useToast();
  const restoreAll = async () => {
    const ok = await confirm({ title: "Restore everything APEX changed?", confirmLabel: "Restore all", body: "Every reversible change still applied (power plan, startup entries, Game Mode, process priorities) is put back to its original value and verified. Deleted temp files cannot be restored. Do this before uninstalling if you want your PC exactly as it was." });
    if (!ok) return;
    try {
      const r = await api.undoEverything();
      const failed = r.filter((x) => !x.ok);
      toast(failed.length ? "bad" : "good", r.length === 0 ? "Nothing to restore." : failed.length ? `${failed.length} of ${r.length} could not be restored — see History.` : `Restored ${r.length} change(s).`);
    } catch (e) {
      toast("bad", e instanceof Error ? e.message : String(e));
    }
  };
  return (
    <div className="page">
      <header className="page-head"><div><h1>Settings</h1></div></header>
      <div className="stack">
        <Card title="Appearance" icon={<Moon size={18} />}>
          <Segmented label="Theme" value={theme} onChange={setTheme} options={[{ value: "system", label: "Use Windows setting" }, { value: "light", label: "Light" }, { value: "dark", label: "Dark" }]} />
        </Card>
        <Card title="Monitoring" sub="How often live charts sample while APEX is visible. Sampling always stops when APEX is minimized.">
          <Segmented label="Sampling interval" value={String(interval)} onChange={(v) => setIv(Number(v))} options={[{ value: "1000", label: "1 second" }, { value: "2000", label: "2 seconds" }, { value: "5000", label: "5 seconds" }]} />
        </Card>
        <Card title="Smart Optimizer default" sub="Mode shown first on the dashboard's opportunities and in the optimizer.">
          <Segmented label="Default mode" value={defaultMode} onChange={setDefaultMode} options={[{ value: "balanced", label: "Balanced" }, { value: "gaming", label: "Gaming" }, { value: "maximumPerformance", label: "Maximum performance" }]} />
        </Card>
        <Card title="Privacy & safety" icon={<Shield size={18} />}>
          <ul style={{ margin: 0, paddingLeft: 18, color: "var(--text-2)" }}>
            <li>No telemetry, accounts, ads or network calls except the tests you start in Network Center.</li>
            <li>No background service: APEX does nothing when it isn't open.</li>
            <li>Never disables Microsoft Defender, Windows Firewall, Windows Update or Windows services.</li>
            <li>Runs without administrator rights. Machine-wide items that would need elevation are shown but not changed.</li>
            <li>Every change is recorded with its original value and can be undone from Optimization History.</li>
          </ul>
        </Card>
        <Card title="Restore" icon={<RotateCcw size={18} />} sub="Uninstalling APEX removes the app, not the settings it changed. Restore first to return everything to how it was.">
          <Button onClick={restoreAll}><RotateCcw size={16} />Restore everything APEX changed</Button>
        </Card>
        <Card title="About" icon={<Info size={18} />}>
          <dl className="kv">
            <dt>Version</dt><dd>{info.data?.version ?? "—"}</dd>
            <dt>Platform</dt><dd>{info.data?.platform ?? "—"}</dd>
            <dt>History database</dt><dd className="mono break">{info.data?.database ?? "—"}</dd>
          </dl>
        </Card>
      </div>
    </div>
  );
}
