//! Stateless PDF page object inspection and ordered edits.

pub mod apply;
pub mod page_objects;

use ariad_core::protocol::ErrorCode;
use ariad_engine_io::EngineFailure;
use pdfium_render::prelude::*;

pub(super) fn invalid(message: impl Into<String>) -> EngineFailure {
    EngineFailure::owned(ErrorCode::InvalidRequest, message.into())
}

pub(super) fn pdfium_error(error: PdfiumError) -> EngineFailure {
    EngineFailure::owned(
        ErrorCode::EngineFailure,
        format!("PDF page edit failed: {error}"),
    )
}

pub(super) fn color(color: PdfColor) -> String {
    if color.alpha() == 255 {
        format!(
            "#{:02X}{:02X}{:02X}",
            color.red(),
            color.green(),
            color.blue()
        )
    } else {
        format!(
            "#{:02X}{:02X}{:02X}{:02X}",
            color.red(),
            color.green(),
            color.blue(),
            color.alpha()
        )
    }
}

pub(super) fn matrix(value: PdfMatrix) -> [f64; 6] {
    [
        value.a(),
        value.b(),
        value.c(),
        value.d(),
        value.e(),
        value.f(),
    ]
    .map(f64::from)
}
