//! Implementation of the `pdf.overlay` tool.

use ariad_core::tools::params::overlay::OverlayParams;

use crate::{
    document::{PdfDoc, ToolCtx},
    error::PdfError,
    tools::OutputDoc,
};

/// Overlays or underlays pages from a second PDF onto selected pages of a base PDF.
pub fn run(
    inputs: &[PdfDoc],
    params: &OverlayParams,
    ctx: &ToolCtx,
) -> Result<Vec<OutputDoc>, PdfError> {
    let _ = (inputs, params);
    ctx.check()?;
    Err(PdfError::NotImplemented("pdf.overlay".to_owned()))
}
