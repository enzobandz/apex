//! Tauri command layer. This is the trust boundary between the web UI and the
//! system: the UI may only *name* recommendations, entries and ids; every action is
//! re-derived and validated here before the executor touches the system.

use apex_core::actions::{Action, PriorityClass, SystemBackend};
use apex_core::bench::{self, BenchKind, Comparison};
use apex_core::executor::{Executor, PlanReport, UndoReport};
use apex_core::ledger::{BatchRecord, BenchmarkRecord, Ledger};
use apex_core::model::*;
use apex_core::net::{self, LatencyResult};
use apex_core::recommend::{recommend, Mode, Recommendation};
use apex_core::safety;
use apex_core::storage::{self, DuplicateGroup, LargeFileScan, TempAnalysis};
use apex_platform::benchrunner::{run_series, SeriesOptions};
use apex_platform::{collect_snapshot, Monitor};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tauri::{Emitter, Manager, State};

type CmdResult<T> = Result<T, String>;

pub struct AppState {
    ledger: Ledger,
    backend: Box<dyn SystemBackend>,
    monitor: Mutex<Monitor>,
    snapshot: Mutex<Option<SystemSnapshot>>,
    db_path: PathBuf,
    /// Serialises all system-modifying operations.
    write_lock: Mutex<()>,
}

type S<'a> = State<'a, Arc<AppState>>;

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> CmdResult<T> + Send + 'static,
) -> CmdResult<T> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(err)?
}

impl AppState {
    fn exec(&self) -> Executor<'_> {
        Executor::new(self.backend.as_ref(), &self.ledger)
    }
    fn fresh_snapshot(&self) -> SystemSnapshot {
        let s = collect_snapshot(&mut self.monitor.lock(), self.backend.as_ref());
        *self.snapshot.lock() = Some(s.clone());
        s
    }
    fn current_snapshot(&self) -> SystemSnapshot {
        if let Some(s) = self.snapshot.lock().clone() {
            if (chrono::Utc::now() - s.captured_at).num_seconds() < 120 {
                return s;
            }
        }
        self.fresh_snapshot()
    }
}

// ------------------------------------------------------------------ diagnostics

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AppInfo {
    version: String,
    database: String,
    platform: String,
    windows: bool,
}

#[tauri::command]
fn app_info(state: S<'_>) -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION").into(),
        database: state.db_path.display().to_string(),
        platform: std::env::consts::OS.into(),
        windows: cfg!(windows),
    }
}

#[tauri::command]
async fn get_snapshot(state: S<'_>, refresh: bool) -> CmdResult<SystemSnapshot> {
    let st = state.inner().clone();
    blocking(move || {
        Ok(if refresh {
            st.fresh_snapshot()
        } else {
            st.current_snapshot()
        })
    })
    .await
}

#[tauri::command]
async fn live_sample(state: S<'_>) -> CmdResult<LiveSample> {
    let st = state.inner().clone();
    blocking(move || Ok(st.monitor.lock().sample())).await
}

#[tauri::command]
async fn list_processes(state: S<'_>) -> CmdResult<Vec<ProcessInfo>> {
    let st = state.inner().clone();
    blocking(move || Ok(st.monitor.lock().processes())).await
}

#[tauri::command]
async fn terminate_process(state: S<'_>, pid: u32, confirmed: bool) -> CmdResult<String> {
    if !confirmed {
        return Err("Termination must be confirmed.".into());
    }
    let st = state.inner().clone();
    blocking(move || st.monitor.lock().terminate(pid)).await
}

#[tauri::command]
async fn set_process_priority(state: S<'_>, pid: u32, priority: String) -> CmdResult<PlanReport> {
    let class = PriorityClass::parse(&priority)
        .ok_or("Unsupported priority (realtime is never allowed).")?;
    let st = state.inner().clone();
    blocking(move || {
        let name = apex_platform::process_name(pid).ok_or("Process not found.")?;
        if safety::is_critical_process(&name, pid) {
            return Err(format!("{name} is protected."));
        }
        let _g = st.write_lock.lock();
        st.exec()
            .apply_plan(
                &format!("Priority: {name}"),
                &[Action::SetProcessPriority {
                    pid,
                    name,
                    priority: class,
                }],
            )
            .map_err(err)
    })
    .await
}

