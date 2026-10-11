//! # opdf-core
//!
//! The document engine of OpenPDF Studio. It owns everything that changes a
//! PDF's *structure* — pages, merge/split, images to PDF, metadata, password
//! protection, optimisation — plus the versioned [`session::DocSession`] that
//! provides undo/redo, saving and crash recovery.
//!
//! Rendering, text selection, search, form filling and annotation editing
//! happen in the viewer (pdf.js in the desktop app); the viewer hands its
//! changes back as bytes via [`session::DocSession::commit_bytes`].
//!
//! Untrusted input is parsed under a panic guard with decompression limits
//! (see [`load`]).

pub mod assemble;
pub mod error;
pub mod fixtures;
pub mod fsio;
pub mod images;
pub mod info;
pub mod load;
pub mod metadata;
pub mod optimize;
pub mod pages;
pub mod ranges;
pub mod recovery;
pub mod security;
pub mod session;

pub use error::{Error, ErrorCode, Result};
pub use load::{load, LoadedPdf, PermissionSummary};
pub use lopdf;
