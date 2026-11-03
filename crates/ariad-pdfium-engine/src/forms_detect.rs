//! Propose form fields from bounded vector and raster signals, without executing actions.

mod geometry;
mod raster;
mod resources;
mod vector;

use crate::{
    output::{Metrics, ToolContext, ToolResult},
    text_layout::{self, map_pdfium_error, read_and_preflight_pdf},
};
use ariad_core::{
    protocol::ErrorCode,
    tools::params::{
        form_model::FieldType,
        forms::{DetectedField, FormsDetectMode, FormsDetectParams},
    },
};
use ariad_engine_io::EngineFailure;
use geometry::{Candidate, Label};
use pdfium_render::prelude::*;
use serde::Serialize;
use serde_json::json;
use std::{collections::HashMap, fs};

const MAX_PAGE_FIELDS: usize = 500;
const MAX_DOCUMENT_FIELDS: usize = 5000;
const MAX_LABELS: usize = 2048;
const MAX_LAYOUT_CHARS: usize = 100_000;

#[derive(Serialize)]
struct Detection {
    fields: Vec<DetectedField>,
    pages_scanned: usize,
    truncated: bool,
    warnings: Vec<String>,
}

fn widget_rectangles(
    ctx: &ToolContext<'_>,
    page: &PdfPage<'_>,
) -> Result<Vec<[f64; 4]>, EngineFailure> {
    let mut result = Vec::new();
    for (index, annotation) in page.annotations().iter().enumerate() {
        if index % 1024 == 0 {
            ctx.check()?;
        }
        if index >= 50_000 || index >= ctx.limits().max_blocks as usize {
            return Err(EngineFailure::limit_exceeded());
        }
        if !matches!(
            annotation.annotation_type(),
            PdfPageAnnotationType::Widget | PdfPageAnnotationType::XfaWidget
        ) {
            continue;
        }
        if let Ok(rect) = annotation.bounds() {
            result.push([
                f64::from(rect.left().value),
                f64::from(rect.bottom().value),
                f64::from(rect.right().value),
                f64::from(rect.top().value),
            ]);
        }
    }
    Ok(result)
}
fn add_warning(list: &mut Vec<String>, code: &str) {
    if !list.iter().any(|w| w == code) {
        list.push(code.into());
    }
}
fn labels_from_layout(layout: &text_layout::PageTextLayout) -> (Vec<Label>, Vec<Candidate>, bool) {
    let mut labels = Vec::new();
    let mut leaders = Vec::new();
    let mut truncated = false;
    for line in &layout.lines {
        if labels.len() >= MAX_LABELS {
            truncated = true;
            break;
        }
        let start = layout
            .words
            .get(line.word_start)
            .map_or(0, |word| word.char_start);
        let end = line
            .word_end
            .checked_sub(1)
            .and_then(|i| layout.words.get(i))
            .map_or(start, |word| word.char_end);
        let chars = layout.chars.get(start..end).unwrap_or(&[]);
        let mut run = Vec::new();
        let mut prefix = Vec::new();
        for c in chars {
            if c.text.chars().all(|v| v == '_' || v == '.') {
                run.push(c);
            } else if run.len() >= 3 {
                break;
            } else {
                prefix.append(&mut run);
                prefix.push(c);
            }
        }
        if run.len() >= 3 {
            let rect = run.iter().fold(
                [
                    f64::INFINITY,
                    f64::INFINITY,
                    f64::NEG_INFINITY,
                    f64::NEG_INFINITY,
                ],
                |r, c| {
                    [
                        r[0].min(c.bbox[0]),
                        r[1].min(c.bbox[1]),
                        r[2].max(c.bbox[2]),
                        r[3].max(c.bbox[3]),
                    ]
                },
            );
            if rect[2] - rect[0] >= 20.0 {
                leaders.push(Candidate {
                    rect: [
                        rect[0],
                        rect[1],
                        rect[2],
                        rect[1] + (line.font_size * 1.2).clamp(10.0, 30.0),
                    ],
                    kind: FieldType::Text,
                    confidence: 0.82,
                    reason: "text_leader",
                });
            }
            if !prefix.is_empty() {
                let bounds = prefix.iter().fold(
                    [
                        f64::INFINITY,
                        f64::INFINITY,
                        f64::NEG_INFINITY,
                        f64::NEG_INFINITY,
                    ],
                    |r, c| {
                        [
                            r[0].min(c.bbox[0]),
                            r[1].min(c.bbox[1]),
                            r[2].max(c.bbox[2]),
                            r[3].max(c.bbox[3]),
                        ]
                    },
                );
                let text = prefix.iter().map(|c| c.text.as_str()).collect::<String>();
                labels.push(Label {
                    text,
                    rect: bounds,
                    size: line.font_size,
                });
            }
        } else {
            labels.push(Label {
                text: line.text.clone(),
                rect: line.bbox,
                size: line.font_size,
            });
        }
    }
    (labels, leaders, truncated)
}

