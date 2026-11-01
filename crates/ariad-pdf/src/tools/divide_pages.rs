//! Implementation of the `pdf.divide_pages` tool.

use ariad_core::tools::params::divide_pages::DividePagesParams;

use crate::{
    document::{PdfDoc, ToolCtx},
    error::PdfError,
    tools::OutputDoc,
};

/// Subdivides each selected page into a `cols × rows` grid of smaller pages.
pub fn run(
    inputs: &[PdfDoc],
    params: &DividePagesParams,
    ctx: &ToolCtx,
) -> Result<Vec<OutputDoc>, PdfError> {
    let _ = (inputs, params);
    ctx.check()?;
    Err(PdfError::NotImplemented("pdf.divide_pages".to_owned()))
}
