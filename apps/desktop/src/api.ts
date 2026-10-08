// Typed bridge to the Rust backend. Types mirror the serde (camelCase) models in apex-core.
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type Mode = "balanced" | "gaming" | "maximumPerformance";
export type Category = "verifiedProblem" | "likelyBottleneck" | "preference" | "experimental";
export type Risk = "low" | "medium" | "high";
export type DiskKind = "ssd" | "hdd" | "unknown";
export type StartupLocation = "userRun" | "machineRun" | "machineRun32" | "userStartupFolder" | "commonStartupFolder";
export type PriorityClass = "idle" | "belowNormal" | "normal" | "aboveNormal" | "high";

export interface PowerScheme { guid: string; name: string }
export interface PowerInfo { active: PowerScheme; available: PowerScheme[]; onBattery: boolean | null; batteryPresent: boolean | null }
export interface DiskInfo { name: string; mountPoint: string; kind: DiskKind; fileSystem: string; totalBytes: number; availableBytes: number; removable: boolean; isSystem: boolean }
export interface GpuInfo { name: string; driverVersion: string | null; dedicatedMemoryBytes: number | null }
export interface StartupEntry { id: string; name: string; command: string; location: StartupLocation; enabled: boolean; executablePath: string | null; executableExists: boolean | null; publisher: string | null }
export interface ProcessInfo { pid: number; parentPid: number | null; name: string; exe: string | null; cpuPercent: number; memoryBytes: number; virtualMemoryBytes: number; diskReadBytesPerSec: number; diskWriteBytesPerSec: number; status: string; runTimeSecs: number; critical: boolean }
export interface SystemSnapshot {
  capturedAt: string;
  os: { name: string; version: string | null; build: number | null; kernel: string | null; hostname: string | null };
  cpu: { brand: string; vendor: string; arch: string; physicalCores: number | null; logicalCores: number; frequencyMhz: number | null; usagePercent: number };
  memory: { totalBytes: number; usedBytes: number; availableBytes: number; swapTotalBytes: number; swapUsedBytes: number };
  disks: DiskInfo[];
  gpus: GpuInfo[];
  power: PowerInfo | null;
  gameModeEnabled: boolean | null;
  startup: StartupEntry[];
  topProcesses: ProcessInfo[];
  tempCleanableBytes: number | null;
  limitations: string[];
}
export interface LiveSample { timestampMs: number; cpuPercent: number; perCorePercent: number[]; memoryUsedBytes: number; memoryTotalBytes: number; swapUsedBytes: number; netRxBytesPerSec: number; netTxBytesPerSec: number; diskReadBytesPerSec: number; diskWriteBytesPerSec: number }

export type Action =
  | { kind: "setPowerScheme"; guid: string; name: string }
  | { kind: "setStartupEntryEnabled"; entryId: string; name: string; enabled: boolean }
  | { kind: "setGameMode"; enabled: boolean }
  | { kind: "setProcessPriority"; pid: number; name: string; priority: PriorityClass }
  | { kind: "cleanTempFiles"; olderThanDays: number };

export interface Recommendation {
  id: string; title: string; category: Category; confidence: number; risk: Risk;
  expectedBenefit: string; downsides: string; evidence: string[];
  requiresAdmin: boolean; reversible: boolean; rollback: string;
  action: Action | null; requiresConfirmation: boolean; selectedByDefault: boolean;
}

export type Outcome =
  | { outcome: "applied" }
  | { outcome: "rolledBack" }
  | { outcome: "skipped"; message: string }
  | { outcome: "failed"; message: string }
  | { outcome: "notAttempted"; message: string };
export type BatchStatus = "inProgress" | "applied" | "partiallyApplied" | "rolledBack" | "reverted" | "failed";
export type ChangeStatus = "pending" | "applied" | "skipped" | "failed" | "rolledBack" | "reverted" | "needsAttention";
export interface ActionResult { changeId: string | null; description: string; reversible: boolean; outcome: Outcome; detail: string | null }
export interface PlanReport { batchId: string; status: BatchStatus; results: ActionResult[]; summary: string }
export interface UndoReport { changeId: string; description: string; ok: boolean; message: string }
export type SettingValue = { type: "text"; value: string } | { type: "bool"; value: boolean } | { type: "absent" };
export interface ChangeRecord { id: string; batchId: string; seq: number; createdAt: string; action: Action; before: SettingValue | null; after: SettingValue | null; status: ChangeStatus; reversible: boolean; error: string | null; detail: string | null; revertedAt: string | null }
export interface BatchRecord { id: string; createdAt: string; label: string; status: BatchStatus; changes: ChangeRecord[] }

export interface StartupEntryView extends StartupEntry { locationPath: string; requiresAdmin: boolean; protected: boolean; note: string | null }
export interface StartupView { entries: StartupEntryView[]; limitations: string[] }

export interface TempAnalysis { roots: string[]; totalFiles: number; totalBytes: number; eligibleFiles: number; eligibleBytes: number; skippedSymlinks: number; unreadableEntries: number; olderThanDays: number }
export interface FileEntry { path: string; sizeBytes: number; modifiedUnix: number | null }
export interface LargeFileScan { root: string; filesScanned: number; bytesScanned: number; unreadableEntries: number; largest: FileEntry[]; byChild: [string, number][]; truncated: boolean }
export interface DuplicateGroup { sizeBytes: number; paths: string[]; reclaimableBytes: number }
export interface KnownFolders { home: string | null; downloads: string | null; documents: string | null; desktop: string | null; systemDrive: string }

