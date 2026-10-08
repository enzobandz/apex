# APEX architecture

## Goals that drove the design

1. **Every change is reversible and verified** — the executor reads the original value, records intent, writes, reads back, and auto-rolls-back the batch on any failure.
2. **Only measured claims** — recommendations fire on facts from the snapshot; benefits are worded as expectations, and the Benchmark Lab decides whether anything actually improved.
3. **Least privilege** — the app runs unelevated. Everything it changes today lives in HKCU, `powercfg`'s per-user active scheme, or the user's own processes and Temp folder. Machine-wide items are shown but refused with `RequiresAdmin`.
4. **Zero bloat** — no service, no tray agent, no telemetry. Sampling stops when the window is hidden.

## Layout

```
crates/
  apex-core/       platform-independent engine (fully unit-tested on any OS)
    model.rs       snapshot / process / startup / display types (serde, camelCase)
    actions.rs     Action, Setting, SettingValue, SystemBackend trait
    executor.rs    transactional apply → verify → rollback, undo, crash recovery
    ledger.rs      SQLite (WAL, synchronous=FULL): batches, changes, benchmarks, settings
    recommend.rs   deterministic rule engine (category, confidence, risk, rollback)
    bench.rs       workloads + statistics (Welch t-test, 95% CI, practical threshold)
    storage.rs     temp analysis/cleanup (root-confined, no symlink following), large files, duplicates
    net.rs         raw DNS-over-UDP timing per resolver, TCP connect latency/jitter/loss
    safety.rs      protected processes, hardware/security startup items
  apex-platform/   real measurements + Windows backend
    metrics.rs     sysinfo: live samples, processes, disks, networks, temperatures
    windows/       winreg + Win32: StartupApproved, Game Mode, powercfg, priority, GPUs, displays
    benchrunner.rs runs series with warm-up, background-load and temperature context
  apex-cli/        `apex` command line over the same engine
apps/desktop/
  src-tauri/       Tauri 2 command layer = trust boundary
  src/             React + strict TypeScript UI (no UI framework; own design system)
```

## Trust boundary

The web UI never sends actions. It sends **recommendation ids**, **startup entry ids**, **PIDs** or **benchmark kinds**; `src-tauri/src/lib.rs` re-derives the concrete `Action` from a fresh/cached snapshot, checks confirmation requirements and safety policy, and only then calls the executor. All filesystem paths from the UI must be absolute existing directories and are used read-only (scans), except the Temp root, which comes from the backend, not the UI. The Tauri capability file grants only `core:default`; there is no shell, fs or http plugin.

All writes are serialised with a process-wide lock.

## The transaction model

```
for each reversible action (in order):
    before = read(setting)
    if before == target: record Skipped; continue
    record Pending(before, target)          ← crash-safe intent
    write(target); verify read == target
    on failure: restore this one, then restore all applied in reverse, stop
irreversible actions (temp cleanup) run only after every reversible one succeeded
```

`SettingValue::Absent` captures "value did not exist", so undo deletes what APEX created instead of leaving a value behind. Undo refuses (unless forced) if the setting was changed by something else since — APEX won't silently overwrite your later choices. On start-up `Executor::recover` resolves any `Pending` rows by reading current state.

## Windows integration choices

| Feature | Mechanism | Why |
|---|---|---|
| Startup enable/disable | `Explorer\StartupApproved\{Run,Run32,StartupFolder}` binary flag | Same as Task Manager; never deletes the Run value |
| Power plan | `%SystemRoot%\System32\powercfg.exe /list`, `/setactive <guid>` | Documented; GUID strictly validated and must be installed; locale-independent parsing |
| Game Mode | `HKCU\Software\Microsoft\GameBar\AutoGameModeEnabled` | Same value the Settings toggle writes |
| Priority | `OpenProcess` + `Get/SetPriorityClass` | Realtime never allowed; protected processes refused |
| GPUs | Display class key `{4d36e968-…}` `DriverDesc`, `HardwareInformation.qwMemorySize` | Readable unelevated |
| Displays | `EnumDisplayDevicesW` / `EnumDisplaySettingsW` | Current vs max refresh at current resolution |
| HAGS | `GraphicsDrivers\HwSchMode` (read-only) | Information only |
| Battery | `GetSystemPowerStatus` | |

Deliberately **not** done: registry "tweaks" without documentation (network throttling index, `Win32PrioritySeparation`, TCP auto-tuning), disabling services, RAM "cleaning", hidden power plan creation, Defender/Firewall/Update changes.

## Competitive assessment (summary)

* **Process Lasso** — excellent priority/affinity automation (ProBalance), but its value is hard to verify without measurement; APEX pairs every change with a benchmark comparison.
* **Razer Cortex** — "boost" that kills processes and clears RAM; effects are largely cosmetic. APEX never auto-kills or purges memory.
* **Microsoft PC Manager** — safe but shallow (temp cleanup, startup); no measurement or history.
* **Chris Titus WinUtil** — powerful, but many tweaks are irreversible, service-disabling, or undocumented. APEX takes the opposite stance: fewer, documented, transactional changes.
* **Autoruns / Sysinternals** — the reference for startup visibility; APEX covers the common user/machine locations with safer toggles, not every extensibility point.

## Known limitations / next steps

1. **Elevation helper**: a signed, per-operation elevated helper (`runas` → `apex-cli apply-setting` with a single validated JSON action) for HKLM startup entries and Windows restore points.
2. **Frame-time capture**: integrate PresentMon (ETW `Microsoft-Windows-DxgKrnl`) for FPS, 1% lows and stutter detection; requires admin or *Performance Log Users*.
3. **Publisher & signature**: `GetFileVersionInfoW` and `WinVerifyTrust` for startup items and processes; `.lnk` target resolution via `IShellLinkW`.
4. **Network**: link speed / Wi-Fi RSSI (`GetIfTable2`, `WlanQueryInterface`), ICMP via `IcmpSendEcho2`.
5. **Benchmarks**: boot time from `Microsoft-Windows-Diagnostics-Performance` event 100, app-launch timing via `WaitForInputIdle`, uncached storage reads with `FILE_FLAG_NO_BUFFERING`.
6. **SSD health**: `IOCTL_STORAGE_QUERY_PROPERTY` / NVMe health log (admin).
