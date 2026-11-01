//! `pdf.detect_scanned`: per-page scan/digital/mixed classification from text-layer character
//! counts and exact union image coverage ratio.

use std::fs;

use ariad_core::{protocol::ErrorCode, tools::params::DetectScannedParams};
use ariad_engine_io::EngineFailure;
use pdfium_render::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    output::{Metrics, ToolContext, ToolResult},
    text_layout::{map_pdfium_error, read_and_preflight_pdf},
};

/// Minimum number of non-whitespace printable characters required for a page to be considered to
/// have a text layer.
pub const DEFAULT_MIN_TEXT_CHARS: usize = 1;

/// Minimum fraction of page area (`0.0..=1.0`) covered by images for a textless page to be
/// classified as `scanned`.
pub const DEFAULT_SCANNED_IMAGE_COVERAGE_MIN: f64 = 0.5;

/// Minimum fraction of page area (`0.0..=1.0`) covered by images for a page with a text layer to
/// be classified as `mixed`.
pub const DEFAULT_MIXED_IMAGE_COVERAGE_MIN: f64 = 0.25;

/// Classification verdict for a page or document.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanVerdict {
    /// Page or document consists of scanned raster images without a digital text layer.
    Scanned,
    /// Page or document is born-digital text (with at most minor image coverage).
    Digital,
    /// Page combines a text layer with significant image coverage, or document mixes scanned and
    /// digital pages.
    Mixed,
}

impl ScanVerdict {
    /// Stable lowercase identifier (`"scanned"`, `"digital"`, `"mixed"`).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Scanned => "scanned",
            Self::Digital => "digital",
            Self::Mixed => "mixed",
        }
    }
}

/// Documented classification thresholds emitted in `detect-scanned.json`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DetectScannedThresholds {
    /// Minimum non-whitespace character count for `has_text_layer` to be `true`.
    pub min_text_chars: usize,
    /// Minimum image coverage ratio (`0.0..=1.0`) for a page without text to be classified as
    /// `scanned`.
    pub scanned_image_coverage_min: f64,
    /// Minimum image coverage ratio (`0.0..=1.0`) for a page with a text layer to be classified as
    /// `mixed`.
    pub mixed_image_coverage_min: f64,
    /// Human-readable description of the decision rule.
    pub rule: String,
}

impl Default for DetectScannedThresholds {
    fn default() -> Self {
        Self {
            min_text_chars: DEFAULT_MIN_TEXT_CHARS,
            scanned_image_coverage_min: DEFAULT_SCANNED_IMAGE_COVERAGE_MIN,
            mixed_image_coverage_min: DEFAULT_MIXED_IMAGE_COVERAGE_MIN,
            rule: "A page has a text layer when text_char_count >= min_text_chars. \
                   Pages without a text layer and image_coverage_ratio >= scanned_image_coverage_min \
                   (or any non-zero image coverage without text) are 'scanned'; pages with a text \
                   layer and image_coverage_ratio >= mixed_image_coverage_min are 'mixed'; all \
                   other pages are 'digital'."
                .to_owned(),
        }
    }
}

/// Per-page classification entry in `detect-scanned.json`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PageScanReport {
    /// 1-based page number.
    pub page: u32,
    /// Whether the page contains at least `thresholds.min_text_chars` non-whitespace characters.
    pub has_text_layer: bool,
    /// Count of non-whitespace, non-control Unicode characters in the page text layer.
    pub text_char_count: usize,
    /// Ratio of the union of image bounding boxes (clipped to the page box) to total page area,
    /// clamped to `[0.0, 1.0]`.
    pub image_coverage_ratio: f64,
    /// Classification verdict for this page.
    pub verdict: ScanVerdict,
}

