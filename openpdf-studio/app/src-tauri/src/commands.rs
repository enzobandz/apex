//! Tauri commands. Every command validates its inputs here; the web UI is
//! treated as untrusted (it renders untrusted PDFs).

use crate::grants::{FileRef, Mode};
use crate::AppState;
use opdf_core::assemble::{MergeInput, MergeOptions, SplitPlan};
use opdf_core::error::ErrorCode;
use opdf_core::images::ImagePageOptions;
use opdf_core::optimize::OptimizeReport;
use opdf_core::recovery::{self, Recoverable};
use opdf_core::session::{DocSession, DocState, Op, OpenRequest};
use opdf_core::{fsio, load, ranges, Error};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::ipc::{InvokeBody, Request, Response};
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

/// Error shape seen by the UI.
#[derive(Debug, Serialize)]
pub struct CmdError {
    pub code: ErrorCode,
    pub message: String,
}

impl From<Error> for CmdError {
    fn from(e: Error) -> Self {
        CmdError {
            code: e.code,
            message: e.message,
        }
    }
}

type CmdResult<T> = Result<T, CmdError>;

fn internal(msg: impl Into<String>) -> CmdError {
    Error::internal(msg).into()
}

/// Run blocking engine work off the async runtime.
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> CmdResult<T> + Send + 'static,
) -> CmdResult<T> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| internal(format!("background task failed: {e}")))?
}

fn doc(state: &AppState, id: u64) -> CmdResult<Arc<Mutex<DocSession>>> {
    state
        .docs
        .lock()
        .get(&id)
        .cloned()
        .ok_or_else(|| Error::invalid_arg("that document is no longer open").into())
}

fn app_state(app: &AppHandle) -> State<'_, AppState> {
    app.state::<AppState>()
}

// ---------------------------------------------------------------- files

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub enum PickKind {
    Pdf,
    Images,
    PdfOrImages,
}

#[tauri::command]
pub async fn pick_open(app: AppHandle, kind: PickKind, multiple: bool) -> CmdResult<Vec<FileRef>> {
    blocking(move || {
        let mut d = app.dialog().file();
        let img = opdf_core::images::IMAGE_EXTENSIONS;
        d = match kind {
            PickKind::Pdf => d.add_filter("PDF documents", &["pdf"]),
            PickKind::Images => d.add_filter("Images", &img),
            PickKind::PdfOrImages => {
                let mut all = vec!["pdf"];
                all.extend(img);
                d.add_filter("PDF and images", &all)
            }
        };
        let picked = if multiple {
            d.blocking_pick_files().unwrap_or_default()
        } else {
            d.blocking_pick_file().into_iter().collect()
        };
        let st = app_state(&app);
        Ok(picked
            .into_iter()
            .filter_map(|f| f.into_path().ok())
            .map(|p| st.grants.grant(&p, Mode::Read))
            .collect())
    })
    .await
}

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub enum SaveKind {
    Pdf,
}

#[tauri::command]
pub async fn pick_save(
    app: AppHandle,
    suggested_name: String,
    kind: SaveKind,
) -> CmdResult<Option<FileRef>> {
    blocking(move || {
        let SaveKind::Pdf = kind;
        let name = fsio::sanitize_file_name(&suggested_name);
        let name = if name.to_ascii_lowercase().ends_with(".pdf") {
            name
        } else {
            format!("{name}.pdf")
        };
        let picked = app
            .dialog()
            .file()
            .add_filter("PDF document", &["pdf"])
            .set_file_name(&name)
            .blocking_save_file();
        let Some(path) = picked.and_then(|f| f.into_path().ok()) else {
            return Ok(None);
        };
        // Some platforms do not append the extension from the filter.
        let path = if path.extension().is_none() {
            path.with_extension("pdf")
        } else {
            path
        };
        Ok(Some(app_state(&app).grants.grant(&path, Mode::Write)))
    })
    .await
}

