//! Implementation of the `pdf.delete_pages` tool.

use ariad_core::tools::params::delete_pages::DeletePagesParams;

use crate::{
    document::{PdfDoc, ToolCtx},
    error::PdfError,
    tools::OutputDoc,
};

/// Deletes selected pages from a PDF document, keeping at least one page.
pub fn run(
    inputs: &[PdfDoc],
    params: &DeletePagesParams,
    ctx: &ToolCtx,
) -> Result<Vec<OutputDoc>, PdfError> {
    let _ = (inputs, params);
    ctx.check()?;
    Err(PdfError::NotImplemented("pdf.delete_pages".to_owned()))
}
