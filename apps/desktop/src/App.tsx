import { useEffect, useState, type ComponentType } from "react";
import { Activity, Cpu, FlaskConical, Gamepad2, Gauge, HardDrive, History as HistoryIcon, LayoutDashboard, ListTree, Rocket, Settings as SettingsIcon, Sparkles, Stethoscope, Wifi } from "lucide-react";
import { api, inTauri, type Mode } from "./api";
import { ConfirmProvider, Notice, ToastProvider } from "./components/ui";
import { LiveProvider } from "./lib/live";
import { Dashboard } from "./pages/Dashboard";
import { Optimizer } from "./pages/Optimizer";
import { Gaming } from "./pages/Gaming";
import { PerfMonitor } from "./pages/Monitor";
import { Startup } from "./pages/Startup";
import { Processes } from "./pages/Processes";
import { Storage } from "./pages/Storage";
import { NetworkCenter } from "./pages/Network";
import { Diagnostics } from "./pages/Diagnostics";
import { Benchmarks } from "./pages/Benchmarks";
import { History } from "./pages/History";
import { Settings, type Theme } from "./pages/Settings";

export type PageId = "dashboard" | "optimizer" | "gaming" | "monitor" | "startup" | "processes" | "storage" | "network" | "diagnostics" | "bench" | "history" | "settings";

const NAV: { id: PageId; label: string; icon: ComponentType<{ size?: number }>; section?: string }[] = [
  { id: "dashboard", label: "Dashboard", icon: LayoutDashboard },
  { id: "optimizer", label: "Smart Optimizer", icon: Sparkles },
  { id: "gaming", label: "Gaming Center", icon: Gamepad2 },
  { id: "monitor", label: "Performance Monitor", icon: Activity, section: "Monitor" },
  { id: "startup", label: "Startup Manager", icon: Rocket },
  { id: "processes", label: "Process Explorer", icon: ListTree },
  { id: "storage", label: "Storage Cleaner", icon: HardDrive, section: "Tools" },
  { id: "network", label: "Network Center", icon: Wifi },
  { id: "diagnostics", label: "System Diagnostics", icon: Stethoscope },
  { id: "bench", label: "Benchmark Lab", icon: FlaskConical },
  { id: "history", label: "Optimization History", icon: HistoryIcon, section: "Changes" },
  { id: "settings", label: "Settings", icon: SettingsIcon },
];

function applyTheme(t: Theme) {
  if (t === "system") delete document.documentElement.dataset.theme;
  else document.documentElement.dataset.theme = t;
}

export function App() {
  const [page, setPage] = useState<PageId>("dashboard");
  const [theme, setThemeState] = useState<Theme>("system");
  const [interval, setIntervalState] = useState(1000);
  const [defaultMode, setDefaultModeState] = useState<Mode>("balanced");

  useEffect(() => {
    if (!inTauri()) return;
    void api.getSetting("theme").then((t) => { if (t === "light" || t === "dark" || t === "system") { setThemeState(t); applyTheme(t); } }).catch(() => {});
    void api.getSetting("liveIntervalMs").then((v) => { const n = Number(v); if ([1000, 2000, 5000].includes(n)) setIntervalState(n); }).catch(() => {});
    void api.getSetting("defaultMode").then((m) => { if (m === "balanced" || m === "gaming" || m === "maximumPerformance") setDefaultModeState(m); }).catch(() => {});
  }, []);

  const setTheme = (t: Theme) => { setThemeState(t); applyTheme(t); void api.setSetting("theme", t).catch(() => {}); };
  const setIv = (n: number) => { setIntervalState(n); void api.setSetting("liveIntervalMs", String(n)).catch(() => {}); };
  const setDefaultMode = (m: Mode) => { setDefaultModeState(m); void api.setSetting("defaultMode", m).catch(() => {}); };

  return (
    <ToastProvider>
      <ConfirmProvider>
        <LiveProvider intervalMs={interval}>
          <div className="shell">
            <nav className="sidebar" aria-label="Main">
              <div className="brand"><span className="brand-mark"><Gauge size={18} /></span>APEX</div>
              {NAV.map((n) => (
                <div key={n.id}>
                  {n.section && <div className="nav-section">{n.section}</div>}
                  <button className="nav-item" aria-current={page === n.id ? "page" : undefined} onClick={() => setPage(n.id)} style={{ width: "100%" }}>
                    <n.icon size={18} />
                    {n.label}
                  </button>
                </div>
              ))}
              <div className="sidebar-foot"><Cpu size={12} style={{ verticalAlign: -1 }} /> Free · open source · no telemetry</div>
            </nav>
            <main className="main" id="main">
              {!inTauri() && <div style={{ padding: "16px 36px 0" }}><Notice tone="warn">You're viewing the APEX interface in a browser. It only works inside the APEX desktop app, which provides the measurements.</Notice></div>}
              {page === "dashboard" && <Dashboard go={setPage} />}
              {page === "optimizer" && <Optimizer go={setPage} initialMode={defaultMode} />}
              {page === "gaming" && <Gaming />}
              {page === "monitor" && <PerfMonitor />}
              {page === "startup" && <Startup />}
              {page === "processes" && <Processes />}
              {page === "storage" && <Storage />}
              {page === "network" && <NetworkCenter />}
              {page === "diagnostics" && <Diagnostics />}
              {page === "bench" && <Benchmarks />}
              {page === "history" && <History />}
              {page === "settings" && <Settings theme={theme} setTheme={setTheme} interval={interval} setInterval={setIv} defaultMode={defaultMode} setDefaultMode={setDefaultMode} />}
            </main>
          </div>
        </LiveProvider>
      </ConfirmProvider>
    </ToastProvider>
  );
}
