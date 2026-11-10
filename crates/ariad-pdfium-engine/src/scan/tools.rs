//! Page-preserving scan split and deskew tools.
use super::{effect, regions, skew};
use crate::{
    blank::{map_pdf_error, open_structural_pdf, read_bounded_input},
    output::{ToolContext, ToolResult, map_pdfium_open_error},
    render::{compute_page_pixels, render_page_rgba},
};
use ariad_core::{
    protocol::ErrorCode,
    tools::{
        PageRange,
        params::{
            DeskewParams, ImageToPdfParams, OverlayParams, RotateContentParams, ScanSplitParams,
            ScannerEffectParams,
        },
    },
};
use ariad_engine_io::EngineFailure;
use ariad_pdf::{
    PdfDoc, ToolCtx,
    scan_pages::{ScanPageRegion, split_regions},
    tools::{OutputDoc, OutputPayload, finalize_tool_output, overlay, rotate_content},
};
use image::{DynamicImage, GrayImage, codecs::jpeg::JpegEncoder};
use pdfium_render::prelude::{PdfColor, PdfDocument};
use std::{collections::HashSet, fs};

fn render_gray(
    ctx: &ToolContext<'_>,
    document: &PdfDocument<'_>,
    page_number: usize,
    dpi: u32,
) -> Result<GrayImage, EngineFailure> {
    ctx.check()?;
    let page = document
        .pages()
        .get(i32::try_from(page_number - 1).map_err(|_| EngineFailure::invalid_request())?)
        .map_err(|_| EngineFailure::new(ErrorCode::EngineFailure, "Failed to load scan page."))?;
    let (width, height) = compute_page_pixels(&page, dpi)?;
    ctx.check_pixels(width, height)?;
    if ctx.limits().max_memory_mb.is_some_and(|mb| {
        width.saturating_mul(height).saturating_mul(8) > u64::from(mb) * 1024 * 1024
    }) {
        return Err(EngineFailure::limit_exceeded());
    }
    let rgba = render_page_rgba(
        &page,
        page_number,
        width,
        height,
        PdfColor::WHITE,
        true,
        ctx.limits().max_memory_mb,
    )?;
    Ok(DynamicImage::ImageRgba8(rgba).into_luma8())
}

fn rotate_page(
    doc: PdfDoc,
    page: usize,
    clockwise: f32,
    ctx: &ToolCtx,
) -> Result<PdfDoc, EngineFailure> {
    let range =
        PageRange::parse(&page.to_string()).map_err(|_| EngineFailure::invalid_request())?;
    let params = RotateContentParams {
        pages: range,
        angle_deg: clockwise,
        expand: true,
    };
    let mut output = rotate_content::run_owned(doc, &params, ctx).map_err(map_pdf_error)?;
    match output
        .pop()
        .ok_or_else(EngineFailure::invalid_request)?
        .payload
    {
        OutputPayload::Pdf(doc) => Ok(*doc),
        OutputPayload::Bytes(_) => Err(EngineFailure::invalid_request()),
    }
}

fn publish(
    ctx: &mut ToolContext<'_>,
    original: &PdfDoc,
    output: OutputDoc,
    pdf_ctx: &ToolCtx,
) -> ToolResult {
    let stem = ctx.input_stem();
    let result = finalize_tool_output(
        ctx.tool(),
        std::slice::from_ref(original),
        &stem,
        vec![output],
        pdf_ctx,
    )
    .map_err(map_pdf_error)?;
    for warning in result.warnings {
        ctx.warn("pdf_warning", &warning)?;
    }
    for warning in result.coded_warnings {
        ctx.warn(&warning.code, &warning.message)?;
    }
    for artifact in result.artifacts {
        if artifact.bytes.len() as u64 > ctx.limits().max_decompressed_bytes {
            return Err(EngineFailure::limit_exceeded());
        }
        let path = ctx.artifact_path(&artifact.name)?;
        fs::write(&path, artifact.bytes).map_err(|_| EngineFailure::io())?;
        ctx.artifact(&path, &artifact.format)?;
    }
    Ok(Some(result.metrics))
}