#[tauri::command]
async fn temperatures() -> CmdResult<Vec<(String, f32)>> {
    blocking(|| Ok(apex_platform::metrics::temperatures())).await
}

#[tauri::command]
async fn gaming_status() -> CmdResult<GamingStatus> {
    blocking(|| Ok(apex_platform::gaming_status())).await
}

// ------------------------------------------------------------------ optimizer

fn parse_mode(m: &str) -> CmdResult<Mode> {
    match m {
        "balanced" => Ok(Mode::Balanced),
        "gaming" => Ok(Mode::Gaming),
        "maximumPerformance" => Ok(Mode::MaximumPerformance),
        _ => Err(format!("unknown mode {m}")),
    }
}

#[tauri::command]
async fn get_recommendations(
    state: S<'_>,
    mode: String,
    refresh: bool,
) -> CmdResult<Vec<Recommendation>> {
    let mode = parse_mode(&mode)?;
    let st = state.inner().clone();
    blocking(move || {
        let s = if refresh {
            st.fresh_snapshot()
        } else {
            st.current_snapshot()
        };
        Ok(recommend(&s, mode))
    })
    .await
}

/// Apply recommendations chosen by id. Actions are re-derived from the cached
/// snapshot server-side; items needing confirmation must be listed in `confirmed`.
#[tauri::command]
async fn apply_plan(
    state: S<'_>,
    mode: String,
    ids: Vec<String>,
    confirmed: Vec<String>,
) -> CmdResult<PlanReport> {
    let mode = parse_mode(&mode)?;
    let st = state.inner().clone();
    blocking(move || {
        let recs = recommend(&st.current_snapshot(), mode);
        let mut actions = Vec::new();
        for id in &ids {
            let r = recs.iter().find(|r| &r.id == id).ok_or_else(|| {
                format!("\"{id}\" is no longer recommended; refresh and try again.")
            })?;
            let a = r
                .action
                .clone()
                .ok_or_else(|| format!("\"{}\" is advice only.", r.title))?;
            if r.requires_confirmation && !confirmed.contains(id) {
                return Err(format!("\"{}\" requires explicit confirmation.", r.title));
            }
            actions.push(a);
        }
        let label = match mode {
            Mode::Balanced => "Smart Optimizer · Balanced",
            Mode::Gaming => "Smart Optimizer · Gaming",
            Mode::MaximumPerformance => "Smart Optimizer · Maximum Performance",
        };
        let _g = st.write_lock.lock();
        let rep = st.exec().apply_plan(label, &actions).map_err(err)?;
        *st.snapshot.lock() = None; // force re-measure
        Ok(rep)
    })
    .await
}

#[tauri::command]
async fn history(state: S<'_>, limit: usize) -> CmdResult<Vec<BatchRecord>> {
    let st = state.inner().clone();
    blocking(move || st.ledger.history(limit.min(1000)).map_err(err)).await
}

#[tauri::command]
async fn undo_change(state: S<'_>, change_id: String, force: bool) -> CmdResult<UndoReport> {
    let st = state.inner().clone();
    blocking(move || {
        let _g = st.write_lock.lock();
        let r = st.exec().undo_change(&change_id, force).map_err(err);
        *st.snapshot.lock() = None;
        r
    })
    .await
}

#[tauri::command]
async fn undo_batch(state: S<'_>, batch_id: String, force: bool) -> CmdResult<Vec<UndoReport>> {
    let st = state.inner().clone();
    blocking(move || {
        let _g = st.write_lock.lock();
        let r = st.exec().undo_batch(&batch_id, force).map_err(err);
        *st.snapshot.lock() = None;
        r
    })
    .await
}

/// Undo every reversible change APEX still has applied (e.g. before uninstalling).
#[tauri::command]
async fn undo_everything(state: S<'_>) -> CmdResult<Vec<UndoReport>> {
    let st = state.inner().clone();
    blocking(move || {
        let _g = st.write_lock.lock();
        let ex = st.exec();
        let mut out = Vec::new();
        for b in st.ledger.history(10_000).map_err(err)? {
            out.extend(ex.undo_batch(&b.id, false).map_err(err)?);
        }
        *st.snapshot.lock() = None;
        Ok(out)
    })
    .await
}

