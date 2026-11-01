//! Implementation of the `pdf.layers` tool.

use ariad_core::tools::params::layers::LayersParams;

use crate::{
    document::{PdfDoc, ToolCtx},
    error::PdfError,
    tools::OutputDoc,
};

/// Lists, toggles default visibility, or removes optional content groups (`/OCProperties`).
pub fn run(
    inputs: &[PdfDoc],
    params: &LayersParams,
    ctx: &ToolCtx,
) -> Result<Vec<OutputDoc>, PdfError> {
    let _ = (inputs, params);
    ctx.check()?;
    Err(PdfError::NotImplemented("pdf.layers".to_owned()))
}
