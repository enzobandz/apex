import { createContext, useCallback, useContext, useEffect, useRef, useState, type ReactNode } from "react";
import { AlertTriangle, CheckCircle2, Info, Loader2, XCircle } from "lucide-react";
import type { PlanReport } from "../api";
import { outcomeLabel } from "../lib/format";

export function Card({ title, sub, icon, actions, children, className = "" }: { title?: ReactNode; sub?: ReactNode; icon?: ReactNode; actions?: ReactNode; children?: ReactNode; className?: string }) {
  return (
    <section className={`card ${className}`}>
      {(title || actions) && (
        <div className="row" style={{ marginBottom: sub ? 0 : undefined }}>
          {title && <h2>{icon}{title}</h2>}
          <span className="spacer" />
          {actions}
        </div>
      )}
      {sub && <p className="card-sub">{sub}</p>}
      {children}
    </section>
  );
}

export function Badge({ tone, children }: { tone?: "good" | "warn" | "bad" | "accent"; children: ReactNode }) {
  return <span className={`badge ${tone ?? ""}`}>{children}</span>;
}

export function Meter({ value, max = 100, warnAt = 75, badAt = 90, label }: { value: number; max?: number; warnAt?: number; badAt?: number; label: string }) {
  const p = max > 0 ? Math.min(100, Math.max(0, (value / max) * 100)) : 0;
  const tone = p >= badAt ? "bad" : p >= warnAt ? "warn" : "";
  return (
    <div className={`meter ${tone}`} role="meter" aria-label={label} aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(p)}>
      <span style={{ width: `${p}%` }} />
    </div>
  );
}

export function Notice({ tone, children }: { tone?: "warn" | "bad" | "good"; children: ReactNode }) {
  const Icon = tone === "bad" ? XCircle : tone === "warn" ? AlertTriangle : tone === "good" ? CheckCircle2 : Info;
  return (
    <div className={`notice ${tone ?? ""}`} role={tone === "bad" ? "alert" : "status"}>
      <Icon size={16} />
      <div>{children}</div>
    </div>
  );
}

export function Spinner({ size = 16 }: { size?: number }) {
  return <Loader2 size={size} className="spin" aria-label="Loading" />;
}

export function Button({ onClick, children, kind, disabled, busy, small, title, type = "button" }: { onClick?: () => void | Promise<unknown>; children: ReactNode; kind?: "primary" | "danger" | "subtle"; disabled?: boolean; busy?: boolean; small?: boolean; title?: string; type?: "button" | "submit" }) {
  const [running, setRunning] = useState(false);
  const isBusy = busy || running;
  return (
    <button
      type={type}
      title={title}
      className={`btn ${kind ?? ""} ${small ? "sm" : ""}`}
      disabled={disabled || isBusy}
      aria-busy={isBusy}
      onClick={async () => {
        if (!onClick) return;
        const r = onClick();
        if (r instanceof Promise) {
          setRunning(true);
          try { await r; } finally { setRunning(false); }
        }
      }}
    >
      {isBusy && <Spinner size={14} />}
      {children}
    </button>
  );
}

export function Segmented<T extends string>({ value, options, onChange, label }: { value: T; options: { value: T; label: string }[]; onChange: (v: T) => void; label: string }) {
  return (
    <div className="seg" role="group" aria-label={label}>
      {options.map((o) => (
        <button key={o.value} type="button" aria-pressed={o.value === value} onClick={() => onChange(o.value)}>
          {o.label}
        </button>
      ))}
    </div>
  );
}

// ---------------------------------------------------------------- toasts

type Toast = { id: number; tone: "good" | "bad" | "info"; text: string };
const ToastCtx = createContext<(tone: Toast["tone"], text: string) => void>(() => {});

