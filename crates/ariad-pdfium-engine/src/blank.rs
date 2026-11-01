//! The `pdf.remove_blank` tool, the shared pixel blank-page detector, and the pixel
//! `blank-separator` mode of `pdf.split`.

use std::fs;

use ariad_core::{
    protocol::ErrorCode,
    tools::params::{
        RemoveBlankParams,
        remove_blank::{DEFAULT_DPI, DEFAULT_THRESHOLD},
        render::{MAX_DPI, MIN_DPI},
    },
};
use ariad_engine_io::EngineFailure;
use ariad_pdf::{
    PdfDoc, PdfError, ToolCtx as PdfToolCtx, document,
    tools::{delete_pages as pdf_delete_pages, split as pdf_split},
};
use image::GrayImage;
use pdfium_render::prelude::{PdfBitmap, PdfDocument, PdfRenderConfig};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::output::{Metrics, ToolContext, ToolResult};

/// Grayscale luminance threshold (`0..=255`) at or above which a pixel is considered white.
pub const WHITE_LUMA_THRESHOLD: u8 = 250;

/// Runs `pdf.remove_blank`: detects blank pages by rendering each page in grayscale at
/// `params.dpi`, removes pages whose white ratio is `>= params.threshold`, writes the trimmed PDF
/// to `<output_dir>/<stem>.remove-blank.pdf`, and emits its artifact and metrics.
pub fn run(ctx: &mut ToolContext<'_>, params: RemoveBlankParams) -> ToolResult {
    ctx.check()?;
    params
        .validate()
        .map_err(|err| EngineFailure::owned(ErrorCode::InvalidRequest, err.to_string()))?;

    let bytes = read_bounded_input(ctx)?;
    let structural_doc = open_structural_pdf(ctx, &bytes)?;
    let (blank_pages, white_ratios) =
        detect_blank_pages_in_bytes(ctx, &bytes, params.dpi, params.threshold)?;

    let pages_before = u32::try_from(white_ratios.len()).unwrap_or(u32::MAX);
    if blank_pages.len() == white_ratios.len() {
        return Err(EngineFailure::new(
            ErrorCode::InvalidRequest,
            "Cannot remove all pages: every page in the document was detected as blank.",
        ));
    }

    let pages_after = pages_before.saturating_sub(u32::try_from(blank_pages.len()).unwrap_or(0));

    let pdf_ctx = PdfToolCtx::default().with_max_pages(ctx.limits().max_pages);
    let (out_bytes, warnings) =
        pdf_delete_pages::delete_page_numbers(&structural_doc, &blank_pages, &pdf_ctx)
            .map_err(map_pdf_error)?;
    for warning in warnings {
        ctx.warn("pdf_warning", &warning)?;
    }

    let stem = ctx.input_stem();
    let out_path = ctx.artifact_path(&format!("{stem}.remove-blank.pdf"))?;
    fs::write(&out_path, &out_bytes).map_err(|_| EngineFailure::io())?;
    ctx.artifact(&out_path, "pdf")?;

    Ok(Some(Metrics::from([
        ("pages_before".to_owned(), json!(pages_before)),
        ("pages_after".to_owned(), json!(pages_after)),
        ("removed_pages".to_owned(), json!(blank_pages)),
        ("white_ratios".to_owned(), json!(white_ratios)),
    ])))
}

/// Parameters accepted by `pdf.split` when routed to `ariad-pdfium-engine` for pixel blank
/// detection.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SplitPixelParams {
    mode: String,
    #[serde(default = "default_detect_mode")]
    detect: String,
    #[serde(default = "default_split_threshold")]
    threshold: f64,
    #[serde(default = "default_split_dpi")]
    dpi: u32,
}

fn default_detect_mode() -> String {
    "pixel".to_owned()
}

const fn default_split_threshold() -> f64 {
    DEFAULT_THRESHOLD
}

const fn default_split_dpi() -> u32 {
    DEFAULT_DPI
}

impl SplitPixelParams {
    fn validate(&self) -> Result<(), EngineFailure> {
        if self.mode != "blank-separator" && self.mode != "blank_separator" {
            return Err(EngineFailure::new(
                ErrorCode::InvalidRequest,
                "pdfium split supports only mode = \"blank-separator\".",
            ));
        }
        if self.detect != "pixel" {
            return Err(EngineFailure::new(
                ErrorCode::InvalidRequest,
                "pdfium split supports only detect = \"pixel\".",
            ));
        }
        let rb = RemoveBlankParams {
            threshold: self.threshold,
            dpi: self.dpi,
        };
        rb.validate()
            .map_err(|err| EngineFailure::owned(ErrorCode::InvalidRequest, err.to_string()))
    }
}