/// Top-level JSON document written by `pdf.detect_scanned`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DetectScannedDocument {
    /// Schema identifier (`"ariad-pdf-detect-scanned/1"`).
    pub schema: String,
    /// Total number of pages in the PDF.
    pub page_count: usize,
    /// Overall document verdict (`scanned`, `digital`, or `mixed`).
    pub verdict: ScanVerdict,
    /// Count of pages classified as `scanned`.
    pub scanned_pages: usize,
    /// Count of pages classified as `digital`.
    pub digital_pages: usize,
    /// Count of pages classified as `mixed`.
    pub mixed_pages: usize,
    /// Classification thresholds and decision rule.
    pub thresholds: DetectScannedThresholds,
    /// Per-page classification results in 1-based page order.
    pub pages: Vec<PageScanReport>,
}

/// Executes `pdf.detect_scanned`.
pub fn run(ctx: &mut ToolContext<'_>, _params: DetectScannedParams) -> ToolResult {
    let (bytes, _lopdf_doc) = read_and_preflight_pdf(ctx)?;
    let pdfium = ctx.pdfium()?;
    let doc = pdfium
        .load_pdf_from_byte_slice(&bytes, None)
        .map_err(map_pdfium_error)?;

    let total_pages =
        usize::try_from(doc.pages().len()).map_err(|_| EngineFailure::limit_exceeded())?;
    if total_pages == 0 {
        return Err(EngineFailure::new(
            ErrorCode::EngineFailure,
            "The PDF has no pages.",
        ));
    }
    ctx.check_page_count(total_pages)?;

    let thresholds = DetectScannedThresholds::default();
    let max_depth = usize::from(ctx.limits().max_nesting_depth);
    let total_u64 = u64::try_from(total_pages).unwrap_or(u64::MAX);

    let mut pages = Vec::with_capacity(total_pages);
    let mut scanned_pages: usize = 0;
    let mut digital_pages: usize = 0;
    let mut mixed_pages: usize = 0;

    for page_idx in 0..total_pages {
        ctx.check()?;
        let page_index =
            PdfPageIndex::try_from(page_idx).map_err(|_| EngineFailure::limit_exceeded())?;
        let page = doc.pages().get(page_index).map_err(map_pdfium_error)?;

        let text_char_count = count_page_text_chars(ctx, &page)?;
        let has_text_layer = text_char_count >= thresholds.min_text_chars;

        let page_rect = page_bounds(&page);
        let mut image_rects = Vec::new();
        collect_image_rects_from_iter(ctx, page.objects().iter(), 0, max_depth, &mut image_rects)?;

        let image_coverage_ratio = compute_image_coverage_ratio(page_rect, &image_rects);
        let verdict = classify_page(has_text_layer, image_coverage_ratio, &thresholds);

        match verdict {
            ScanVerdict::Scanned => scanned_pages += 1,
            ScanVerdict::Digital => digital_pages += 1,
            ScanVerdict::Mixed => mixed_pages += 1,
        }

        pages.push(PageScanReport {
            page: u32::try_from(page_idx + 1).unwrap_or(u32::MAX),
            has_text_layer,
            text_char_count,
            image_coverage_ratio,
            verdict,
        });

        let done = u64::try_from(page_idx + 1).unwrap_or(u64::MAX);
        ctx.progress("detect_scanned", done, total_u64)?;
    }

    let overall_verdict = classify_document(scanned_pages, digital_pages, mixed_pages);
    let report = DetectScannedDocument {
        schema: "ariad-pdf-detect-scanned/1".to_owned(),
        page_count: total_pages,
        verdict: overall_verdict,
        scanned_pages,
        digital_pages,
        mixed_pages,
        thresholds,
        pages,
    };

    let json_bytes = serde_json::to_vec_pretty(&report).map_err(|_| EngineFailure::io())?;
    let stem = ctx.input_stem();
    let artifact_name = format!("{stem}.detect-scanned.json");
    let artifact_path = ctx.artifact_path(&artifact_name)?;
    fs::write(&artifact_path, &json_bytes).map_err(|_| EngineFailure::io())?;
    ctx.artifact(&artifact_path, "json")?;

    let mut metrics = Metrics::new();
    metrics.insert("pages".to_owned(), json!(total_u64));
    metrics.insert(
        "scanned_pages".to_owned(),
        json!(u64::try_from(scanned_pages).unwrap_or(u64::MAX)),
    );
    metrics.insert(
        "digital_pages".to_owned(),
        json!(u64::try_from(digital_pages).unwrap_or(u64::MAX)),
    );
    metrics.insert(
        "mixed_pages".to_owned(),
        json!(u64::try_from(mixed_pages).unwrap_or(u64::MAX)),
    );
    Ok(Some(metrics))
}

