//! Error model shared by the engine, the CLI and the desktop shell.
//!
//! Every error carries a stable machine-readable [`ErrorCode`] so the UI can
//! react (e.g. show a password prompt) without parsing messages.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// The document is encrypted and no (or an empty) password was supplied.
    PasswordRequired,
    /// A password was supplied but it opens neither the user nor owner role.
    WrongPassword,
    /// The bytes are not a PDF, or are too damaged to parse.
    InvalidPdf,
    /// The document's security settings forbid this operation.
    PermissionDenied,
    /// The input exceeds a safety limit (size, page count, decompression).
    LimitExceeded,
    /// A caller-supplied argument is invalid (bad page range, empty selection...).
    InvalidArgument,
    /// The PDF uses a feature this operation does not support.
    Unsupported,
    /// File system error.
    Io,
    /// The file on disk changed since it was opened; saving would overwrite
    /// someone else's changes.
    FileChanged,
    /// A bug or an unexpected condition (including a caught panic).
    Internal,
}

#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct Error {
    pub code: ErrorCode,
    pub message: String,
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
    pub fn invalid_pdf(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidPdf, message)
    }
    pub fn invalid_arg(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidArgument, message)
    }
    pub fn unsupported(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Unsupported, message)
    }
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Internal, message)
    }
    pub fn denied(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::PermissionDenied, message)
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::new(ErrorCode::Io, e.to_string())
    }
}

impl From<lopdf::Error> for Error {
    fn from(e: lopdf::Error) -> Self {
        match e {
            lopdf::Error::IO(io) => io.into(),
            lopdf::Error::Decryption(d) => {
                Self::new(ErrorCode::WrongPassword, format!("could not decrypt: {d}"))
            }
            other => Self::invalid_pdf(other.to_string()),
        }
    }
}

impl From<image::ImageError> for Error {
    fn from(e: image::ImageError) -> Self {
        match e {
            image::ImageError::Unsupported(u) => Self::unsupported(u.to_string()),
            image::ImageError::Limits(l) => Self::new(ErrorCode::LimitExceeded, l.to_string()),
            image::ImageError::IoError(io) => io.into(),
            other => Self::invalid_arg(format!("image could not be decoded: {other}")),
        }
    }
}

/// Run `f`, converting a panic inside third-party parsing code into an
/// [`ErrorCode::Internal`] error instead of tearing down the process.
pub fn guard<T>(what: &str, f: impl FnOnce() -> Result<T>) -> Result<T> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(r) => r,
        Err(payload) => {
            let detail = payload
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "unknown panic".into());
            tracing::error!(target: "opdf", "panic during {what}: {detail}");
            Err(Error::internal(format!(
                "{what} failed unexpectedly while processing this file ({detail})"
            )))
        }
    }
}
