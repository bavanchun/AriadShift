//! The `pdf.crop_whitespace` tool.
//!
//! Renders each selected page at low resolution (`72` DPI) through PDFium, finds the bounding box
//! of non-white pixels (`luma < 250`), maps that box back to unrotated PDF user space (respecting
//! `/CropBox`, `/MediaBox`, and `/Rotate`, including inherited attributes), expands by
//! `padding_pt`, clamps to `/MediaBox`, and updates `/CropBox` on the page.
//! Blank pages keep their existing `/CropBox` and emit a warning event.

use std::{collections::BTreeSet, fs};

use ariad_core::{
    protocol::ErrorCode,
    tools::{DuplicatePolicy, params::CropWhitespaceParams},
};
use ariad_engine_io::EngineFailure;
use ariad_pdf::info;
use image::GrayImage;
use pdfium_render::prelude::{PdfPage, PdfPageIndex, PdfRect, PdfRenderConfig};
use serde_json::json;

use crate::{
    blank::{
        WHITE_LUMA_THRESHOLD, bitmap_to_gray, map_pdf_error, open_structural_pdf,
        read_bounded_input, save_pdf_deterministic,
    },
    output::{Metrics, ToolContext, ToolResult},
};

/// Resolution in dots per inch used when detecting content bounding boxes.
const ANALYSIS_DPI: u32 = 72;

