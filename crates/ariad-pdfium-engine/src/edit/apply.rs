//! Apply edits against IDs in the supplied revision, with deterministic font subsets and saves.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{self, Write},
    sync::Arc,
};

use ariad_core::{
    protocol::ErrorCode,
    tools::params::{EditApplyParams, EditSaveMode, PageEditOp},
};
use ariad_engine_io::EngineFailure;
use ariad_pdf::fonts::{
    BundledFontChoice, ResolvedEditFont, embed_subset_type0_font, resolve_font_for_text,
};
use lopdf::{
    Document, Object, Stream, StringFormat,
    content::{Content, Operation},
    dictionary,
};
use pdfium_render::prelude::*;
use serde_json::json;
use unicode_normalization::UnicodeNormalization;

use super::{invalid, pdfium_error};
use crate::{
    output::{Metrics, ToolContext, ToolResult},
    text_layout::{map_pdf_error, read_and_preflight_pdf},
};

const MAX_TEXT_BYTES: usize = 1_048_576;

#[derive(Clone)]
struct TextState {
    text: String,
    name: String,
    embedded: Option<Arc<[u8]>>,
    bold: bool,
    italic: bool,
    appearance: Option<TextAppearance>,
}

#[derive(Clone)]
struct TextAppearance {
    matrix: PdfMatrix,
    size: f64,
    fill: PdfColor,
    stroke: PdfColor,
    mode: PdfPageTextRenderMode,
}

struct Slot {
    count: usize,
    text: Option<TextState>,
    image: bool,
}

struct PreparedText {
    text: String,
    choice: BundledFontChoice,
    from: String,
    lines: Vec<Option<usize>>,
}

/// Bound aggregate serialized output, including the original revision in incremental mode.
pub(super) fn output_limit(ctx: &ToolContext<'_>) -> u64 {
    ctx.limits().max_decompressed_bytes.min(
        ctx.limits()
            .max_memory_mb
            .map_or(u64::MAX, |n| u64::from(n) * 1024 * 1024),
    )
}

