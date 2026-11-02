//! Implementation of the `pdf.delete_pages` tool.

use std::collections::BTreeSet;

use ariad_core::tools::{page_range::DuplicatePolicy, params::delete_pages::DeletePagesParams};

use crate::{
    document::{PdfDoc, ToolCtx},
    error::PdfError,
    tools::{
        OutputDoc,
        page_set::{PageSetOptions, build_page_set_output, doc_page_count},
    },
};

/// Deletes selected pages from a PDF document, keeping at least one page.
pub fn run(
    inputs: &[PdfDoc],
    params: &DeletePagesParams,
    ctx: &ToolCtx,
) -> Result<Vec<OutputDoc>, PdfError> {
    ctx.check()?;
    if inputs.len() != 1 {
        return Err(PdfError::InvalidParams(format!(
            "pdf.delete_pages requires exactly 1 input document (got {})",
            inputs.len()
        )));
    }
    let source = &inputs[0];
    let page_count = doc_page_count(source);
    let deleted = params
        .pages
        .expand(page_count, DuplicatePolicy::Reject)
        .map_err(|e| PdfError::InvalidParams(e.to_string()))?;
    let deleted_set: BTreeSet<u32> = deleted.into_iter().collect();

    let surviving: Vec<u32> = (1..=page_count)
        .filter(|p| !deleted_set.contains(p))
        .collect();

    if surviving.is_empty() {
        return Err(PdfError::InvalidParams(
            "cannot delete all pages; at least 1 page must remain".to_owned(),
        ));
    }

    let trimmed = build_page_set_output(source, &surviving, PageSetOptions::default(), ctx)?;
    Ok(vec![trimmed])
}

/// Deletes the 1-based `deleted_pages` from `source`, keeping at least 1 page, and returns the
/// serialized deterministic PDF bytes and any non-fatal warnings.
pub fn delete_page_numbers(
    source: &PdfDoc,
    deleted_pages: &[u32],
    ctx: &ToolCtx,
) -> Result<(Vec<u8>, Vec<String>), PdfError> {
    ctx.check()?;
    let page_count = doc_page_count(source);
    let deleted_set: BTreeSet<u32> = deleted_pages.iter().copied().collect();
    let surviving: Vec<u32> = (1..=page_count)
        .filter(|p| !deleted_set.contains(p))
        .collect();
    if surviving.is_empty() {
        return Err(PdfError::InvalidParams(
            "cannot delete all pages; at least 1 page must remain".to_owned(),
        ));
    }
    let trimmed = build_page_set_output(source, &surviving, PageSetOptions::default(), ctx)?;
    let tool_out = crate::tools::finalize_tool_output(
        "pdf.delete_pages",
        std::slice::from_ref(source),
        "document",
        vec![trimmed],
        ctx,
    )?;
    let bytes = tool_out
        .artifacts
        .into_iter()
        .next()
        .map(|a| a.bytes)
        .unwrap_or_default();
    Ok((bytes, tool_out.warnings))
}
