import type { Action, BatchStatus, Category, ChangeStatus, Outcome, Risk, SettingValue, Verdict } from "../api";

export function bytes(n: number | null | undefined, digits = 1): string {
  if (n == null || !Number.isFinite(n)) return "—";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let v = Math.abs(n);
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${n < 0 ? "-" : ""}${i === 0 ? v.toFixed(0) : v.toFixed(digits)} ${units[i]}`;
}

export const rate = (n: number | null | undefined) => (n == null ? "—" : `${bytes(n)}/s`);
export const pct = (n: number | null | undefined, digits = 0) => (n == null || !Number.isFinite(n) ? "—" : `${n.toFixed(digits)}%`);
export const ms = (n: number | null | undefined) => (n == null ? "—" : `${n < 10 ? n.toFixed(1) : n.toFixed(0)} ms`);

export function duration(secs: number): string {
  if (secs < 60) return `${secs}s`;
  const m = Math.floor(secs / 60);
  if (m < 60) return `${m}m`;
  const h = Math.floor(m / 60);
  if (h < 48) return `${h}h ${m % 60}m`;
  return `${Math.floor(h / 24)}d`;
}

export function when(iso: string): string {
  const d = new Date(iso);
  return d.toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
}

export const categoryLabel: Record<Category, string> = {
  verifiedProblem: "Verified problem",
  likelyBottleneck: "Likely bottleneck",
  preference: "Optional",
  experimental: "Experimental",
};
export const riskLabel: Record<Risk, string> = { low: "Low risk", medium: "Medium risk", high: "High risk" };

export const batchStatusLabel: Record<BatchStatus, string> = {
  inProgress: "In progress",
  applied: "Applied",
  partiallyApplied: "Partially applied",
  rolledBack: "Rolled back",
  reverted: "Undone",
  failed: "Failed",
};
export const changeStatusLabel: Record<ChangeStatus, string> = {
  pending: "Interrupted",
  applied: "Applied",
  skipped: "Already set",
  failed: "Failed",
  rolledBack: "Rolled back",
  reverted: "Undone",
  needsAttention: "Needs attention",
};
export const verdictLabel: Record<Verdict, string> = {
  improved: "Measurably better",
  regressed: "Measurably worse",
  noSignificantChange: "No measurable change",
  inconclusive: "Inconclusive",
};

export function outcomeLabel(o: Outcome): string {
  switch (o.outcome) {
    case "applied": return "Applied & verified";
    case "rolledBack": return "Rolled back";
    case "skipped": return `Skipped — ${o.message}`;
    case "failed": return `Failed — ${o.message}`;
    case "notAttempted": return `Not attempted — ${o.message}`;
  }
}

export function settingValue(v: SettingValue | null): string {
  if (!v) return "—";
  switch (v.type) {
    case "absent": return "(not set — Windows default)";
    case "bool": return v.value ? "On / enabled" : "Off / disabled";
    case "text": return v.value;
  }
}

export function describeAction(a: Action): string {
  switch (a.kind) {
    case "setPowerScheme": return `Switch power plan to “${a.name}”`;
    case "setStartupEntryEnabled": return `${a.enabled ? "Enable" : "Disable"} startup entry “${a.name}”`;
    case "setGameMode": return `Turn Game Mode ${a.enabled ? "on" : "off"}`;
    case "setProcessPriority": return `Set ${a.name} (PID ${a.pid}) priority to ${a.priority}`;
    case "cleanTempFiles": return `Delete temp files older than ${a.olderThanDays} days`;
  }
}
