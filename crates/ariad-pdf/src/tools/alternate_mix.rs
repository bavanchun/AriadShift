//! Implementation of the `pdf.alternate_mix` tool.

use ariad_core::tools::params::alternate_mix::AlternateMixParams;

use crate::{
    document::{PdfDoc, ToolCtx},
    error::PdfError,
    tools::OutputDoc,
};

/// Alternately interleaves pages from two PDF documents.
pub fn run(
    inputs: &[PdfDoc],
    params: &AlternateMixParams,
    ctx: &ToolCtx,
) -> Result<Vec<OutputDoc>, PdfError> {
    let _ = (inputs, params);
    ctx.check()?;
    Err(PdfError::NotImplemented("pdf.alternate_mix".to_owned()))
}