/// Validate the whole list, prepare covering fonts, then apply operations without publishing
/// an artifact until every operation and serialization succeeds.
pub fn run(ctx: &mut ToolContext<'_>, params: EditApplyParams) -> ToolResult {
    params.validate().map_err(invalid)?;
    let (bytes, mut source) = read_and_preflight_pdf(ctx)?;
    ctx.warn_repaired_xref(&source)?;
    let original = if params.save == EditSaveMode::Incremental {
        let original = fs::read(ctx.input()).map_err(|_| EngineFailure::io())?;
        if original != bytes || source.inner().xref_start == 0 || source.repaired_xref() {
            return Err(invalid(
                "incremental save requires an unchanged input with a valid cross-reference table",
            ));
        }
        Some(original)
    } else {
        None
    };
    let previous = original.as_ref().map(|_| source.inner().clone());
    let canonical = crate::canonical::canonicalize(ctx, &mut source, bytes.len())?;
    let pdfium = ctx.pdfium()?;
    let initial = pdfium
        .load_pdf_from_byte_slice(&canonical, None)
        .map_err(pdfium_error)?;
    let page_count =
        usize::try_from(initial.pages().len()).map_err(|_| EngineFailure::limit_exceeded())?;
    ctx.check_page_count(page_count)?;
    let mut slots = inspect_slots(ctx, &initial, &params.ops)?;
    let mut prepared = plan_text(ctx, &initial, &params.ops, &mut slots)?;
    // Validation simulated deletions and text changes; live object indices start in the
    // supplied revision and are updated only as the corresponding operation executes.
    for page in slots.values_mut() {
        for slot in page {
            slot.count = 1;
        }
    }
    drop(initial);
    drop(canonical);
    let bank = add_text_bank(
        ctx,
        source.inner_mut().map_err(map_pdf_error)?,
        &mut prepared,
    )?;
    let seeded = crate::canonical::canonicalize(ctx, &mut source, bytes.len())?;
    drop(source);
    drop(bytes);
    let doc = pdfium
        .load_pdf_from_byte_slice(&seeded, None)
        .map_err(pdfium_error)?;
    let mut text_objects = Vec::new();
    if let Some(bank_index) = bank {
        let mut page = doc.pages().get(bank_index).map_err(pdfium_error)?;
        page.set_content_regeneration_strategy(PdfPageContentRegenerationStrategy::Manual);
        let expected = prepared
            .values()
            .flat_map(|p| &p.lines)
            .filter(|l| l.is_some())
            .count();
        if page.objects().len() != expected {
            return Err(EngineFailure::new(
                ErrorCode::EngineFailure,
                "Prepared text object count changed.",
            ));
        }
        for _ in 0..expected {
            text_objects.push(Some(
                page.objects_mut()
                    .remove_object_at_index(0)
                    .map_err(pdfium_error)?,
            ));
        }
        page.delete().map_err(pdfium_error)?;
    }
    for (index, op) in params.ops.iter().enumerate() {
        ctx.check()?;
        execute(
            ctx,
            &doc,
            op,
            &mut slots,
            prepared.get(&index),
            &mut text_objects,
        )?;
        ctx.progress("edit", (index + 1) as u64, params.ops.len() as u64)?;
    }
    let saved = doc.save_to_bytes().map_err(pdfium_error)?;
    let mut edited = ariad_pdf::document::open(&saved, ctx.limits()).map_err(map_pdf_error)?;
    // PDFium may generate a random trailer ID. The shared writer removes it and derives IDs,
    // dates, object order and dictionary order from content, making crash replay reproducible.
    let full =
        ariad_pdf::writer::save(&mut edited, &ariad_pdf::ToolCtx::new()).map_err(map_pdf_error)?;
    let output = if let Some((original, previous)) = original.zip(previous) {
        incremental(ctx, original, previous, &full)?
    } else {
        full
    };
    if output.len() as u64 > output_limit(ctx) {
        return Err(EngineFailure::limit_exceeded());
    }
    ctx.check()?;
    let path = ctx.artifact_path(&format!("{}.edited.pdf", ctx.input_stem()))?;
    fs::write(&path, output).map_err(|_| EngineFailure::io())?;
    ctx.artifact(&path, "pdf")?;
    Ok(Some(Metrics::from([
        ("page_count".into(), json!(page_count)),
        ("op_count".into(), json!(params.ops.len())),
    ])))
}

fn page_and_id(op: &PageEditOp) -> (u32, Option<&str>) {
    match op {
        PageEditOp::ReplaceText {
            page, object_id, ..
        }
        | PageEditOp::SetTextStyle {
            page, object_id, ..
        }
        | PageEditOp::MoveObject {
            page, object_id, ..
        }
        | PageEditOp::DeleteObject { page, object_id }
        | PageEditOp::ReplaceImage {
            page, object_id, ..
        } => (*page, Some(object_id)),
        PageEditOp::AddText { page, .. } | PageEditOp::AddImage { page, .. } => (*page, None),
    }
}

fn object_index(id: &str) -> Result<usize, EngineFailure> {
    id.split_once("_obj")
        .and_then(|(_, i)| i.parse().ok())
        .ok_or_else(|| invalid("object_id out of range"))
}

fn retained_appearance(slots: &mut [Slot], index: Option<usize>) -> Option<&mut TextAppearance> {
    index
        .and_then(|i| slots.get_mut(i))
        .and_then(|slot| slot.text.as_mut())
        .and_then(|text| text.appearance.as_mut())
}

