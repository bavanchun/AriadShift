//! Implementation of the `pdf.insert_blank` tool.

use ariad_core::tools::params::insert_blank::InsertBlankParams;

use crate::{
    document::{PdfDoc, ToolCtx},
    error::PdfError,
    tools::OutputDoc,
};

/// Inserts blank pages after selected pages in a PDF document.
pub fn run(
    inputs: &[PdfDoc],
    params: &InsertBlankParams,
    ctx: &ToolCtx,
) -> Result<Vec<OutputDoc>, PdfError> {
    let _ = (inputs, params);
    ctx.check()?;
    Err(PdfError::NotImplemented("pdf.insert_blank".to_owned()))
}