// ------------------------------------------------------------------ startup

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StartupView {
    entries: Vec<StartupEntryView>,
    limitations: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StartupEntryView {
    #[serde(flatten)]
    entry: StartupEntry,
    location_path: &'static str,
    requires_admin: bool,
    protected: bool,
    note: Option<&'static str>,
}

#[tauri::command]
async fn list_startup() -> CmdResult<StartupView> {
    blocking(|| {
        let (entries, limitations) = apex_platform::startup_entries();
        Ok(StartupView {
            entries: entries
                .into_iter()
                .map(|e| StartupEntryView {
                    location_path: e.location.describe(),
                    requires_admin: e.location.requires_admin(),
                    protected: safety::is_protected_startup(&e.name, &e.command),
                    note: safety::on_demand_reason(&e.name, &e.command),
                    entry: e,
                })
                .collect(),
            limitations,
        })
    })
    .await
}

#[tauri::command]
async fn set_startup_enabled(
    state: S<'_>,
    entry_id: String,
    enabled: bool,
    confirmed: bool,
) -> CmdResult<PlanReport> {
    let st = state.inner().clone();
    blocking(move || {
        let (entries, _) = apex_platform::startup_entries();
        let e = entries.iter().find(|e| e.id == entry_id).ok_or("Startup entry not found; refresh the list.")?;
        if !enabled && safety::is_protected_startup(&e.name, &e.command) && !confirmed {
            return Err(format!("\"{}\" looks like hardware or security software. Disabling it may break a device feature or protection; confirm to proceed.", e.name));
        }
        let _g = st.write_lock.lock();
        let r = st
            .exec()
            .apply_plan(
                "Startup Manager",
                &[Action::SetStartupEntryEnabled { entry_id: e.id.clone(), name: e.name.clone(), enabled }],
            )
            .map_err(err);
        *st.snapshot.lock() = None;
        r
    })
    .await
}

// ------------------------------------------------------------------ storage

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct KnownFolders {
    home: Option<String>,
    downloads: Option<String>,
    documents: Option<String>,
    desktop: Option<String>,
    system_drive: String,
}

#[tauri::command]
fn known_folders(app: tauri::AppHandle) -> KnownFolders {
    let p = app.path();
    let s = |r: tauri::Result<PathBuf>| r.ok().map(|p| p.display().to_string());
    KnownFolders {
        home: s(p.home_dir()),
        downloads: s(p.download_dir()),
        documents: s(p.document_dir()),
        desktop: s(p.desktop_dir()),
        system_drive: apex_platform::metrics::system_mount_point(),
    }
}

fn validate_dir(path: &str) -> CmdResult<PathBuf> {
    let p = PathBuf::from(path);
    if !p.is_absolute() || !p.is_dir() {
        return Err(format!("{path} is not an existing folder."));
    }
    Ok(p)
}

#[tauri::command]
async fn temp_analysis(state: S<'_>, days: u32) -> CmdResult<TempAnalysis> {
    let st = state.inner().clone();
    blocking(move || {
        Ok(storage::analyze_temp(
            &st.backend.temp_roots(),
            days.clamp(1, 365),
        ))
    })
    .await
}

#[tauri::command]
async fn clean_temp(state: S<'_>, days: u32, confirmed: bool) -> CmdResult<PlanReport> {
    if !confirmed {
        return Err("Deleting files is not reversible and must be confirmed.".into());
    }
    let st = state.inner().clone();
    blocking(move || {
        let _g = st.write_lock.lock();
        let r = st
            .exec()
            .apply_plan(
                "Storage Cleaner",
                &[Action::CleanTempFiles {
                    older_than_days: days.clamp(1, 365),
                }],
            )
            .map_err(err);
        *st.snapshot.lock() = None;
        r
    })
    .await
}

#[tauri::command]
async fn scan_large_files(path: String) -> CmdResult<LargeFileScan> {
    let p = validate_dir(&path)?;
    blocking(move || storage::scan_large_files(&p, 50, 3_000_000).map_err(err)).await
}

#[tauri::command]
async fn find_duplicates(path: String, min_size_mb: u64) -> CmdResult<Vec<DuplicateGroup>> {
    let p = validate_dir(&path)?;
    blocking(move || {
        storage::find_duplicates(&p, min_size_mb.max(1) * 1024 * 1024, 500_000).map_err(err)
    })
    .await
}

/// Opens Explorer with the file selected so the *user* decides what to delete.
#[tauri::command]
fn reveal_in_explorer(path: String) -> CmdResult<()> {
    let p = PathBuf::from(&path);
    if !p.is_absolute() || !p.exists() {
        return Err("File not found.".into());
    }
    #[cfg(windows)]
    {
        let root = std::env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into());
        std::process::Command::new(PathBuf::from(root).join("explorer.exe"))
            .arg(format!("/select,{}", p.display()))
            .spawn()
            .map(|_| ())
            .map_err(err)
    }
    #[cfg(not(windows))]
    {
        Err("Only supported on Windows.".into())
    }
}