/// Classifies a single page given its `has_text_layer` flag, `image_coverage_ratio` (`0.0..=1.0`),
/// and `thresholds`.
#[must_use]
pub fn classify_page(
    has_text_layer: bool,
    image_coverage_ratio: f64,
    thresholds: &DetectScannedThresholds,
) -> ScanVerdict {
    let ratio = image_coverage_ratio.clamp(0.0, 1.0);
    if !has_text_layer {
        if ratio >= thresholds.scanned_image_coverage_min || ratio > 0.0 {
            ScanVerdict::Scanned
        } else {
            ScanVerdict::Digital
        }
    } else if ratio >= thresholds.mixed_image_coverage_min {
        ScanVerdict::Mixed
    } else {
        ScanVerdict::Digital
    }
}

/// Computes the overall document verdict from per-page counts.
#[must_use]
pub const fn classify_document(
    scanned_pages: usize,
    digital_pages: usize,
    mixed_pages: usize,
) -> ScanVerdict {
    if mixed_pages > 0 || (scanned_pages > 0 && digital_pages > 0) {
        ScanVerdict::Mixed
    } else if scanned_pages > 0 {
        ScanVerdict::Scanned
    } else {
        ScanVerdict::Digital
    }
}

/// Computes the exact 2D union area of `image_rects` intersected with `page_rect` (`[left, bottom,
/// right, top]`), divided by the area of `page_rect` and clamped to `[0.0, 1.0]`.
#[must_use]
pub fn compute_image_coverage_ratio(page_rect: [f64; 4], image_rects: &[[f64; 4]]) -> f64 {
    let p_left = page_rect[0].min(page_rect[2]);
    let p_right = page_rect[0].max(page_rect[2]);
    let p_bottom = page_rect[1].min(page_rect[3]);
    let p_top = page_rect[1].max(page_rect[3]);

    let page_w = (p_right - p_left).max(0.0);
    let page_h = (p_top - p_bottom).max(0.0);
    let page_area = page_w * page_h;
    if !page_area.is_finite() || page_area <= 0.0 || image_rects.is_empty() {
        return 0.0;
    }

    let mut clipped: Vec<[f64; 4]> = Vec::with_capacity(image_rects.len());
    let mut xs: Vec<f64> = Vec::with_capacity(image_rects.len() * 2);

    for rect in image_rects {
        if !rect.iter().all(|v| v.is_finite()) {
            continue;
        }
        let l = rect[0].min(rect[2]).clamp(p_left, p_right);
        let r = rect[0].max(rect[2]).clamp(p_left, p_right);
        let b = rect[1].min(rect[3]).clamp(p_bottom, p_top);
        let t = rect[1].max(rect[3]).clamp(p_bottom, p_top);
        if r > l && t > b {
            clipped.push([l, b, r, t]);
            xs.push(l);
            xs.push(r);
        }
    }

    if clipped.is_empty() {
        return 0.0;
    }

    xs.sort_by(f64::total_cmp);
    xs.dedup_by(|a, b| (*a - *b).abs() <= 1e-9);

    let mut union_area = 0.0;
    let mut y_intervals: Vec<(f64, f64)> = Vec::with_capacity(clipped.len());

    for window in xs.windows(2) {
        let x0 = window[0];
        let x1 = window[1];
        let dx = x1 - x0;
        if dx <= 0.0 {
            continue;
        }
        let mid_x = (x0 + x1) * 0.5;
        y_intervals.clear();
        for rect in &clipped {
            if rect[0] <= mid_x && rect[2] >= mid_x {
                y_intervals.push((rect[1], rect[3]));
            }
        }
        if y_intervals.is_empty() {
            continue;
        }
        y_intervals.sort_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.total_cmp(&b.1)));

        let mut cur_start = y_intervals[0].0;
        let mut cur_end = y_intervals[0].1;
        let mut covered_y = 0.0;
        for &(y0, y1) in &y_intervals[1..] {
            if y0 <= cur_end {
                cur_end = cur_end.max(y1);
            } else {
                covered_y += (cur_end - cur_start).max(0.0);
                cur_start = y0;
                cur_end = y1;
            }
        }
        covered_y += (cur_end - cur_start).max(0.0);
        union_area += dx * covered_y;
    }

    let raw_ratio = (union_area / page_area).clamp(0.0, 1.0);
    ((raw_ratio * 10_000.0).round() / 10_000.0).clamp(0.0, 1.0)
}

