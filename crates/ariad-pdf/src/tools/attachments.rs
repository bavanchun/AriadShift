//! Implementation of the `pdf.attachments` tool.

use ariad_core::tools::params::attachments::AttachmentsParams;

use crate::{
    document::{PdfDoc, ToolCtx},
    error::PdfError,
    tools::OutputDoc,
};

/// Lists, embeds, extracts, or removes embedded file attachments in a PDF document.
pub fn run(
    inputs: &[PdfDoc],
    params: &AttachmentsParams,
    ctx: &ToolCtx,
) -> Result<Vec<OutputDoc>, PdfError> {
    let _ = (inputs, params);
    ctx.check()?;
    Err(PdfError::NotImplemented("pdf.attachments".to_owned()))
}