// ------------------------------------------------------------------ network

#[tauri::command]
async fn network_interfaces() -> CmdResult<Vec<NetInterface>> {
    blocking(|| Ok(apex_platform::metrics::network_interfaces())).await
}

#[derive(Deserialize)]
struct ResolverInput {
    ip: String,
    label: String,
}

#[tauri::command]
async fn dns_compare(resolvers: Vec<ResolverInput>, rounds: u32) -> CmdResult<Vec<LatencyResult>> {
    if resolvers.is_empty() || resolvers.len() > 12 {
        return Err("Choose between 1 and 12 resolvers.".into());
    }
    let parsed: Vec<(IpAddr, String)> = resolvers
        .into_iter()
        .map(|r| {
            r.ip.trim()
                .parse::<IpAddr>()
                .map(|ip| (ip, r.label))
                .map_err(|_| format!("{} is not an IP address", r.ip))
        })
        .collect::<Result<_, _>>()?;
    let rounds = rounds.clamp(1, 10);
    blocking(move || {
        let handles: Vec<_> = parsed
            .into_iter()
            .map(|(ip, label)| {
                std::thread::spawn(move || {
                    net::dns_test(
                        ip,
                        &label,
                        net::TEST_NAMES,
                        rounds,
                        Duration::from_millis(1500),
                    )
                })
            })
            .collect();
        Ok(handles.into_iter().filter_map(|h| h.join().ok()).collect())
    })
    .await
}

#[tauri::command]
fn known_resolvers() -> Vec<(String, String)> {
    net::KNOWN_RESOLVERS
        .iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect()
}

#[tauri::command]
async fn latency_test(target: String, count: u32) -> CmdResult<LatencyResult> {
    let addr: SocketAddr = target
        .trim()
        .parse()
        .map_err(|_| "Enter an IP address and port, e.g. 1.1.1.1:443".to_string())?;
    blocking(move || {
        Ok(net::tcp_latency(
            addr,
            &target,
            count.clamp(3, 50),
            Duration::from_secs(2),
        ))
    })
    .await
}

// ------------------------------------------------------------------ benchmarks

