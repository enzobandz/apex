//! Document facts shown in the UI and used to warn before destructive edits.

use crate::load::{LoadedPdf, PermissionSummary};
use crate::metadata::{self, Metadata};
use crate::pages;
use lopdf::{Document, Object};
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocSummary {
    pub page_count: usize,
    pub pdf_version: String,
    pub metadata: Metadata,
    pub permissions: PermissionSummary,
    pub has_forms: bool,
    /// The file carries at least one applied digital signature. Rewriting the
    /// file (any page operation) invalidates it; annotation/form saves are
    /// incremental and keep earlier signatures verifiable.
    pub has_signatures: bool,
    pub has_outline: bool,
    pub tagged: bool,
    pub has_javascript: bool,
    pub has_embedded_files: bool,
}

pub fn summarize(pdf: &LoadedPdf) -> DocSummary {
    let doc = &pdf.doc;
    let catalog = doc.catalog().ok();
    let has = |k: &[u8]| catalog.map(|c| c.has(k)).unwrap_or(false);
    let names = catalog
        .and_then(|c| c.get_deref(b"Names", doc).ok())
        .and_then(|n| n.as_dict().ok());
    DocSummary {
        page_count: pages::page_count(doc),
        pdf_version: doc.version.clone(),
        metadata: metadata::read(doc),
        permissions: pdf.permissions(),
        has_forms: has_form_fields(doc),
        has_signatures: has_signatures(doc),
        has_outline: has(b"Outlines"),
        tagged: has(b"StructTreeRoot"),
        has_javascript: names.map(|n| n.has(b"JavaScript")).unwrap_or(false)
            || has(b"OpenAction") && open_action_is_js(doc),
        has_embedded_files: names.map(|n| n.has(b"EmbeddedFiles")).unwrap_or(false),
    }
}

fn has_form_fields(doc: &Document) -> bool {
    doc.catalog()
        .ok()
        .and_then(|c| c.get_deref(b"AcroForm", doc).ok())
        .and_then(|a| a.as_dict().ok())
        .and_then(|a| a.get_deref(b"Fields", doc).ok())
        .and_then(|f| f.as_array().ok())
        .map(|f| !f.is_empty())
        .unwrap_or(false)
}

/// A signature value dictionary has /ByteRange and /Contents.
pub fn has_signatures(doc: &Document) -> bool {
    doc.objects.values().any(|o| match o {
        Object::Dictionary(d) => d.has(b"ByteRange") && d.has(b"Contents"),
        _ => false,
    })
}

fn open_action_is_js(doc: &Document) -> bool {
    doc.catalog()
        .ok()
        .and_then(|c| c.get_deref(b"OpenAction", doc).ok())
        .and_then(|a| a.as_dict().ok())
        .and_then(|a| a.get(b"S").ok())
        .and_then(|s| s.as_name().ok())
        .map(|s| s == b"JavaScript")
        .unwrap_or(false)
}