/// Deskew selected pages by page transforms without re-encoding original images.
pub fn deskew(ctx: &mut ToolContext<'_>, params: DeskewParams) -> ToolResult {
    if params.min_angle_deg > params.max_angle_deg {
        return Err(EngineFailure::invalid_request());
    }
    let bytes = read_bounded_input(ctx)?;
    let original = open_structural_pdf(ctx, &bytes)?;
    let canonical = crate::canonical::canonicalize(ctx, &mut original.clone(), bytes.len())?;
    let pdfium = ctx.pdfium()?;
    let document = pdfium
        .load_pdf_from_byte_slice(&canonical, None)
        .map_err(map_pdfium_open_error)?;
    let selected = ctx.resolve_pages(
        &params.pages,
        usize::try_from(document.pages().len()).unwrap_or(0),
    )?;
    let pdf_ctx = ctx.pdf_tool_context();
    let mut output = original.clone();
    for (index, &page) in selected.iter().enumerate() {
        let gray = render_gray(ctx, &document, page, params.dpi)?;
        match skew::estimate_checked(&gray, params.max_angle_deg, || ctx.check())? {
            Some(value)
                if value.confidence >= 0.02 && value.angle_deg.abs() >= params.min_angle_deg =>
            {
                output = rotate_page(output, page, value.angle_deg, &pdf_ctx)?;
            }
            Some(value) if value.confidence >= 0.02 => {}
            _ => ctx.warn(
                "skew_uncertain",
                "No reliable skew was detected on a selected page.",
            )?,
        }
        ctx.progress("pdf.deskew", (index + 1) as u64, selected.len() as u64)?;
    }
    publish(ctx, &original, OutputDoc::unnamed_pdf(output), &pdf_ctx)
}

/// Split detected scan regions into pages while retaining the original image data.
pub fn scan_split(ctx: &mut ToolContext<'_>, params: ScanSplitParams) -> ToolResult {
    let bytes = read_bounded_input(ctx)?;
    let original = open_structural_pdf(ctx, &bytes)?;
    let canonical = crate::canonical::canonicalize(ctx, &mut original.clone(), bytes.len())?;
    let pdfium = ctx.pdfium()?;
    let document = pdfium
        .load_pdf_from_byte_slice(&canonical, None)
        .map_err(map_pdfium_open_error)?;
    let total = usize::try_from(document.pages().len()).unwrap_or(0);
    let selected: HashSet<usize> = ctx
        .resolve_pages(&params.pages, total)?
        .into_iter()
        .collect();
    let mut selections = Vec::new();
    let mut angles = Vec::new();
    let cap = usize::try_from(ctx.limits().max_pages.unwrap_or(10_000))
        .unwrap_or(10_000)
        .min(10_000);
    for page in 1..=total {
        ctx.check()?;
        if !selected.contains(&page) {
            selections.push(ScanPageRegion {
                page: page as u32,
                rect: None,
            });
            angles.push(0.0);
        } else {
            let gray = render_gray(ctx, &document, page, params.dpi)?;
            let padding = (params.padding_pt * params.dpi as f32 / 72.0).ceil() as u32;
            let found =
                regions::detect_checked(&gray, params.min_area_ratio, padding, || ctx.check())?;
            if found.is_empty() {
                ctx.warn(
                    "scan_regions_not_found",
                    "No scan regions were detected on a selected page; it was retained.",
                )?;
                selections.push(ScanPageRegion {
                    page: page as u32,
                    rect: None,
                });
                angles.push(0.0);
            } else {
                for region in found {
                    selections.push(ScanPageRegion {
                        page: page as u32,
                        rect: Some([
                            f64::from(region.x0) / f64::from(gray.width()),
                            f64::from(region.y0) / f64::from(gray.height()),
                            f64::from(region.x1) / f64::from(gray.width()),
                            f64::from(region.y1) / f64::from(gray.height()),
                        ]),
                    });
                    if params.straighten && region.angle_confidence < 0.1 {
                        ctx.warn(
                            "scan_orientation_uncertain",
                            "A scan region could not be reliably straightened.",
                        )?;
                    }
                    angles.push(if params.straighten && region.angle_confidence >= 0.1 {
                        region.angle_deg
                    } else {
                        0.0
                    });
                }
            }
            ctx.progress("pdf.scan_split", page as u64, total as u64)?;
        }
        if selections.len() > cap {
            return Err(EngineFailure::limit_exceeded());
        }
    }
    let pdf_ctx = ctx.pdf_tool_context();
    let mut result = split_regions(&original, &selections, &pdf_ctx).map_err(map_pdf_error)?;
    if let OutputPayload::Pdf(doc) = result.payload {
        let mut doc = *doc;
        for (index, angle) in angles.into_iter().enumerate() {
            if angle.abs() >= 0.1 {
                doc = rotate_page(doc, index + 1, angle, &pdf_ctx)?;
            }
        }
        result.payload = OutputPayload::Pdf(Box::new(doc));
    } else {
        return Err(EngineFailure::invalid_request());
    }
    publish(ctx, &original, result, &pdf_ctx)
}

