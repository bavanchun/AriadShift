//! Implementation of the `pdf.page_labels` tool.

use ariad_core::tools::params::page_labels::PageLabelsParams;

use crate::{
    document::{PdfDoc, ToolCtx},
    error::PdfError,
    tools::OutputDoc,
};

/// Updates the `/PageLabels` number tree on the PDF catalog.
pub fn run(
    inputs: &[PdfDoc],
    params: &PageLabelsParams,
    ctx: &ToolCtx,
) -> Result<Vec<OutputDoc>, PdfError> {
    let _ = (inputs, params);
    ctx.check()?;
    Err(PdfError::NotImplemented("pdf.page_labels".to_owned()))
}
