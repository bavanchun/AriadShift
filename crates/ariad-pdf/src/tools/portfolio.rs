//! Implementation of the `pdf.portfolio` tool.

use ariad_core::tools::params::portfolio::PortfolioParams;

use crate::{
    document::{PdfDoc, ToolCtx},
    error::PdfError,
    tools::OutputDoc,
};

/// Creates, lists, or extracts a PDF collection portfolio (`/Collection`).
pub fn run(
    inputs: &[PdfDoc],
    params: &PortfolioParams,
    ctx: &ToolCtx,
) -> Result<Vec<OutputDoc>, PdfError> {
    let _ = (inputs, params);
    ctx.check()?;
    Err(PdfError::NotImplemented("pdf.portfolio".to_owned()))
}
