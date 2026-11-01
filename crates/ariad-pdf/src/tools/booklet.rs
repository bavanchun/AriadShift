//! Implementation of the `pdf.booklet` tool.

use ariad_core::tools::params::booklet::BookletParams;

use crate::{
    document::{PdfDoc, ToolCtx},
    error::PdfError,
    tools::OutputDoc,
};

/// Imposes pages into saddle-stitch booklet spreads, padding with blank pages to a multiple of 4.
pub fn run(
    inputs: &[PdfDoc],
    params: &BookletParams,
    ctx: &ToolCtx,
) -> Result<Vec<OutputDoc>, PdfError> {
    let _ = (inputs, params);
    ctx.check()?;
    Err(PdfError::NotImplemented("pdf.booklet".to_owned()))
}
