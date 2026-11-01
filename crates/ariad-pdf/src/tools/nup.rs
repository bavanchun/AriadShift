//! Implementation of the `pdf.nup` tool.

use ariad_core::tools::params::nup::NupParams;

use crate::{
    document::{PdfDoc, ToolCtx},
    error::PdfError,
    tools::OutputDoc,
};

/// Places multiple input pages onto each output sheet in an N-up grid layout.
pub fn run(
    inputs: &[PdfDoc],
    params: &NupParams,
    ctx: &ToolCtx,
) -> Result<Vec<OutputDoc>, PdfError> {
    let _ = (inputs, params);
    ctx.check()?;
    Err(PdfError::NotImplemented("pdf.nup".to_owned()))
}