fn page_bounds(page: &PdfPage<'_>) -> [f64; 4] {
    if let Ok(box_boundary) = page
        .boundaries()
        .crop()
        .or_else(|_| page.boundaries().media())
    {
        let r = box_boundary.bounds;
        let l = f64::from(r.left().value);
        let right = f64::from(r.right().value);
        let b = f64::from(r.bottom().value);
        let t = f64::from(r.top().value);
        if (right - l).abs() > 0.0 && (t - b).abs() > 0.0 {
            return [l.min(right), b.min(t), l.max(right), b.max(t)];
        }
    }
    let w = f64::from(page.width().value).max(0.0);
    let h = f64::from(page.height().value).max(0.0);
    [0.0, 0.0, w, h]
}

fn count_page_text_chars(
    ctx: &ToolContext<'_>,
    page: &PdfPage<'_>,
) -> Result<usize, EngineFailure> {
    let text_page = page.text().map_err(map_pdfium_error)?;
    let mut count: usize = 0;
    for (idx, char_item) in text_page.chars().iter().enumerate() {
        if idx % 256 == 0 {
            ctx.check()?;
        }
        let s = char_item
            .unicode_string()
            .or_else(|| char_item.unicode_char().map(|c| c.to_string()))
            .unwrap_or_default();
        for ch in s.chars() {
            if ch != '\0' && ch != '\u{feff}' && !ch.is_whitespace() && !ch.is_control() {
                count = count.saturating_add(1);
            }
        }
    }
    Ok(count)
}

fn collect_image_rects_from_iter<'a>(
    ctx: &ToolContext<'_>,
    iter: PdfPageObjectsIterator<'a>,
    depth: usize,
    max_depth: usize,
    out: &mut Vec<[f64; 4]>,
) -> Result<(), EngineFailure> {
    if depth > max_depth {
        return Err(EngineFailure::limit_exceeded());
    }
    for (idx, obj) in iter.enumerate() {
        if idx % 64 == 0 {
            ctx.check()?;
        }
        match obj {
            PdfPageObject::Image(img_obj) => {
                if let Ok(quad) = img_obj.bounds() {
                    let rect = quad.to_rect();
                    let l = f64::from(rect.left().value);
                    let r = f64::from(rect.right().value);
                    let b = f64::from(rect.bottom().value);
                    let t = f64::from(rect.top().value);
                    out.push([l.min(r), b.min(t), l.max(r), b.max(t)]);
                }
            }
            PdfPageObject::XObjectForm(form_obj) => {
                collect_image_rects_from_iter(ctx, form_obj.iter(), depth + 1, max_depth, out)?;
            }
            _ => {}
        }
    }
    Ok(())
}
