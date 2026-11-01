//! Implementation of the `pdf.extract_pages` tool.

use ariad_core::tools::params::extract_pages::ExtractPagesParams;

use crate::{
    document::{PdfDoc, ToolCtx},
    error::PdfError,
    tools::OutputDoc,
};

/// Extracts selected pages from a PDF document into a new PDF.
pub fn run(
    inputs: &[PdfDoc],
    params: &ExtractPagesParams,
    ctx: &ToolCtx,
) -> Result<Vec<OutputDoc>, PdfError> {
    let _ = (inputs, params);
    ctx.check()?;
    Err(PdfError::NotImplemented("pdf.extract_pages".to_owned()))
}