#[tauri::command]
pub async fn pick_folder(app: AppHandle) -> CmdResult<Option<FileRef>> {
    blocking(move || {
        let picked = app.dialog().file().blocking_pick_folder();
        Ok(picked
            .and_then(|f| f.into_path().ok())
            .map(|p| app_state(&app).grants.grant(&p, Mode::Folder)))
    })
    .await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentFile {
    pub file: FileRef,
    pub opened_at: i64,
}

#[tauri::command]
pub fn recent_files(state: State<'_, AppState>) -> Vec<RecentFile> {
    state
        .settings
        .recent()
        .into_iter()
        .filter(|r| r.path.is_file())
        .map(|r| RecentFile {
            file: state.grants.grant(&r.path, Mode::Read),
            opened_at: r.opened_at,
        })
        .collect()
}

#[tauri::command]
pub fn forget_recent(state: State<'_, AppState>, file_id: Option<String>) -> CmdResult<()> {
    match file_id {
        Some(id) => state
            .settings
            .remove_recent(&state.grants.resolve(&id, Mode::Read)?),
        None => state.settings.clear_recent(),
    }
    Ok(())
}

/// Files passed on the command line / by the OS before the UI was ready.
#[tauri::command]
pub fn take_startup_files(state: State<'_, AppState>) -> Vec<FileRef> {
    std::mem::take(&mut *state.startup_files.lock())
}

// ---------------------------------------------------------------- documents

fn register(
    state: &AppState,
    mut make: impl FnMut(u64, Option<PathBuf>) -> opdf_core::Result<DocSession>,
) -> CmdResult<DocState> {
    let id = state.next_id();
    let store = if state.recovery_enabled() {
        state.session_dir.as_ref().and_then(|s| s.doc_dir(id).ok())
    } else {
        None
    };
    let mut session = match make(id, store.clone()) {
        Ok(s) => s,
        Err(e) => {
            if let Some(d) = store {
                let _ = std::fs::remove_dir_all(d);
            }
            return Err(e.into());
        }
    };
    let st = session.state();
    state.docs.lock().insert(id, Arc::new(Mutex::new(session)));
    Ok(st)
}

#[tauri::command]
pub async fn doc_open(
    app: AppHandle,
    file_id: String,
    password: Option<String>,
) -> CmdResult<DocState> {
    blocking(move || {
        let state = app_state(&app);
        let path = state.grants.resolve(&file_id, Mode::Read)?;
        // Already open? Focus it instead of opening a second copy.
        let existing: Vec<Arc<Mutex<DocSession>>> = state.docs.lock().values().cloned().collect();
        for d in existing {
            let mut d = d.lock();
            if d.path() == Some(path.as_path()) {
                return Ok(d.state());
            }
        }
        let bytes = fsio::read_limited(&path, load::MAX_FILE_SIZE)?;
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let st = register(&state, |id, store| {
            DocSession::open(
                id,
                OpenRequest {
                    bytes: bytes.clone(),
                    password: password.clone(),
                    name: name.clone(),
                    path: Some(path.clone()),
                    store,
                    label: "Opened".into(),
                },
            )
        })?;
        state.settings.add_recent(&path);
        Ok(st)
    })
    .await
}

/// Current bytes of a document, as a raw binary response (no JSON).
#[tauri::command]
pub async fn doc_bytes(app: AppHandle, doc_id: u64) -> CmdResult<Response> {
    let d = doc(&app_state(&app), doc_id)?;
    let bytes = d.lock().bytes();
    Ok(Response::new(bytes.as_ref().clone()))
}

#[tauri::command]
pub async fn doc_state(app: AppHandle, doc_id: u64) -> CmdResult<DocState> {
    let d = doc(&app_state(&app), doc_id)?;
    blocking(move || Ok(d.lock().state())).await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyResult {
    pub state: DocState,
    pub report: Option<OptimizeReport>,
}

#[tauri::command]
pub async fn doc_apply(app: AppHandle, doc_id: u64, op: Op) -> CmdResult<ApplyResult> {
    let d = doc(&app_state(&app), doc_id)?;
    blocking(move || {
        let mut s = d.lock();
        let report = s.apply(&op)?;
        Ok(ApplyResult {
            state: s.state(),
            report,
        })
    })
    .await
}

#[tauri::command]
pub async fn doc_insert_from(
    app: AppHandle,
    doc_id: u64,
    file_id: String,
    password: Option<String>,
    pages: Option<String>,
    at: usize,
) -> CmdResult<DocState> {
    let st = app_state(&app);
    let d = doc(&st, doc_id)?;
    let path = st.grants.resolve(&file_id, Mode::Read)?;
    blocking(move || {
        let bytes = fsio::read_limited(&path, load::MAX_FILE_SIZE)?;
        let src = load::load(&bytes, password.as_deref())?;
        let count = src.doc.get_pages().len();
        let sel = match pages.as_deref().map(str::trim) {
            None | Some("") => None,
            Some(spec) => Some(ranges::parse(spec, count)?),
        };
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut s = d.lock();
        s.insert_from(&src, sel, at, &name)?;
        Ok(s.state())
    })
    .await
}

fn header<'a>(req: &'a Request<'_>, name: &str) -> CmdResult<&'a str> {
    req.headers()
        .get(name)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| Error::invalid_arg(format!("missing {name}")).into())
}

fn raw_body(req: &Request<'_>) -> CmdResult<Vec<u8>> {
    match req.body() {
        InvokeBody::Raw(b) => Ok(b.clone()),
        _ => Err(Error::invalid_arg("expected binary data").into()),
    }
}

/// Bytes saved by the viewer (annotations / form values).
/// Headers: `x-doc-id`, `x-label` (URI-encoded).
#[tauri::command]
pub async fn doc_commit(app: AppHandle, request: Request<'_>) -> CmdResult<DocState> {
    let id: u64 = header(&request, "x-doc-id")?
        .parse()
        .map_err(|_| internal("bad document id"))?;
    let label = percent_decode(header(&request, "x-label").unwrap_or("Edit"));
    let label: String = label.chars().take(80).collect();
    let bytes = raw_body(&request)?;
    let d = doc(&app_state(&app), id)?;
    blocking(move || {
        let mut s = d.lock();
        s.commit_bytes(bytes, &label)?;
        Ok(s.state())
    })
    .await
}

fn percent_decode(s: &str) -> String {
    url::form_urlencoded::parse(format!("x={s}").as_bytes())
        .next()
        .map(|(_, v)| v.into_owned())
        .unwrap_or_default()
}

#[tauri::command]
pub async fn doc_undo(app: AppHandle, doc_id: u64) -> CmdResult<DocState> {
    let d = doc(&app_state(&app), doc_id)?;
    blocking(move || {
        let mut s = d.lock();
        s.undo()?;
        Ok(s.state())
    })
    .await
}

#[tauri::command]
pub async fn doc_redo(app: AppHandle, doc_id: u64) -> CmdResult<DocState> {
    let d = doc(&app_state(&app), doc_id)?;
    blocking(move || {
        let mut s = d.lock();
        s.redo()?;
        Ok(s.state())
    })
    .await
}

#[tauri::command]
pub async fn doc_save(app: AppHandle, doc_id: u64, force: bool) -> CmdResult<DocState> {
    let st = app_state(&app);
    let d = doc(&st, doc_id)?;
    let res = blocking(move || {
        let mut s = d.lock();
        s.save(force)?;
        Ok(s.state())
    })
    .await?;
    if let Some(p) = &res.path {
        st.settings.add_recent(p);
    }
    Ok(res)
}

#[tauri::command]
pub async fn doc_save_as(app: AppHandle, doc_id: u64, file_id: String) -> CmdResult<DocState> {
    let st = app_state(&app);
    let d = doc(&st, doc_id)?;
    let path = st.grants.resolve(&file_id, Mode::Write)?;
    // Refuse to overwrite a file open in another tab.
    for (id, other) in st.docs.lock().iter() {
        if *id != doc_id {
            if let Some(o) = other.try_lock() {
                if o.path() == Some(path.as_path()) {
                    return Err(Error::invalid_arg(
                        "that file is open in another tab; close it first",
                    )
                    .into());
                }
            }
        }
    }
    let res = blocking(move || {
        let mut s = d.lock();
        s.save_as(&path)?;
        Ok(s.state())
    })
    .await?;
    if let Some(p) = &res.path {
        st.settings.add_recent(p);
    }
    Ok(res)
}

#[tauri::command]
pub async fn doc_extract(
    app: AppHandle,
    doc_id: u64,
    pages: Vec<usize>,
    file_id: String,
) -> CmdResult<String> {
    let st = app_state(&app);
    let d = doc(&st, doc_id)?;
    let path = st.grants.resolve(&file_id, Mode::Write)?;
    blocking(move || {
        let bytes = d.lock().extract(&pages)?;
        fsio::atomic_write(&path, &bytes)?;
        Ok(path.display().to_string())
    })
    .await
}

/// Extract pages into a new untitled tab instead of a file.
#[tauri::command]
pub async fn doc_extract_to_tab(
    app: AppHandle,
    doc_id: u64,
    pages: Vec<usize>,
) -> CmdResult<DocState> {
    let st = app_state(&app);
    let d = doc(&st, doc_id)?;
    blocking(move || {
        let (bytes, name, password) = {
            let mut s = d.lock();
            let bytes = s.extract(&pages)?;
            let stem = Path::new(s.name())
                .file_stem()
                .map(|x| x.to_string_lossy().into_owned())
                .unwrap_or_default();
            (
                bytes,
                format!("{stem} (pages {}).pdf", ranges::format(&pages)),
                s.password().map(str::to_owned),
            )
        };
        let st = app_state(&app);
        register(&st, |id, store| {
            DocSession::open(
                id,
                OpenRequest {
                    bytes: bytes.clone(),
                    password: password.clone(),
                    name: name.clone(),
                    path: None,
                    store,
                    label: "Extracted pages".into(),
                },
            )
        })
    })
    .await
}

#[tauri::command]
pub async fn doc_split(
    app: AppHandle,
    doc_id: u64,
    plan: SplitPlan,
    folder_id: String,
) -> CmdResult<Vec<String>> {
    let st = app_state(&app);
    let d = doc(&st, doc_id)?;
    let dir = st.grants.resolve(&folder_id, Mode::Folder)?;
    blocking(move || {
        let (parts, stem) = {
            let mut s = d.lock();
            let stem = Path::new(s.name())
                .file_stem()
                .map(|x| x.to_string_lossy().into_owned())
                .unwrap_or_else(|| "document".into());
            (s.split(&plan)?, stem)
        };
        let mut written = Vec::with_capacity(parts.len());
        for (label, bytes) in parts {
            let p = fsio::unique_path(
                &dir,
                &format!("{stem} - pages {}", label.replace(", ", "_")),
                "pdf",
            );
            fsio::atomic_write(&p, &bytes)?;
            written.push(p.display().to_string());
        }
        Ok(written)
    })
    .await
}

#[tauri::command]
pub fn doc_close(state: State<'_, AppState>, doc_id: u64) {
    if let Some(d) = state.docs.lock().remove(&doc_id) {
        // Remove recovery files in the background; the lock may be held by a
        // finishing operation.
        std::thread::spawn(move || d.lock().discard_recovery());
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MergeItem {
    pub file_id: String,
    pub password: Option<String>,
    /// Page range spec; empty = all pages.
    pub pages: Option<String>,
}

#[tauri::command]
pub async fn merge_files(
    app: AppHandle,
    items: Vec<MergeItem>,
    bookmarks: bool,
) -> CmdResult<DocState> {
    if items.is_empty() || items.len() > 500 {
        return Err(Error::invalid_arg("choose between 1 and 500 files").into());
    }
    let st = app_state(&app);
    let paths: Vec<PathBuf> = items
        .iter()
        .map(|i| st.grants.resolve(&i.file_id, Mode::Read))
        .collect::<Result<_, _>>()?;
    blocking(move || {
        let mut inputs = Vec::with_capacity(items.len());
        for (item, path) in items.iter().zip(&paths) {
            let name = path
                .file_stem()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let bytes = fsio::read_limited(path, load::MAX_FILE_SIZE)?;
            let pdf = load::load(&bytes, item.password.as_deref()).map_err(|mut e| {
                e.message = format!("{name}: {}", e.message);
                e
            })?;
            let count = pdf.doc.get_pages().len();
            let pages = match item.pages.as_deref().map(str::trim) {
                None | Some("") => None,
                Some(spec) => Some(ranges::parse(spec, count).map_err(|mut e| {
                    e.message = format!("{name}: {}", e.message);
                    e
                })?),
            };
            inputs.push(MergeInput { name, pdf, pages });
        }
        let doc = opdf_core::error::guard("merging", || {
            opdf_core::assemble::merge(
                inputs,
                MergeOptions {
                    bookmark_per_file: bookmarks,
                },
            )
        })?;
        let bytes = load::serialize(&doc, None)?;
        let st = app_state(&app);
        register(&st, |id, store| {
            DocSession::open(
                id,
                OpenRequest {
                    bytes: bytes.clone(),
                    password: None,
                    name: "Merged.pdf".into(),
                    path: None,
                    store,
                    label: "Merged files".into(),
                },
            )
        })
    })
    .await
}

#[tauri::command]
pub async fn images_to_pdf(
    app: AppHandle,
    file_ids: Vec<String>,
    options: ImagePageOptions,
) -> CmdResult<DocState> {
    if file_ids.is_empty() || file_ids.len() > 2000 {
        return Err(Error::invalid_arg("choose between 1 and 2000 images").into());
    }
    let st = app_state(&app);
    let paths: Vec<PathBuf> = file_ids
        .iter()
        .map(|i| st.grants.resolve(i, Mode::Read))
        .collect::<Result<_, _>>()?;
    blocking(move || {
        let images = paths
            .iter()
            .map(|p| fsio::read_limited(p, 512 * 1024 * 1024))
            .collect::<opdf_core::Result<Vec<_>>>()?;
        let doc = opdf_core::error::guard("converting images", || {
            opdf_core::images::images_to_pdf(&images, &options)
        })?;
        let bytes = load::serialize(&doc, None)?;
        let name = if paths.len() == 1 {
            format!(
                "{}.pdf",
                paths[0]
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default()
            )
        } else {
            "Images.pdf".into()
        };
        let st = app_state(&app);
        register(&st, |id, store| {
            DocSession::open(
                id,
                OpenRequest {
                    bytes: bytes.clone(),
                    password: None,
                    name: name.clone(),
                    path: None,
                    store,
                    label: "Created from images".into(),
                },
            )
        })
    })
    .await
}

/// Write an exported file (e.g. a rendered page image) into a granted
/// folder. Headers: `x-folder` (grant id), `x-name` (URI-encoded file name).
#[tauri::command]
pub async fn write_export(app: AppHandle, request: Request<'_>) -> CmdResult<String> {
    let st = app_state(&app);
    let dir = st
        .grants
        .resolve(header(&request, "x-folder")?, Mode::Folder)?;
    let name = fsio::sanitize_file_name(&percent_decode(header(&request, "x-name")?));
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e))
            if matches!(
                e.to_ascii_lowercase().as_str(),
                "png" | "jpg" | "jpeg" | "pdf" | "txt"
            ) =>
        {
            (s.to_string(), e.to_ascii_lowercase())
        }
        _ => return Err(Error::invalid_arg("unsupported export file type").into()),
    };
    let bytes = raw_body(&request)?;
    blocking(move || {
        let p = fsio::unique_path(&dir, &stem, &ext);
        fsio::atomic_write(&p, &bytes)?;
        Ok(p.display().to_string())
    })
    .await
}

