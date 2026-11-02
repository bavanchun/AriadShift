//! The `pdf.render` and `pdf.to_images` tools.
//!
//! # JavaScript Non-Execution Guarantee
//!
//! Rendering untrusted PDFs never executes embedded `/OpenAction`, `/AA`, `/Names /JavaScript`,
//! or form-field JavaScript actions:
//! 1. The bundled Chromium 8125 `libpdfium` binary is compiled with `pdf_enable_v8=false` and
//!    `pdf_enable_xfa=false`.
//! 2. `pdfium-render` initializes `FPDF_FORMFILLINFO` with `m_pJsPlatform = null` and every
//!    external action callback (`FFI_DoURIAction`, `FFI_ExecuteNamedAction`, `FFI_GotoURL`,
//!    `FFI_OpenFile`, `FFI_DownloadFromURL`, `FFI_PostRequestURL`, `FFI_PutRequestURL`) set to
//!    `None`.
//! 3. This module only invokes `load_pdf_from_byte_slice`, `pages().get(idx)`, and
//!    `page.render_with_config(...)`; it never calls `FORM_DoDocumentJSAction`,
//!    `FORM_DoDocumentOpenAction`, `FORM_DoDocumentAAction`, `FORM_OnAfterLoadPage`, or
//!    `FORM_DoPageAAction`.
//! 4. The out-of-process engine executes without network access.

use std::{fs, io::Write};

use ariad_core::{
    protocol::ErrorCode,
    tools::params::{Background, Combine, RasterFormat, RenderParams},
};
use ariad_engine_io::EngineFailure;
use image::{Rgba, RgbaImage, imageops};
use pdfium_render::prelude::{
    PdfColor, PdfPage, PdfRenderConfig, PdfiumError, PdfiumInternalError,
};
use serde_json::json;
use zip::{CompressionMethod, DateTime, ZipWriter, write::SimpleFileOptions};

use crate::output::{
    Metrics, ToolContext, ToolResult, bitmap_to_rgba, encode_raster_image, map_pdfium_open_error,
    page_artifact_name,
};

/// Selected-page count above which uncombined per-page images are packaged into a single `.zip`
/// artifact instead of individual image files.
pub const ZIP_PAGE_THRESHOLD: usize = 50;

