//! Pure-Rust, WASM-capable structural PDF manipulation and inspection (`ariad-pdf`).
//!
//! All PDF loads delegate to [`ariad_core::reader::pdf::load_bounded`] and enforce resource
//! bounds from [`ariad_core::limits::Limits`]. Mutating document handles ([`document::open`])
//! refuse encrypted inputs (including owner-password-only files that decrypt with the empty user
//! password) so structural tools never silently strip encryption on save.

#![forbid(unsafe_code)]

pub mod document;
pub mod error;
pub mod gc;
pub mod image_to_pdf;
pub mod info;
pub mod naming;
pub mod paper;
pub mod tools;
pub mod writer;
pub mod xobject;

/// Re-export of [`document::ToolCtx`] so modules referencing `crate::ctx::ToolCtx` or
/// `ariad_pdf::ToolCtx` share the single cooperative cancellation and deadline context.
pub mod ctx {
    pub use crate::document::ToolCtx;
}

pub use document::{PdfDoc, ToolCtx, TrailerProvenance};
pub use error::PdfError;
pub use info::{PageInfo, PdfInfo, PdfMetadata};
pub use tools::{InputDoc, OutputArtifact, OutputDoc, REPAIRED_XREF_WARNING, ToolOutput, run_tool};