// ---------------------------------------------------------------- recovery

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryItem {
    pub id: String,
    #[serde(flatten)]
    pub info: Recoverable,
}

#[tauri::command]
pub fn recovery_list(state: State<'_, AppState>) -> Vec<RecoveryItem> {
    let items = state.recoverable.lock();
    items
        .iter()
        .map(|(id, r)| RecoveryItem {
            id: id.clone(),
            info: r.clone(),
        })
        .collect()
}

#[tauri::command]
pub async fn recovery_restore(
    app: AppHandle,
    id: String,
    password: Option<String>,
) -> CmdResult<DocState> {
    blocking(move || {
        let st = app_state(&app);
        let dir = st
            .recoverable
            .lock()
            .iter()
            .find(|(i, _)| *i == id)
            .map(|(_, r)| r.dir.clone())
            .ok_or_else(|| Error::invalid_arg("that recovery entry no longer exists"))?;
        let (manifest, bytes) = recovery::restore(&st.recovery_root, &dir)?;
        let doc = register(&st, |nid, store| {
            DocSession::open(
                nid,
                OpenRequest {
                    bytes: bytes.clone(),
                    password: password.clone(),
                    name: manifest.name.clone(),
                    // Restored work is "new": the user decides where to save.
                    path: None,
                    store,
                    label: "Recovered".into(),
                },
            )
        })?;
        let _ = recovery::discard(&st.recovery_root, &dir);
        st.recoverable.lock().retain(|(i, _)| *i != id);
        Ok(doc)
    })
    .await
}