/// Runs `pdf.split` with `mode = "blank-separator"` and `detect = "pixel"`.
///
/// Splits the input PDF at blank separator pages (dropping the blank separator pages themselves)
/// and writes `<stem>.part-001.pdf`, `<stem>.part-002.pdf`, ... into `ctx.output_dir()`.
pub fn split_pixel(ctx: &mut ToolContext<'_>, params: &Value) -> ToolResult {
    ctx.check()?;
    let split_params: SplitPixelParams =
        serde_json::from_value(params.clone()).map_err(|_| EngineFailure::invalid_request())?;
    split_params.validate()?;

    let bytes = read_bounded_input(ctx)?;
    let structural_doc = open_structural_pdf(ctx, &bytes)?;
    let (blank_pages, white_ratios) =
        detect_blank_pages_in_bytes(ctx, &bytes, split_params.dpi, split_params.threshold)?;

    if blank_pages.len() == white_ratios.len() {
        return Err(EngineFailure::new(
            ErrorCode::InvalidRequest,
            "Cannot split document: every page was detected as a blank separator.",
        ));
    }

    let stem = ctx.input_stem();
    let pdf_ctx = PdfToolCtx::default().with_max_pages(ctx.limits().max_pages);
    let (artifacts, warnings) =
        pdf_split::split_at_separator_pages(&structural_doc, &stem, &blank_pages, &pdf_ctx)
            .map_err(map_pdf_error)?;
    for warning in warnings {
        ctx.warn("pdf_warning", &warning)?;
    }

    let total_parts = artifacts.len();
    for (idx, artifact) in artifacts.iter().enumerate() {
        ctx.check()?;
        let part_no = u32::try_from(idx + 1).unwrap_or(u32::MAX);
        let out_path = ctx.artifact_path(&artifact.name)?;
        fs::write(&out_path, &artifact.bytes).map_err(|_| EngineFailure::io())?;
        ctx.artifact(&out_path, "pdf")?;
        ctx.progress("pdf.split", u64::from(part_no), total_parts as u64)?;
    }

    Ok(Some(Metrics::from([
        ("parts".to_owned(), json!(total_parts)),
        ("blank_pages".to_owned(), json!(blank_pages)),
        ("white_ratios".to_owned(), json!(white_ratios)),
    ])))
}

/// Computes the per-page white ratio (`0.0..=1.0`) for every page of `ctx.input()` at `dpi`.
pub fn page_white_ratios(ctx: &mut ToolContext<'_>, dpi: u32) -> Result<Vec<f64>, EngineFailure> {
    if !(MIN_DPI..=MAX_DPI).contains(&dpi) {
        return Err(EngineFailure::invalid_request());
    }
    let bytes = read_bounded_input(ctx)?;
    let _ = open_structural_pdf(ctx, &bytes)?;
    let (_, ratios) = detect_blank_pages_in_bytes(ctx, &bytes, dpi, DEFAULT_THRESHOLD)?;
    Ok(ratios)
}

/// Detects 1-based blank page numbers and per-page white ratios (`0.0..=1.0`) for `ctx.input()`.
pub fn detect_blank_pages(
    ctx: &mut ToolContext<'_>,
    dpi: u32,
    threshold: f64,
) -> Result<(Vec<u32>, Vec<f64>), EngineFailure> {
    let params = RemoveBlankParams { threshold, dpi };
    params
        .validate()
        .map_err(|err| EngineFailure::owned(ErrorCode::InvalidRequest, err.to_string()))?;
    let bytes = read_bounded_input(ctx)?;
    let _ = open_structural_pdf(ctx, &bytes)?;
    detect_blank_pages_in_bytes(ctx, &bytes, dpi, threshold)
}

