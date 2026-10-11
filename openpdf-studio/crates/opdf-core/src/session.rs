//! An open document: version history (undo/redo), saving, and the on-disk
//! recovery copy of unsaved work.
//!
//! Every change produces a complete new version of the file. That keeps undo
//! exact (no inverse operations to get wrong), makes crash recovery trivial
//! (the latest version *is* the recovery file) and lets the viewer simply
//! reload. Only the current version is held in memory when a recovery
//! directory is available; older versions live on disk.

use crate::assemble::{self, SplitPlan};
use crate::error::{Error, ErrorCode, Result};
use crate::fsio::{self, FileStamp};
use crate::info::{self, DocSummary};
use crate::load::{self, LoadedPdf, PermissionSummary, Security};
use crate::metadata::{self, MetadataUpdate};
use crate::optimize::{self, OptimizeOptions, OptimizeReport};
use crate::pages;
use crate::recovery::{self, Manifest, ManifestVersion};
use crate::security::{self, ProtectOptions};
use lopdf::{dictionary, Object};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Undo steps kept per document.
pub const MAX_UNDO: usize = 60;
/// History budget for documents without a recovery directory (in memory).
pub const MAX_MEMORY_HISTORY: u64 = 1024 * 1024 * 1024;

/// A structural or document-level edit.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Op {
    Rotate {
        pages: Vec<usize>,
        degrees: i64,
    },
    Delete {
        pages: Vec<usize>,
    },
    Reorder {
        order: Vec<usize>,
    },
    Move {
        pages: Vec<usize>,
        to: usize,
    },
    InsertBlank {
        at: usize,
        width: f32,
        height: f32,
        count: usize,
    },
    Duplicate {
        pages: Vec<usize>,
    },
    SetMetadata {
        update: MetadataUpdate,
    },
    Optimize {
        options: OptimizeOptions,
    },
    Protect {
        options: ProtectOptions,
    },
    Unprotect,
}

impl Op {
    pub fn label(&self) -> String {
        let n = |p: &Vec<usize>| {
            if p.len() == 1 {
                "page".to_string()
            } else {
                format!("{} pages", p.len())
            }
        };
        match self {
            Op::Rotate { pages, .. } => format!("Rotate {}", n(pages)),
            Op::Delete { pages } => format!("Delete {}", n(pages)),
            Op::Reorder { .. } => "Reorder pages".into(),
            Op::Move { pages, .. } => format!("Move {}", n(pages)),
            Op::InsertBlank { count, .. } => {
                if *count == 1 {
                    "Insert blank page".into()
                } else {
                    format!("Insert {count} blank pages")
                }
            }
            Op::Duplicate { pages } => format!("Duplicate {}", n(pages)),
            Op::SetMetadata { .. } => "Edit document properties".into(),
            Op::Optimize { .. } => "Optimize file size".into(),
            Op::Protect { .. } => "Add password protection".into(),
            Op::Unprotect => "Remove password protection".into(),
        }
    }

    fn check(&self, p: &PermissionSummary) -> Result<()> {
        let ok = match self {
            Op::Rotate { .. }
            | Op::Delete { .. }
            | Op::Reorder { .. }
            | Op::Move { .. }
            | Op::InsertBlank { .. }
            | Op::Duplicate { .. } => p.assemble,
            Op::SetMetadata { .. } | Op::Optimize { .. } => p.modify,
            Op::Protect { .. } | Op::Unprotect => p.owner_access,
        };
        if ok {
            Ok(())
        } else {
            Err(Error::denied(match self {
                Op::Protect { .. } | Op::Unprotect => {
                    "changing security requires the document's permissions (owner) password"
                }
                _ => "the document's security settings do not allow this change",
            }))
        }
    }
}

#[derive(Clone)]
struct Version {
    label: String,
    file: Option<PathBuf>,
    mem: Option<Arc<Vec<u8>>>,
    size: u64,
    /// Password that opens this version (in memory only).
    password: Option<String>,
    compact: bool,
}

