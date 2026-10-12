//! Safe loading and serialisation of untrusted PDF bytes.
//!
//! * Every parse runs under [`guard`] so a panic in the parser becomes an error.
//! * Streams are decompressed with a per-stream ceiling (decompression bombs).
//! * Encrypted documents are decrypted in memory; the original security
//!   handler is kept in [`Security`] and re-applied by [`serialize`], so saving
//!   never silently strips a document's protection.

use crate::error::{guard, Error, ErrorCode, Result};
use lopdf::{Document, EncryptionState, Object, Permissions};
use serde::Serialize;

/// Hard ceiling for files we try to parse (bytes).
pub const MAX_FILE_SIZE: u64 = 2 * 1024 * 1024 * 1024;
/// Per-stream decompression ceiling while loading (object and xref streams).
pub const MAX_STREAM_DECOMPRESSED: usize = 512 * 1024 * 1024;
/// Documents with more pages are rejected by structural operations.
pub const MAX_PAGES: usize = 100_000;

/// The security handler of an encrypted document, kept so the document can
/// be re-encrypted identically when written.
#[derive(Clone, Debug)]
pub struct Security {
    pub state: EncryptionState,
    /// `true` when the supplied password authenticated the owner role (or the
    /// owner password is empty), which unlocks every operation.
    pub owner_access: bool,
}

/// What the current user may do with a document, derived from its security
/// handler. Unencrypted documents allow everything.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PermissionSummary {
    pub encrypted: bool,
    pub owner_access: bool,
    pub print: bool,
    pub modify: bool,
    pub copy: bool,
    pub annotate: bool,
    pub fill_forms: bool,
    pub assemble: bool,
}

impl PermissionSummary {
    pub const ALL: PermissionSummary = PermissionSummary {
        encrypted: false,
        owner_access: true,
        print: true,
        modify: true,
        copy: true,
        annotate: true,
        fill_forms: true,
        assemble: true,
    };

    pub fn from_security(sec: Option<&Security>) -> Self {
        let Some(sec) = sec else { return Self::ALL };
        if sec.owner_access {
            return Self {
                encrypted: true,
                ..Self::ALL
            };
        }
        let p = sec.state.permissions();
        let modify = p.contains(Permissions::MODIFIABLE);
        Self {
            encrypted: true,
            owner_access: false,
            print: p.contains(Permissions::PRINTABLE),
            modify,
            copy: p.contains(Permissions::COPYABLE),
            annotate: p.contains(Permissions::ANNOTABLE),
            fill_forms: p.contains(Permissions::FILLABLE) || p.contains(Permissions::ANNOTABLE),
            // Revision >= 3: assemble is its own bit; MODIFIABLE implies it.
            assemble: modify || p.contains(Permissions::ASSEMBLABLE),
        }
    }
}

/// A parsed document plus its security handler (if it was encrypted).
#[derive(Clone, Debug)]
pub struct LoadedPdf {
    pub doc: Document,
    pub security: Option<Security>,
}

impl LoadedPdf {
    pub fn permissions(&self) -> PermissionSummary {
        PermissionSummary::from_security(self.security.as_ref())
    }

    /// Serialise (re-encrypting if the source was encrypted).
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        serialize(&self.doc, self.security.as_ref().map(|s| &s.state))
    }
}

/// Quick check that `bytes` look like a PDF (header within the first KiB,
/// as tolerated by mainstream readers).
pub fn looks_like_pdf(bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(1024)];
    head.windows(5).any(|w| w == b"%PDF-")
}

