//! Private, bounded PDFium geometry evidence for redaction.

use std::fs;

use ariad_core::{
    protocol::ErrorCode,
    tools::params::redact_internal::{
        RedactGeometry, RedactGeometryChar, RedactGeometryPage, RedactInspectParams,
    },
};
use ariad_engine_io::EngineFailure;

use crate::{
    output::{ToolContext, ToolResult},
    text_layout::{map_pdfium_error, rotation_degrees},
};

const MAX_CHARS: usize = 1_000_000;
const MAX_OUTPUT: usize = 128 * 1024 * 1024;

/// Writes evidence only inside the private engine workspace.
pub fn inspect(ctx: &mut ToolContext<'_>, params: RedactInspectParams) -> ToolResult {
    let (bytes, source_sha256) = ctx.load_validated_pdf_with_source_digest(Some(&params.pages))?;
    let pdfium = ctx.pdfium()?;
    let doc = pdfium
        .load_pdf_from_byte_slice(&bytes, None)
        .map_err(map_pdfium_error)?;
    let count = usize::try_from(doc.pages().len()).map_err(|_| EngineFailure::limit_exceeded())?;
    let selected = ctx.resolve_pages(&params.pages, count)?;
    let mut evidence = RedactGeometry {
        schema: "ariad-redact-geometry/1".into(),
        source_sha256,
        page_count: count,
        pages: Vec::with_capacity(selected.len()),
    };
    let mut char_count = 0usize;
    for (position, page_number) in selected.iter().copied().enumerate() {
        ctx.check()?;
        let index = i32::try_from(page_number - 1).map_err(|_| EngineFailure::limit_exceeded())?;
        let page = doc.pages().get(index).map_err(map_pdfium_error)?;
        let width = f64::from(page.width().value);
        let height = f64::from(page.height().value);
        if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
            return Err(EngineFailure::new(
                ErrorCode::EngineFailure,
                "Invalid page geometry.",
            ));
        }
        let rotation = rotation_degrees(page.rotation().map_err(map_pdfium_error)?);
        let page_text = page.text().map_err(map_pdfium_error)?;
        let mut chars = Vec::new();
        for (index, character) in page_text.chars().iter().enumerate() {
            if index % 128 == 0 {
                ctx.check()?;
            }
            char_count = char_count
                .checked_add(1)
                .ok_or_else(EngineFailure::limit_exceeded)?;
            if char_count > MAX_CHARS {
                return Err(EngineFailure::limit_exceeded());
            }
            let value = character
                .unicode_char()
                .filter(|c| *c != '\0')
                .ok_or_else(|| {
                    EngineFailure::new(
                        ErrorCode::EngineFailure,
                        "A character cannot be mapped to Unicode.",
                    )
                })?;
            let generated = character.is_generated().map_err(map_pdfium_error)?;
            let origin = match character.origin() {
                Ok((x, y)) => {
                    let point = [f64::from(x.value), f64::from(y.value)];
                    if !point.iter().all(|n| n.is_finite()) {
                        return Err(EngineFailure::new(
                            ErrorCode::EngineFailure,
                            "Invalid character origin.",
                        ));
                    }
                    Some(point)
                }
                Err(_) if generated || value.is_whitespace() => None,
                Err(_) => {
                    return Err(EngineFailure::new(
                        ErrorCode::EngineFailure,
                        "A character has no usable origin.",
                    ));
                }
            };
            let mut bbox: Option<[f64; 4]> = None;
            for rect in [character.tight_bounds(), character.loose_bounds()]
                .into_iter()
                .flatten()
            {
                let values = [
                    f64::from(rect.left().value),
                    f64::from(rect.bottom().value),
                    f64::from(rect.right().value),
                    f64::from(rect.top().value),
                ];
                if !values.iter().all(|v| v.is_finite()) {
                    return Err(EngineFailure::new(
                        ErrorCode::EngineFailure,
                        "Invalid character geometry.",
                    ));
                }
                let bounds = [
                    values[0].min(values[2]),
                    values[1].min(values[3]),
                    values[0].max(values[2]),
                    values[1].max(values[3]),
                ];
                bbox = Some(match bbox {
                    Some(previous) => [
                        previous[0].min(bounds[0]),
                        previous[1].min(bounds[1]),
                        previous[2].max(bounds[2]),
                        previous[3].max(bounds[3]),
                    ],
                    None => bounds,
                });
            }
            if bbox.is_none() && !generated && !value.is_whitespace() {
                return Err(EngineFailure::new(
                    ErrorCode::EngineFailure,
                    "A character has no usable geometry.",
                ));
            }
            chars.push(RedactGeometryChar {
                value,
                index,
                generated,
                origin,
                bbox,
            });
        }
        evidence.pages.push(RedactGeometryPage {
            page: u32::try_from(page_number).map_err(|_| EngineFailure::limit_exceeded())?,
            width,
            height,
            rotation,
            chars,
        });
        ctx.progress(
            "redact.inspect",
            (position + 1) as u64,
            selected.len() as u64,
        )?;
    }
    let data = serde_json::to_vec(&evidence).map_err(|_| EngineFailure::io())?;
    if data.len() > MAX_OUTPUT || data.len() as u64 > ctx.limits().max_decompressed_bytes {
        return Err(EngineFailure::limit_exceeded());
    }
    let path = ctx.artifact_path("redact-geometry.json")?;
    fs::write(&path, data).map_err(|_| EngineFailure::io())?;
    ctx.artifact(&path, "json")?;
    Ok(None)
}
