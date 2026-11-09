//! Document comparison tool (`pdf.compare`).
//!
//! Provides semantic text diffing and rasterized visual diffing between two PDF documents,
//! outputting a structured JSON difference report and an annotated PDF report highlighting changes.

pub mod report;
pub mod text;
pub mod visual;

use ariad_core::tools::{
    page_range::DuplicatePolicy,
    params::{CompareMode, CompareParams},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub use report::build_report_pdf;
pub use text::{TextChange, TextChangeKind, WordItem, diff_words, extract_words_from_reader};
pub use visual::{VisualChange, visual_diff_images};

use crate::{InputDoc, OutputArtifact, PdfError, ToolCtx, ToolOutput, tools::InputSource};

/// Comparison status of a specific page.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PageStatus {
    /// Page content is identical between documents.
    Identical,
    /// Differences were detected on this page.
    Changed,
    /// Page exists only in the after document (new page added).
    Added,
    /// Page exists only in the before document (page deleted).
    Removed,
}

/// Comparison result for a single page.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PageCompareResult {
    /// 1-based page index in the comparison sequence.
    pub page_number: u32,
    /// 1-based page number in the before document, if present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_before: Option<u32>,
    /// 1-based page number in the after document, if present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_after: Option<u32>,
    /// Overall comparison status of this page.
    pub status: PageStatus,
    /// Text differences found on this page.
    #[serde(default)]
    pub text_changes: Vec<TextChange>,
    /// Visual raster differences found on this page.
    #[serde(default)]
    pub visual_changes: Vec<VisualChange>,
}

/// Aggregate summary metrics of the comparison.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CompareSummary {
    /// Total pages in the before document.
    pub pages_before: usize,
    /// Total pages in the after document.
    pub pages_after: usize,
    /// Total pages compared.
    pub pages_compared: usize,
    /// Number of pages with zero differences.
    pub pages_identical: usize,
    /// Number of pages with detected differences.
    pub pages_changed: usize,
    /// Total count of detected changes across all compared pages.
    pub total_changes: usize,
    /// Total count of text changes across all compared pages.
    pub text_changes: usize,
    /// Total count of visual changes across all compared pages.
    pub visual_changes: usize,
}

/// Complete report produced by `pdf.compare`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CompareReport {
    /// Aggregate comparison metrics.
    pub summary: CompareSummary,
    /// Per-page comparison results.
    pub pages: Vec<PageCompareResult>,
}

fn doc_bytes(input: &InputDoc) -> Result<Vec<u8>, PdfError> {
    match &input.source {
        InputSource::Bytes(bytes) => Ok(bytes.clone()),
        InputSource::Doc(doc) => {
            let mut cloned = (**doc).clone();
            cloned.save_bytes()
        }
    }
}

/// Executes `pdf.compare` in-process on `inputs`.
pub fn run_inputs(
    inputs: &[InputDoc],
    params: &CompareParams,
    ctx: &ToolCtx,
) -> Result<ToolOutput, PdfError> {
    ctx.check()?;

    if inputs.len() != 2 {
        return Err(PdfError::InvalidParams(format!(
            "pdf.compare requires exactly 2 input PDF documents (before and after), got {}",
            inputs.len()
        )));
    }

    if params.mode != CompareMode::Text {
        return Err(PdfError::InvalidParams(
            "visual comparison requires the out-of-process pdfium engine".into(),
        ));
    }

    let limits = &inputs[0].limits;
    let bytes_before = doc_bytes(&inputs[0])?;
    let bytes_after = doc_bytes(&inputs[1])?;

    let read_before =
        ariad_core::reader::pdf::read(&bytes_before, limits).map_err(|_| PdfError::Malformed)?;
    let read_after =
        ariad_core::reader::pdf::read(&bytes_after, limits).map_err(|_| PdfError::Malformed)?;

    let pages_before = read_before
        .document
        .layout
        .as_ref()
        .map(|l| l.pages.len())
        .unwrap_or(0)
        .max(
            ariad_core::reader::pdf::PdfStats::from_document(&read_before.document)
                .map(|s| s.pages as usize)
                .unwrap_or(1),
        );
    let pages_after = read_after
        .document
        .layout
        .as_ref()
        .map(|l| l.pages.len())
        .unwrap_or(0)
        .max(
            ariad_core::reader::pdf::PdfStats::from_document(&read_after.document)
                .map(|s| s.pages as usize)
                .unwrap_or(1),
        );
    let max_pages = pages_before.max(pages_after).max(1);

    let page_numbers = if let Some(ref range) = params.pages {
        range
            .expand(max_pages as u32, DuplicatePolicy::Reject)
            .map_err(|e| PdfError::InvalidParams(e.to_string()))?
    } else {
        (1..=max_pages as u32).collect()
    };

    let mut page_results = Vec::with_capacity(page_numbers.len());
    let mut total_text_changes = 0usize;
    let mut identical_count = 0usize;
    let mut changed_count = 0usize;

    for &page_no in &page_numbers {
        ctx.check()?;

        let has_before = (page_no as usize) <= pages_before;
        let has_after = (page_no as usize) <= pages_after;

        if has_before && has_after {
            let before_words = extract_words_from_reader(&read_before.document, page_no, limits);
            let after_words = extract_words_from_reader(&read_after.document, page_no, limits);

            let text_changes = diff_words(&before_words, &after_words);
            let is_identical = text_changes.is_empty();

            if is_identical {
                identical_count += 1;
            } else {
                changed_count += 1;
            }
            total_text_changes += text_changes.len();

            page_results.push(PageCompareResult {
                page_number: page_no,
                page_before: Some(page_no),
                page_after: Some(page_no),
                status: if is_identical {
                    PageStatus::Identical
                } else {
                    PageStatus::Changed
                },
                text_changes,
                visual_changes: Vec::new(),
            });
        } else if !has_before && has_after {
            changed_count += 1;
            page_results.push(PageCompareResult {
                page_number: page_no,
                page_before: None,
                page_after: Some(page_no),
                status: PageStatus::Added,
                text_changes: Vec::new(),
                visual_changes: Vec::new(),
            });
        } else if has_before && !has_after {
            changed_count += 1;
            page_results.push(PageCompareResult {
                page_number: page_no,
                page_before: Some(page_no),
                page_after: None,
                status: PageStatus::Removed,
                text_changes: Vec::new(),
                visual_changes: Vec::new(),
            });
        }
    }

    let summary = CompareSummary {
        pages_before,
        pages_after,
        pages_compared: page_results.len(),
        pages_identical: identical_count,
        pages_changed: changed_count,
        total_changes: total_text_changes,
        text_changes: total_text_changes,
        visual_changes: 0,
    };

    let report = CompareReport {
        summary,
        pages: page_results,
    };

    let report_pdf_bytes = build_report_pdf(&bytes_after, &report, limits, ctx)?;

    let json_val =
        serde_json::to_value(&report).map_err(|e| PdfError::InvalidParams(e.to_string()))?;
    let json_bytes =
        serde_json::to_vec_pretty(&report).map_err(|e| PdfError::InvalidParams(e.to_string()))?;

    Ok(ToolOutput {
        artifacts: vec![
            OutputArtifact {
                name: "report.json".into(),
                format: "json".into(),
                bytes: json_bytes,
            },
            OutputArtifact {
                name: "report.pdf".into(),
                format: "pdf".into(),
                bytes: report_pdf_bytes,
            },
        ],
        warnings: Vec::new(),
        coded_warnings: Vec::new(),
        metrics: Default::default(),
        json: Some(json_val),
    })
}