export function ToastProvider({ children }: { children: ReactNode }) {
  const [toasts, setToasts] = useState<Toast[]>([]);
  const next = useRef(1);
  const push = useCallback((tone: Toast["tone"], text: string) => {
    const id = next.current++;
    setToasts((t) => [...t, { id, tone, text }]);
    setTimeout(() => setToasts((t) => t.filter((x) => x.id !== id)), tone === "bad" ? 9000 : 5000);
  }, []);
  return (
    <ToastCtx.Provider value={push}>
      {children}
      <div className="toasts" aria-live="polite">
        {toasts.map((t) => (
          <div key={t.id} className={`toast ${t.tone}`}>
            {t.tone === "bad" ? <XCircle size={16} color="var(--bad)" /> : t.tone === "good" ? <CheckCircle2 size={16} color="var(--good)" /> : <Info size={16} />}
            <span>{t.text}</span>
          </div>
        ))}
      </div>
    </ToastCtx.Provider>
  );
}
export const useToast = () => useContext(ToastCtx);

// ---------------------------------------------------------------- dialog

export function Dialog({ title, children, onClose, footer }: { title: string; children: ReactNode; onClose: () => void; footer: ReactNode }) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const prev = document.activeElement as HTMLElement | null;
    ref.current?.querySelector<HTMLElement>("button, input, select")?.focus();
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
      prev?.focus();
    };
  }, [onClose]);
  return (
    <div className="backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="dialog" role="dialog" aria-modal="true" aria-label={title} ref={ref}>
        <div className="dialog-body">
          <h2>{title}</h2>
          {children}
        </div>
        <div className="dialog-foot">{footer}</div>
      </div>
    </div>
  );
}

/** Imperative confirm dialog: `const confirm = useConfirm(); if (await confirm({...})) ...` */
type ConfirmOpts = { title: string; body: ReactNode; confirmLabel: string; danger?: boolean };
const ConfirmCtx = createContext<(o: ConfirmOpts) => Promise<boolean>>(async () => false);

export function ConfirmProvider({ children }: { children: ReactNode }) {
  const [state, setState] = useState<(ConfirmOpts & { resolve: (b: boolean) => void }) | null>(null);
  const confirm = useCallback((o: ConfirmOpts) => new Promise<boolean>((resolve) => setState({ ...o, resolve })), []);
  const close = (v: boolean) => {
    state?.resolve(v);
    setState(null);
  };
  return (
    <ConfirmCtx.Provider value={confirm}>
      {children}
      {state && (
        <Dialog
          title={state.title}
          onClose={() => close(false)}
          footer={
            <>
              <button className="btn" onClick={() => close(false)}>Cancel</button>
              <button className={`btn ${state.danger ? "danger" : "primary"}`} onClick={() => close(true)}>{state.confirmLabel}</button>
            </>
          }
        >
          <div className="muted">{state.body}</div>
        </Dialog>
      )}
    </ConfirmCtx.Provider>
  );
}
export const useConfirm = () => useContext(ConfirmCtx);

// ---------------------------------------------------------------- async data

export function useAsync<T>(fn: () => Promise<T>, deps: unknown[] = []) {
  const [data, setData] = useState<T | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const fnRef = useRef(fn);
  fnRef.current = fn;
  const reload = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      setData(await fnRef.current());
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }, []);
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(() => { void reload(); }, deps);
  return { data, error, loading, reload, setData };
}

export function ErrorBox({ error, retry }: { error: string; retry?: () => void }) {
  return (
    <Notice tone="bad">
      {error} {retry && <button className="btn sm" style={{ marginLeft: 8 }} onClick={retry}>Retry</button>}
    </Notice>
  );
}

export function Loading({ label }: { label: string }) {
  return (
    <div className="empty" aria-busy="true">
      <Spinner size={22} />
      <div style={{ marginTop: 8 }}>{label}</div>
    </div>
  );
}

// ---------------------------------------------------------------- plan report

export function PlanReportView({ report }: { report: PlanReport }) {
  const tone = report.status === "applied" ? "good" : report.status === "rolledBack" ? "warn" : report.status === "failed" || report.status === "partiallyApplied" ? "bad" : undefined;
  return (
    <div className="stack" style={{ gap: 8 }}>
      <Notice tone={tone}>{report.summary}</Notice>
      <ul style={{ margin: 0, paddingLeft: 18, fontSize: 13 }}>
        {report.results.map((r, i) => (
          <li key={r.changeId ?? i}>
            <strong>{r.description}</strong> — <span className="muted">{outcomeLabel(r.outcome)}</span>
            {r.detail && <div className="faint">{r.detail}</div>}
          </li>
        ))}
      </ul>
    </div>
  );
}