fn inspect_slots(
    ctx: &ToolContext<'_>,
    doc: &PdfDocument<'_>,
    ops: &[PageEditOp],
) -> Result<BTreeMap<u32, Vec<Slot>>, EngineFailure> {
    let mut result = BTreeMap::new();
    let targets = ops
        .iter()
        .filter(|op| {
            matches!(
                op,
                PageEditOp::ReplaceText { .. } | PageEditOp::SetTextStyle { .. }
            )
        })
        .map(|op| {
            let (page, id) = page_and_id(op);
            Ok((
                page,
                object_index(id.ok_or_else(|| invalid("missing text object id"))?)?,
            ))
        })
        .collect::<Result<BTreeSet<_>, EngineFailure>>()?;
    let mut embedded_bytes = 0u64;
    for op in ops {
        let (number, _) = page_and_id(op);
        if result.contains_key(&number) {
            continue;
        }
        ctx.check()?;
        let page = doc
            .pages()
            .get(
                (number - 1)
                    .try_into()
                    .map_err(|_| invalid("page out of range"))?,
            )
            .map_err(|_| invalid("page out of range"))?;
        if page.objects().len() > 100_000 {
            return Err(EngineFailure::limit_exceeded());
        }
        let mut slots = Vec::new();
        for index in 0..page.objects().len() {
            ctx.check()?;
            let object = page.objects().get(index).map_err(pdfium_error)?;
            let text = object
                .as_text_object()
                .filter(|_| targets.contains(&(number, index)))
                .map(|text| {
                    let font = text.font();
                    let embedded = if font.is_embedded().unwrap_or(false) {
                        font.data().ok()
                    } else {
                        None
                    };
                    if let Some(bytes) = &embedded {
                        embedded_bytes = embedded_bytes.saturating_add(bytes.len() as u64);
                        if embedded_bytes > output_limit(ctx) {
                            return Err(EngineFailure::limit_exceeded());
                        }
                    }
                    Ok(TextState {
                        text: text.text(),
                        name: font.name(),
                        embedded: embedded.map(Arc::from),
                        bold: font
                            .weight()
                            .ok()
                            .is_some_and(|w| super::page_objects::font_weight(w) >= 600),
                        italic: font.is_italic(),
                        appearance: None,
                    })
                })
                .transpose()?;
            slots.push(Slot {
                count: 1,
                text,
                image: object.as_image_object().is_some(),
            });
        }
        result.insert(number, slots);
    }
    Ok(result)
}

fn plan_text(
    ctx: &ToolContext<'_>,
    initial: &PdfDocument<'_>,
    ops: &[PageEditOp],
    pages: &mut BTreeMap<u32, Vec<Slot>>,
) -> Result<BTreeMap<usize, PreparedText>, EngineFailure> {
    let mut prepared = BTreeMap::new();
    let mut total_text = 0usize;
    for (index, op) in ops.iter().enumerate() {
        ctx.check()?;
        let (number, id) = page_and_id(op);
        let slots = pages
            .get_mut(&number)
            .ok_or_else(|| invalid("page out of range"))?;
        let mut slot = id
            .map(|id| {
                object_index(id).and_then(|i| {
                    slots
                        .get_mut(i)
                        .filter(|s| s.count != 0)
                        .ok_or_else(|| invalid("object_id is absent or was deleted"))
                })
            })
            .transpose()?;
        let (text, requested, state) = match op {
            PageEditOp::ReplaceText { text, .. } => (
                text.as_str(),
                None,
                slot.as_ref()
                    .and_then(|s| s.text.as_ref())
                    .ok_or_else(|| invalid("replace_text requires a text object"))?
                    .clone(),
            ),
            PageEditOp::SetTextStyle { font_name, .. } => {
                let state = slot
                    .as_ref()
                    .and_then(|s| s.text.as_ref())
                    .ok_or_else(|| invalid("set_text_style requires a text object"))?
                    .clone();
                if font_name.is_none() {
                    continue;
                }
                ("", font_name.as_deref(), state)
            }
            PageEditOp::AddText {
                text, font_name, ..
            } => (
                text.as_str(),
                font_name.as_deref(),
                TextState {
                    text: String::new(),
                    name: "unspecified".into(),
                    embedded: None,
                    bold: false,
                    italic: false,
                    appearance: None,
                },
            ),
            PageEditOp::DeleteObject { .. } => {
                if let Some(slot) = slot {
                    slot.count = 0;
                }
                continue;
            }
            PageEditOp::ReplaceImage { .. } => {
                if !slot.is_some_and(|s| s.image) {
                    return Err(invalid("replace_image requires an image object"));
                }
                continue;
            }
            _ => continue,
        };
        let text: String = if matches!(op, PageEditOp::SetTextStyle { .. }) {
            state.text.nfc().collect()
        } else {
            text.nfc().collect()
        };
        total_text = total_text
            .checked_add(text.len())
            .filter(|n| *n <= MAX_TEXT_BYTES)
            .ok_or_else(EngineFailure::limit_exceeded)?;
        let name = requested.unwrap_or(&state.name);
        let lower = name.to_ascii_lowercase();
        let bold = state.bold || lower.contains("bold");
        let italic = state.italic || lower.contains("italic") || lower.contains("oblique");
        let embedded = if requested.is_none() && !text.contains('\n') {
            state
                .embedded
                .as_deref()
                .map(|bytes| (state.name.as_str(), bytes))
        } else {
            None
        };
        let mut resolved = resolve_font_for_text(embedded, Some(name), bold, italic, &text)
            .map_err(|e| EngineFailure::owned(ErrorCode::InvalidRequest, e.to_string()))?;
        // Coverage in the embedded program is necessary but not sufficient: the PDF's CMap
        // also needs a reversible encoding. Probe the disposable inspection document; the
        // seeded document still comes from the untouched, preflighted source graph.
        if matches!(resolved, ResolvedEditFont::Embedded { .. })
            && !can_reencode(initial, number, id, &text)?
        {
            resolved = resolve_font_for_text(None, Some(name), bold, italic, &text)
                .map_err(|e| EngineFailure::owned(ErrorCode::InvalidRequest, e.to_string()))?;
        }
        let mut next = state.clone();
        next.text.clone_from(&text);
        if let ResolvedEditFont::Substituted { choice, to, .. } = resolved {
            prepared.insert(
                index,
                PreparedText {
                    text,
                    choice,
                    from: if matches!(op, PageEditOp::AddText { .. }) {
                        requested.unwrap_or("unspecified").to_owned()
                    } else {
                        state.name
                    },
                    lines: Vec::new(),
                },
            );
            next.name = to.into();
            next.embedded = None;
            next.bold = bold;
            next.italic = italic;
        }
        if let Some(slot) = slot.as_mut() {
            slot.text = Some(next);
        }
    }
    Ok(prepared)
}

