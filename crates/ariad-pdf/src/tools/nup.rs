//! Implementation of the `pdf.nup` tool and shared multi-page layout helpers for
//! `pdf.booklet`, `pdf.divide_pages`, and `pdf.posterize`.

use std::collections::{HashMap, HashSet};

use ariad_core::{
    reader::pdf::ReadError,
    tools::params::{
        common::{GridOrder, PaperSize},
        nup::NupParams,
        scale::ScaleFit,
    },
};
use lopdf::{Dictionary, Document, Object, ObjectId, Stream, dictionary};

use crate::{
    document::{DecodeBudget, PdfDoc, ToolCtx},
    error::PdfError,
    paper::{self, PaperDimensions},
    tools::OutputDoc,
    xobject::{self, FormXObject, Matrix, ObjectCopier, PdfRect},
};

/// Maximum outline hierarchy depth traversed when remapping bookmark destinations.
const MAX_OUTLINE_DEPTH: usize = 32;

/// Maximum number of indirect reference hops followed when inspecting outline dictionaries.
const MAX_REF_HOPS: usize = 8;

/// Places multiple input pages onto each output sheet in an N-up grid layout.
pub fn run(
    inputs: &[PdfDoc],
    params: &NupParams,
    ctx: &ToolCtx,
) -> Result<Vec<OutputDoc>, PdfError> {
    ctx.check()?;
    if inputs.len() != 1 {
        return Err(PdfError::InvalidParams(format!(
            "pdf.nup requires exactly 1 input document, got {}",
            inputs.len()
        )));
    }
    if !matches!(params.per_sheet, 2 | 4 | 6 | 8 | 9 | 16) {
        return Err(PdfError::InvalidParams(format!(
            "invalid per_sheet {}: expected 2, 4, 6, 8, 9, or 16",
            params.per_sheet
        )));
    }

    let src_pdf = &inputs[0];
    let src_doc = src_pdf.inner();
    let src_pages = collect_ordered_page_ids(src_doc, ctx)?;
    let sheet_dims = paper::resolve_paper_size(&params.sheet)?;

    let (margin_top, margin_right, margin_bottom, margin_left) = params.margins_pt.to_trbl();
    if !margin_top.is_finite()
        || !margin_right.is_finite()
        || !margin_bottom.is_finite()
        || !margin_left.is_finite()
        || margin_top < 0.0
        || margin_right < 0.0
        || margin_bottom < 0.0
        || margin_left < 0.0
    {
        return Err(PdfError::InvalidParams(
            "margins_pt values must be finite non-negative numbers".to_owned(),
        ));
    }

    let usable_left = margin_left;
    let usable_bottom = margin_bottom;
    let usable_right = sheet_dims.width_pt - margin_right;
    let usable_top = sheet_dims.height_pt - margin_top;
    let usable_w = usable_right - usable_left;
    let usable_h = usable_top - usable_bottom;
    if usable_w <= 1.0 || usable_h <= 1.0 {
        return Err(PdfError::InvalidParams(format!(
            "margins_pt ({margin_top}, {margin_right}, {margin_bottom}, {margin_left}) exceed sheet dimensions ({} x {} pt)",
            sheet_dims.width_pt, sheet_dims.height_pt
        )));
    }

    let (cols, rows) = grid_dimensions(params.per_sheet, sheet_dims);
    let cell_w = usable_w / f64::from(cols);
    let cell_h = usable_h / f64::from(rows);

    let mut target_doc = Document::with_version(effective_version(src_doc));
    let pages_root_id = target_doc.new_object_id();
    let mut copier = ObjectCopier::new();
    let mut budget = src_pdf.decode_budget();

    let per_sheet = usize::from(params.per_sheet);
    let out_page_count = src_pages.len().div_ceil(per_sheet);
    let cap = super::page_set::output_page_cap(ctx.max_pages);
    if (out_page_count as u64) > cap {
        return Err(PdfError::LimitExceeded(ReadError::TooManyPages {
            actual: out_page_count as u64,
            limit: u32::try_from(cap).unwrap_or(u32::MAX),
        }));
    }
    let mut new_page_ids = Vec::with_capacity(out_page_count);
    let mut page_map: HashMap<ObjectId, ObjectId> = HashMap::with_capacity(src_pages.len());

    let sheet_box = PdfRect::from_size(sheet_dims.width_pt, sheet_dims.height_pt);

    for chunk in src_pages.chunks(per_sheet) {
        ctx.check()?;
        let mut xobjects_dict = Dictionary::new();
        let mut content_buf = Vec::new();
        let sheet_page_id = target_doc.new_object_id();

        for (slot_idx, &src_page_id) in chunk.iter().enumerate() {
            ctx.check()?;
            let form = convert_page_with_budget(
                src_doc,
                src_page_id,
                &mut target_doc,
                &mut copier,
                &mut budget,
                ctx,
            )?;
            page_map.entry(src_page_id).or_insert(sheet_page_id);

            let (col, row) = match params.order {
                GridOrder::Row => (slot_idx % usize::from(cols), slot_idx / usize::from(cols)),
                GridOrder::Column => (slot_idx / usize::from(rows), slot_idx % usize::from(rows)),
            };

            let cell_left = usable_left + (col as f64) * cell_w;
            let cell_right = cell_left + cell_w;
            let cell_top = usable_top - (row as f64) * cell_h;
            let cell_bottom = cell_top - cell_h;
            let cell_rect = PdfRect::new(cell_left, cell_bottom, cell_right, cell_top);

            let fm_name = format!("Fm{}", slot_idx + 1);
            xobjects_dict.set(fm_name.as_bytes().to_vec(), Object::Reference(form.id));

            let matrix = xobject::placement_matrix(
                form.crop_box,
                form.rotation,
                cell_rect,
                ScaleFit::Contain,
            );
            append_clipped_xobject(&mut content_buf, cell_rect, &fm_name, matrix);

            if params.borders {
                append_rect_border(&mut content_buf, cell_rect);
            }
        }

        insert_sheet_page(
            &mut target_doc,
            sheet_page_id,
            pages_root_id,
            sheet_box,
            xobjects_dict,
            content_buf,
        );
        new_page_ids.push(sheet_page_id);
    }

    finalize_layout_document(
        src_pdf,
        target_doc,
        pages_root_id,
        &new_page_ids,
        &page_map,
        &mut copier,
        false,
        ctx,
    )
}