/// Runs `pdf.render` (and `pdf.to_images` via `RenderParams::from(ToImagesParams)`):
/// renders selected pages, emits artifacts through `ctx`, and returns result metrics.
pub fn run(ctx: &mut ToolContext<'_>, params: RenderParams) -> ToolResult {
    params
        .validate()
        .map_err(|err| EngineFailure::owned(ErrorCode::InvalidRequest, err.to_string()))?;

    if params.format == RasterFormat::Webp && params.quality.is_some_and(|q| q < 100) {
        ctx.warn(
            "webp_lossless",
            "WebP output uses lossless VP8L encoding; the 'quality' parameter was ignored.",
        )?;
    }

    let pdf_bytes = ctx.load_validated_pdf_bytes()?;
    let pdfium = ctx.pdfium()?;
    let doc = pdfium
        .load_pdf_from_byte_slice(&pdf_bytes, None)
        .map_err(map_pdfium_open_error)?;

    let total_pages = usize::try_from(doc.pages().len()).unwrap_or(0);
    let selected_pages = ctx.resolve_pages(&params.pages, total_pages)?;
    let total_selected = selected_pages.len();

    let clear_color = match params.background {
        Background::White => PdfColor::WHITE,
        Background::Transparent => PdfColor::new(0, 0, 0, 0),
    };

    let stem = ctx.input_stem();
    let ext = params.format.as_str();

    match params.combine {
        Combine::Vertical => {
            let mut rendered_pages: Vec<RgbaImage> = Vec::with_capacity(total_selected);
            let mut combined_width: u64 = 0;
            let mut combined_height: u64 = 0;

            for (idx, &page_num) in selected_pages.iter().enumerate() {
                ctx.check()?;
                let page_index = i32::try_from(page_num - 1).map_err(|_| {
                    EngineFailure::new(ErrorCode::InvalidRequest, "Page index overflow.")
                })?;
                let page = doc.pages().get(page_index).map_err(|err| {
                    EngineFailure::owned(
                        ErrorCode::EngineFailure,
                        format!("Failed to load page {page_num}: {err}"),
                    )
                })?;

                let (width_px, height_px) = compute_page_pixels(&page, params.dpi)?;
                ctx.check_pixels(width_px, height_px)?;

                combined_width = combined_width.max(width_px);
                combined_height = combined_height.saturating_add(height_px);
                ctx.check_pixels(combined_width, combined_height)?;

                let rgba = render_page_rgba(
                    &page,
                    page_num,
                    width_px,
                    height_px,
                    clear_color,
                    params.annotations,
                    ctx.limits().max_memory_mb,
                )?;
                ctx.check_pixels(u64::from(rgba.width()), u64::from(rgba.height()))?;
                rendered_pages.push(rgba);

                ctx.progress("render", (idx + 1) as u64, total_selected as u64)?;
            }

            ctx.check()?;
            let actual_width = rendered_pages
                .iter()
                .map(|img| u64::from(img.width()))
                .max()
                .unwrap_or(0);
            let actual_height: u64 = rendered_pages
                .iter()
                .map(|img| u64::from(img.height()))
                .sum();
            ctx.check_pixels(actual_width, actual_height)?;

            let bg_pixel = match params.background {
                Background::White => Rgba([255, 255, 255, 255]),
                Background::Transparent => Rgba([0, 0, 0, 0]),
            };
            let mut canvas =
                RgbaImage::from_pixel(actual_width as u32, actual_height as u32, bg_pixel);
            let mut y_offset: i64 = 0;
            for page_img in &rendered_pages {
                imageops::replace(&mut canvas, page_img, 0, y_offset);
                y_offset += i64::from(page_img.height());
            }

            let encoded = encode_raster_image(&canvas, params.format, params.quality)?;
            if encoded.len() as u64 > ctx.limits().max_decompressed_bytes {
                return Err(EngineFailure::limit_exceeded());
            }
            let file_name = format!("{stem}.combined.{ext}");
            let out_path = ctx.artifact_path(&file_name)?;
            fs::write(&out_path, &encoded).map_err(|_| EngineFailure::io())?;
            ctx.artifact(&out_path, ext)?;
        }
        Combine::None => {
            let zipped = total_selected > ZIP_PAGE_THRESHOLD;
            let zip_name = format!("{stem}.pages.zip");
            let zip_scratch = ctx.work_dir().join(&zip_name);
            let mut zip = if zipped {
                Some(ZipWriter::new(
                    fs::File::create(&zip_scratch).map_err(|_| EngineFailure::io())?,
                ))
            } else {
                None
            };
            let options = SimpleFileOptions::default()
                .compression_method(CompressionMethod::Deflated)
                .last_modified_time(DateTime::default());
            let mut files = Vec::with_capacity(total_selected.min(ZIP_PAGE_THRESHOLD));
            let mut output_bytes = 0_u64;

            for (idx, &page_num) in selected_pages.iter().enumerate() {
                ctx.check()?;
                let page_index = i32::try_from(page_num - 1).map_err(|_| {
                    EngineFailure::new(ErrorCode::InvalidRequest, "Page index overflow.")
                })?;
                let page = doc.pages().get(page_index).map_err(|err| {
                    EngineFailure::owned(
                        ErrorCode::EngineFailure,
                        format!("Failed to load page {page_num}: {err}"),
                    )
                })?;

                let (width_px, height_px) = compute_page_pixels(&page, params.dpi)?;
                ctx.check_pixels(width_px, height_px)?;

                let rgba = render_page_rgba(
                    &page,
                    page_num,
                    width_px,
                    height_px,
                    clear_color,
                    params.annotations,
                    ctx.limits().max_memory_mb,
                )?;
                ctx.check_pixels(u64::from(rgba.width()), u64::from(rgba.height()))?;

                let encoded = encode_raster_image(&rgba, params.format, params.quality)?;
                let page_u32 = u32::try_from(page_num).map_err(|_| {
                    EngineFailure::new(ErrorCode::InvalidRequest, "Page number overflow.")
                })?;
                output_bytes = crate::output::charge_output_bytes(
                    output_bytes,
                    encoded.len(),
                    ctx.limits().max_decompressed_bytes,
                )?;
                let name = page_artifact_name(&stem, page_u32, ext);
                if let Some(writer) = &mut zip {
                    writer.start_file(&name, options).map_err(|err| {
                        EngineFailure::owned(ErrorCode::EngineFailure, err.to_string())
                    })?;
                    writer
                        .write_all(&encoded)
                        .map_err(|_| EngineFailure::io())?;
                } else {
                    let scratch = ctx.work_dir().join(&name);
                    fs::write(&scratch, &encoded).map_err(|_| EngineFailure::io())?;
                    files.push((scratch, name));
                }

                ctx.progress("render", (idx + 1) as u64, total_selected as u64)?;
            }

            ctx.check()?;
            if let Some(writer) = zip {
                writer.finish().map_err(|err| {
                    EngineFailure::owned(ErrorCode::EngineFailure, err.to_string())
                })?;
                if fs::metadata(&zip_scratch)
                    .map_err(|_| EngineFailure::io())?
                    .len()
                    > ctx.limits().max_decompressed_bytes
                {
                    return Err(EngineFailure::limit_exceeded());
                }
                files.push((zip_scratch, zip_name));
            }
            let format = if zipped { "zip" } else { ext };
            for (scratch, name) in files {
                let path = ctx.artifact_path(&name)?;
                crate::output::move_scratch_file(&scratch, &path)?;
                ctx.artifact(&path, format)?;
            }
        }
    }

    let zipped = params.combine == Combine::None && total_selected > ZIP_PAGE_THRESHOLD;
    let combined = params.combine == Combine::Vertical;
    let metrics = Metrics::from([
        ("pages".to_owned(), json!(total_selected)),
        ("dpi".to_owned(), json!(params.dpi)),
        ("format".to_owned(), json!(ext)),
        ("combined".to_owned(), json!(combined)),
        ("zipped".to_owned(), json!(zipped)),
    ]);
    Ok(Some(metrics))
}

