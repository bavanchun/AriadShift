//! Implementation of the `pdf.posterize` tool.

use ariad_core::tools::params::posterize::PosterizeParams;

use crate::{
    document::{PdfDoc, ToolCtx},
    error::PdfError,
    tools::OutputDoc,
};

/// Enlarges selected pages across multiple tiled output sheets with overlap and crop marks.
pub fn run(
    inputs: &[PdfDoc],
    params: &PosterizeParams,
    ctx: &ToolCtx,
) -> Result<Vec<OutputDoc>, PdfError> {
    let _ = (inputs, params);
    ctx.check()?;
    Err(PdfError::NotImplemented("pdf.posterize".to_owned()))
}
