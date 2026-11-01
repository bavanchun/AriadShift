//! Implementation of the `pdf.split` tool.

use ariad_core::tools::params::split::SplitParams;

use crate::{
    document::{PdfDoc, ToolCtx},
    error::PdfError,
    tools::OutputDoc,
};

/// Splits a PDF document into multiple part PDFs.
pub fn run(
    inputs: &[PdfDoc],
    params: &SplitParams,
    ctx: &ToolCtx,
) -> Result<Vec<OutputDoc>, PdfError> {
    let _ = (inputs, params);
    ctx.check()?;
    Err(PdfError::NotImplemented("pdf.split".to_owned()))
}