/// Snapshot of a document for the UI.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocState {
    pub id: u64,
    pub name: String,
    pub path: Option<PathBuf>,
    /// Changes whenever the bytes change (edit, undo, redo); the viewer
    /// reloads when it sees a new revision.
    pub revision: u64,
    pub dirty: bool,
    pub can_undo: bool,
    pub can_redo: bool,
    pub undo_label: Option<String>,
    pub redo_label: Option<String>,
    pub size: u64,
    /// `None` when the engine could not parse the file (view-only mode).
    pub summary: Option<DocSummary>,
    pub view_only_reason: Option<String>,
    /// Password the viewer needs for the current bytes (encrypted documents).
    pub view_password: Option<String>,
    pub recovery: bool,
}

pub struct DocSession {
    pub id: u64,
    name: String,
    path: Option<PathBuf>,
    disk_stamp: Option<FileStamp>,
    history: Vec<Version>,
    cursor: usize,
    saved_at: Option<usize>,
    store: Option<PathBuf>,
    current: Arc<Vec<u8>>,
    parsed: Option<LoadedPdf>,
    parse_error: Option<String>,
    summary: Option<DocSummary>,
    revision: u64,
    seq: u64,
}

pub struct OpenRequest {
    pub bytes: Vec<u8>,
    pub password: Option<String>,
    pub name: String,
    /// File the document came from (Save writes here). `None` for new
    /// documents such as merge results, which start out unsaved.
    pub path: Option<PathBuf>,
    /// Directory for history/recovery files (`None` = keep history in memory).
    pub store: Option<PathBuf>,
    /// Label of the first history entry.
    pub label: String,
}

