//! The `pdf.thumbnails` tool.

use std::fs;

use ariad_core::{
    protocol::ErrorCode,
    tools::params::{RasterFormat, ThumbnailsParams},
};
use ariad_engine_io::EngineFailure;
use pdfium_render::prelude::{PdfColor, PdfPage};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    output::{
        Metrics, ToolContext, ToolResult, encode_raster_image, map_pdfium_open_error,
        page_artifact_name,
    },
    render::render_page_rgba,
};

/// One entry in the `<stem>.thumbnails.json` index artifact.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThumbnailEntry {
    /// 1-based page number in the source document.
    pub page: u32,
    /// File name of the rendered PNG thumbnail inside the output directory.
    pub file: String,
    /// Rendered thumbnail width in pixels.
    pub width: u32,
    /// Rendered thumbnail height in pixels.
    pub height: u32,
}

/// Runs `pdf.thumbnails`: renders per-page PNG thumbnails whose longest side equals
/// `params.max_px` while preserving aspect ratio, writes `<stem>.thumbnails.json`, and
/// emits all artifacts through `ctx`.
pub fn run(ctx: &mut ToolContext<'_>, params: ThumbnailsParams) -> ToolResult {
    params
        .validate()
        .map_err(|err| EngineFailure::owned(ErrorCode::InvalidRequest, err.to_string()))?;

    let pdf_bytes = ctx.load_validated_pdf_bytes()?;
    let pdfium = ctx.pdfium()?;
    let doc = pdfium
        .load_pdf_from_byte_slice(&pdf_bytes, None)
        .map_err(map_pdfium_open_error)?;

    let total_pages = usize::try_from(doc.pages().len()).unwrap_or(0);
    let selected_pages = ctx.resolve_pages(&params.pages, total_pages)?;
    let total_selected = selected_pages.len();

    let stem = ctx.input_stem();
    let mut index_entries: Vec<ThumbnailEntry> = Vec::with_capacity(total_selected);

    for (idx, &page_num) in selected_pages.iter().enumerate() {
        ctx.check()?;
        let page_index = i32::try_from(page_num - 1)
            .map_err(|_| EngineFailure::new(ErrorCode::InvalidRequest, "Page index overflow."))?;
        let page = doc.pages().get(page_index).map_err(|err| {
            EngineFailure::owned(
                ErrorCode::EngineFailure,
                format!("Failed to load page {page_num}: {err}"),
            )
        })?;

        let (width_px, height_px) = compute_thumbnail_pixels(&page, params.max_px)?;
        ctx.check_pixels(width_px, height_px)?;

        let rgba = render_page_rgba(&page, page_num, width_px, height_px, PdfColor::WHITE, true)?;
        ctx.check_pixels(u64::from(rgba.width()), u64::from(rgba.height()))?;

        let png_bytes = encode_raster_image(&rgba, RasterFormat::Png, None)?;
        let page_u32 = u32::try_from(page_num)
            .map_err(|_| EngineFailure::new(ErrorCode::InvalidRequest, "Page number overflow."))?;
        let file_name = page_artifact_name(&stem, page_u32, "png");
        let png_path = ctx.artifact_path(&file_name)?;
        fs::write(&png_path, &png_bytes).map_err(|_| EngineFailure::io())?;
        ctx.artifact(&png_path, "png")?;

        index_entries.push(ThumbnailEntry {
            page: page_u32,
            file: file_name,
            width: rgba.width(),
            height: rgba.height(),
        });

        ctx.progress("thumbnails", (idx + 1) as u64, total_selected as u64)?;
    }

    ctx.check()?;
    let index_name = format!("{stem}.thumbnails.json");
    let index_path = ctx.artifact_path(&index_name)?;
    let index_json = serde_json::to_vec_pretty(&index_entries).map_err(|err| {
        EngineFailure::owned(
            ErrorCode::EngineFailure,
            format!("Failed to serialize thumbnail index JSON: {err}"),
        )
    })?;
    fs::write(&index_path, &index_json).map_err(|_| EngineFailure::io())?;
    ctx.artifact(&index_path, "json")?;

    let metrics = Metrics::from([
        ("pages".to_owned(), json!(total_selected)),
        ("max_px".to_owned(), json!(params.max_px)),
        ("index".to_owned(), json!(index_name)),
    ]);
    Ok(Some(metrics))
}

/// Computes `(width_px, height_px)` so the longest side equals `max_px` while preserving the
/// page's aspect ratio.
fn compute_thumbnail_pixels(page: &PdfPage<'_>, max_px: u32) -> Result<(u64, u64), EngineFailure> {
    let width_pt = f64::from(page.width().value);
    let height_pt = f64::from(page.height().value);
    if !width_pt.is_finite() || !height_pt.is_finite() || width_pt <= 0.0 || height_pt <= 0.0 {
        return Err(EngineFailure::new(
            ErrorCode::EngineFailure,
            "PDF page has invalid non-positive dimensions.",
        ));
    }
    let max_f = f64::from(max_px);
    let (w, h) = if width_pt >= height_pt {
        let scaled_h = ((height_pt / width_pt) * max_f).round().clamp(1.0, max_f) as u64;
        (u64::from(max_px), scaled_h)
    } else {
        let scaled_w = ((width_pt / height_pt) * max_f).round().clamp(1.0, max_f) as u64;
        (scaled_w, u64::from(max_px))
    };
    Ok((w, h))
}
