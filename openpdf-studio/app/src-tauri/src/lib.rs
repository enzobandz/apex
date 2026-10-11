//! OpenPDF Studio desktop shell.

mod commands;
#[cfg(debug_assertions)]
mod e2e;
mod grants;
mod settings;

use grants::{FileRef, Grants, Mode};
use opdf_core::recovery::{self, Recoverable, SessionDir};
use opdf_core::session::DocSession;
use parking_lot::Mutex;
use settings::Settings;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tauri::{Emitter, Manager, RunEvent, WindowEvent};

pub struct AppState {
    pub docs: Mutex<HashMap<u64, Arc<Mutex<DocSession>>>>,
    next_id: AtomicU64,
    pub grants: Grants,
    pub settings: Settings,
    pub recovery_root: PathBuf,
    pub session_dir: Option<SessionDir>,
    /// Recoverable documents found at start-up, keyed by opaque id.
    pub recoverable: Mutex<Vec<(String, Recoverable)>>,
    pub startup_files: Mutex<Vec<FileRef>>,
}

impl AppState {
    pub fn next_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    /// Users can turn off on-disk recovery copies in Settings.
    pub fn recovery_enabled(&self) -> bool {
        self.settings
            .ui()
            .get("recovery")
            .and_then(|v| v.as_bool())
            .unwrap_or(true)
    }
}

/// Turn command-line arguments into read grants for existing files.
fn files_from_args(state: &AppState, args: &[String], cwd: Option<&Path>) -> Vec<FileRef> {
    args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|a| {
            let p = PathBuf::from(a);
            match (p.is_relative(), cwd) {
                (true, Some(c)) => c.join(p),
                _ => p,
            }
        })
        .filter(|p| p.is_file())
        .map(|p| state.grants.grant(&p, Mode::Read))
        .filter(|f| f.kind == "pdf" || f.kind == "image")
        .collect()
}

fn deliver_files(app: &tauri::AppHandle, files: Vec<FileRef>) {
    if files.is_empty() {
        return;
    }
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
    let _ = app.emit("files-opened", files);
}

/// The command list, plus the end-to-end test seam in debug builds.
macro_rules! handlers {
    ($($cmd:path),* $(,)?) => {{
        #[cfg(debug_assertions)]
        {
            tauri::generate_handler![$($cmd,)* e2e::e2e_enabled, e2e::e2e_grant]
        }
        #[cfg(not(debug_assertions))]
        {
            tauri::generate_handler![$($cmd),*]
        }
    }};
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "warn,opdf=info,ui=info".into()),
        )
        .try_init();

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, argv, cwd| {
            let state = app.state::<AppState>();
            let files =
                files_from_args(&state, argv.get(1..).unwrap_or(&[]), Some(Path::new(&cwd)));
            deliver_files(app, files);
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let data_dir = app
                .path()
                .app_local_data_dir()
                .unwrap_or_else(|_| std::env::temp_dir().join("openpdf-studio"));
            let settings = Settings::open(&data_dir.join("settings.sqlite3")).unwrap_or_else(|e| {
                tracing::warn!("settings unavailable, using defaults: {e}");
                Settings::in_memory()
            });
            let recovery_root = data_dir.join("recovery");
            // Scan for crashed sessions *before* creating ours.
            let recoverable = recovery::scan(&recovery_root)
                .into_iter()
                .map(|r| (uuid::Uuid::new_v4().simple().to_string(), r))
                .collect();
            let session_dir = match SessionDir::create(&recovery_root) {
                Ok(s) => Some(s),
                Err(e) => {
                    tracing::warn!("crash recovery disabled: {e}");
                    None
                }
            };
            let state = AppState {
                docs: Mutex::new(HashMap::new()),
                next_id: AtomicU64::new(1),
                grants: Grants::default(),
                settings,
                recovery_root,
                session_dir,
                recoverable: Mutex::new(recoverable),
                startup_files: Mutex::new(Vec::new()),
            };
            let args: Vec<String> = std::env::args().skip(1).collect();
            let files = files_from_args(&state, &args, std::env::current_dir().ok().as_deref());
            *state.startup_files.lock() = files;
            app.manage(state);
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::DragDrop(tauri::DragDropEvent::Drop { paths, .. }) = event {
                let app = window.app_handle();
                let state = app.state::<AppState>();
                let files: Vec<FileRef> = paths
                    .iter()
                    .filter(|p| p.is_file())
                    .map(|p| state.grants.grant(p, Mode::Read))
                    .collect();
                let _ = app.emit("files-dropped", files);
            }
        })
        .invoke_handler(handlers!(
            commands::pick_open,
            commands::pick_save,
            commands::pick_folder,
            commands::recent_files,
            commands::forget_recent,
            commands::take_startup_files,
            commands::doc_open,
            commands::doc_bytes,
            commands::doc_state,
            commands::doc_apply,
            commands::doc_insert_from,
            commands::doc_commit,
            commands::doc_undo,
            commands::doc_redo,
            commands::doc_save,
            commands::doc_save_as,
            commands::doc_extract,
            commands::doc_extract_to_tab,
            commands::doc_split,
            commands::doc_close,
            commands::merge_files,
            commands::images_to_pdf,
            commands::write_export,
            commands::recovery_list,
            commands::recovery_restore,
            commands::recovery_discard,
            commands::settings_get,
            commands::settings_set,
            commands::open_external,
            commands::reveal_document,
            commands::app_info,
            commands::print_window,
            commands::client_log,
        ))
        .build(tauri::generate_context!())
        .expect("failed to build the application");

    app.run(|app, event| {
        match event {
            // macOS delivers "Open With" / double-clicked files as URLs.
            #[cfg(any(target_os = "macos", target_os = "ios"))]
            RunEvent::Opened { urls } => {
                let state = app.state::<AppState>();
                let files: Vec<FileRef> = urls
                    .iter()
                    .filter_map(|u| u.to_file_path().ok())
                    .filter(|p| p.is_file())
                    .map(|p| state.grants.grant(&p, Mode::Read))
                    .collect();
                state.startup_files.lock().extend(files.iter().cloned());
                deliver_files(app, files);
            }
            RunEvent::Exit => {
                // Drop documents first so their recovery files are released,
                // then the session directory removes itself.
                let state = app.state::<AppState>();
                state.docs.lock().clear();
            }
            _ => {}
        }
    });
}
