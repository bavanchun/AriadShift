//! Implementation of the `pdf.crop` tool.

use ariad_core::tools::params::crop::CropParams;

use crate::{
    document::{PdfDoc, ToolCtx},
    error::PdfError,
    tools::OutputDoc,
};

/// Sets `/CropBox` or `/MediaBox` on selected pages of a PDF document.
pub fn run(
    inputs: &[PdfDoc],
    params: &CropParams,
    ctx: &ToolCtx,
) -> Result<Vec<OutputDoc>, PdfError> {
    let _ = (inputs, params);
    ctx.check()?;
    Err(PdfError::NotImplemented("pdf.crop".to_owned()))
}