/// Runs `pdf.crop_whitespace`: updates `/CropBox` on each selected non-blank page, writes the
/// cropped PDF to `<output_dir>/<stem>.cropped.pdf`, and emits its artifact and metrics.
pub fn run(ctx: &mut ToolContext<'_>, params: CropWhitespaceParams) -> ToolResult {
    ctx.check()?;
    params
        .validate()
        .map_err(|err| EngineFailure::owned(ErrorCode::InvalidRequest, err.to_string()))?;

    let bytes = read_bounded_input(ctx)?;
    let pdf_doc = open_structural_pdf(ctx, &bytes)?;
    let pdf_info = info::from_doc(&pdf_doc).map_err(map_pdf_error)?;
    let page_count = pdf_info.page_count;

    let selected = params
        .pages
        .expand(page_count, DuplicatePolicy::Reject)
        .map_err(|err| EngineFailure::owned(ErrorCode::InvalidRequest, err.to_string()))?;
    let selected_set: BTreeSet<u32> = selected.into_iter().collect();

    let pdfium = ctx.pdfium()?;
    let render_doc = pdfium.load_pdf_from_byte_slice(&bytes, None).map_err(|_| {
        EngineFailure::new(
            ErrorCode::InvalidRequest,
            "The input PDF could not be opened.",
        )
    })?;

    let scale = (ANALYSIS_DPI as f32) / 72.0;
    let total_selected = selected_set.len() as u64;
    let mut cropped_pages = Vec::new();
    let mut unchanged_blank_pages = Vec::new();

    for (done_idx, &page_no) in selected_set.iter().enumerate() {
        ctx.check()?;
        let page_idx =
            PdfPageIndex::try_from(page_no - 1).map_err(|_| EngineFailure::invalid_request())?;
        let mut page = render_doc.pages().get(page_idx).map_err(|_| {
            EngineFailure::new(
                ErrorCode::InvalidRequest,
                "A selected page in the input PDF could not be loaded.",
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
        let gray = {
            let bitmap = page.render_with_config(&render_cfg).map_err(|err| {
                crate::render::map_render_error(
                    err,
                    (page.width().value * scale).round() as u64,
                    (page.height().value * scale).round() as u64,
                    ctx.limits().max_memory_mb,
                    page_no as usize,
                )
            })?;
            bitmap_to_gray(ctx, &bitmap)?
        };

        if let Some(px_bbox) = find_non_white_pixel_bbox(&gray) {
            let fallback_dims = pdf_info
                .pages
                .get((page_no - 1) as usize)
                .map_or((612.0, 792.0), |pi| (pi.width_pt, pi.height_pt));
            let media_box = effective_media_box(&page, fallback_dims);
            let current_crop = effective_crop_box(&page, media_box);
            let rotation = page
                .rotation()
                .map_or(0, |rot| rot.as_degrees().round() as i32);

            let [x0, y0, x1, y1] = compute_cropped_box(
                px_bbox,
                (gray.width(), gray.height()),
                current_crop,
                media_box,
                rotation,
                params.padding_pt,
            );

            page.boundaries_mut()
                .set_crop(PdfRect::new_from_values(
                    y0 as f32, x0 as f32, y1 as f32, x1 as f32,
                ))
                .map_err(|_| {
                    EngineFailure::new(ErrorCode::EngineFailure, "Failed to update page CropBox.")
                })?;
            cropped_pages.push(page_no);
        } else {
            unchanged_blank_pages.push(page_no);
            ctx.warn(
                "blank_page_unchanged",
                &format!("Page {page_no} is blank; its CropBox was left unchanged."),
            )?;
        }

        ctx.progress("pdf.crop_whitespace", (done_idx + 1) as u64, total_selected)?;
    }

    let out_bytes = save_pdf_deterministic(&render_doc)?;
    let stem = ctx.input_stem();
    let out_path = ctx.artifact_path(&format!("{stem}.cropped.pdf"))?;
    fs::write(&out_path, &out_bytes).map_err(|_| EngineFailure::io())?;
    ctx.artifact(&out_path, "pdf")?;

    Ok(Some(Metrics::from([
        ("pages".to_owned(), json!(page_count)),
        ("cropped_pages".to_owned(), json!(cropped_pages)),
        (
            "unchanged_blank_pages".to_owned(),
            json!(unchanged_blank_pages),
        ),
    ])))
}

/// Pixel bounding box `(min_x, min_y, max_x, max_y)` inclusive in bitmap coordinates.
fn find_non_white_pixel_bbox(gray: &GrayImage) -> Option<(u32, u32, u32, u32)> {
    let width = gray.width();
    let height = gray.height();
    if width == 0 || height == 0 {
        return None;
    }

    let raw = gray.as_raw();
    let mut min_x = width;
    let mut min_y = height;
    let mut max_x = 0_u32;
    let mut max_y = 0_u32;
    let mut found = false;

    for y in 0..height {
        let row_offset = (y as usize) * (width as usize);
        for x in 0..width {
            if raw[row_offset + (x as usize)] < WHITE_LUMA_THRESHOLD {
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
                found = true;
            }
        }
    }

    found.then_some((min_x, min_y, max_x, max_y))
}

/// Maps a bitmap pixel bounding box `(min_px, min_py, max_px, max_py)` back into unrotated PDF
/// user space `[x0, y0, x1, y1]`, expands by `padding_pt`, and clamps to `media_box`.
fn compute_cropped_box(
    px_bbox: (u32, u32, u32, u32),
    bmp_size: (u32, u32),
    current_crop: [f64; 4],
    media_box: [f64; 4],
    rotation_deg: i32,
    padding_pt: f64,
) -> [f64; 4] {
    let (min_px, min_py, max_px, max_py) = px_bbox;
    let (bmp_w, bmp_h) = (f64::from(bmp_size.0.max(1)), f64::from(bmp_size.1.max(1)));

    // Normalized displayed coordinates in [0.0, 1.0]:
    // u: left (0.0) -> right (1.0)
    // v: top (0.0)  -> bottom (1.0)
    let u0 = f64::from(min_px) / bmp_w;
    let u1 = (f64::from(max_px) + 1.0) / bmp_w;
    let v0 = f64::from(min_py) / bmp_h;
    let v1 = (f64::from(max_py) + 1.0) / bmp_h;

    // Map displayed (u, v) to unrotated user space normalized (x_norm, y_norm) where:
    // x_norm: left (0.0) -> right (1.0)
    // y_norm: bottom (0.0) -> top (1.0)
    let (xn0, xn1, yn0, yn1) = match rotation_deg.rem_euclid(360) {
        90 => (v0, v1, u0, u1),
        180 => (1.0 - u1, 1.0 - u0, v0, v1),
        270 => (1.0 - v1, 1.0 - v0, 1.0 - u1, 1.0 - u0),
        _ => (u0, u1, 1.0 - v1, 1.0 - v0),
    };

    let crop_w = (current_crop[2] - current_crop[0]).max(1.0);
    let crop_h = (current_crop[3] - current_crop[1]).max(1.0);

    let raw_x0 = current_crop[0] + xn0 * crop_w;
    let raw_x1 = current_crop[0] + xn1 * crop_w;
    let raw_y0 = current_crop[1] + yn0 * crop_h;
    let raw_y1 = current_crop[1] + yn1 * crop_h;

    let x0 = (raw_x0 - padding_pt).max(media_box[0]);
    let y0 = (raw_y0 - padding_pt).max(media_box[1]);
    let x1 = (raw_x1 + padding_pt).min(media_box[2]);
    let y1 = (raw_y1 + padding_pt).min(media_box[3]);

    if x1 - x0 < 1.0 || y1 - y0 < 1.0 {
        return current_crop;
    }

    [round_2(x0), round_2(y0), round_2(x1), round_2(y1)]
}

fn effective_media_box(page: &PdfPage<'_>, fallback_dims: (f64, f64)) -> [f64; 4] {
    if let Ok(b) = page.boundaries().media() {
        let r = b.bounds;
        let rect = [
            f64::from(r.left().value),
            f64::from(r.bottom().value),
            f64::from(r.right().value),
            f64::from(r.top().value),
        ];
        if rect[2] - rect[0] >= 1.0 && rect[3] - rect[1] >= 1.0 {
            return rect;
        }
    }
    [0.0, 0.0, fallback_dims.0.max(1.0), fallback_dims.1.max(1.0)]
}

fn effective_crop_box(page: &PdfPage<'_>, media: [f64; 4]) -> [f64; 4] {
    if let Ok(b) = page.boundaries().crop() {
        let r = b.bounds;
        let crop = [
            f64::from(r.left().value).max(media[0]),
            f64::from(r.bottom().value).max(media[1]),
            f64::from(r.right().value).min(media[2]),
            f64::from(r.top().value).min(media[3]),
        ];
        if crop[2] - crop[0] >= 1.0 && crop[3] - crop[1] >= 1.0 {
            return crop;
        }
    }
    media
}

fn round_2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}