impl DocSession {
    pub fn open(id: u64, req: OpenRequest) -> Result<DocSession> {
        let password = req.password.filter(|p| !p.is_empty());
        let (parsed, parse_error) = match load::load(&req.bytes, password.as_deref()) {
            Ok(pdf) => (Some(pdf), None),
            Err(e) => match e.code {
                // Damaged-but-PDF-looking files can often still be displayed.
                ErrorCode::InvalidPdf | ErrorCode::Internal if load::looks_like_pdf(&req.bytes) => {
                    (None, Some(e.message))
                }
                _ => return Err(e),
            },
        };
        let size = req.bytes.len() as u64;
        let disk_stamp = req.path.as_deref().and_then(fsio::stamp);
        let saved_at = req.path.as_ref().map(|_| 0);
        let current = Arc::new(req.bytes);
        let mut s = DocSession {
            id,
            name: req.name,
            path: req.path,
            disk_stamp,
            history: vec![Version {
                label: req.label,
                file: None,
                mem: Some(current.clone()),
                size,
                password,
                compact: false,
            }],
            cursor: 0,
            saved_at,
            store: req.store,
            current,
            parsed,
            parse_error,
            summary: None,
            revision: 1,
            seq: 0,
        };
        if s.is_dirty() {
            s.persist_current()?;
            s.write_manifest();
        }
        Ok(s)
    }

    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }
    pub fn bytes(&self) -> Arc<Vec<u8>> {
        self.current.clone()
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn is_dirty(&self) -> bool {
        self.saved_at != Some(self.cursor)
    }
    pub fn password(&self) -> Option<&str> {
        self.history[self.cursor].password.as_deref()
    }

    /// Parsed current version (parsed lazily after undo/redo).
    pub fn parsed(&mut self) -> Result<&LoadedPdf> {
        if self.parsed.is_none() {
            if let Some(e) = &self.parse_error {
                return Err(Error::invalid_pdf(format!(
                    "this file is damaged and can only be viewed ({e}). Use \"Save as\" from a repaired copy to edit it"
                )));
            }
            let pw = self.history[self.cursor].password.clone();
            self.parsed = Some(load::load(&self.current, pw.as_deref())?);
        }
        Ok(self.parsed.as_ref().expect("just set"))
    }

    pub fn permissions(&mut self) -> Result<PermissionSummary> {
        Ok(self.parsed()?.permissions())
    }

    pub fn state(&mut self) -> DocState {
        if self.summary.is_none() {
            if let Ok(p) = self.parsed() {
                self.summary = Some(info::summarize(p));
            }
        }
        let prev = self.cursor.checked_sub(1);
        DocState {
            id: self.id,
            name: self.name.clone(),
            path: self.path.clone(),
            revision: self.revision,
            dirty: self.is_dirty(),
            can_undo: prev.is_some(),
            can_redo: self.cursor + 1 < self.history.len(),
            undo_label: prev.map(|_| self.history[self.cursor].label.clone()),
            redo_label: self.history.get(self.cursor + 1).map(|v| v.label.clone()),
            size: self.current.len() as u64,
            summary: self.summary.clone(),
            view_only_reason: self.parse_error.clone(),
            view_password: self.password().map(str::to_owned),
            recovery: self.store.is_some(),
        }
    }

    /// Apply an edit, creating a new version.
    pub fn apply(&mut self, op: &Op) -> Result<Option<OptimizeReport>> {
        op.check(&self.permissions()?)?;
        let mut pdf = self.parsed()?.clone();
        let mut password = self.password().map(str::to_owned);
        let mut compact = self.history[self.cursor].compact;
        let mut report = None;
        crate::error::guard("editing the document", || {
            let doc = &mut pdf.doc;
            match op {
                Op::Rotate { pages, degrees } => pages::rotate(doc, pages, *degrees)?,
                Op::Delete { pages } => pages::delete(doc, pages)?,
                Op::Reorder { order } => pages::reorder(doc, order)?,
                Op::Move { pages, to } => pages::move_pages(doc, pages, *to)?,
                Op::InsertBlank {
                    at,
                    width,
                    height,
                    count,
                } => pages::insert_blank(doc, *at, (*width, *height), *count)?,
                Op::Duplicate { pages } => pages::duplicate(doc, pages)?,
                Op::SetMetadata { update } => metadata::write(doc, update)?,
                Op::Optimize { options } => {
                    report = Some(optimize::optimize(doc, options)?);
                    compact = true;
                }
                Op::Protect { options } => {
                    let state = security::build_state(options)?;
                    mark_aes256(doc);
                    pdf.security = Some(Security {
                        state,
                        owner_access: true,
                    });
                    password = Some(if options.owner_password.is_empty() {
                        options.user_password.clone()
                    } else {
                        options.owner_password.clone()
                    });
                }
                Op::Unprotect => {
                    pdf.security = None;
                    password = None;
                }
            }
            Ok(())
        })?;
        if !matches!(op, Op::SetMetadata { .. }) {
            metadata::touch(&mut pdf.doc);
        }
        let bytes = serialize(&pdf, compact)?;
        self.push(op.label(), bytes, Some(pdf), password, compact)?;
        Ok(report)
    }

    /// Insert pages of another (already opened and authorised) PDF.
    pub fn insert_from(
        &mut self,
        source: &LoadedPdf,
        pages_sel: Option<Vec<usize>>,
        at: usize,
        source_name: &str,
    ) -> Result<()> {
        if !self.permissions()?.assemble {
            return Err(Error::denied(
                "the document's security settings do not allow inserting pages",
            ));
        }
        let mut pdf = self.parsed()?.clone();
        let n = crate::error::guard("inserting pages", || {
            assemble::insert_from(&mut pdf.doc, source, pages_sel, at, source_name)
        })?;
        metadata::touch(&mut pdf.doc);
        let compact = self.history[self.cursor].compact;
        let bytes = serialize(&pdf, compact)?;
        let label = if n == 1 {
            format!("Insert page from {source_name}")
        } else {
            format!("Insert {n} pages from {source_name}")
        };
        let password = self.password().map(str::to_owned);
        self.push(label, bytes, Some(pdf), password, compact)
    }

    /// Accept bytes produced by the viewer (annotations, form values). The
    /// viewer writes an incremental update, so existing signatures remain
    /// verifiable. The result must parse and keep the page count.
    pub fn commit_bytes(&mut self, bytes: Vec<u8>, label: &str) -> Result<()> {
        let perms = self.permissions()?;
        if !(perms.annotate || perms.fill_forms) {
            return Err(Error::denied(
                "the document's security settings do not allow annotations or form filling",
            ));
        }
        let password = self.password().map(str::to_owned);
        let pdf = load::load(&bytes, password.as_deref())?;
        let before = pages::page_count(&self.parsed()?.doc);
        if pages::page_count(&pdf.doc) != before {
            return Err(Error::internal("the viewer produced a document with a different page count; the change was not applied"));
        }
        let compact = self.history[self.cursor].compact;
        self.push(label.to_string(), bytes, Some(pdf), password, compact)
    }

    fn push(
        &mut self,
        label: String,
        bytes: Vec<u8>,
        parsed: Option<LoadedPdf>,
        password: Option<String>,
        compact: bool,
    ) -> Result<()> {
        // Previous current version moves to history: make sure it is stored.
        self.persist_current()?;
        self.release_current_mem();
        // Drop the redo branch.
        for v in self.history.drain(self.cursor + 1..) {
            if let Some(f) = v.file {
                let _ = fs::remove_file(f);
            }
        }
        if let Some(s) = self.saved_at {
            if s > self.cursor {
                self.saved_at = None;
            }
        }
        let bytes = Arc::new(bytes);
        self.history.push(Version {
            label,
            file: None,
            mem: Some(bytes.clone()),
            size: bytes.len() as u64,
            password,
            compact,
        });
        self.cursor = self.history.len() - 1;
        self.current = bytes;
        self.parsed = parsed;
        self.parse_error = None;
        self.summary = None;
        self.revision += 1;
        self.trim_history();
        if self.is_dirty() {
            // A failed recovery write must not lose the edit itself.
            if let Err(e) = self.persist_current() {
                tracing::warn!(target: "opdf", "recovery copy not written: {e}");
            }
        }
        self.write_manifest();
        Ok(())
    }

    fn version_bytes(&self, i: usize) -> Result<Arc<Vec<u8>>> {
        let v = &self.history[i];
        if let Some(m) = &v.mem {
            return Ok(m.clone());
        }
        match &v.file {
            Some(f) => Ok(Arc::new(fs::read(f)?)),
            None => Err(Error::internal("a history entry is missing")),
        }
    }

    /// Write the current version into the recovery store (no-op without one).
    fn persist_current(&mut self) -> Result<()> {
        let Some(store) = self.store.clone() else {
            return Ok(());
        };
        if self.history[self.cursor].file.is_some() {
            return Ok(());
        }
        fs::create_dir_all(&store)?;
        self.seq += 1;
        let file = store.join(format!("v{}.pdf", self.seq));
        fsio::atomic_write(&file, &self.current)?;
        self.history[self.cursor].file = Some(file);
        Ok(())
    }

    /// Keep only the current version in memory when it is also on disk.
    fn release_current_mem(&mut self) {
        let v = &mut self.history[self.cursor];
        if v.file.is_some() {
            v.mem = None;
        }
    }

    fn trim_history(&mut self) {
        let in_memory = self.store.is_none();
        loop {
            let over_count = self.history.len() > MAX_UNDO + 1;
            let mem_total: u64 = self
                .history
                .iter()
                .filter(|v| v.mem.is_some())
                .map(|v| v.size)
                .sum();
            let over_mem = in_memory && mem_total > MAX_MEMORY_HISTORY;
            if !(over_count || over_mem) || self.cursor == 0 {
                break;
            }
            let v = self.history.remove(0);
            if let Some(f) = v.file {
                let _ = fs::remove_file(f);
            }
            self.cursor -= 1;
            self.saved_at = match self.saved_at {
                Some(0) | None => None,
                Some(s) => Some(s - 1),
            };
        }
    }

    fn goto(&mut self, index: usize) -> Result<()> {
        let bytes = self.version_bytes(index)?;
        if self.persist_current().is_ok() {
            self.release_current_mem();
        }
        self.cursor = index;
        self.history[index].mem = Some(bytes.clone());
        self.current = bytes;
        self.parsed = None;
        self.parse_error = None;
        self.summary = None;
        self.revision += 1;
        if self.is_dirty() {
            let _ = self.persist_current();
        }
        self.write_manifest();
        Ok(())
    }

    pub fn undo(&mut self) -> Result<()> {
        match self.cursor.checked_sub(1) {
            Some(i) => self.goto(i),
            None => Err(Error::invalid_arg("nothing to undo")),
        }
    }

    pub fn redo(&mut self) -> Result<()> {
        if self.cursor + 1 < self.history.len() {
            self.goto(self.cursor + 1)
        } else {
            Err(Error::invalid_arg("nothing to redo"))
        }
    }

    /// Save to the file the document came from. Fails with
    /// [`ErrorCode::FileChanged`] if another program modified it since it was
    /// opened or last saved, unless `force` is set.
    pub fn save(&mut self, force: bool) -> Result<()> {
        let path = self.path.clone().ok_or_else(|| {
            Error::invalid_arg("this document has not been saved yet; choose a location")
        })?;
        if !force {
            if let Some(stamp) = self.disk_stamp {
                if fsio::stamp(&path) != Some(stamp) {
                    return Err(Error::new(
                        ErrorCode::FileChanged,
                        "the file was changed by another program since it was opened",
                    ));
                }
            }
        }
        self.write_to(&path)
    }

    /// Save to a new location; it becomes the document's file.
    pub fn save_as(&mut self, path: &Path) -> Result<()> {
        self.write_to(path)
    }

    fn write_to(&mut self, path: &Path) -> Result<()> {
        fsio::atomic_write(path, &self.current)?;
        self.path = Some(path.to_path_buf());
        if let Some(n) = path.file_name() {
            self.name = n.to_string_lossy().into_owned();
        }
        self.saved_at = Some(self.cursor);
        self.disk_stamp = fsio::stamp(path);
        self.write_manifest();
        Ok(())
    }

    /// Bytes of the selected pages as a stand-alone PDF (keeps the source's
    /// password protection, if any).
    pub fn extract(&mut self, indices: &[usize]) -> Result<Vec<u8>> {
        let pdf = self.parsed()?.clone();
        assemble::require_assemble(&pdf, &self.name)?;
        let compact = self.history[self.cursor].compact;
        let doc = crate::error::guard("extracting pages", || pages::extract(&pdf.doc, indices))?;
        serialize(
            &LoadedPdf {
                doc,
                security: pdf.security,
            },
            compact,
        )
    }

    /// Split into parts (each keeps the source's password protection).
    pub fn split(&mut self, plan: &SplitPlan) -> Result<Vec<(String, Vec<u8>)>> {
        let pdf = self.parsed()?.clone();
        let compact = self.history[self.cursor].compact;
        let parts = crate::error::guard("splitting the document", || assemble::split(&pdf, plan))?;
        parts
            .into_iter()
            .map(|p| {
                let bytes = serialize(
                    &LoadedPdf {
                        doc: p.doc,
                        security: pdf.security.clone(),
                    },
                    compact,
                )?;
                Ok((p.label, bytes))
            })
            .collect()
    }

    fn write_manifest(&self) {
        let Some(store) = &self.store else { return };
        if !self.history[self.cursor].file.is_some() && self.is_dirty() {
            return;
        }
        let m = Manifest {
            name: self.name.clone(),
            path: self.path.clone(),
            versions: self
                .history
                .iter()
                .map(|v| ManifestVersion {
                    file: v
                        .file
                        .as_ref()
                        .and_then(|f| f.file_name())
                        .map(|f| f.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    label: v.label.clone(),
                })
                .collect(),
            cursor: self.cursor,
            saved_at: self.saved_at,
            encrypted: self.history[self.cursor].password.is_some(),
            updated_at: chrono::Local::now().to_rfc3339(),
        };
        if let Err(e) = recovery::write_manifest(store, &m) {
            tracing::warn!(target: "opdf", "recovery manifest not written: {e}");
        }
    }

    /// Delete this document's recovery files (on close).
    pub fn discard_recovery(&mut self) {
        if let Some(store) = self.store.take() {
            let _ = fs::remove_dir_all(store);
        }
    }
}

fn serialize(pdf: &LoadedPdf, compact: bool) -> Result<Vec<u8>> {
    let state = pdf.security.as_ref().map(|s| &s.state);
    if compact {
        load::serialize_compact(&pdf.doc, state)
    } else {
        load::serialize(&pdf.doc, state)
    }
}

/// AES-256 (revision 6) is defined by PDF 2.0 / Adobe extension level 8.
fn mark_aes256(doc: &mut lopdf::Document) {
    if doc.version.as_str() < "1.7" {
        doc.version = "1.7".into();
    }
    if doc.version.as_str() < "2.0" {
        if let Ok(c) = doc.catalog_mut() {
            c.set(
                "Extensions",
                dictionary! { "ADBE" => dictionary! {
                    "BaseVersion" => Object::Name(b"1.7".to_vec()),
                    "ExtensionLevel" => 8,
                }},
            );
        }
    }
}
