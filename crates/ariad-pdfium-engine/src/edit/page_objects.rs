//! Inspect page objects in PDFium's original drawing order.

use std::fs;

use ariad_core::tools::{
    PageRange,
    params::{
        EditPageObjectsParams, PageObjectEntry, PageObjectKind, PageObjectsDocument,
        PageObjectsPage,
    },
};
use pdfium_render::prelude::*;
use serde_json::json;

use super::{color, invalid, matrix, pdfium_error};
use crate::{
    output::{Metrics, ToolContext, ToolResult},
    text_layout::{expand_pages, read_and_preflight_pdf, rotation_degrees},
};

/// Enumerate supported top-level objects. Indices include unsupported objects, so IDs never
/// shift when an intervening form or shading is omitted from the report.
pub fn run(ctx: &mut ToolContext<'_>, params: EditPageObjectsParams) -> ToolResult {
    params.validate().map_err(invalid)?;
    let (bytes, mut source) = read_and_preflight_pdf(ctx)?;
    ctx.warn_repaired_xref(&source)?;
    let range = PageRange::parse(params.pages.as_deref().unwrap_or("all"))
        .map_err(|e| invalid(e.to_string()))?;
    let canonical =
        crate::canonical::canonicalize_selected(ctx, &mut source, bytes.len(), Some(&range))?;
    let pdfium = ctx.pdfium()?;
    let doc = pdfium
        .load_pdf_from_byte_slice(&canonical, None)
        .map_err(pdfium_error)?;
    let page_count = usize::try_from(doc.pages().len())
        .map_err(|_| ariad_engine_io::EngineFailure::limit_exceeded())?;
    let selected = expand_pages(&range, page_count)?;
    ctx.check_page_count(selected.len())?;
    let mut pages = Vec::new();
    let mut object_count = 0usize;
    for (done, number) in selected.iter().copied().enumerate() {
        ctx.check()?;
        let page = doc
            .pages()
            .get(
                (number - 1)
                    .try_into()
                    .map_err(|_| invalid("page out of range"))?,
            )
            .map_err(pdfium_error)?;
        let mut objects = Vec::new();
        for index in 0..page.objects().len() {
            ctx.check()?;
            let object = page.objects().get(index).map_err(pdfium_error)?;
            let kind = match object {
                PdfPageObject::Text(_) => PageObjectKind::Text,
                PdfPageObject::Image(_) => PageObjectKind::Image,
                PdfPageObject::Path(_) => PageObjectKind::Path,
                _ => continue,
            };
            object_count += 1;
            if object_count > 100_000 {
                return Err(ariad_engine_io::EngineFailure::limit_exceeded());
            }
            let bounds = object.bounds().map_err(pdfium_error)?.to_rect();
            let mut entry = PageObjectEntry {
                id: format!("p{number}_obj{index}"),
                page: number,
                index,
                kind,
                bbox: [
                    bounds.left().value,
                    bounds.bottom().value,
                    bounds.right().value,
                    bounds.top().value,
                ]
                .map(f64::from),
                text: None,
                font_name: None,
                font_size: None,
                weight: None,
                italic: None,
                fill_color: object.fill_color().ok().map(color),
                stroke_color: object.stroke_color().ok().map(color),
                width_px: None,
                height_px: None,
                matrix: matrix(object.matrix().map_err(pdfium_error)?),
            };
            if let Some(text) = object.as_text_object() {
                let font = text.font();
                entry.text = Some(text.text());
                entry.font_name = Some(font.name());
                entry.font_size = Some(f64::from(text.unscaled_font_size().value));
                entry.weight = font.weight().ok().map(font_weight);
                entry.italic = Some(font.is_italic());
            }
            if let Some(image) = object.as_image_object() {
                entry.width_px = image.width().ok().and_then(|n| n.try_into().ok());
                entry.height_px = image.height().ok().and_then(|n| n.try_into().ok());
            }
            objects.push(entry);
        }
        pages.push(PageObjectsPage {
            page: number,
            width_pt: f64::from(page.width().value),
            height_pt: f64::from(page.height().value),
            rotation_deg: rotation_degrees(page.rotation().map_err(pdfium_error)?),
            objects,
        });
        ctx.progress("page_objects", (done + 1) as u64, selected.len() as u64)?;
    }
    let report = PageObjectsDocument::new(page_count, pages);
    let bytes =
        serde_json::to_vec_pretty(&report).map_err(|_| ariad_engine_io::EngineFailure::io())?;
    if bytes.len() as u64 > super::apply::output_limit(ctx) {
        return Err(ariad_engine_io::EngineFailure::limit_exceeded());
    }
    let path = ctx.artifact_path(&format!("{}.page-objects.json", ctx.input_stem()))?;
    fs::write(&path, bytes).map_err(|_| ariad_engine_io::EngineFailure::io())?;
    ctx.artifact(&path, "json")?;
    Ok(Some(Metrics::from([
        ("page_count".into(), json!(page_count)),
        ("object_count".into(), json!(object_count)),
    ])))
}

pub(super) fn font_weight(weight: PdfFontWeight) -> u16 {
    match weight {
        PdfFontWeight::Weight100 => 100,
        PdfFontWeight::Weight200 => 200,
        PdfFontWeight::Weight300 => 300,
        PdfFontWeight::Weight400Normal => 400,
        PdfFontWeight::Weight500 => 500,
        PdfFontWeight::Weight600 => 600,
        PdfFontWeight::Weight700Bold => 700,
        PdfFontWeight::Weight800 => 800,
        PdfFontWeight::Weight900 => 900,
        PdfFontWeight::Custom(value) => value.try_into().unwrap_or(400),
    }
}
