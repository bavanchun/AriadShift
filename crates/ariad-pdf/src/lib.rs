//! Pure-Rust, WASM-capable structural PDF manipulation and inspection (`ariad-pdf`).
//!
//! All PDF loads delegate to [`ariad_core::reader::pdf::load_bounded`] and enforce resource
//! bounds from [`ariad_core::limits::Limits`]. Mutating document handles ([`document::open`])
//! refuse encrypted inputs (including owner-password-only files that decrypt with the empty user
//! password) so structural tools never silently strip encryption on save.

#![forbid(unsafe_code)]

pub mod annotations;
pub mod cbz;
pub mod color;
pub mod document;
pub mod draw;
pub mod error;
pub mod flatten;
#[cfg(feature = "codecs")]
pub mod fonts;
pub mod forms;
pub mod gc;
pub mod image_to_pdf;
#[cfg(feature = "codecs")]
pub mod images;
pub mod info;
pub mod interchange;
pub mod naming;
pub mod ocr_layer;
#[cfg(feature = "codecs")]
pub mod optimize;
pub mod paper;
mod pdf_text;
#[cfg(feature = "codecs")]
pub mod recolor;
pub mod scan_pages;
pub mod scan_text;
pub mod tools;
pub mod writer;
pub mod xobject;

/// Re-export of [`document::ToolCtx`] so modules referencing `crate::ctx::ToolCtx` or
/// `ariad_pdf::ToolCtx` share the single cooperative cancellation and deadline context.
pub mod ctx {
    pub use crate::document::ToolCtx;
}

pub use document::{PdfDoc, ToolClock, ToolCtx, TrailerProvenance};
pub use error::PdfError;
pub use info::{PageInfo, PdfInfo, PdfMetadata};
pub use tools::{InputDoc, OutputArtifact, OutputDoc, REPAIRED_XREF_WARNING, ToolOutput, run_tool};