pub(crate) fn detect_blank_pages_in_bytes(
    ctx: &mut ToolContext<'_>,
    bytes: &[u8],
    dpi: u32,
    threshold: f64,
) -> Result<(Vec<u32>, Vec<f64>), EngineFailure> {
    let pdfium = ctx.pdfium()?;
    let doc = pdfium.load_pdf_from_byte_slice(bytes, None).map_err(|_| {
        EngineFailure::new(
            ErrorCode::InvalidRequest,
            "The input PDF could not be opened.",
        )
    })?;

    let raw_count = doc.pages().len();
    let page_count = usize::try_from(raw_count).unwrap_or(0);
    if page_count == 0 {
        return Err(EngineFailure::new(
            ErrorCode::InvalidRequest,
            "The input PDF has no pages.",
        ));
    }
    ctx.check_page_count(page_count)?;

    let scale = (dpi as f32) / 72.0;
    let mut blank_pages = Vec::new();
    let mut white_ratios = Vec::with_capacity(page_count);
    let stage = ctx.tool().to_owned();

    for idx in 0..raw_count {
        ctx.check()?;
        let page = doc.pages().get(idx).map_err(|_| {
            EngineFailure::new(
                ErrorCode::InvalidRequest,
                "A page in the input PDF could not be loaded.",
            )
        })?;

        let width_pt = f64::from(page.width().value).abs().max(1.0);
        let height_pt = f64::from(page.height().value).abs().max(1.0);
        let est_w = (width_pt * f64::from(scale)).ceil().max(1.0) as u64;
        let est_h = (height_pt * f64::from(scale)).ceil().max(1.0) as u64;
        ctx.check_pixels(est_w, est_h)?;

        let render_cfg = PdfRenderConfig::new()
            .scale_page_by_factor(scale)
            .render_form_data(true)
            .render_annotations(true);
        let bitmap = page.render_with_config(&render_cfg).map_err(|_| {
            EngineFailure::new(ErrorCode::EngineFailure, "Failed to render PDF page.")
        })?;
        let gray = bitmap_to_gray(ctx, &bitmap)?;

        let ratio = compute_white_ratio(&gray, dpi);
        let page_no = u32::try_from(idx).unwrap_or(0) + 1;
        if ratio >= threshold {
            blank_pages.push(page_no);
        }
        white_ratios.push(ratio);
        ctx.progress(&stage, u64::from(page_no), page_count as u64)?;
    }

    Ok((blank_pages, white_ratios))
}

pub(crate) fn bitmap_to_gray(
    ctx: &ToolContext<'_>,
    bitmap: &PdfBitmap<'_>,
) -> Result<GrayImage, EngineFailure> {
    let w = u32::try_from(bitmap.width()).map_err(|_| {
        EngineFailure::new(
            ErrorCode::EngineFailure,
            "Invalid rendered bitmap dimensions.",
        )
    })?;
    let h = u32::try_from(bitmap.height()).map_err(|_| {
        EngineFailure::new(
            ErrorCode::EngineFailure,
            "Invalid rendered bitmap dimensions.",
        )
    })?;
    ctx.check_pixels(u64::from(w), u64::from(h))?;

    let rgba = bitmap.as_rgba_bytes();
    let luma: Vec<u8> = rgba
        .as_chunks::<4>()
        .0
        .iter()
        .map(|px| {
            ((u16::from(px[0]) * 77 + u16::from(px[1]) * 150 + u16::from(px[2]) * 29) >> 8) as u8
        })
        .collect();
    GrayImage::from_raw(w, h, luma).ok_or_else(|| {
        EngineFailure::new(
            ErrorCode::EngineFailure,
            "Failed to construct grayscale buffer from rendered bitmap.",
        )
    })
}

