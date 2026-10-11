//! Test seam for end-to-end tests. Compiled into debug builds only: release
//! builds have no way to grant file access except through native dialogs,
//! drag-and-drop and launch arguments.

use crate::grants::{FileRef, Mode};
use crate::AppState;
use tauri::State;

#[tauri::command]
pub fn e2e_enabled() -> bool {
    std::env::var_os("OPDF_E2E").is_some()
}

#[tauri::command]
pub fn e2e_grant(
    state: State<'_, AppState>,
    path: String,
    mode: String,
) -> Result<FileRef, String> {
    if std::env::var_os("OPDF_E2E").is_none() {
        return Err("e2e mode is not enabled".into());
    }
    let mode = match mode.as_str() {
        "read" => Mode::Read,
        "write" => Mode::Write,
        "folder" => Mode::Folder,
        _ => return Err("bad mode".into()),
    };
    Ok(state.grants.grant(std::path::Path::new(&path), mode))
}