/// Returns `(cols, rows)` for `per_sheet` (`2 | 4 | 6 | 8 | 9 | 16`), orienting rectangular grids
/// to match whether `sheet` is portrait (`cols <= rows`) or landscape (`cols >= rows`).
#[must_use]
pub(crate) const fn grid_dimensions(per_sheet: u8, sheet: PaperDimensions) -> (u8, u8) {
    let landscape = sheet.width_pt >= sheet.height_pt;
    match per_sheet {
        2 => {
            if landscape {
                (2, 1)
            } else {
                (1, 2)
            }
        }
        4 => (2, 2),
        6 => {
            if landscape {
                (3, 2)
            } else {
                (2, 3)
            }
        }
        8 => {
            if landscape {
                (4, 2)
            } else {
                (2, 4)
            }
        }
        9 => (3, 3),
        16 => (4, 4),
        _ => (2, 1),
    }
}

/// Collects the ordered leaf `/Page` object IDs of `doc`, rejecting empty or corrupt page trees.
pub(crate) fn collect_ordered_page_ids(
    doc: &Document,
    ctx: &ToolCtx,
) -> Result<Vec<ObjectId>, PdfError> {
    let mut seen = HashSet::new();
    let mut page_ids = Vec::new();
    for id in doc.page_iter() {
        ctx.check()?;
        if seen.insert(id) {
            page_ids.push(id);
        }
    }
    if page_ids.is_empty() {
        return Err(PdfError::Malformed);
    }
    Ok(page_ids)
}

/// Returns the PDF version string of `doc` (defaulting to `"1.5"` when empty).
#[must_use]
pub(crate) fn effective_version(doc: &Document) -> String {
    if doc.version.trim().is_empty() {
        "1.5".to_owned()
    } else {
        doc.version.clone()
    }
}

/// Converts `page_id` from `source_doc` into a [`FormXObject`] in `target_doc`, decoding and
/// charging content and resource streams once against `budget`.
pub(crate) fn convert_page_with_budget(
    source_doc: &Document,
    page_id: ObjectId,
    target_doc: &mut Document,
    copier: &mut ObjectCopier,
    budget: &mut DecodeBudget,
    ctx: &ToolCtx,
) -> Result<FormXObject, PdfError> {
    xobject::page_to_form_xobject(source_doc, page_id, target_doc, copier, budget, ctx)
}

