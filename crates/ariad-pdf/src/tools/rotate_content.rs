//! Implementation of the `pdf.rotate_content` tool.

use ariad_core::tools::params::rotate_content::RotateContentParams;

use crate::{
    document::{PdfDoc, ToolCtx},
    error::PdfError,
    tools::OutputDoc,
};

/// Rotates page content streams by an arbitrary angle via Form XObject placement matrices.
pub fn run(
    inputs: &[PdfDoc],
    params: &RotateContentParams,
    ctx: &ToolCtx,
) -> Result<Vec<OutputDoc>, PdfError> {
    let _ = (inputs, params);
    ctx.check()?;
    Err(PdfError::NotImplemented("pdf.rotate_content".to_owned()))
}
