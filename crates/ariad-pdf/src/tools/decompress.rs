//! Implementation of the `pdf.decompress` tool.

use ariad_core::tools::params::decompress::DecompressParams;

use crate::{
    document::{PdfDoc, ToolCtx},
    error::PdfError,
    tools::OutputDoc,
};

/// Decompresses PDF streams and expands object streams within decoded-size limits.
pub fn run(
    inputs: &[PdfDoc],
    params: &DecompressParams,
    ctx: &ToolCtx,
) -> Result<Vec<OutputDoc>, PdfError> {
    let _ = (inputs, params);
    ctx.check()?;
    Err(PdfError::NotImplemented("pdf.decompress".to_owned()))
}
