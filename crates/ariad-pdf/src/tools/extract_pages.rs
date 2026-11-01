//! Implementation of the `pdf.extract_pages` tool.

use ariad_core::tools::{page_range::DuplicatePolicy, params::extract_pages::ExtractPagesParams};

use super::page_set::{
    PageSetOptions, build_page_set_output, doc_page_count, expand_page_range_capped,
};
use crate::{
    document::{PdfDoc, ToolCtx},
    error::PdfError,
    tools::OutputDoc,
};

/// Extracts selected pages from a PDF document into a new PDF.
pub fn run(
    inputs: &[PdfDoc],
    params: &ExtractPagesParams,
    ctx: &ToolCtx,
) -> Result<Vec<OutputDoc>, PdfError> {
    ctx.check()?;
    if inputs.len() != 1 {
        return Err(PdfError::InvalidParams(format!(
            "pdf.extract_pages requires exactly 1 input document (got {})",
            inputs.len()
        )));
    }
    let source = &inputs[0];
    let page_count = doc_page_count(source);
    let selected = expand_page_range_capped(
        &params.pages,
        page_count,
        DuplicatePolicy::KeepInOrder,
        ctx.max_pages,
    )?;
    if selected.is_empty() {
        return Err(PdfError::InvalidParams(
            "selected page range produced 0 pages".to_owned(),
        ));
    }

    let extracted = build_page_set_output(source, &selected, PageSetOptions::default(), ctx)?;
    Ok(vec![extracted])
}