/// Apply a deterministic scan-like raster overlay to selected pages.
pub fn scanner_effect(ctx: &mut ToolContext<'_>, params: ScannerEffectParams) -> ToolResult {
    let bytes = read_bounded_input(ctx)?;
    let original = open_structural_pdf(ctx, &bytes)?;
    let canonical = crate::canonical::canonicalize(ctx, &mut original.clone(), bytes.len())?;
    let pdfium = ctx.pdfium()?;
    let document = pdfium
        .load_pdf_from_byte_slice(&canonical, None)
        .map_err(map_pdfium_open_error)?;
    let selected = ctx.resolve_pages(
        &params.pages,
        usize::try_from(document.pages().len()).unwrap_or(0),
    )?;
    let pdf_ctx = ctx.pdf_tool_context();
    let mut output = original.clone();
    let pages = original.inner().get_pages();
    let mut annotation_warning = false;
    for (index, &page_number) in selected.iter().enumerate() {
        ctx.check()?;
        let page = document
            .pages()
            .get(i32::try_from(page_number - 1).map_err(|_| EngineFailure::invalid_request())?)
            .map_err(|_| {
                EngineFailure::new(ErrorCode::EngineFailure, "Failed to load scan page.")
            })?;
        let (width, height) = compute_page_pixels(&page, params.dpi)?;
        ctx.check_pixels(width, height)?;
        if ctx.limits().max_memory_mb.is_some_and(|mb| {
            width.saturating_mul(height).saturating_mul(20) > u64::from(mb) * 1024 * 1024
        }) {
            return Err(EngineFailure::limit_exceeded());
        }
        let rgba = render_page_rgba(
            &page,
            page_number,
            width,
            height,
            PdfColor::WHITE,
            false,
            ctx.limits().max_memory_mb,
        )?;
        let mut per_page = params.clone();
        per_page.seed = per_page.seed.wrapping_add(page_number as u64);
        let treated = effect::apply_checked(&rgba, &per_page, || ctx.check())?;
        let rgb = DynamicImage::ImageRgba8(treated).into_rgb8();
        let mut jpeg = Vec::new();
        JpegEncoder::new_with_quality(&mut jpeg, params.quality)
            .encode_image(&rgb)
            .map_err(|_| {
                EngineFailure::new(ErrorCode::EngineFailure, "Failed to encode scan effect.")
            })?;
        if jpeg.len() as u64 > ctx.limits().max_decompressed_bytes {
            return Err(EngineFailure::limit_exceeded());
        }
        let mut internal_limits = ctx.limits().clone();
        internal_limits.max_input_bytes = None;
        let overlay_bytes = ariad_pdf::image_to_pdf::run_with_ctx(
            [jpeg],
            &ImageToPdfParams::default(),
            &internal_limits,
            &pdf_ctx,
        )
        .map_err(map_pdf_error)?;
        let overlay_doc =
            ariad_pdf::document::open_with_ctx(&overlay_bytes, &internal_limits, &pdf_ctx)
                .map_err(map_pdf_error)?;
        let range = PageRange::parse(&page_number.to_string())
            .map_err(|_| EngineFailure::invalid_request())?;
        let options = OverlayParams {
            pages: range,
            ..OverlayParams::default()
        };
        let mut result =
            overlay::run_owned(output, overlay_doc, &options, &pdf_ctx).map_err(map_pdf_error)?;
        output = match result
            .pop()
            .ok_or_else(EngineFailure::invalid_request)?
            .payload
        {
            OutputPayload::Pdf(doc) => *doc,
            OutputPayload::Bytes(_) => return Err(EngineFailure::invalid_request()),
        };
        if !annotation_warning
            && pages
                .get(&(page_number as u32))
                .and_then(|id| original.inner().get_object(*id).ok())
                .and_then(|object| object.as_dict().ok())
                .and_then(|page| page.get(b"Annots").ok())
                .is_some()
        {
            ctx.warn(
                "annotations_unaffected",
                "Annotations remain separate and are not given the scan effect.",
            )?;
            annotation_warning = true;
        }
        ctx.progress(
            "pdf.scanner_effect",
            (index + 1) as u64,
            selected.len() as u64,
        )?;
    }
    publish(ctx, &original, OutputDoc::unnamed_pdf(output), &pdf_ctx)
}
