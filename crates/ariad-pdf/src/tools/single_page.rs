//! Implementation of the `pdf.single_page` tool.
//!
//! Concatenates all pages of a PDF document vertically into a single continuous tall page
//! (`width = max_i visual_w_i`, `height = sum_i visual_h_i`) with page 1 at the top and page N
//! at the bottom.

use std::collections::HashSet;

use ariad_core::tools::params::{scale::ScaleFit, single_page::SinglePageParams};
use lopdf::{Dictionary, Document, Object, ObjectId, Stream, dictionary};

use crate::{
    document::{PdfDoc, ToolCtx},
    error::PdfError,
    paper::MAX_PAPER_SIDE_PT,
    tools::{
        OutputDoc,
        rotate::{collect_page_ids, materialize_inferred_rotations},
        scale::{charge_page_contents, transform_page_annotations},
    },
    xobject::{self, FormXObject, PdfRect},
};

/// Concatenates all pages of a PDF document vertically into a single continuous page.
pub fn run(
    inputs: &[PdfDoc],
    _params: &SinglePageParams,
    ctx: &ToolCtx,
) -> Result<Vec<OutputDoc>, PdfError> {
    ctx.check()?;
    if inputs.len() != 1 {
        return Err(PdfError::InvalidParams(format!(
            "pdf.single_page requires exactly 1 input document, got {}",
            inputs.len()
        )));
    }

    let mut doc = inputs[0].clone();
    let mut budget = doc.decode_budget();
    let page_ids = collect_page_ids(doc.inner(), ctx)?;
    let first_page_id = page_ids[0];

    let inner = doc.inner_mut()?;
    materialize_inferred_rotations(inner, &page_ids, ctx)?;

    let mut page_forms: Vec<(ObjectId, FormXObject, f64, f64)> = Vec::with_capacity(page_ids.len());
    let mut total_width = 0.0_f64;
    let mut total_height = 0.0_f64;

    for &page_id in &page_ids {
        ctx.check()?;
        charge_page_contents(inner, page_id, &mut budget, ctx)?;
        let form = xobject::page_to_form_xobject_in_place(inner, page_id, ctx)?;
        let (visual_w, visual_h) = if matches!(form.rotation.rem_euclid(360), 90 | 270) {
            (form.bbox.height(), form.bbox.width())
        } else {
            (form.bbox.width(), form.bbox.height())
        };
        total_width = total_width.max(visual_w);
        total_height += visual_h;
        page_forms.push((page_id, form, visual_w, visual_h));
    }

    if !total_width.is_finite()
        || !total_height.is_finite()
        || total_width <= 0.0
        || total_height <= 0.0
        || total_width > MAX_PAPER_SIDE_PT
        || total_height > MAX_PAPER_SIDE_PT
    {
        return Err(PdfError::InvalidParams(format!(
            "combined single-page dimensions ({total_width} x {total_height} pt) exceed the maximum allowed page size ({MAX_PAPER_SIDE_PT} pt)"
        )));
    }

    let mut cursor_top = total_height;
    let mut combined_content = Vec::new();
    let mut xobjects_dict = Dictionary::new();
    let mut combined_annots = Vec::new();

    for (idx, (page_id, form, visual_w, visual_h)) in page_forms.into_iter().enumerate() {
        ctx.check()?;
        let slot_bottom = cursor_top - visual_h;
        let slot_top = cursor_top;
        cursor_top = slot_bottom;

        let slot_left = (total_width - visual_w) * 0.5;
        let slot_right = slot_left + visual_w;
        let target_rect = PdfRect::new(slot_left, slot_bottom, slot_right, slot_top);

        let matrix =
            xobject::placement_matrix(form.bbox, form.rotation, target_rect, ScaleFit::Contain);
        let xobj_name = format!("Fm{idx}");
        combined_content
            .extend_from_slice(&xobject::render_place_xobject(xobj_name.as_bytes(), matrix));
        xobjects_dict.set(xobj_name, Object::Reference(form.id));

        let mut page_annots =
            transform_page_annotations(inner, page_id, matrix, Some(first_page_id), ctx)?;
        combined_annots.append(&mut page_annots);
    }

    let mut content_stream = Stream::new(dictionary! {}, combined_content);
    let _ = content_stream.compress();
    let content_id = inner.add_object(Object::Stream(content_stream));

    let root_pages_id = resolve_root_pages_id(inner)?;
    let removed_pages: HashSet<ObjectId> = page_ids[1..].iter().copied().collect();

    for &removed_id in &removed_pages {
        inner.objects.remove(&removed_id);
    }

    // Rewrite references to merged pages `2..=N` (in outlines, destinations, etc.) to `first_page_id`.
    if !removed_pages.is_empty() {
        for (&obj_id, obj) in &mut inner.objects {
            if obj_id == root_pages_id {
                continue;
            }
            rewrite_merged_page_refs(obj, &removed_pages, first_page_id);
        }
    }

    if let Ok(pages_dict) = inner
        .get_object_mut(root_pages_id)
        .and_then(Object::as_dict_mut)
    {
        pages_dict.set("Type", Object::Name(b"Pages".to_vec()));
        pages_dict.set(
            "Kids",
            Object::Array(vec![Object::Reference(first_page_id)]),
        );
        pages_dict.set("Count", Object::Integer(1));
        pages_dict.remove(b"Rotate");
        pages_dict.remove(b"CropBox");
        pages_dict.remove(b"MediaBox");
        pages_dict.remove(b"Resources");
    }

    let full_rect = PdfRect::from_size(total_width, total_height);
    if let Ok(page_dict) = inner
        .get_object_mut(first_page_id)
        .and_then(Object::as_dict_mut)
    {
        page_dict.set("Type", Object::Name(b"Page".to_vec()));
        page_dict.set("Parent", Object::Reference(root_pages_id));
        page_dict.set("MediaBox", Object::Array(full_rect.to_pdf_array()));
        page_dict.remove(b"CropBox");
        page_dict.remove(b"TrimBox");
        page_dict.remove(b"BleedBox");
        page_dict.remove(b"ArtBox");
        page_dict.remove(b"Rotate");
        page_dict.set(
            "Resources",
            Object::Dictionary(dictionary! {
                "XObject" => Object::Dictionary(xobjects_dict),
            }),
        );
        page_dict.set("Contents", Object::Reference(content_id));
        if combined_annots.is_empty() {
            page_dict.remove(b"Annots");
        } else {
            page_dict.set("Annots", Object::Array(combined_annots));
        }
    }

    // Strip any intermediate `/Pages` nodes now that `root_pages_id` directly parents `first_page_id`.
    if let Ok(catalog_id) = inner.trailer.get(b"Root").and_then(Object::as_reference)
        && let Ok(catalog_dict) = inner
            .get_object_mut(catalog_id)
            .and_then(Object::as_dict_mut)
    {
        catalog_dict.remove(b"PageLabels");
    }

    Ok(vec![OutputDoc::unnamed_pdf(doc)])
}

fn resolve_root_pages_id(doc: &Document) -> Result<ObjectId, PdfError> {
    let catalog = doc.catalog().map_err(|_| PdfError::Malformed)?;
    catalog
        .get(b"Pages")
        .and_then(Object::as_reference)
        .map_err(|_| PdfError::Malformed)
}

fn rewrite_merged_page_refs(
    object: &mut Object,
    removed_pages: &HashSet<ObjectId>,
    first_page_id: ObjectId,
) {
    match object {
        Object::Reference(id) => {
            if removed_pages.contains(id) {
                *id = first_page_id;
            }
        }
        Object::Array(items) => {
            for item in items {
                rewrite_merged_page_refs(item, removed_pages, first_page_id);
            }
        }
        Object::Dictionary(dict) => {
            for (_, val) in dict.iter_mut() {
                rewrite_merged_page_refs(val, removed_pages, first_page_id);
            }
        }
        Object::Stream(stream) => {
            for (_, val) in stream.dict.iter_mut() {
                rewrite_merged_page_refs(val, removed_pages, first_page_id);
            }
        }
        _ => {}
    }
}
