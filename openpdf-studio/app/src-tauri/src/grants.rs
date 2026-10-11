//! File access grants — the trust boundary between the web UI and the disk.
//!
//! The UI never names a path to read or write. Paths enter the backend only
//! through native dialogs, drag-and-drop, launch arguments and the recent
//! files list; each gets an opaque grant id and a mode. Commands take grant
//! ids and check the mode, so a compromised page (e.g. via a malicious PDF
//! exploiting the viewer) cannot read or overwrite arbitrary files.

use opdf_core::{Error, Result};
use parking_lot::Mutex;
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    /// Read an existing file.
    Read,
    /// Create or overwrite this file (chosen in a save dialog).
    Write,
    /// Create new files inside this directory.
    Folder,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileRef {
    pub id: String,
    pub name: String,
    /// For display only.
    pub path: String,
    pub size: Option<u64>,
    /// "pdf", "image", "folder" or "other" (by extension).
    pub kind: &'static str,
    pub mode: Mode,
}

#[derive(Default)]
pub struct Grants {
    by_id: Mutex<HashMap<String, (PathBuf, Mode)>>,
    by_path: Mutex<HashMap<(PathBuf, Mode), String>>,
}

pub fn kind_of(path: &Path, mode: Mode) -> &'static str {
    if mode == Mode::Folder {
        return "folder";
    }
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if ext == "pdf" {
        "pdf"
    } else if opdf_core::images::IMAGE_EXTENSIONS.contains(&ext.as_str()) {
        "image"
    } else {
        "other"
    }
}

impl Grants {
    pub fn grant(&self, path: &Path, mode: Mode) -> FileRef {
        let path = if mode == Mode::Write {
            // The file may not exist yet; canonicalise its directory.
            match (
                path.parent().and_then(|p| std::fs::canonicalize(p).ok()),
                path.file_name(),
            ) {
                (Some(dir), Some(name)) => dir.join(name),
                _ => path.to_path_buf(),
            }
        } else {
            std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
        };
        let key = (path.clone(), mode);
        let id = {
            let mut by_path = self.by_path.lock();
            by_path
                .entry(key)
                .or_insert_with(|| uuid::Uuid::new_v4().simple().to_string())
                .clone()
        };
        self.by_id.lock().insert(id.clone(), (path.clone(), mode));
        FileRef {
            id,
            name: path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.display().to_string()),
            path: path.display().to_string(),
            size: std::fs::metadata(&path)
                .ok()
                .filter(|m| m.is_file())
                .map(|m| m.len()),
            kind: kind_of(&path, mode),
            mode,
        }
    }

    /// Resolve a grant, requiring `mode`. A Write grant also allows reading
    /// back the same file.
    pub fn resolve(&self, id: &str, mode: Mode) -> Result<PathBuf> {
        let g = self.by_id.lock();
        match g.get(id) {
            Some((p, m)) if *m == mode || (mode == Mode::Read && *m == Mode::Write) => {
                Ok(p.clone())
            }
            Some(_) => Err(Error::denied(
                "this file was not opened for that kind of access",
            )),
            None => Err(Error::denied(
                "unknown file reference; choose the file again",
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grants_are_mode_checked_and_stable() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("a.pdf");
        std::fs::write(&f, b"x").unwrap();
        let g = Grants::default();
        let r = g.grant(&f, Mode::Read);
        assert_eq!(r.kind, "pdf");
        assert_eq!(g.grant(&f, Mode::Read).id, r.id, "same file, same id");
        assert!(g.resolve(&r.id, Mode::Read).is_ok());
        assert!(
            g.resolve(&r.id, Mode::Write).is_err(),
            "read grant cannot write"
        );
        assert!(g.resolve("forged", Mode::Read).is_err());
        let w = g.grant(&dir.path().join("new.pdf"), Mode::Write);
        assert!(g.resolve(&w.id, Mode::Write).is_ok());
        assert!(g.resolve(&w.id, Mode::Read).is_ok());
        assert!(g.resolve(&w.id, Mode::Folder).is_err());
    }
}