#[tauri::command]
pub fn recovery_discard(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    let mut items = state.recoverable.lock();
    if let Some(pos) = items.iter().position(|(i, _)| *i == id) {
        let (_, r) = items.remove(pos);
        recovery::discard(&state.recovery_root, &r.dir)?;
    }
    Ok(())
}

// ---------------------------------------------------------------- misc

#[tauri::command]
pub fn settings_get(state: State<'_, AppState>) -> serde_json::Value {
    state.settings.ui()
}

#[tauri::command]
pub fn settings_set(state: State<'_, AppState>, value: serde_json::Value) -> CmdResult<()> {
    state.settings.set_ui(&value)?;
    Ok(())
}

/// Open a web or mail link from a document in the system browser. Only
/// http(s) and mailto are allowed; the UI asks the user first.
#[tauri::command]
pub fn open_external(app: AppHandle, url: String) -> CmdResult<()> {
    let parsed = url::Url::parse(&url).map_err(|_| Error::invalid_arg("not a valid link"))?;
    if !matches!(parsed.scheme(), "http" | "https" | "mailto") {
        return Err(Error::denied("only web and e-mail links can be opened").into());
    }
    app.opener()
        .open_url(parsed.as_str(), None::<&str>)
        .map_err(|e| internal(format!("could not open the link: {e}")))
}

