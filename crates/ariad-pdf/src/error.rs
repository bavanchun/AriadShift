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

    /// A resource limit from [`ariad_core::limits::Limits`] was exceeded, or execution was
    /// stopped by a [`crate::ToolCtx`] deadline or cancellation flag.
    #[error("{}", format_limit_or_ctx_error(.0))]
    LimitExceeded(ReadError),

    /// Tool parameters failed validation.
    #[error("invalid tool parameters: {0}")]
    InvalidParams(String),

    /// The document has no table to extract.
    #[error("no tables found in document")]
    NoTables,

    /// The extracted tables exceed the output size cap.
    #[error("the extracted tables exceed the output size limit")]
    TablesTooLarge,

    /// The requested tool is not implemented yet, or a redaction safety refusal.
    ///
    /// Safety refusals carry a message starting with one of [`REFUSAL_PREFIXES`]; they keep
    /// this variant so the machine contract (`tool_missing`, exit 5) stays stable, but their
    /// text reads as a refusal, not as a missing tool.
    #[error("{}", format_not_implemented(.0))]
    NotImplemented(String),
}

/// Message prefixes that mark a [`PdfError::NotImplemented`] as a redaction safety refusal.
///
/// Every refusal reason following these prefixes is a fixed string and never carries
/// document content, names, paths or secret values.
pub const REFUSAL_PREFIXES: [&str; 2] = ["redaction refused: ", "uninspected carrier refused: "];

/// Reports whether `message` is a redaction safety refusal (see [`REFUSAL_PREFIXES`]).
#[must_use]
pub fn is_refusal_message(message: &str) -> bool {
    REFUSAL_PREFIXES
        .iter()
        .any(|prefix| message.starts_with(prefix))
}

fn format_not_implemented(message: &str) -> String {
    if is_refusal_message(message) {
        message.to_owned()
    } else {
        format!("tool `{message}` is not implemented")
    }
}

fn format_limit_or_ctx_error(error: &ReadError) -> String {
    match error {
        ReadError::InputTooLarge {
            actual: 0,
            limit: 0,
        } => "tool execution was cancelled".to_owned(),
        ReadError::TooManyPages {
            actual: 0,
            limit: 0,
        } => "tool execution exceeded the configured timeout".to_owned(),
        other => other.to_string(),
    }
}

impl PdfError {
    /// Sentinel value returned when a [`crate::ToolCtx`] cancellation flag is set.
    pub const CANCELLED: Self = Self::LimitExceeded(ReadError::InputTooLarge {
        actual: 0,
        limit: 0,
    });

    /// Sentinel value returned when a [`crate::ToolCtx`] deadline has expired.
    pub const TIMED_OUT: Self = Self::LimitExceeded(ReadError::TooManyPages {
        actual: 0,
        limit: 0,
    });

    /// Constructs the error returned when cooperative cancellation is triggered.
    #[must_use]
    pub const fn cancelled() -> Self {
        Self::CANCELLED
    }

    /// Constructs the error returned when a cooperative deadline is exceeded.
    #[must_use]
    pub const fn timed_out() -> Self {
        Self::TIMED_OUT
    }

    /// Returns `true` when this error was caused by cooperative cancellation.
    #[must_use]
    pub const fn is_cancelled(&self) -> bool {
        matches!(
            self,
            Self::LimitExceeded(ReadError::InputTooLarge {
                actual: 0,
                limit: 0,
            })
        )
    }

    /// Returns `true` when this error was caused by a [`crate::ToolCtx`] deadline expiring.
    #[must_use]
    pub const fn is_timed_out(&self) -> bool {
        matches!(
            self,
            Self::LimitExceeded(ReadError::TooManyPages {
                actual: 0,
                limit: 0,
            })
        )
    }

    /// Stable fine-grained error code for programmatic diagnostics.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        if self.is_cancelled() {
            return "cancelled";
        }
        if self.is_timed_out() {
            return "limit_exceeded";
        }
        match self {
            Self::Malformed => "malformed_pdf",
            Self::Encrypted { .. } => "encrypted",
            Self::LimitExceeded(_) => "limit_exceeded",
            Self::InvalidParams(_) => "invalid_input",
            Self::NoTables => "no_tables",
            Self::TablesTooLarge => "limit_exceeded",
            Self::NotImplemented(_) => "not_implemented",
        }
    }

    /// Stable CLI/MCP surface error code matching `docs/tools.md`.
    #[must_use]
    pub const fn cli_error_code(&self) -> &'static str {
        if self.is_cancelled() {
            return "interrupted";
        }
        match self {
            Self::Malformed | Self::Encrypted { .. } | Self::InvalidParams(_) => "invalid_input",
            Self::LimitExceeded(_) | Self::TablesTooLarge => "limit_exceeded",
            Self::NoTables | Self::NotImplemented(_) => "engine_failed",
        }
    }

    /// Process exit code matching `docs/tools.md`.
    #[must_use]
    pub const fn exit_code(&self) -> u8 {
        if self.is_cancelled() {
            return 130;
        }
        match self {
            Self::Malformed | Self::Encrypted { .. } | Self::InvalidParams(_) => 2,
            Self::LimitExceeded(_) | Self::TablesTooLarge => 7,
            Self::NoTables | Self::NotImplemented(_) => 6,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refusals_read_as_refusals_and_keep_the_contract() {
        for text in [
            "redaction refused: original image remains reachable",
            "uninspected carrier refused: inline_image",
        ] {
            let error = PdfError::NotImplemented(text.into());
            assert_eq!(error.to_string(), text);
            assert!(is_refusal_message(text));
            assert_eq!(error.code(), "not_implemented");
            assert_eq!(error.cli_error_code(), "engine_failed");
            assert_eq!(error.exit_code(), 6);
        }
    }

    #[test]
    fn unimplemented_tools_keep_their_text() {
        let error = PdfError::NotImplemented("pdf.future".into());
        assert_eq!(error.to_string(), "tool `pdf.future` is not implemented");
        assert!(!is_refusal_message("pdf.future"));
    }
}
