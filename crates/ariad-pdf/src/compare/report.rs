//! Build an annotated PDF report using Phase 8 annotation code.

use ariad_core::limits::Limits;

use crate::{PdfError, ToolCtx, compare::CompareReport};

#[cfg(feature = "codecs")]
use crate::{annotations::write, compare::TextChangeKind, document};
#[cfg(feature = "codecs")]
use ariad_core::tools::params::annotations::{AnnotationItem, AnnotationType};

/// Builds the annotated report PDF by decorating the `after_pdf` with highlight and rectangle annotations.
#[cfg(feature = "codecs")]
pub fn build_report_pdf(
    after_pdf_bytes: &[u8],
    report: &CompareReport,
    limits: &Limits,
    ctx: &ToolCtx,
) -> Result<Vec<u8>, PdfError> {
    ctx.check()?;

    let mut items = Vec::new();

    for page in &report.pages {
        let Some(page_no) = page.page_after else {
            continue;
        };

        // Text changes
        for change in &page.text_changes {
            let [x0, y0, x1, y1] = change.bbox;
            let (annotation_type, color, quads) = match change.kind {
                TextChangeKind::Insert => (
                    AnnotationType::Highlight,
                    "#10B981", // Emerald green
                    Some(vec![[x0, y1, x1, y1, x0, y0, x1, y0]]),
                ),
                TextChangeKind::Replace => (
                    AnnotationType::Highlight,
                    "#F59E0B", // Amber
                    Some(vec![[x0, y1, x1, y1, x0, y0, x1, y0]]),
                ),
                TextChangeKind::Delete => (
                    AnnotationType::Rectangle,
                    "#EF4444", // Red
                    None,
                ),
                TextChangeKind::Move => (
                    AnnotationType::Rectangle,
                    "#3B82F6", // Blue
                    None,
                ),
            };

            let contents = match change.kind {
                TextChangeKind::Insert => {
                    format!("Insert: {}", change.text_after.as_deref().unwrap_or(""))
                }
                TextChangeKind::Delete => {
                    format!("Delete: {}", change.text_before.as_deref().unwrap_or(""))
                }
                TextChangeKind::Replace => format!(
                    "Replace: \"{}\" -> \"{}\"",
                    change.text_before.as_deref().unwrap_or(""),
                    change.text_after.as_deref().unwrap_or("")
                ),
                TextChangeKind::Move => {
                    format!("Move: \"{}\"", change.text_after.as_deref().unwrap_or(""))
                }
            };

            let item = AnnotationItem {
                id: None,
                page: page_no,
                annotation_type,
                rect: Some([x0, y0, x1, y1]),
                quads,
                points: None,
                strokes: None,
                color: Some(color.to_string()),
                fill: None,
                opacity: 0.6,
                line_width: 1.5,
                contents: Some(contents.clone()),
                author: Some("AriadShift Compare".to_string()),
                subject: Some(format!("{:?}", change.kind)),
                text: Some(contents),
                font_size: 10.0,
                stamp: None,
                uri: None,
                dest_page: None,
                flags: None,
            };
            item.validate().map_err(PdfError::InvalidParams)?;
            items.push(item);
        }

        // Visual changes
        for change in &page.visual_changes {
            let [x0, y0, x1, y1] = change.bbox;
            let contents = format!("Visual change ({} diff pixels)", change.diff_pixels);

            let item = AnnotationItem {
                id: None,
                page: page_no,
                annotation_type: AnnotationType::Rectangle,
                rect: Some([x0, y0, x1, y1]),
                quads: None,
                points: None,
                strokes: None,
                color: Some("#8B5CF6".to_string()), // Purple
                fill: None,
                opacity: 0.5,
                line_width: 1.5,
                contents: Some(contents.clone()),
                author: Some("AriadShift Compare".to_string()),
                subject: Some("VisualDiff".to_string()),
                text: Some(contents),
                font_size: 10.0,
                stamp: None,
                uri: None,
                dest_page: None,
                flags: None,
            };
            item.validate().map_err(PdfError::InvalidParams)?;
            items.push(item);
        }
    }

    if items.is_empty() {
        return Ok(after_pdf_bytes.to_vec());
    }

    let mut doc = document::open_with_ctx(after_pdf_bytes, limits, ctx)?;
    let _ = write::add(&mut doc, &items, limits, ctx)?;
    doc.save_bytes()
}

/// Fallback when `codecs` feature is not enabled.
#[cfg(not(feature = "codecs"))]
pub fn build_report_pdf(
    after_pdf_bytes: &[u8],
    _report: &CompareReport,
    _limits: &Limits,
    _ctx: &ToolCtx,
) -> Result<Vec<u8>, PdfError> {
    Ok(after_pdf_bytes.to_vec())
}
