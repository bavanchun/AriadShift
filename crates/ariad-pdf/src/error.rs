//! Typed error definitions for `ariad-pdf`.

use ariad_core::reader::pdf::ReadError;
use thiserror::Error;

/// Typed errors returned by `ariad-pdf` document and tool operations.
///
/// Error messages never include document content or secret parameter values.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum PdfError {
    /// The input is not a valid or readable PDF document.
    #[error("the file is not a readable PDF")]
    Malformed,

    /// The PDF is encrypted.
    ///
    /// When `needs_password` is `true`, a non-empty user password is required to open the file.
    /// When `needs_password` is `false`, the file opens with the empty user password (owner-password
    /// protection only), which read-only inspection (`pdf.info`) may report as `encrypted: true`
    /// while mutating tools refuse to open or save without explicit unlock.
    #[error(
        "the PDF is encrypted ({})",
        if *.needs_password { "password required" } else { "owner-password protected" }
    )]
    Encrypted {
        /// Whether a non-empty user password is required to decrypt the document.
        needs_password: bool,
    },

    /// A resource limit from [`ariad_core::limits::Limits`] was exceeded.
    #[error("{0}")]
    LimitExceeded(ReadError),

    /// Tool parameters failed validation.
    #[error("invalid tool parameters: {0}")]
    InvalidParams(String),

    /// The requested tool is not implemented yet.
    #[error("tool `{0}` is not implemented")]
    NotImplemented(String),
}

impl PdfError {
    /// Stable fine-grained error code for programmatic diagnostics.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Malformed => "malformed_pdf",
            Self::Encrypted { .. } => "encrypted",
            Self::LimitExceeded(_) => "limit_exceeded",
            Self::InvalidParams(_) => "invalid_input",
            Self::NotImplemented(_) => "not_implemented",
        }
    }

    /// Stable CLI/MCP surface error code matching `docs/tools.md`.
    #[must_use]
    pub const fn cli_error_code(&self) -> &'static str {
        match self {
            Self::Malformed | Self::Encrypted { .. } | Self::InvalidParams(_) => "invalid_input",
            Self::LimitExceeded(_) => "limit_exceeded",
            Self::NotImplemented(_) => "engine_failed",
        }
    }

    /// Process exit code matching `docs/tools.md`.
    #[must_use]
    pub const fn exit_code(&self) -> u8 {
        match self {
            Self::Malformed | Self::Encrypted { .. } | Self::InvalidParams(_) => 2,
            Self::LimitExceeded(_) => 7,
            Self::NotImplemented(_) => 6,
        }
    }
}

impl From<ReadError> for PdfError {
    fn from(error: ReadError) -> Self {
        match error {
            ReadError::Malformed => Self::Malformed,
            ReadError::Encrypted => Self::Encrypted {
                needs_password: true,
            },
            other => Self::LimitExceeded(other),
        }
    }
}