// ---------------------------------------------------------------- charts

export interface Series { values: number[]; color: string; label: string }

/** Area/line chart for time series. `max` fixes the y-scale (e.g. 100 for percentages). */
export function LineChart({ series, height = 120, max, format }: { series: Series[]; height?: number; max?: number; format?: (v: number) => string }) {
  const w = 600;
  const n = Math.max(2, ...series.map((s) => s.values.length));
  const top = max ?? Math.max(1, ...series.flatMap((s) => s.values)) * 1.15;
  const x = (i: number, len: number) => ((i + (n - len)) / (n - 1)) * w;
  const y = (v: number) => height - (Math.min(v, top) / top) * (height - 4) - 2;
  return (
    <div>
      <svg className="chart" viewBox={`0 0 ${w} ${height}`} preserveAspectRatio="none" style={{ height }} role="img" aria-label={series.map((s) => `${s.label}: ${format ? format(s.values.at(-1) ?? 0) : s.values.at(-1)}`).join(", ")}>
        {[0.25, 0.5, 0.75].map((f) => (
          <line key={f} className="grid-line" x1={0} x2={w} y1={height * f} y2={height * f} vectorEffect="non-scaling-stroke" />
        ))}
        {series.map((s) => {
          if (s.values.length < 2) return null;
          const pts = s.values.map((v, i) => `${x(i, s.values.length).toFixed(1)},${y(v).toFixed(1)}`);
          const firstX = x(0, s.values.length).toFixed(1);
          return (
            <g key={s.label}>
              <path d={`M${firstX},${height} L${pts.join(" L")} L${w},${height} Z`} fill={s.color} opacity={0.12} />
              <polyline points={pts.join(" ")} fill="none" stroke={s.color} strokeWidth={2} vectorEffect="non-scaling-stroke" strokeLinejoin="round" />
            </g>
          );
        })}
      </svg>
      {series.length > 1 && (
        <div className="legend" style={{ marginTop: 6 }}>
          {series.map((s) => (
            <span key={s.label}><i style={{ background: s.color }} />{s.label}{format && s.values.length ? `: ${format(s.values.at(-1) ?? 0)}` : ""}</span>
          ))}
        </div>
      )}
    </div>
  );
}

/** Dot plot of individual runs with mean line — shows run-to-run variability honestly. */
export function RunsPlot({ groups, unit }: { groups: { label: string; values: number[]; color: string }[]; unit: string }) {
  const all = groups.flatMap((g) => g.values);
  if (!all.length) return null;
  const lo = Math.min(...all);
  const hi = Math.max(...all);
  const pad = (hi - lo || hi * 0.05 || 1) * 0.25;
  const min = lo - pad;
  const max = hi + pad;
  const w = 600;
  const rowH = 34;
  const h = groups.length * rowH + 18;
  const x = (v: number) => 110 + ((v - min) / (max - min)) * (w - 130);
  return (
    <svg className="chart" viewBox={`0 0 ${w} ${h}`} style={{ height: h }} role="img" aria-label={`Individual runs in ${unit}`}>
      {groups.map((g, gi) => {
        const cy = gi * rowH + 18;
        const mean = g.values.reduce((a, b) => a + b, 0) / g.values.length;
        return (
          <g key={g.label}>
            <text x={0} y={cy + 4} style={{ fontSize: 12, fill: "var(--text-2)" }}>{g.label}</text>
            <line className="grid-line" x1={110} x2={w - 20} y1={cy} y2={cy} />
            {g.values.map((v, i) => <circle key={i} cx={x(v)} cy={cy} r={5} fill={g.color} opacity={0.75} />)}
            <line x1={x(mean)} x2={x(mean)} y1={cy - 11} y2={cy + 11} stroke={g.color} strokeWidth={2.5} />
          </g>
        );
      })}
      <text x={110} y={h - 1}>{min.toFixed(1)} {unit}</text>
      <text x={w - 20} y={h - 1} textAnchor="end">{max.toFixed(1)} {unit}</text>
    </svg>
  );
}