/// Detect candidate fields. Proposals require user confirmation before creation.
pub fn run(ctx: &mut ToolContext<'_>, params: FormsDetectParams) -> ToolResult {
    params
        .validate()
        .map_err(|e| EngineFailure::owned(ErrorCode::InvalidRequest, e))?;
    let (bytes, mut source) = read_and_preflight_pdf(ctx)?;
    ctx.warn_repaired_xref(&source)?;
    let canonical = crate::canonical::canonicalize(ctx, &mut source, bytes.len())?;
    drop(bytes);
    let page_ids = source.inner().get_pages();
    let pdfium = ctx.pdfium()?;
    let document = pdfium
        .load_pdf_from_byte_slice(&canonical, None)
        .map_err(map_pdfium_error)?;
    let total = document.pages().len() as usize;
    let mut pages = ctx.resolve_pages(&params.pages, total)?;
    pages.sort_unstable();
    let mut names = HashMap::new();
    let mut fields = Vec::new();
    let mut warnings = Vec::new();
    let mut truncated = false;
    let mut pages_scanned = 0usize;
    for (index, page_number) in pages.iter().copied().enumerate() {
        ctx.check()?;
        let page_index =
            PdfPageIndex::try_from(page_number - 1).map_err(|_| EngineFailure::limit_exceeded())?;
        let page = match document.pages().get(page_index) {
            Ok(page) => page,
            Err(_) => {
                add_warning(&mut warnings, "page_load_failed");
                continue;
            }
        };
        if let Some(id) = page_ids.get(&(page_number as u32)) {
            let (cycle, limited) = resources::inspect(ctx, source.inner(), *id)?;
            if cycle {
                add_warning(&mut warnings, "cyclic_form_xobject");
                continue;
            }
            if limited {
                truncated = true;
                add_warning(&mut warnings, "page_objects_truncated");
            }
        }
        pages_scanned += 1;
        let mut total_chars = 0usize;
        let layout = match text_layout::extract_visible_page_layout(
            ctx,
            &page,
            page_number as u32,
            &mut total_chars,
            MAX_LAYOUT_CHARS,
        ) {
            Ok(layout) => Some(layout),
            Err(_) => {
                ctx.check()?;
                add_warning(&mut warnings, "text_layout_unavailable");
                None
            }
        };
        let (labels, mut leaders, labels_truncated) = layout
            .as_ref()
            .map_or_else(|| (Vec::new(), Vec::new(), false), labels_from_layout);
        truncated |= labels_truncated;
        let words: Vec<[f64; 4]> = layout
            .as_ref()
            .map(|layout| {
                layout
                    .words
                    .iter()
                    .take(5000)
                    .map(|word| word.bbox)
                    .collect()
            })
            .unwrap_or_default();
        if layout
            .as_ref()
            .is_some_and(|layout| layout.words.len() > 5000)
        {
            truncated = true;
        }
        let mut signals = vector::VectorSignals::default();
        if params.mode != FormsDetectMode::Raster {
            vector::collect(
                ctx,
                page.objects().iter(),
                PdfMatrix::IDENTITY,
                0,
                &mut signals,
            )?;
        }
        if signals.truncated {
            truncated = true;
            add_warning(&mut warnings, "page_objects_truncated");
        }
        let widgets = widget_rectangles(ctx, &page)?;
        let has_vector_rules = signals.edges.iter().any(|edge| {
            (edge.a[1] - edge.b[1]).abs() <= 1.25 && (edge.a[0] - edge.b[0]).abs() >= 36.0
        }) || signals.boxes.iter().any(|rect| {
            let w = rect[2] - rect[0];
            let h = rect[3] - rect[1];
            (w >= 36.0 && h >= 12.0) || ((6.0..=20.0).contains(&w) && (6.0..=20.0).contains(&h))
        });
        let raster = params.mode == FormsDetectMode::Raster
            || (params.mode == FormsDetectMode::Auto && labels.is_empty() && !has_vector_rules);
        let candidates = if raster {
            add_warning(&mut warnings, "raster_detection");
            raster::detect(ctx, &page, page_number)?
        } else {
            if labels.is_empty() && !has_vector_rules {
                add_warning(&mut warnings, "no_vector_content");
            }
            let (mut found, hit_limit) = geometry::candidates(
                &signals.edges,
                &signals.boxes,
                &labels,
                &words,
                &widgets,
                ctx,
            )?;
            truncated |= hit_limit;
            found.append(&mut leaders);
            found
        };
        let mut filtered = Vec::new();
        for candidate in candidates {
            if !geometry::overlaps_widget(candidate.rect, &widgets, ctx)? {
                filtered.push(candidate);
            }
        }
        let mut proposed = geometry::proposals(
            filtered,
            &labels,
            page_number as u32,
            params.min_confidence,
            &mut names,
            ctx,
        )?;
        if proposed.len() > MAX_PAGE_FIELDS {
            proposed.truncate(MAX_PAGE_FIELDS);
            truncated = true;
        }
        let available = MAX_DOCUMENT_FIELDS.saturating_sub(fields.len());
        if proposed.len() > available {
            proposed.truncate(available);
            truncated = true;
        }
        fields.extend(proposed);
        ctx.progress("forms_detect", (index + 1) as u64, pages.len() as u64)?;
        if fields.len() >= MAX_DOCUMENT_FIELDS {
            if index + 1 < pages.len() {
                truncated = true;
            }
            break;
        }
    }
    for warning in &warnings {
        ctx.warn(warning, "Form detection reported a page-level limitation.")?;
    }
    let report = Detection {
        fields,
        pages_scanned,
        truncated,
        warnings,
    };
    let bytes = serde_json::to_vec_pretty(&report).map_err(|_| EngineFailure::io())?;
    if bytes.len() as u64 > ctx.limits().max_decompressed_bytes {
        return Err(EngineFailure::limit_exceeded());
    }
    let path = ctx.artifact_path(&format!("{}.detected-fields.json", ctx.input_stem()))?;
    fs::write(&path, bytes).map_err(|_| EngineFailure::io())?;
    ctx.artifact(&path, "json")?;
    Ok(Some(Metrics::from([
        ("pages_scanned".into(), json!(pages_scanned)),
        ("fields".into(), json!(report.fields.len())),
        ("truncated".into(), json!(truncated)),
    ])))
}
