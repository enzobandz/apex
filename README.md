# APEX — evidence-based Windows performance optimizer

APEX diagnoses what is slowing a Windows PC, applies only documented and **reversible** changes, verifies each one, and lets you **measure** whether it helped. Free, no ads, no accounts, no telemetry, no background service.

Rust engine · Tauri 2 desktop shell · React + strict TypeScript UI · SQLite history.

## What it does today

| Area | Implemented |
|---|---|
| **Dashboard** | Live CPU / memory / disk / network, health summary, top opportunities, recent changes, latest benchmarks |
| **Smart Optimizer** | Balanced / Gaming / Maximum-performance modes. Each recommendation shows category (verified problem, likely bottleneck, optional, experimental), confidence, risk, evidence, downsides and rollback. Batch apply with automatic rollback on failure |
| **Gaming Center** | Game Mode, GPU scheduling, power plan, per-display current vs. maximum refresh rate, background load, **game profiles** that apply settings as one reversible session |
| **Performance Monitor** | Live charts, per-core load, page file, OS-reported temperatures (when available) |
| **Startup Manager** | HKCU/HKLM Run (+WOW64) and Startup folders; Task-Manager-compatible enable/disable; missing-program and started-twice detection; hardware/security items flagged |
| **Process Explorer** | Sortable list with CPU, memory, disk I/O; end process and priority changes with protection for system/security processes (realtime never offered) |
| **Storage Cleaner** | Drive overview, user-Temp cleanup (age-based, root-confined, never follows links), space explorer, byte-exact duplicate finder (report only) |
| **Network Center** | Adapters, DNS resolver comparison with real queries, TCP latency / jitter / loss |
| **System Diagnostics** | OS build, CPU, RAM, GPUs, drives, power; explicit list of what couldn't be measured; JSON export |
| **Benchmark Lab** | CPU single/multi, memory bandwidth, flushed storage write; warm-up run, background-load and temperature context; Welch t-test comparison that only reports "improved" when beyond noise **and** ≥2% |
| **Optimization History** | Timeline of every change with original/new values; per-change and per-batch undo; conflict detection; "restore everything" before uninstall |

Not yet implemented (see [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md#known-limitations--next-steps)): live FPS / frame-time capture, elevation for machine-wide changes and restore points, publisher/signature checks, Wi-Fi signal & link speed, boot-time and app-launch benchmarks, SSD SMART health. The UI states these limitations instead of showing placeholder data.

## Safety rules (enforced in code)

* Never disables Defender, Firewall, Windows Update or any service; no undocumented registry tweaks; no RAM "cleaning".
* Every system change: read original → record intent → write → read back. A failed step rolls back the whole batch.
* Undo restores the exact original, including "value did not exist". Undo refuses to overwrite a setting you changed elsewhere unless you confirm.
* Interrupted changes are reconciled on next launch (crash recovery).
* File deletion is limited to old files in your user Temp folder, after confirmation, and is clearly marked as not reversible.
* Runs without administrator rights; installs per-user.

## Install

**Download** the installer artifact (`APEX_x.y.z_x64-setup.exe` or `.msi`) from the latest successful **CI** run or a GitHub Release. Builds are unsigned unless a code-signing certificate is configured, so Windows SmartScreen will warn — check the published SHA-256.

### Build it yourself (Windows 10/11)

1. Install [Rust](https://rustup.rs) (stable, MSVC), [Node.js 22](https://nodejs.org), and *Microsoft C++ Build Tools* (“Desktop development with C++”). WebView2 ships with Windows 11.
2. ```powershell
   cd apps\desktop
   npm ci
   npx tauri build        # installers land in target\release\bundle\{nsis,msi}
   ```
   For development: `npx tauri dev`.

### Get an installer without a Windows dev setup

Push this repository to GitHub. The **CI** workflow builds and tests on `windows-latest` and uploads `apex-windows-installers` as an artifact. Pushing a tag `v*` runs **Release**, which creates a draft release with installers and checksums (signed if `WINDOWS_CERTIFICATE` / `WINDOWS_CERTIFICATE_PASSWORD` secrets are set).

## CLI

The same engine is available as `apex` (`cargo run -p apex-cli -- …`):

```
apex snapshot                     # measured diagnostics
apex recommend gaming             # recommendations with evidence
apex apply storage.clean-temp --yes
apex history                      # batches and change ids
apex undo <batch-id>              # or: apex undo --change <change-id>
apex bench cpu-single --runs 5 --label before
apex compare <before-id> <after-id>
apex dns                          # resolver response times
```

## Development & testing

```
cargo test                         # engine + platform tests (any OS)
cargo clippy --all-targets -- -D warnings
cd apps/desktop && npm test && npm run typecheck
```

On Windows, `cargo test -p apex-platform` additionally runs integration tests against the real registry, `powercfg` and process APIs; every test restores what it changes.

## License

MIT