/// Parse `bytes`, decrypting with `password` if the file is encrypted.
///
/// Errors: [`ErrorCode::PasswordRequired`] when the file needs a password and
/// none (or an empty one) was supplied, [`ErrorCode::WrongPassword`] when the
/// supplied password is wrong, [`ErrorCode::InvalidPdf`] for damaged input.
pub fn load(bytes: &[u8], password: Option<&str>) -> Result<LoadedPdf> {
    if bytes.len() as u64 > MAX_FILE_SIZE {
        return Err(Error::new(
            ErrorCode::LimitExceeded,
            "files larger than 2 GiB are not supported",
        ));
    }
    if !looks_like_pdf(bytes) {
        return Err(Error::invalid_pdf("this file is not a PDF document"));
    }
    let password = password.filter(|p| !p.is_empty());
    guard("opening the PDF", || {
        let options = lopdf::LoadOptions {
            password: password.map(str::to_owned),
            max_decompressed_size: Some(MAX_STREAM_DECOMPRESSED),
            ..Default::default()
        };
        let doc = match Document::load_mem_with_options(bytes, options) {
            Ok(doc) => doc,
            Err(lopdf::Error::InvalidPassword) => {
                return Err(Error::new(
                    ErrorCode::WrongPassword,
                    "the password is incorrect",
                ))
            }
            Err(lopdf::Error::Decryption(e)) => {
                return Err(Error::unsupported(format!(
                    "this document's encryption is not supported ({e})"
                )))
            }
            Err(e) => {
                return Err(Error::invalid_pdf(format!(
                    "the PDF could not be read: {e}"
                )))
            }
        };
        if doc.is_encrypted() {
            // lopdf leaves the document encrypted when no password matched.
            return Err(Error::new(
                ErrorCode::PasswordRequired,
                "this document is password protected",
            ));
        }
        if doc.catalog().is_err() {
            return Err(Error::invalid_pdf("the PDF has no document catalog"));
        }
        let security = match doc.encryption_state.clone() {
            None => None,
            Some(state) => {
                let owner_access = owner_authenticates(&doc, &state, password.unwrap_or(""));
                Some(Security {
                    state,
                    owner_access,
                })
            }
        };
        let pages = doc.get_pages().len();
        if pages > MAX_PAGES {
            return Err(Error::new(
                ErrorCode::LimitExceeded,
                format!("documents with more than {MAX_PAGES} pages are not supported"),
            ));
        }
        Ok(LoadedPdf { doc, security })
    })
}

/// Rebuild a minimal document holding the original `/Encrypt` dictionary and
/// trailer `/ID` so the owner password can be checked after decryption.
fn owner_authenticates(doc: &Document, state: &EncryptionState, password: &str) -> bool {
    let Ok(dict) = state.encode() else {
        return false;
    };
    let mut probe = Document::with_version(doc.version.clone());
    let id = probe.add_object(Object::Dictionary(dict));
    probe.trailer.set("Encrypt", Object::Reference(id));
    if let Ok(file_id) = doc.trailer.get(b"ID") {
        probe.trailer.set("ID", file_id.clone());
    }
    probe.authenticate_owner_password(password).is_ok()
}

/// Give the document a trailer `/ID` if it lacks a valid one (required for
/// encrypted files, recommended for all).
pub fn ensure_file_id(doc: &mut Document) {
    let valid = matches!(
        doc.trailer.get(b"ID"),
        Ok(Object::Array(a)) if a.len() == 2 && a.iter().all(|o| o.as_str().is_ok())
    );
    if !valid {
        let id: [u8; 16] = rand::random();
        let s = Object::String(id.to_vec(), lopdf::StringFormat::Hexadecimal);
        doc.trailer.set("ID", Object::Array(vec![s.clone(), s]));
    }
}

/// Write `doc` to bytes, encrypting with `security` when given.
pub fn serialize(doc: &Document, security: Option<&EncryptionState>) -> Result<Vec<u8>> {
    guard("writing the PDF", || {
        let mut out = Vec::new();
        let mut d = doc.clone();
        ensure_file_id(&mut d);
        if let Some(state) = security {
            d.encryption_state = None;
            d.encrypt(state)?;
        }
        d.save_to(&mut out)?;
        Ok(out)
    })
}

/// Serialise with object and cross-reference streams (smaller files).
pub fn serialize_compact(doc: &Document, security: Option<&EncryptionState>) -> Result<Vec<u8>> {
    guard("writing the PDF", || {
        let mut d = doc.clone();
        ensure_file_id(&mut d);
        if let Some(state) = security {
            d.encryption_state = None;
            d.encrypt(state)?;
        }
        let mut out = Vec::new();
        let options = lopdf::SaveOptions::builder()
            .use_object_streams(true)
            .use_xref_streams(true)
            .compression_level(9)
            .build();
        d.save_with_options(&mut out, options)?;
        Ok(out)
    })
}
