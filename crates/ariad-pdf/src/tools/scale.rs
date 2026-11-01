//! Implementation of the `pdf.scale` tool.

use ariad_core::tools::params::scale::ScaleParams;

use crate::{
    document::{PdfDoc, ToolCtx},
    error::PdfError,
    tools::OutputDoc,
};

/// Scales selected pages of a PDF document to a target paper size using Form XObjects.
pub fn run(
    inputs: &[PdfDoc],
    params: &ScaleParams,
    ctx: &ToolCtx,
) -> Result<Vec<OutputDoc>, PdfError> {
    let _ = (inputs, params);
    ctx.check()?;
    Err(PdfError::NotImplemented("pdf.scale".to_owned()))
}
