//! Implementation of the `pdf.reorder` tool.

use ariad_core::tools::params::reorder::ReorderParams;

use crate::{
    document::{PdfDoc, ToolCtx},
    error::PdfError,
    tools::OutputDoc,
};

/// Reorders pages in a PDF document using a preset or explicit page range sequence.
pub fn run(
    inputs: &[PdfDoc],
    params: &ReorderParams,
    ctx: &ToolCtx,
) -> Result<Vec<OutputDoc>, PdfError> {
    let _ = (inputs, params);
    ctx.check()?;
    Err(PdfError::NotImplemented("pdf.reorder".to_owned()))
}