/// Appends a clipped Form XObject placement (`q x y w h re W n a b c d e f cm /<name> Do Q\n`)
/// to `buf`.
pub(crate) fn append_clipped_xobject(
    buf: &mut Vec<u8>,
    clip_rect: PdfRect,
    xobject_name: &str,
    matrix: Matrix,
) {
    let [a, b, c, d, e, f] = matrix.0;
    let clean_name = xobject_name.trim_start_matches('/');
    let snippet = format!(
        "q {} {} {} {} re W n {} {} {} {} {} {} cm /{clean_name} Do Q\n",
        fmt_f64(clip_rect.left),
        fmt_f64(clip_rect.bottom),
        fmt_f64(clip_rect.width()),
        fmt_f64(clip_rect.height()),
        fmt_f64(a),
        fmt_f64(b),
        fmt_f64(c),
        fmt_f64(d),
        fmt_f64(e),
        fmt_f64(f),
    );
    buf.extend_from_slice(snippet.as_bytes());
}

/// Appends a thin `0.5 pt` black stroke rectangle around `rect` (`q 0.5 w 0 0 0 RG x y w h re S Q\n`).
pub(crate) fn append_rect_border(buf: &mut Vec<u8>, rect: PdfRect) {
    let snippet = format!(
        "q 0.5 w 0 0 0 RG {} {} {} {} re S Q\n",
        fmt_f64(rect.left),
        fmt_f64(rect.bottom),
        fmt_f64(rect.width()),
        fmt_f64(rect.height()),
    );
    buf.extend_from_slice(snippet.as_bytes());
}

/// Formats a floating-point coordinate or matrix component deterministically without trailing zeros.
#[must_use]
pub(crate) fn fmt_f64(v: f64) -> String {
    let clean = if v.abs() < 1e-12 { 0.0 } else { v };
    if (clean - clean.round()).abs() < 1e-9 {
        format!("{}", clean.round() as i64)
    } else {
        let s = format!("{clean:.6}");
        s.trim_end_matches('0').trim_end_matches('.').to_owned()
    }
}

/// Inserts a newly constructed `/Page` object and its compressed `/Contents` stream into `target_doc`.
pub(crate) fn insert_sheet_page(
    target_doc: &mut Document,
    page_id: ObjectId,
    pages_root_id: ObjectId,
    sheet_box: PdfRect,
    xobjects_dict: Dictionary,
    content_bytes: Vec<u8>,
) {
    let mut resources = Dictionary::new();
    if !xobjects_dict.is_empty() {
        resources.set("XObject", Object::Dictionary(xobjects_dict));
    }

    let mut page_dict = dictionary! {
        "Type" => "Page",
        "Parent" => Object::Reference(pages_root_id),
        "MediaBox" => sheet_box.to_pdf_array(),
        "CropBox" => sheet_box.to_pdf_array(),
        "Resources" => Object::Dictionary(resources),
    };

    if !content_bytes.is_empty() {
        let mut stream = Stream::new(Dictionary::new(), content_bytes);
        let _ = stream.compress();
        let content_id = target_doc.add_object(Object::Stream(stream));
        page_dict.set("Contents", Object::Reference(content_id));
    }

    target_doc
        .objects
        .insert(page_id, Object::Dictionary(page_dict));
}

