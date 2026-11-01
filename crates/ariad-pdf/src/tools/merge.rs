//! Implementation of the `pdf.merge` tool.

use ariad_core::tools::params::merge::MergeParams;

use crate::{
    document::{PdfDoc, ToolCtx},
    error::PdfError,
    tools::OutputDoc,
};

/// Merges multiple PDF documents into a single PDF.
pub fn run(
    inputs: &[PdfDoc],
    params: &MergeParams,
    ctx: &ToolCtx,
) -> Result<Vec<OutputDoc>, PdfError> {
    let _ = (inputs, params);
    ctx.check()?;
    Err(PdfError::NotImplemented("pdf.merge".to_owned()))
}
