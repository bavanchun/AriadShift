//! Implementation of the `pdf.rotate` tool.

use ariad_core::tools::params::rotate::RotateParams;

use crate::{
    document::{PdfDoc, ToolCtx},
    error::PdfError,
    tools::OutputDoc,
};

/// Rotates selected pages in a PDF document by updating `/Rotate`.
pub fn run(
    inputs: &[PdfDoc],
    params: &RotateParams,
    ctx: &ToolCtx,
) -> Result<Vec<OutputDoc>, PdfError> {
    let _ = (inputs, params);
    ctx.check()?;
    Err(PdfError::NotImplemented("pdf.rotate".to_owned()))
}
