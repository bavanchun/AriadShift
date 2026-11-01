//! Implementation of the `pdf.outline` tool.

use ariad_core::tools::params::outline::OutlineParams;

use crate::{
    document::{PdfDoc, ToolCtx},
    error::PdfError,
    tools::OutputDoc,
};

/// Exports, imports, clears, or generates bookmarks (`/Outlines`) in a PDF document.
pub fn run(
    inputs: &[PdfDoc],
    params: &OutlineParams,
    ctx: &ToolCtx,
) -> Result<Vec<OutputDoc>, PdfError> {
    let _ = (inputs, params);
    ctx.check()?;
    Err(PdfError::NotImplemented("pdf.outline".to_owned()))
}
