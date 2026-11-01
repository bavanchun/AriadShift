//! Implementation of the `pdf.single_page` tool.

use ariad_core::tools::params::single_page::SinglePageParams;

use crate::{
    document::{PdfDoc, ToolCtx},
    error::PdfError,
    tools::OutputDoc,
};

/// Concatenates all pages of a PDF document vertically into a single continuous page.
pub fn run(
    inputs: &[PdfDoc],
    params: &SinglePageParams,
    ctx: &ToolCtx,
) -> Result<Vec<OutputDoc>, PdfError> {
    let _ = (inputs, params);
    ctx.check()?;
    Err(PdfError::NotImplemented("pdf.single_page".to_owned()))
}
