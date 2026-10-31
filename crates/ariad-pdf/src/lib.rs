//! Pure-Rust, WASM-capable structural PDF manipulation and inspection (`ariad-pdf`).
//!
//! All PDF loads delegate to [`ariad_core::reader::pdf::load_bounded`] and enforce resource
//! bounds from [`ariad_core::limits::Limits`]. Mutating document handles ([`document::open`])
//! refuse encrypted inputs (including owner-password-only files that decrypt with the empty user
//! password) so structural tools never silently strip encryption on save.

#![forbid(unsafe_code)]

pub mod document;
pub mod error;
pub mod info;

pub use document::{PdfDoc, TrailerProvenance};
pub use error::PdfError;
pub use info::{PageInfo, PdfInfo, PdfMetadata};