fn can_reencode(
    doc: &PdfDocument<'_>,
    number: u32,
    id: Option<&str>,
    text: &str,
) -> Result<bool, EngineFailure> {
    let page = doc
        .pages()
        .get(
            (number - 1)
                .try_into()
                .map_err(|_| invalid("page out of range"))?,
        )
        .map_err(pdfium_error)?;
    let mut object = page
        .objects()
        .get(object_index(
            id.ok_or_else(|| invalid("missing text object id"))?,
        )?)
        .map_err(pdfium_error)?;
    let object = object
        .as_text_object_mut()
        .ok_or_else(|| invalid("operation requires text"))?;
    Ok(object.set_text(text).is_ok() && object.text() == text)
}

/// Prepare real text objects using the shared Identity-H subset writer. A private page holds
/// them only until PDFium opens the document; they are transferred, retaining exact character
/// codes, into their final positions and the private page is removed before saving.
fn add_text_bank(
    ctx: &ToolContext<'_>,
    doc: &mut Document,
    prepared: &mut BTreeMap<usize, PreparedText>,
) -> Result<Option<PdfPageIndex>, EngineFailure> {
    if prepared.is_empty() {
        return Ok(None);
    }
    let count = doc.get_pages().len();
    let bank_index = count
        .try_into()
        .map_err(|_| EngineFailure::limit_exceeded())?;
    let root = doc
        .catalog()
        .and_then(|c| c.get(b"Pages"))
        .and_then(Object::as_reference)
        .map_err(|_| invalid("invalid page tree"))?;
    let mut fonts = lopdf::Dictionary::new();
    let mut operations = Vec::new();
    let mut object_index = 0;
    for (index, text) in prepared {
        ctx.check()?;
        let (font, codes) =
            embed_subset_type0_font(doc, text.choice, &text.text).map_err(map_pdf_error)?;
        let name = format!("EditFont{index}");
        fonts.set(name.clone(), font);
        for line in text.text.split('\n') {
            if line.is_empty() {
                text.lines.push(None);
                continue;
            }
            let bytes = line
                .chars()
                .map(|c| {
                    codes.get(&c).ok_or_else(|| {
                        EngineFailure::new(
                            ErrorCode::InvalidRequest,
                            "glyph_unavailable: subset encoding is incomplete",
                        )
                    })
                })
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .flat_map(|c| c.to_be_bytes())
                .collect();
            operations.extend([
                Operation::new("q", vec![]),
                Operation::new("BT", vec![]),
                Operation::new(
                    "Tf",
                    vec![Object::Name(name.as_bytes().to_vec()), 12.into()],
                ),
                Operation::new(
                    "Tm",
                    vec![1.into(), 0.into(), 0.into(), 1.into(), 0.into(), 0.into()],
                ),
                Operation::new("Tj", vec![Object::String(bytes, StringFormat::Hexadecimal)]),
                Operation::new("ET", vec![]),
                Operation::new("Q", vec![]),
            ]);
            text.lines.push(Some(object_index));
            object_index += 1;
        }
    }
    let stream = doc.add_object(Stream::new(
        dictionary! {},
        Content { operations }
            .encode()
            .map_err(|_| EngineFailure::io())?,
    ));
    let page = doc.add_object(dictionary! {"Type" => "Page", "Parent" => root, "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()], "Resources" => dictionary! {"Font" => fonts}, "Contents" => stream});
    let pages = doc
        .get_dictionary_mut(root)
        .map_err(|_| invalid("invalid page tree"))?;
    let count = pages
        .get(b"Count")
        .and_then(Object::as_i64)
        .map_err(|_| invalid("invalid page count"))?;
    pages
        .get_mut(b"Kids")
        .and_then(Object::as_array_mut)
        .map_err(|_| invalid("invalid page tree"))?
        .push(page.into());
    pages.set("Count", count + 1);
    Ok(Some(bank_index))
}

fn execute<'a>(
    ctx: &mut ToolContext<'_>,
    doc: &PdfDocument<'a>,
    op: &PageEditOp,
    pages: &mut BTreeMap<u32, Vec<Slot>>,
    prepared: Option<&PreparedText>,
    bank: &mut [Option<PdfPageObject<'a>>],
) -> Result<(), EngineFailure> {
    let (number, id) = page_and_id(op);
    let slots = pages
        .get_mut(&number)
        .ok_or_else(|| invalid("page out of range"))?;
    let index = id.map(object_index).transpose()?;
    let start = index.map_or_else(
        || slots.iter().map(|s| s.count).sum(),
        |i| slots[..i].iter().map(|s| s.count).sum(),
    );
    let count = index.map_or(0, |i| slots[i].count);
    let mut page = doc
        .pages()
        .get(
            (number - 1)
                .try_into()
                .map_err(|_| invalid("page out of range"))?,
        )
        .map_err(pdfium_error)?;
    page.set_content_regeneration_strategy(PdfPageContentRegenerationStrategy::Manual);
    match op {
        PageEditOp::ReplaceText { text, .. } if prepared.is_none() => {
            let mut object = page.objects().get(start).map_err(pdfium_error)?;
            let text_object = object
                .as_text_object_mut()
                .ok_or_else(|| invalid("replace_text requires a text object"))?;
            let normalized: String = text.nfc().collect();
            text_object
                .set_text(normalized.clone())
                .map_err(pdfium_error)?;
            if text_object.text() != normalized {
                return Err(EngineFailure::new(
                    ErrorCode::InvalidRequest,
                    "glyph_unavailable: embedded font cannot re-encode the replacement",
                ));
            }
        }
        PageEditOp::ReplaceText { .. }
        | PageEditOp::AddText { .. }
        | PageEditOp::SetTextStyle {
            font_name: Some(_), ..
        } => {
            let prepared = prepared.ok_or_else(|| invalid("missing prepared font"))?;
            let object_id = id.map_or_else(
                || format!("p{number}_obj{}", page.objects().len()),
                str::to_owned,
            );
            let mut appearance = if let PageEditOp::AddText {
                x_pt,
                y_pt,
                font_size,
                color,
                ..
            } = op
            {
                TextAppearance {
                    matrix: PdfMatrix::new(
                        1.0,
                        0.0,
                        0.0,
                        1.0,
                        point(*x_pt)?.value,
                        point(*y_pt)?.value,
                    ),
                    size: *font_size,
                    fill: parse_color(color.as_deref().unwrap_or("#000000"))?,
                    stroke: PdfColor::BLACK,
                    mode: PdfPageTextRenderMode::FilledUnstroked,
                }
            } else if let Some(appearance) = retained_appearance(slots, index) {
                appearance.clone()
            } else if count == 0 {
                return Err(invalid("text object has no retained appearance"));
            } else {
                let old = page.objects().get(start).map_err(pdfium_error)?;
                let text = old
                    .as_text_object()
                    .ok_or_else(|| invalid("operation requires a text object"))?;
                TextAppearance {
                    matrix: old.matrix().map_err(pdfium_error)?,
                    size: f64::from(text.unscaled_font_size().value),
                    fill: old.fill_color().map_err(pdfium_error)?,
                    stroke: old.stroke_color().map_err(pdfium_error)?,
                    mode: text.render_mode(),
                }
            };
            if let PageEditOp::SetTextStyle {
                font_size, color, ..
            } = op
            {
                if let Some(size) = font_size {
                    appearance.size = *size;
                }
                if let Some(color) = color {
                    appearance.fill = parse_color(color)?;
                }
            };
            for _ in 0..count {
                drop(
                    page.objects_mut()
                        .remove_object_at_index(start)
                        .map_err(pdfium_error)?,
                );
            }
            let mut inserted = 0;
            for (line, bank_index) in prepared.lines.iter().enumerate() {
                let Some(bank_index) = bank_index else {
                    continue;
                };
                let mut object = bank[*bank_index]
                    .take()
                    .ok_or_else(|| invalid("prepared text already consumed"))?;
                object
                    .as_text_object_mut()
                    .ok_or_else(|| invalid("prepared object is not text"))?
                    .set_unscaled_font_size(point(appearance.size)?)
                    .map_err(pdfium_error)?;
                let line_matrix = PdfMatrix::new(
                    appearance.matrix.a(),
                    appearance.matrix.b(),
                    appearance.matrix.c(),
                    appearance.matrix.d(),
                    appearance.matrix.e()
                        - appearance.matrix.c() * line as f32 * appearance.size as f32 * 1.2,
                    appearance.matrix.f()
                        - appearance.matrix.d() * line as f32 * appearance.size as f32 * 1.2,
                );
                object.reset_matrix(line_matrix).map_err(pdfium_error)?;
                object
                    .set_fill_color(appearance.fill)
                    .map_err(pdfium_error)?;
                object
                    .set_stroke_color(appearance.stroke)
                    .map_err(pdfium_error)?;
                object
                    .as_text_object_mut()
                    .ok_or_else(|| invalid("prepared object is not text"))?
                    .set_render_mode(appearance.mode)
                    .map_err(pdfium_error)?;
                let at = if index.is_some() {
                    start + inserted
                } else {
                    page.objects().len()
                };
                page.objects_mut()
                    .insert_object_at_index(at, object)
                    .map_err(pdfium_error)?;
                inserted += 1;
            }
            if let Some(i) = index {
                slots[i].count = inserted;
                if let Some(text) = slots[i].text.as_mut() {
                    text.appearance = Some(appearance);
                }
            }
            ctx.warn("font_substituted", &json!({"page": number, "object_id": object_id, "from": prepared.from, "to": prepared.choice.postscript_name()}).to_string())?;
        }
        PageEditOp::SetTextStyle {
            font_size, color, ..
        } => {
            for i in start..start + count {
                let mut object = page.objects().get(i).map_err(pdfium_error)?;
                if let Some(size) = font_size {
                    object
                        .as_text_object_mut()
                        .ok_or_else(|| invalid("set_text_style requires text"))?
                        .set_unscaled_font_size(point(*size)?)
                        .map_err(pdfium_error)?;
                }
                if let Some(color) = color {
                    object
                        .set_fill_color(parse_color(color)?)
                        .map_err(pdfium_error)?;
                }
            }
            if let Some(appearance) = retained_appearance(slots, index) {
                if let Some(size) = font_size {
                    appearance.size = *size;
                }
                if let Some(color) = color {
                    appearance.fill = parse_color(color)?;
                }
            }
        }
        PageEditOp::MoveObject { dx_pt, dy_pt, .. } => {
            for i in start..start + count {
                let mut object = page.objects().get(i).map_err(pdfium_error)?;
                let m = object.matrix().map_err(pdfium_error)?;
                object
                    .reset_matrix(translated_matrix(m, *dx_pt, *dy_pt)?)
                    .map_err(pdfium_error)?;
            }
            if let Some(appearance) = retained_appearance(slots, index) {
                appearance.matrix = translated_matrix(appearance.matrix, *dx_pt, *dy_pt)?;
            }
        }
        PageEditOp::DeleteObject { .. } => {
            for _ in 0..count {
                drop(
                    page.objects_mut()
                        .remove_object_at_index(start)
                        .map_err(pdfium_error)?,
                );
            }
            if let Some(i) = index {
                slots[i].count = 0;
            }
        }
        PageEditOp::AddImage {
            bbox, image_base64, ..
        } => {
            let mut object = PdfPageImageObject::new(doc).map_err(pdfium_error)?;
            set_image(ctx, &mut object, image_base64)?;
            object
                .reset_matrix(PdfMatrix::new(
                    point(bbox[2] - bbox[0])?.value,
                    0.0,
                    0.0,
                    point(bbox[3] - bbox[1])?.value,
                    point(bbox[0])?.value,
                    point(bbox[1])?.value,
                ))
                .map_err(pdfium_error)?;
            page.objects_mut()
                .add_object(PdfPageObject::Image(object))
                .map_err(pdfium_error)?;
        }
        PageEditOp::ReplaceImage { image_base64, .. } => {
            let mut object = page.objects().get(start).map_err(pdfium_error)?;
            let matrix = object.matrix().map_err(pdfium_error)?;
            set_image(
                ctx,
                object
                    .as_image_object_mut()
                    .ok_or_else(|| invalid("replace_image requires an image object"))?,
                image_base64,
            )?;
            object.reset_matrix(matrix).map_err(pdfium_error)?;
        }
    }
    page.regenerate_content().map_err(pdfium_error)
}

fn point(value: f64) -> Result<PdfPoints, EngineFailure> {
    if !value.is_finite() || value.abs() > 1_000_000.0 {
        return Err(invalid("coordinates exceed the supported PDF range"));
    }
    Ok(PdfPoints::new(value as f32))
}

fn translated_matrix(
    matrix: PdfMatrix,
    dx_pt: f64,
    dy_pt: f64,
) -> Result<PdfMatrix, EngineFailure> {
    Ok(PdfMatrix::new(
        matrix.a(),
        matrix.b(),
        matrix.c(),
        matrix.d(),
        point(f64::from(matrix.e()) + dx_pt)?.value,
        point(f64::from(matrix.f()) + dy_pt)?.value,
    ))
}

fn parse_color(value: &str) -> Result<PdfColor, EngineFailure> {
    let value = value
        .strip_prefix('#')
        .ok_or_else(|| invalid("invalid color"))?;
    let component =
        |i| u8::from_str_radix(&value[i..i + 2], 16).map_err(|_| invalid("invalid color"));
    Ok(PdfColor::new(
        component(0)?,
        component(2)?,
        component(4)?,
        if value.len() == 8 { component(6)? } else { 255 },
    ))
}

fn set_image(
    ctx: &ToolContext<'_>,
    object: &mut PdfPageImageObject<'_>,
    encoded: &str,
) -> Result<(), EngineFailure> {
    let limit = ctx
        .limits()
        .max_asset_bytes
        .unwrap_or(output_limit(ctx))
        .min(output_limit(ctx));
    if encoded.len() as u64 > limit.saturating_mul(4).div_ceil(3) + 4 {
        return Err(EngineFailure::limit_exceeded());
    }
    let bytes = decode_base64(encoded)?;
    if bytes.len() as u64 > limit {
        return Err(EngineFailure::limit_exceeded());
    }
    let mut reader = image::ImageReader::new(io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| invalid("image format is unsupported"))?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(limit);
    limits.max_image_width = Some(16_384);
    limits.max_image_height = Some(16_384);
    reader.limits(limits);
    let image = reader
        .decode()
        .map_err(|_| invalid("image is invalid or exceeds resource limits"))?;
    ctx.check_pixels(u64::from(image.width()), u64::from(image.height()))?;
    if u64::from(image.width()) * u64::from(image.height()) * 4 > limit {
        return Err(EngineFailure::limit_exceeded());
    }
    let image = image.into_rgba8();
    let (width, height) = image.dimensions();
    let mut pixels = image.into_raw();
    for pixel in pixels.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
    }
    let bitmap = PdfBitmap::from_bytes(
        width
            .try_into()
            .map_err(|_| EngineFailure::limit_exceeded())?,
        height
            .try_into()
            .map_err(|_| EngineFailure::limit_exceeded())?,
        PdfBitmapFormat::BGRA,
        &mut pixels,
    )
    .map_err(pdfium_error)?;
    object.set_bitmap(&bitmap).map_err(pdfium_error)
}

fn decode_base64(value: &str) -> Result<Vec<u8>, EngineFailure> {
    if value.is_empty() || !value.len().is_multiple_of(4) {
        return Err(invalid("image_base64 must be canonical base64"));
    }
    let mut result = Vec::with_capacity(value.len() / 4 * 3);
    for (index, chunk) in value.as_bytes().as_chunks::<4>().0.iter().enumerate() {
        let mut word = 0u32;
        let mut padding = 0;
        for byte in chunk {
            let n = match *byte {
                b'A'..=b'Z' => byte - b'A',
                b'a'..=b'z' => byte - b'a' + 26,
                b'0'..=b'9' => byte - b'0' + 52,
                b'+' => 62,
                b'/' => 63,
                b'=' => {
                    padding += 1;
                    0
                }
                _ => return Err(invalid("image_base64 is invalid")),
            };
            if padding > 0 && *byte != b'=' {
                return Err(invalid("image_base64 padding is invalid"));
            }
            word = (word << 6) | u32::from(n);
        }
        if padding > 2
            || (padding > 0 && index + 1 != value.len() / 4)
            || (padding == 1 && word & 0xff != 0)
            || (padding == 2 && word & 0xffff != 0)
        {
            return Err(invalid("image_base64 padding is invalid"));
        }
        result.extend_from_slice(&word.to_be_bytes()[1..4 - padding]);
    }
    Ok(result)
}

struct LimitedOutput<'a, 'b> {
    ctx: &'a ToolContext<'b>,
    bytes: Vec<u8>,
}
impl Write for LimitedOutput<'_, '_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.ctx
            .check()
            .map_err(|_| io::Error::other("interrupted"))?;
        if (self.bytes.len() as u64).saturating_add(bytes.len() as u64) > output_limit(self.ctx) {
            return Err(io::Error::other("output limit"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn incremental(
    ctx: &ToolContext<'_>,
    original: Vec<u8>,
    mut previous: Document,
    full: &[u8],
) -> Result<Vec<u8>, EngineFailure> {
    let mut updated =
        Document::load_mem(full).map_err(|_| invalid("edited PDF could not be reopened"))?;
    updated.trailer.set("Prev", previous.xref_start as i64);
    updated.max_id = updated.max_id.max(previous.max_id);
    previous.reference_table.cross_reference_type = lopdf::xref::XrefType::CrossReferenceTable;
    let mut update = lopdf::IncrementalDocument::create_from(original, previous);
    update.new_document = updated;
    let mut output = LimitedOutput {
        ctx,
        bytes: Vec::new(),
    };
    if update.save_to(&mut output).is_err() {
        ctx.check()?;
        return Err(EngineFailure::limit_exceeded());
    }
    Ok(output.bytes)
}
