//! Crash recovery.
//!
//! Each running app instance owns a *session directory* under the recovery
//! root and holds an OS file lock on `session.lock` for its lifetime. Each
//! open document keeps its version history there (`doc-<id>/v<N>.pdf` plus a
//! `manifest.json`). After a crash the lock is released by the OS, so the next
//! instance can tell dead sessions from live ones and offer to restore
//! documents that had unsaved changes. Passwords are never written to disk;
//! versions of encrypted documents stay encrypted.

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};

pub struct SessionDir {
    pub path: PathBuf,
    lock: Option<File>,
}

impl SessionDir {
    /// Create a new locked session directory under `root`.
    pub fn create(root: &Path) -> Result<SessionDir> {
        fs::create_dir_all(root)?;
        restrict_permissions(root);
        let path = root.join(format!("session-{}", uuid::Uuid::new_v4().simple()));
        fs::create_dir(&path)?;
        restrict_permissions(&path);
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(path.join("session.lock"))?;
        lock.try_lock()
            .map_err(|e| Error::internal(format!("could not lock the recovery session: {e}")))?;
        Ok(SessionDir {
            path,
            lock: Some(lock),
        })
    }

    pub fn doc_dir(&self, doc_id: u64) -> Result<PathBuf> {
        let d = self.path.join(format!("doc-{doc_id}"));
        fs::create_dir_all(&d)?;
        restrict_permissions(&d);
        Ok(d)
    }
}

impl Drop for SessionDir {
    /// A clean shutdown removes everything; only crashes leave files behind.
    fn drop(&mut self) {
        // Windows cannot delete a directory holding an open file.
        drop(self.lock.take());
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[cfg(unix)]
fn restrict_permissions(p: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(p, fs::Permissions::from_mode(0o700));
}
#[cfg(not(unix))]
fn restrict_permissions(_p: &Path) {}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestVersion {
    pub file: String,
    pub label: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub name: String,
    pub path: Option<PathBuf>,
    pub versions: Vec<ManifestVersion>,
    pub cursor: usize,
    pub saved_at: Option<usize>,
    pub encrypted: bool,
    pub updated_at: String,
}

impl Manifest {
    pub fn has_unsaved_changes(&self) -> bool {
        self.saved_at != Some(self.cursor)
    }
}

pub fn write_manifest(dir: &Path, m: &Manifest) -> Result<()> {
    let json = serde_json::to_vec_pretty(m).map_err(|e| Error::internal(e.to_string()))?;
    crate::fsio::atomic_write(&dir.join("manifest.json"), &json)
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Recoverable {
    /// Opaque handle (the document's recovery directory).
    pub dir: PathBuf,
    pub name: String,
    pub path: Option<PathBuf>,
    pub last_change: String,
    pub updated_at: String,
    pub encrypted: bool,
    pub size: u64,
}

/// List documents with unsaved changes left behind by sessions that are no
/// longer running. Dead sessions without such documents are deleted.
pub fn scan(root: &Path) -> Vec<Recoverable> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(root) else {
        return out;
    };
    for entry in entries.flatten() {
        let session = entry.path();
        if !session.is_dir() || !entry.file_name().to_string_lossy().starts_with("session-") {
            continue;
        }
        // A session whose lock we can take is dead (its process is gone).
        let Ok(lock) = OpenOptions::new()
            .write(true)
            .open(session.join("session.lock"))
        else {
            // No lock file: an incomplete directory; clean it up.
            let _ = fs::remove_dir_all(&session);
            continue;
        };
        if lock.try_lock().is_err() {
            continue; // live session (another window/instance)
        }
        let mut found = false;
        if let Ok(docs) = fs::read_dir(&session) {
            for d in docs.flatten() {
                let dir = d.path();
                let Some(m) = read_manifest(&dir) else {
                    continue;
                };
                if !m.has_unsaved_changes() {
                    continue;
                }
                let Some(v) = m.versions.get(m.cursor) else {
                    continue;
                };
                let file = dir.join(&v.file);
                let Ok(meta) = fs::metadata(&file) else {
                    continue;
                };
                found = true;
                out.push(Recoverable {
                    dir: dir.clone(),
                    name: m.name.clone(),
                    path: m.path.clone(),
                    last_change: v.label.clone(),
                    updated_at: m.updated_at.clone(),
                    encrypted: m.encrypted,
                    size: meta.len(),
                });
            }
        }
        drop(lock);
        if !found {
            let _ = fs::remove_dir_all(&session);
        }
    }
    out.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    out
}

pub fn read_manifest(dir: &Path) -> Option<Manifest> {
    let bytes = fs::read(dir.join("manifest.json")).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Validate that `dir` is a document directory of a *dead* session under
/// `root` (the UI only ever passes back handles produced by [`scan`]).
fn checked_doc_dir(root: &Path, dir: &Path) -> Result<PathBuf> {
    let root = fs::canonicalize(root)?;
    let dir = fs::canonicalize(dir)
        .map_err(|_| Error::invalid_arg("that recovery entry no longer exists"))?;
    let session = dir
        .parent()
        .ok_or_else(|| Error::invalid_arg("invalid recovery entry"))?;
    if session.parent() != Some(root.as_path())
        || !dir
            .file_name()
            .map(|n| n.to_string_lossy().starts_with("doc-"))
            .unwrap_or(false)
    {
        return Err(Error::invalid_arg("invalid recovery entry"));
    }
    let lock = OpenOptions::new()
        .write(true)
        .open(session.join("session.lock"))
        .map_err(|_| Error::invalid_arg("invalid recovery entry"))?;
    lock.try_lock()
        .map_err(|_| Error::invalid_arg("that document is open in another window"))?;
    Ok(dir)
}

/// Read the latest version of a recoverable document.
pub fn restore(root: &Path, dir: &Path) -> Result<(Manifest, Vec<u8>)> {
    let dir = checked_doc_dir(root, dir)?;
    let m =
        read_manifest(&dir).ok_or_else(|| Error::invalid_arg("the recovery data is damaged"))?;
    let v = m
        .versions
        .get(m.cursor)
        .ok_or_else(|| Error::invalid_arg("the recovery data is damaged"))?;
    let bytes = fs::read(dir.join(&v.file))?;
    Ok((m, bytes))
}

/// Delete a recoverable document (after restoring or discarding it).
pub fn discard(root: &Path, dir: &Path) -> Result<()> {
    let dir = checked_doc_dir(root, dir)?;
    fs::remove_dir_all(&dir)?;
    if let Some(session) = dir.parent() {
        let only_lock = fs::read_dir(session)
            .map(|it| it.flatten().all(|e| e.file_name() == "session.lock"))
            .unwrap_or(false);
        if only_lock {
            let _ = fs::remove_dir_all(session);
        }
    }
    Ok(())
}