/// Computes target pixel dimensions `(width_px, height_px)` for `page` at `dpi`.
pub(crate) fn compute_page_pixels(
    page: &PdfPage<'_>,
    dpi: u32,
) -> Result<(u64, u64), EngineFailure> {
    let width_pt = f64::from(page.width().value);
    let height_pt = f64::from(page.height().value);
    if !width_pt.is_finite() || !height_pt.is_finite() || width_pt <= 0.0 || height_pt <= 0.0 {
        return Err(EngineFailure::new(
            ErrorCode::EngineFailure,
            "PDF page has invalid non-positive dimensions.",
        ));
    }
    let scale = f64::from(dpi) / 72.0;
    let width_px = (width_pt * scale).round().max(1.0) as u64;
    let height_px = (height_pt * scale).round().max(1.0) as u64;
    Ok((width_px, height_px))
}

/// Renders `page` into an [`RgbaImage`] of size `width_px` x `height_px`.
pub(crate) fn render_page_rgba(
    page: &PdfPage<'_>,
    page_num: usize,
    width_px: u64,
    height_px: u64,
    clear_color: PdfColor,
    annotations: bool,
    memory_limit_mb: Option<u32>,
) -> Result<RgbaImage, EngineFailure> {
    let target_w = i32::try_from(width_px).map_err(|_| EngineFailure::limit_exceeded())?;
    let target_h = i32::try_from(height_px).map_err(|_| EngineFailure::limit_exceeded())?;
    let config = PdfRenderConfig::new()
        .set_target_size(target_w, target_h)
        .clear_before_rendering(true)
        .set_clear_color(clear_color)
        .render_annotations(annotations)
        .render_form_data(annotations);

    let bitmap = page
        .render_with_config(&config)
        .map_err(|err| map_render_error(err, width_px, height_px, memory_limit_mb, page_num))?;
    bitmap_to_rgba(&bitmap)
}

pub(crate) fn map_render_error(
    err: PdfiumError,
    width_px: u64,
    height_px: u64,
    memory_limit_mb: Option<u32>,
    page_num: usize,
) -> EngineFailure {
    // pdfium-render 0.9.5 produces this error here only when
    // FPDFBitmap_CreateEx returns null after dimensions were validated.
    if width_px > 0
        && height_px > 0
        && matches!(
            err,
            PdfiumError::PdfiumLibraryInternalError(PdfiumInternalError::Unknown)
        )
    {
        let limit = memory_limit_mb.map_or_else(
            || "no configured memory limit".to_owned(),
            |mb| format!("memory limit {mb} MiB"),
        );
        return EngineFailure::owned(
            ErrorCode::LimitExceeded,
            format!("PDFium could not allocate a {width_px} x {height_px} pixel bitmap ({limit})."),
        );
    }
    EngineFailure::owned(
        ErrorCode::EngineFailure,
        format!("PDFium failed to render page {page_num}: {err}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn other_render_errors_remain_engine_failures() {
        let failure = map_render_error(PdfiumError::UnknownBitmapFormat, 100, 100, Some(256), 1);
        assert_eq!(failure.code, ErrorCode::EngineFailure);
        let failure = map_render_error(
            PdfiumError::PdfiumLibraryInternalError(PdfiumInternalError::Unknown),
            0,
            100,
            Some(256),
            1,
        );
        assert_eq!(failure.code, ErrorCode::EngineFailure);
    }
}