/// Computes the white-space ratio (`0.0..=1.0`) of a grayscale page image rendered at `dpi`.
///
/// Partitions the page into `12pt x 12pt` (`1 em`) typographic cells so that sparse text lines
/// (whose glyph strokes cover ~20% of their character cells) register their full typographic
/// footprint while truly blank pages (`1.0`) and isolated scanner dust speckles (`>= 0.999`) stay
/// above the default `0.99` blank threshold.
#[must_use]
pub fn compute_white_ratio(gray: &GrayImage, dpi: u32) -> f64 {
    let width = gray.width();
    let height = gray.height();
    if width == 0 || height == 0 {
        return 1.0;
    }

    let raw = gray.as_raw();
    if raw.iter().all(|&luma| luma >= WHITE_LUMA_THRESHOLD) {
        return 1.0;
    }

    let cell_px = (dpi.saturating_mul(12).div_ceil(72)).max(1);
    let cols = width.div_ceil(cell_px);
    let rows = height.div_ceil(cell_px);
    let total_cells = u64::from(cols) * u64::from(rows);
    if total_cells == 0 {
        return 1.0;
    }

    let mut non_white_cells = 0_u64;
    for cy in 0..rows {
        let y_start = cy * cell_px;
        let y_end = ((cy + 1) * cell_px).min(height);
        for cx in 0..cols {
            let x_start = cx * cell_px;
            let x_end = ((cx + 1) * cell_px).min(width);
            let mut has_ink = false;
            'cell: for y in y_start..y_end {
                let row_offset = (y as usize) * (width as usize);
                for x in x_start..x_end {
                    if raw[row_offset + (x as usize)] < WHITE_LUMA_THRESHOLD {
                        has_ink = true;
                        break 'cell;
                    }
                }
            }
            if has_ink {
                non_white_cells += 1;
            }
        }
    }

    let white_cells = total_cells.saturating_sub(non_white_cells);
    let ratio = (white_cells as f64) / (total_cells as f64);
    (ratio * 1_000_000.0).round() / 1_000_000.0
}

pub(crate) fn save_pdf_deterministic(doc: &PdfDocument<'_>) -> Result<Vec<u8>, EngineFailure> {
    let mut bytes = doc.save_to_bytes().map_err(|_| {
        EngineFailure::new(
            ErrorCode::EngineFailure,
            "Failed to serialize PDF document.",
        )
    })?;
    let tail_start = bytes.len().saturating_sub(1024);
    let tail = &mut bytes[tail_start..];
    if let Some(pos) = tail.windows(5).rposition(|w| w == b"/ID[<") {
        let mut i = pos + 4;
        while i < tail.len() && tail[i] != b']' {
            if tail[i].is_ascii_hexdigit() {
                tail[i] = b'0';
            }
            i += 1;
        }
    }
    Ok(bytes)
}

pub(crate) fn read_bounded_input(ctx: &ToolContext<'_>) -> Result<Vec<u8>, EngineFailure> {
    let meta = fs::metadata(ctx.input()).map_err(|_| EngineFailure::io())?;
    if let Some(max_bytes) = ctx.limits().max_input_bytes
        && meta.len() > max_bytes
    {
        return Err(EngineFailure::limit_exceeded());
    }
    let bytes = fs::read(ctx.input()).map_err(|_| EngineFailure::io())?;
    if let Some(max_bytes) = ctx.limits().max_input_bytes
        && (bytes.len() as u64) > max_bytes
    {
        return Err(EngineFailure::limit_exceeded());
    }
    Ok(bytes)
}

pub(crate) fn open_structural_pdf(
    ctx: &ToolContext<'_>,
    bytes: &[u8],
) -> Result<PdfDoc, EngineFailure> {
    let pdf_ctx = PdfToolCtx::default();
    let doc = document::open_with_ctx(bytes, ctx.limits(), &pdf_ctx).map_err(map_pdf_error)?;
    let page_count = doc.inner().get_pages().len();
    if page_count == 0 {
        return Err(EngineFailure::new(
            ErrorCode::InvalidRequest,
            "The input PDF has no pages.",
        ));
    }
    ctx.check_page_count(page_count)?;
    Ok(doc)
}

pub(crate) fn map_pdf_error(err: PdfError) -> EngineFailure {
    if err.is_cancelled() {
        return EngineFailure::new(ErrorCode::EngineFailure, "The job was cancelled.");
    }
    if err.is_timed_out() {
        return EngineFailure::new(ErrorCode::LimitExceeded, "The job exceeded its time limit.");
    }
    match err {
        PdfError::Malformed => EngineFailure::new(
            ErrorCode::InvalidRequest,
            "The input file is not a readable PDF.",
        ),
        PdfError::Encrypted { .. } => EngineFailure::new(
            ErrorCode::InvalidRequest,
            "The input PDF is encrypted. Decrypt it with pdf.decrypt first.",
        ),
        PdfError::LimitExceeded(_) => EngineFailure::limit_exceeded(),
        PdfError::InvalidParams(msg) => EngineFailure::owned(ErrorCode::InvalidRequest, msg),
        PdfError::NotImplemented(msg) => EngineFailure::owned(ErrorCode::EngineFailure, msg),
    }
}