#[tauri::command]
pub fn reveal_document(app: AppHandle, doc_id: u64) -> CmdResult<()> {
    let st = app_state(&app);
    let d = doc(&st, doc_id)?;
    let path = d.lock().path().map(Path::to_path_buf);
    match path {
        Some(p) => app
            .opener()
            .reveal_item_in_dir(p)
            .map_err(|e| internal(e.to_string())),
        None => Err(Error::invalid_arg("this document has not been saved yet").into()),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub version: String,
    pub recovery_dir: String,
    pub platform: &'static str,
}

#[tauri::command]
pub fn app_info(app: AppHandle) -> AppInfo {
    let st = app_state(&app);
    AppInfo {
        version: app.package_info().version.to_string(),
        recovery_dir: st.recovery_root.display().to_string(),
        platform: std::env::consts::OS,
    }
}

/// Native print dialog for the current page contents (needed on macOS, where
/// WKWebView does not implement `window.print()`).
#[tauri::command]
pub fn print_window(window: tauri::WebviewWindow) -> CmdResult<()> {
    window
        .print()
        .map_err(|e| internal(format!("printing is not available: {e}")))
}

/// Forward UI errors to the local log (stderr). Nothing leaves the machine.
#[tauri::command]
pub fn client_log(level: String, message: String) {
    let message: String = message.chars().take(4000).collect();
    match level.as_str() {
        "error" => tracing::error!(target: "ui", "{message}"),
        "warn" => tracing::warn!(target: "ui", "{message}"),
        _ => tracing::info!(target: "ui", "{message}"),
    }
}
