//! Implementation of the `pdf.metadata` tool.

use ariad_core::tools::params::metadata::MetadataParams;

use crate::{
    document::{PdfDoc, ToolCtx},
    error::PdfError,
    tools::OutputDoc,
};

/// Updates, removes, or strips document metadata across `/Info` and XMP `/Metadata`.
pub fn run(
    inputs: &[PdfDoc],
    params: &MetadataParams,
    ctx: &ToolCtx,
) -> Result<Vec<OutputDoc>, PdfError> {
    let _ = (inputs, params);
    ctx.check()?;
    Err(PdfError::NotImplemented("pdf.metadata".to_owned()))
}