fn parse_bench(k: &str) -> CmdResult<BenchKind> {
    Ok(match k {
        "cpuSingle" => BenchKind::CpuSingle,
        "cpuMulti" => BenchKind::CpuMulti,
        "memoryBandwidth" => BenchKind::MemoryBandwidth,
        "storageSequential" => BenchKind::StorageSequential,
        _ => return Err(format!("unknown benchmark {k}")),
    })
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct BenchProgress {
    kind: String,
    done: usize,
    total: usize,
}

#[tauri::command]
async fn run_benchmark(
    app: tauri::AppHandle,
    state: S<'_>,
    kind: String,
    runs: usize,
    label: String,
) -> CmdResult<BenchmarkRecord> {
    let k = parse_bench(&kind)?;
    let st = state.inner().clone();
    blocking(move || {
        let dir = std::env::temp_dir();
        let label = if label.trim().is_empty() {
            "Untitled".to_string()
        } else {
            label.chars().take(80).collect()
        };
        let rec = run_series(
            &SeriesOptions {
                kind: k,
                runs,
                label,
                storage_dir: Path::new(&dir),
            },
            |done, total| {
                let _ = app.emit(
                    "bench-progress",
                    BenchProgress {
                        kind: kind.clone(),
                        done,
                        total,
                    },
                );
            },
        )
        .map_err(err)?;
        st.ledger.save_benchmark(&rec).map_err(err)?;
        Ok(rec)
    })
    .await
}

#[tauri::command]
async fn list_benchmarks(state: S<'_>, kind: Option<String>) -> CmdResult<Vec<BenchmarkRecord>> {
    let st = state.inner().clone();
    blocking(move || st.ledger.benchmarks(kind.as_deref(), 500).map_err(err)).await
}

#[tauri::command]
async fn compare_benchmarks(
    state: S<'_>,
    before_id: String,
    after_id: String,
) -> CmdResult<Comparison> {
    let st = state.inner().clone();
    blocking(move || {
        let b = st.ledger.get_benchmark(&before_id).map_err(err)?;
        let a = st.ledger.get_benchmark(&after_id).map_err(err)?;
        if a.kind != b.kind {
            return Err("Only results of the same benchmark can be compared.".into());
        }
        bench::compare(&b.samples, &a.samples, b.higher_is_better)
            .ok_or_else(|| "Empty result.".into())
    })
    .await
}

// ------------------------------------------------------------------ game profiles

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct GameProfile {
    id: String,
    name: String,
    /// Power scheme to use during the session (must be installed), or none to leave unchanged.
    power_scheme_guid: Option<String>,
    enable_game_mode: bool,
    notes: String,
}

fn load_profiles(l: &Ledger) -> CmdResult<Vec<GameProfile>> {
    Ok(l.kv_get("game_profiles")
        .map_err(err)?
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default())
}

#[tauri::command]
async fn get_game_profiles(state: S<'_>) -> CmdResult<Vec<GameProfile>> {
    let st = state.inner().clone();
    blocking(move || load_profiles(&st.ledger)).await
}

#[tauri::command]
async fn save_game_profiles(state: S<'_>, profiles: Vec<GameProfile>) -> CmdResult<()> {
    if profiles.len() > 200 {
        return Err("Too many profiles.".into());
    }
    for p in &profiles {
        if let Some(g) = &p.power_scheme_guid {
            if !apex_platform::powercfg_parse::is_guid(g) {
                return Err(format!("Invalid power plan in profile {}", p.name));
            }
        }
    }
    let st = state.inner().clone();
    blocking(move || {
        st.ledger
            .kv_set(
                "game_profiles",
                &serde_json::to_string(&profiles).map_err(err)?,
            )
            .map_err(err)
    })
    .await
}

/// Apply a profile as one reversible batch. End the session with `undo_batch`.
#[tauri::command]
async fn start_game_session(state: S<'_>, profile_id: String) -> CmdResult<PlanReport> {
    let st = state.inner().clone();
    blocking(move || {
        let p = load_profiles(&st.ledger)?
            .into_iter()
            .find(|p| p.id == profile_id)
            .ok_or("Profile not found.")?;
        let mut actions = Vec::new();
        if let Some(g) = &p.power_scheme_guid {
            let name = st
                .current_snapshot()
                .power
                .and_then(|pi| {
                    pi.available
                        .into_iter()
                        .find(|s| s.guid.eq_ignore_ascii_case(g))
                        .map(|s| s.name)
                })
                .ok_or("The profile's power plan is not installed on this PC.")?;
            actions.push(Action::SetPowerScheme {
                guid: g.clone(),
                name,
            });
        }
        if p.enable_game_mode {
            actions.push(Action::SetGameMode { enabled: true });
        }
        if actions.is_empty() {
            return Err("This profile doesn't change anything.".into());
        }
        let _g = st.write_lock.lock();
        let r = st
            .exec()
            .apply_plan(&format!("Game session: {}", p.name), &actions)
            .map_err(err);
        *st.snapshot.lock() = None;
        r
    })
    .await
}

// ------------------------------------------------------------------ settings & export

const ALLOWED_SETTINGS: &[&str] = &["theme", "liveIntervalMs", "defaultMode"];

#[tauri::command]
async fn get_setting(state: S<'_>, key: String) -> CmdResult<Option<String>> {
    if !ALLOWED_SETTINGS.contains(&key.as_str()) {
        return Err("unknown setting".into());
    }
    let st = state.inner().clone();
    blocking(move || st.ledger.kv_get(&format!("setting.{key}")).map_err(err)).await
}

#[tauri::command]
async fn set_setting(state: S<'_>, key: String, value: String) -> CmdResult<()> {
    if !ALLOWED_SETTINGS.contains(&key.as_str()) || value.len() > 200 {
        return Err("invalid setting".into());
    }
    let st = state.inner().clone();
    blocking(move || {
        st.ledger
            .kv_set(&format!("setting.{key}"), &value)
            .map_err(err)
    })
    .await
}

/// Writes a JSON report to the user's Downloads folder. Only runs when the user clicks Export.
#[tauri::command]
async fn export_report(app: tauri::AppHandle, state: S<'_>) -> CmdResult<String> {
    let dir = app
        .path()
        .download_dir()
        .or_else(|_| app.path().document_dir())
        .map_err(err)?;
    let st = state.inner().clone();
    blocking(move || {
        let report = serde_json::json!({
            "generatedBy": format!("APEX {}", env!("CARGO_PKG_VERSION")),
            "generatedAt": chrono::Utc::now().to_rfc3339(),
            "snapshot": st.current_snapshot(),
            "history": st.ledger.history(200).map_err(err)?,
            "benchmarks": st.ledger.benchmarks(None, 500).map_err(err)?,
        });
        std::fs::create_dir_all(&dir).map_err(err)?;
        let path = dir.join(format!(
            "APEX-report-{}.json",
            chrono::Local::now().format("%Y%m%d-%H%M%S")
        ));
        std::fs::write(&path, serde_json::to_vec_pretty(&report).map_err(err)?).map_err(err)?;
        Ok(path.display().to_string())
    })
    .await
}

// ------------------------------------------------------------------ app

fn init_logging(dir: &Path) {
    use tracing_subscriber::{fmt, EnvFilter};
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("apex.log"));
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    match file {
        Ok(f) => {
            let _ = fmt()
                .json()
                .with_env_filter(filter)
                .with_writer(std::sync::Mutex::new(f))
                .try_init();
        }
        Err(_) => {
            let _ = fmt().with_env_filter(filter).try_init();
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let dir = app.path().app_local_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            init_logging(&dir);
            let db_path = dir.join("apex.db");
            let ledger = Ledger::open(&db_path)
                .map_err(|e| Box::<dyn std::error::Error>::from(e.to_string()))?;
            let backend = apex_platform::backend();
            // Crash recovery: resolve any change interrupted mid-write.
            match Executor::new(backend.as_ref(), &ledger).recover() {
                Ok(r) if !r.is_empty() => {
                    tracing::warn!(count = r.len(), "recovered interrupted changes")
                }
                Ok(_) => {}
                Err(e) => tracing::error!(error = %e, "crash recovery failed"),
            }
            let mut monitor = Monitor::new();
            let _ = monitor.sample();
            app.manage(Arc::new(AppState {
                ledger,
                backend,
                monitor: Mutex::new(monitor),
                snapshot: Mutex::new(None),
                db_path,
                write_lock: Mutex::new(()),
            }));
            tracing::info!("APEX started");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_info,
            get_snapshot,
            live_sample,
            list_processes,
            terminate_process,
            set_process_priority,
            temperatures,
            gaming_status,
            get_recommendations,
            apply_plan,
            history,
            undo_change,
            undo_batch,
            undo_everything,
            list_startup,
            set_startup_enabled,
            known_folders,
            temp_analysis,
            clean_temp,
            scan_large_files,
            find_duplicates,
            reveal_in_explorer,
            network_interfaces,
            dns_compare,
            known_resolvers,
            latency_test,
            run_benchmark,
            list_benchmarks,
            compare_benchmarks,
            get_game_profiles,
            save_game_profiles,
            start_game_session,
            get_setting,
            set_setting,
            export_report,
        ])
        .run(tauri::generate_context!())
        .expect("error while running APEX");
}