export interface NetInterface { name: string; mac: string; addresses: string[]; mtu: number; state: string; totalReceivedBytes: number; totalTransmittedBytes: number; totalErrors: number }
export interface LatencyResult { target: string; label: string; method: string; sent: number; received: number; lossPercent: number; samplesMs: number[]; meanMs: number | null; medianMs: number | null; minMs: number | null; maxMs: number | null; jitterMs: number | null; errors: string[] }

export type BenchKind = "cpuSingle" | "cpuMulti" | "memoryBandwidth" | "storageSequential";
export interface BenchmarkRecord { id: string; createdAt: string; kind: BenchKind; label: string; unit: string; higherIsBetter: boolean; samples: number[]; context: { warnings?: string[]; temperatureAvailable?: boolean; backgroundCpuPercentBeforeEachRun?: number[]; [k: string]: unknown } }
export interface Stats { n: number; mean: number; median: number; min: number; max: number; stdev: number; cvPercent: number; ci95: number }
export type Verdict = "improved" | "regressed" | "noSignificantChange" | "inconclusive";
export interface Comparison { before: Stats; after: Stats; improvementPercent: number; tStatistic: number; degreesOfFreedom: number; significant: boolean; verdict: Verdict; explanation: string }

export interface DisplayInfo { device: string; adapter: string; primary: boolean; width: number; height: number; currentHz: number; maxHzAtCurrentResolution: number }
export interface GamingStatus { gameModeEnabled: boolean | null; hardwareGpuScheduling: boolean | null; displays: DisplayInfo[]; power: PowerInfo | null; limitations: string[] }
export interface GameProfile { id: string; name: string; powerSchemeGuid: string | null; enableGameMode: boolean; notes: string }
export interface AppInfo { version: string; database: string; platform: string; windows: boolean }

export const inTauri = (): boolean => typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (!inTauri()) throw new Error("APEX's interface must run inside the APEX desktop app.");
  try {
    return await invoke<T>(cmd, args);
  } catch (e) {
    throw new Error(typeof e === "string" ? e : e instanceof Error ? e.message : JSON.stringify(e));
  }
}

export const api = {
  appInfo: () => call<AppInfo>("app_info"),
  snapshot: (refresh = false) => call<SystemSnapshot>("get_snapshot", { refresh }),
  liveSample: () => call<LiveSample>("live_sample"),
  processes: () => call<ProcessInfo[]>("list_processes"),
  terminate: (pid: number) => call<string>("terminate_process", { pid, confirmed: true }),
  setPriority: (pid: number, priority: PriorityClass) => call<PlanReport>("set_process_priority", { pid, priority }),
  temperatures: () => call<[string, number][]>("temperatures"),
  gaming: () => call<GamingStatus>("gaming_status"),
  recommendations: (mode: Mode, refresh = false) => call<Recommendation[]>("get_recommendations", { mode, refresh }),
  applyPlan: (mode: Mode, ids: string[], confirmed: string[]) => call<PlanReport>("apply_plan", { mode, ids, confirmed }),
  history: (limit = 100) => call<BatchRecord[]>("history", { limit }),
  undoChange: (changeId: string, force = false) => call<UndoReport>("undo_change", { changeId, force }),
  undoBatch: (batchId: string, force = false) => call<UndoReport[]>("undo_batch", { batchId, force }),
  undoEverything: () => call<UndoReport[]>("undo_everything"),
  startup: () => call<StartupView>("list_startup"),
  setStartup: (entryId: string, enabled: boolean, confirmed = false) => call<PlanReport>("set_startup_enabled", { entryId, enabled, confirmed }),
  knownFolders: () => call<KnownFolders>("known_folders"),
  tempAnalysis: (days: number) => call<TempAnalysis>("temp_analysis", { days }),
  cleanTemp: (days: number) => call<PlanReport>("clean_temp", { days, confirmed: true }),
  largeFiles: (path: string) => call<LargeFileScan>("scan_large_files", { path }),
  duplicates: (path: string, minSizeMb: number) => call<DuplicateGroup[]>("find_duplicates", { path, minSizeMb }),
  reveal: (path: string) => call<void>("reveal_in_explorer", { path }),
  interfaces: () => call<NetInterface[]>("network_interfaces"),
  knownResolvers: () => call<[string, string][]>("known_resolvers"),
  dnsCompare: (resolvers: { ip: string; label: string }[], rounds: number) => call<LatencyResult[]>("dns_compare", { resolvers, rounds }),
  latency: (target: string, count: number) => call<LatencyResult>("latency_test", { target, count }),
  runBenchmark: (kind: BenchKind, runs: number, label: string) => call<BenchmarkRecord>("run_benchmark", { kind, runs, label }),
  benchmarks: (kind?: BenchKind) => call<BenchmarkRecord[]>("list_benchmarks", { kind: kind ?? null }),
  compare: (beforeId: string, afterId: string) => call<Comparison>("compare_benchmarks", { beforeId, afterId }),
  gameProfiles: () => call<GameProfile[]>("get_game_profiles"),
  saveGameProfiles: (profiles: GameProfile[]) => call<void>("save_game_profiles", { profiles }),
  startSession: (profileId: string) => call<PlanReport>("start_game_session", { profileId }),
  getSetting: (key: string) => call<string | null>("get_setting", { key }),
  setSetting: (key: string, value: string) => call<void>("set_setting", { key, value }),
  exportReport: () => call<string>("export_report"),
  onBenchProgress: (cb: (p: { kind: BenchKind; done: number; total: number }) => void): Promise<UnlistenFn> =>
    inTauri() ? listen<{ kind: BenchKind; done: number; total: number }>("bench-progress", (e) => cb(e.payload)) : Promise.resolve(() => {}),
};