/// Completes a newly built layout [`Document`] by attaching `/Pages`, copying document-level
/// metadata (`/Info`, `/Lang`, `/Metadata`, `/ViewerPreferences`), remapping surviving `/Outlines`
/// destinations via `page_map`, and wrapping the result in [`OutputDoc`].
#[allow(clippy::too_many_arguments)]
pub(crate) fn finalize_layout_document(
    src_pdf: &PdfDoc,
    mut target_doc: Document,
    pages_root_id: ObjectId,
    new_page_ids: &[ObjectId],
    page_map: &HashMap<ObjectId, ObjectId>,
    copier: &mut ObjectCopier,
    keep_page_labels: bool,
    ctx: &ToolCtx,
) -> Result<Vec<OutputDoc>, PdfError> {
    ctx.check()?;
    let src_doc = src_pdf.inner();

    let kids: Vec<Object> = new_page_ids
        .iter()
        .copied()
        .map(Object::Reference)
        .collect();
    let count = i64::try_from(new_page_ids.len()).unwrap_or(i64::MAX);
    target_doc.objects.insert(
        pages_root_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => Object::Array(kids),
            "Count" => Object::Integer(count),
        }),
    );

    let mut catalog = dictionary! {
        "Type" => "Catalog",
        "Pages" => Object::Reference(pages_root_id),
    };

    if let Ok(src_cat) = src_doc.catalog() {
        for key in [
            b"Lang".as_slice(),
            b"Metadata".as_slice(),
            b"ViewerPreferences".as_slice(),
        ] {
            if let Ok(val) = src_cat.get(key) {
                let copied = copier.copy_object(src_doc, &mut target_doc, val, ctx)?;
                catalog.set(key.to_vec(), copied);
            }
        }
        if keep_page_labels && let Ok(labels) = src_cat.get(b"PageLabels") {
            let copied = copier.copy_object(src_doc, &mut target_doc, labels, ctx)?;
            catalog.set("PageLabels", copied);
        }
        if let Some(outlines_id) =
            copy_remapped_outlines(src_doc, src_cat, &mut target_doc, page_map, copier, ctx)?
        {
            catalog.set("Outlines", Object::Reference(outlines_id));
        }
    }

    let catalog_id = target_doc.add_object(Object::Dictionary(catalog));
    target_doc
        .trailer
        .set("Root", Object::Reference(catalog_id));

    if let Ok(info_obj) = src_doc.trailer.get(b"Info") {
        let copied_info = copier.copy_object(src_doc, &mut target_doc, info_obj, ctx)?;
        target_doc.trailer.set("Info", copied_info);
    }

    let mut out_pdf = src_pdf.clone();
    *out_pdf.inner_mut()? = target_doc;
    Ok(vec![OutputDoc::unnamed_pdf(out_pdf)])
}

#[derive(Clone, Debug)]
struct OutlineNode {
    title: Object,
    target_page: ObjectId,
    children: Vec<Self>,
}

fn copy_remapped_outlines(
    src_doc: &Document,
    src_catalog: &Dictionary,
    target_doc: &mut Document,
    page_map: &HashMap<ObjectId, ObjectId>,
    copier: &mut ObjectCopier,
    ctx: &ToolCtx,
) -> Result<Option<ObjectId>, PdfError> {
    let Ok(outlines_ref) = src_catalog.get(b"Outlines") else {
        return Ok(None);
    };
    let Some(outlines_dict) = deref_dict(src_doc, outlines_ref) else {
        return Ok(None);
    };
    let Ok(first_id) = outlines_dict.get(b"First").and_then(Object::as_reference) else {
        return Ok(None);
    };

    let mut visited = HashSet::new();
    let nodes = read_outline_siblings(src_doc, first_id, page_map, 0, &mut visited, ctx)?;
    if nodes.is_empty() {
        return Ok(None);
    }

    let root_id = target_doc.new_object_id();
    let (first_child, last_child, total_count) =
        write_outline_siblings(src_doc, target_doc, root_id, &nodes, copier, ctx)?;
    target_doc.objects.insert(
        root_id,
        Object::Dictionary(dictionary! {
            "Type" => "Outlines",
            "First" => Object::Reference(first_child),
            "Last" => Object::Reference(last_child),
            "Count" => Object::Integer(total_count),
        }),
    );
    Ok(Some(root_id))
}

