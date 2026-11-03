//! Bounded page rendering for OCR.
use super::{raster, skew};
use crate::{
    output::{ToolContext, ToolResult, charge_output_bytes, map_pdfium_open_error},
    render::{compute_page_pixels, render_page_rgba},
};
use ariad_core::{protocol::ErrorCode, tools::params::ScanRenderParams};
use ariad_engine_io::EngineFailure;
use image::{DynamicImage, ImageFormat};
use pdfium_render::prelude::PdfColor;
use serde::Serialize;
use std::{fs, io::Cursor};

#[derive(Serialize)]
struct ScanPage {
    page: usize,
    width_px: u32,
    height_px: u32,
    dpi: u32,
    skew_deg: f32,
    skew_confidence: f32,
    cleaned: bool,
}

/// Render selected pages as grayscale images and emit a scan metadata index.
pub fn run(ctx: &mut ToolContext<'_>, params: ScanRenderParams) -> ToolResult {
    let bytes = ctx.load_validated_pdf_bytes_for_pages(Some(&params.pages))?;
    let pdfium = ctx.pdfium()?;
    let doc = pdfium
        .load_pdf_from_byte_slice(&bytes, None)
        .map_err(map_pdfium_open_error)?;
    let selected = ctx.resolve_pages(
        &params.pages,
        usize::try_from(doc.pages().len()).unwrap_or(0),
    )?;
    let mut pages = Vec::with_capacity(selected.len());
    let mut output_bytes = 0;
    for (index, &page_num) in selected.iter().enumerate() {
        ctx.check()?;
        let page_index =
            i32::try_from(page_num - 1).map_err(|_| EngineFailure::invalid_request())?;
        let page = doc
            .pages()
            .get(page_index)
            .map_err(|_| EngineFailure::new(ErrorCode::EngineFailure, "Failed to load page."))?;
        let (width, height) = compute_page_pixels(&page, params.dpi)?;
        ctx.check_pixels(width, height)?;
        // Rendering, grayscale conversion and optional rotation can overlap in memory.
        let required = width.saturating_mul(height).saturating_mul(10);
        if ctx
            .limits()
            .max_memory_mb
            .is_some_and(|mb| required > u64::from(mb) * 1024 * 1024)
        {
            return Err(EngineFailure::limit_exceeded());
        }
        let rgba = render_page_rgba(
            &page,
            page_num,
            width,
            height,
            PdfColor::WHITE,
            true,
            ctx.limits().max_memory_mb,
        )?;
        let mut gray = DynamicImage::ImageRgba8(rgba).into_luma8();
        ctx.check()?;
        let angle = skew::estimate_checked(&gray, 10.0, || ctx.check())?;
        if params.deskew
            && let Some(value) = angle
            && value.confidence >= 0.02
            && value.angle_deg.abs() >= 0.1
        {
            gray = raster::rotate_checked(&gray, -value.angle_deg, || ctx.check())?;
        }
        if params.clean {
            gray = raster::clean_checked(&gray, || ctx.check())?;
        }
        ctx.check()?;
        let mut encoded = Cursor::new(Vec::new());
        DynamicImage::ImageLuma8(gray)
            .write_to(&mut encoded, ImageFormat::Png)
            .map_err(|_| {
                EngineFailure::new(ErrorCode::EngineFailure, "Failed to encode scan image.")
            })?;
        let encoded = encoded.into_inner();
        output_bytes = charge_output_bytes(
            output_bytes,
            encoded.len(),
            ctx.limits().max_decompressed_bytes,
        )?;
        let path = ctx.artifact_path(&format!("p{page_num:04}.png"))?;
        fs::write(&path, encoded).map_err(|_| EngineFailure::io())?;
        ctx.artifact(&path, "png")?;
        pages.push(ScanPage {
            page: page_num,
            width_px: width as u32,
            height_px: height as u32,
            dpi: params.dpi,
            skew_deg: angle.map_or(0.0, |v| v.angle_deg),
            skew_confidence: angle.map_or(0.0, |v| v.confidence),
            cleaned: params.clean,
        });
        ctx.progress("scan.render", (index + 1) as u64, selected.len() as u64)?;
    }
    let index = serde_json::to_vec(&pages).map_err(|_| EngineFailure::invalid_request())?;
    charge_output_bytes(
        output_bytes,
        index.len(),
        ctx.limits().max_decompressed_bytes,
    )?;
    let path = ctx.artifact_path("scan.json")?;
    fs::write(&path, index).map_err(|_| EngineFailure::io())?;
    ctx.artifact(&path, "json")?;
    Ok(None)
}