fn read_outline_siblings(
    src_doc: &Document,
    first_id: ObjectId,
    page_map: &HashMap<ObjectId, ObjectId>,
    depth: usize,
    visited: &mut HashSet<ObjectId>,
    ctx: &ToolCtx,
) -> Result<Vec<OutlineNode>, PdfError> {
    if depth >= MAX_OUTLINE_DEPTH {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    let mut current = Some(first_id);
    while let Some(item_id) = current {
        ctx.check()?;
        if !visited.insert(item_id) {
            break;
        }
        let Ok(dict) = src_doc.get_dictionary(item_id) else {
            break;
        };
        current = dict.get(b"Next").ok().and_then(|o| o.as_reference().ok());

        let title = dict
            .get(b"Title")
            .cloned()
            .unwrap_or_else(|_| Object::string_literal("Bookmark"));
        let mapped_dest = extract_outline_dest(src_doc, dict)
            .and_then(|source_page| page_map.get(&source_page).copied());

        let children = if let Ok(child_first) = dict.get(b"First").and_then(Object::as_reference) {
            read_outline_siblings(src_doc, child_first, page_map, depth + 1, visited, ctx)?
        } else {
            Vec::new()
        };

        if let Some(target_page) = mapped_dest {
            out.push(OutlineNode {
                title,
                target_page,
                children,
            });
        } else if !children.is_empty() {
            // If parent destination was dropped, promote surviving children in order.
            out.extend(children);
        }
    }
    Ok(out)
}

fn extract_outline_dest(doc: &Document, item_dict: &Dictionary) -> Option<ObjectId> {
    if let Ok(dest) = item_dict.get(b"Dest")
        && let Some(resolved) = dest_from_object(doc, dest)
    {
        return Some(resolved);
    }
    if let Ok(action_obj) = item_dict.get(b"A")
        && let Some(action_dict) = deref_dict(doc, action_obj)
        && let Ok(subtype) = action_dict.get(b"S")
        && deref_name(doc, subtype) == Some(b"GoTo")
        && let Ok(dest) = action_dict.get(b"D")
    {
        return dest_from_object(doc, dest);
    }
    None
}

/// Returns the source page an explicit destination array points at. The remainder of the array
/// (for example `/XYZ x y zoom`) is in source-page coordinates, which mean nothing on the
/// re-laid-out output sheet, so it is intentionally not returned.
fn dest_from_object(doc: &Document, dest: &Object) -> Option<ObjectId> {
    let arr = resolve_object_bounded(doc, dest)?.as_array().ok()?;
    arr.first()?.as_reference().ok()
}

fn write_outline_siblings(
    src_doc: &Document,
    target_doc: &mut Document,
    parent_id: ObjectId,
    nodes: &[OutlineNode],
    copier: &mut ObjectCopier,
    ctx: &ToolCtx,
) -> Result<(ObjectId, ObjectId, i64), PdfError> {
    let ids: Vec<ObjectId> = (0..nodes.len())
        .map(|_| target_doc.new_object_id())
        .collect();
    let mut total_count = 0_i64;

    for (idx, node) in nodes.iter().enumerate() {
        ctx.check()?;
        let id = ids[idx];
        let copied_title = copier.copy_object(src_doc, target_doc, &node.title, ctx)?;
        // Source-page coordinates do not map onto the output sheet, so every destination fits
        // the whole sheet page.
        let dest_arr = vec![
            Object::Reference(node.target_page),
            Object::Name(b"Fit".to_vec()),
        ];
        let mut dict = dictionary! {
            "Title" => copied_title,
            "Parent" => Object::Reference(parent_id),
            "Dest" => Object::Array(dest_arr),
        };
        if idx > 0 {
            dict.set("Prev", Object::Reference(ids[idx - 1]));
        }
        if idx + 1 < ids.len() {
            dict.set("Next", Object::Reference(ids[idx + 1]));
        }
        if !node.children.is_empty() {
            let (c_first, c_last, c_count) =
                write_outline_siblings(src_doc, target_doc, id, &node.children, copier, ctx)?;
            dict.set("First", Object::Reference(c_first));
            dict.set("Last", Object::Reference(c_last));
            dict.set("Count", Object::Integer(c_count));
            total_count = total_count.saturating_add(c_count);
        }
        target_doc.objects.insert(id, Object::Dictionary(dict));
        total_count = total_count.saturating_add(1);
    }

    Ok((ids[0], *ids.last().unwrap(), total_count))
}

fn resolve_object_bounded<'a>(doc: &'a Document, mut obj: &'a Object) -> Option<&'a Object> {
    for _ in 0..MAX_REF_HOPS {
        match obj {
            Object::Reference(id) => {
                obj = doc.objects.get(id)?;
            }
            other => return Some(other),
        }
    }
    match obj {
        Object::Reference(_) => None,
        other => Some(other),
    }
}

fn deref_dict<'a>(doc: &'a Document, obj: &'a Object) -> Option<&'a Dictionary> {
    resolve_object_bounded(doc, obj)?.as_dict().ok()
}

fn deref_name<'a>(doc: &'a Document, obj: &'a Object) -> Option<&'a [u8]> {
    resolve_object_bounded(doc, obj)?.as_name().ok()
}

#[allow(dead_code)]
pub(crate) fn resolve_sheet_dims(sheet: &PaperSize) -> Result<PaperDimensions, PdfError> {
    paper::resolve_paper_size(sheet)
}
