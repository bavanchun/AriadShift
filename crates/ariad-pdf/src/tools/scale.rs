//! Implementation of the `pdf.scale` tool.
//!
//! Wraps selected pages in Form XObjects and places them onto the target paper size using a 2D
//! affine transformation matrix (`contain`, `cover`, or `stretch`).

use ariad_core::tools::{page_range::DuplicatePolicy, params::scale::ScaleParams};
use lopdf::{Document, Object, ObjectId, Stream, dictionary};

use crate::{
    document::{DecodeBudget, PdfDoc, ToolCtx},
    error::PdfError,
    paper::resolve_paper_size,
    tools::{
        OutputDoc,
        crop::ancestor_has_key,
        rotate::{collect_page_ids, materialize_inferred_rotations},
    },
    xobject::{self, Matrix, PdfRect},
};

/// Scales selected pages of a PDF document to a target paper size using Form XObjects.
pub fn run(
    inputs: &[PdfDoc],
    params: &ScaleParams,
    ctx: &ToolCtx,
) -> Result<Vec<OutputDoc>, PdfError> {
    ctx.check()?;
    if inputs.len() != 1 {
        return Err(PdfError::InvalidParams(format!(
            "pdf.scale requires exactly 1 input document, got {}",
            inputs.len()
        )));
    }

    let target_dims = resolve_paper_size(&params.to)?;
    let target_rect = PdfRect::from_size(target_dims.width_pt, target_dims.height_pt);

    let mut doc = inputs[0].clone();
    let mut budget = doc.decode_budget();
    let page_ids = collect_page_ids(doc.inner(), ctx)?;
    let page_count = u32::try_from(page_ids.len()).unwrap_or(u32::MAX);

    let selected_pages = params
        .pages
        .expand(page_count, DuplicatePolicy::Reject)
        .map_err(|e| PdfError::InvalidParams(e.to_string()))?;

    let inner = doc.inner_mut()?;
    materialize_inferred_rotations(inner, &page_ids, ctx)?;

    for page_num in selected_pages {
        ctx.check()?;
        let page_id = page_ids[(page_num - 1) as usize];
        charge_page_contents(inner, page_id, &mut budget, ctx)?;

        let form = xobject::page_to_form_xobject_in_place(inner, page_id, ctx)?;
        let matrix = xobject::placement_matrix(form.bbox, form.rotation, target_rect, params.fit);

        let content_bytes = xobject::render_place_xobject(b"Fm0", matrix);
        let mut content_stream = Stream::new(dictionary! {}, content_bytes);
        let _ = content_stream.compress();
        let content_id = inner.add_object(Object::Stream(content_stream));

        transform_page_annotations(inner, page_id, matrix, None, ctx)?;
        apply_transformed_page_state(inner, page_id, target_rect, form.id, content_id);
    }

    Ok(vec![OutputDoc::unnamed_pdf(doc)])
}

/// Decodes and charges every content stream referenced by `page_id` against `budget`.
pub(crate) fn charge_page_contents(
    doc: &Document,
    page_id: ObjectId,
    budget: &mut DecodeBudget,
    ctx: &ToolCtx,
) -> Result<(), PdfError> {
    ctx.check()?;
    let Ok(page_dict) = doc.get_dictionary(page_id) else {
        return Ok(());
    };
    let Ok(contents_obj) = page_dict.get(b"Contents") else {
        return Ok(());
    };

    let mut stream_ids = Vec::new();
    match contents_obj {
        Object::Reference(id) => match doc.get_object(*id) {
            Ok(Object::Array(arr)) => {
                for item in arr {
                    if let Ok(sid) = item.as_reference() {
                        stream_ids.push(sid);
                    }
                }
            }
            _ => stream_ids.push(*id),
        },
        Object::Array(arr) => {
            for item in arr {
                if let Ok(sid) = item.as_reference() {
                    stream_ids.push(sid);
                }
            }
        }
        Object::Stream(stream) => {
            budget.decode(stream)?;
        }
        _ => {}
    }

    for sid in stream_ids {
        ctx.check()?;
        if let Ok(Object::Stream(stream)) = doc.get_object(sid) {
            budget.decode(stream)?;
        }
    }
    Ok(())
}

/// Applies the new `target_rect`, Form XObject resource `/Fm0`, and placement `content_id` to
/// `page_id`, resetting `/Rotate` to `0`.
pub(crate) fn apply_transformed_page_state(
    doc: &mut Document,
    page_id: ObjectId,
    target_rect: PdfRect,
    form_id: ObjectId,
    content_id: ObjectId,
) {
    let ancestor_has_crop = ancestor_has_key(doc, page_id, b"CropBox");
    let ancestor_has_rotate = ancestor_has_key(doc, page_id, b"Rotate");
    let media_arr = Object::Array(target_rect.to_pdf_array());
    let resources_dict = dictionary! {
        "XObject" => dictionary! {
            "Fm0" => Object::Reference(form_id),
        },
    };

    if let Ok(page_dict) = doc.get_object_mut(page_id).and_then(Object::as_dict_mut) {
        page_dict.set("MediaBox", media_arr.clone());
        if ancestor_has_crop {
            page_dict.set("CropBox", media_arr);
        } else {
            page_dict.remove(b"CropBox");
        }
        page_dict.remove(b"TrimBox");
        page_dict.remove(b"BleedBox");
        page_dict.remove(b"ArtBox");
        if ancestor_has_rotate {
            page_dict.set("Rotate", Object::Integer(0));
        } else {
            page_dict.remove(b"Rotate");
        }
        page_dict.set("Resources", Object::Dictionary(resources_dict));
        page_dict.set("Contents", Object::Reference(content_id));
    }
}

/// Transforms `/Rect` (and optionally updates `/P` to `new_parent_page`) for every annotation on
/// `page_id` and returns the resulting annotation entries.
pub(crate) fn transform_page_annotations(
    doc: &mut Document,
    page_id: ObjectId,
    matrix: Matrix,
    new_parent_page: Option<ObjectId>,
    ctx: &ToolCtx,
) -> Result<Vec<Object>, PdfError> {
    ctx.check()?;
    let annots_obj = doc
        .get_dictionary(page_id)
        .ok()
        .and_then(|d| d.get(b"Annots").ok())
        .cloned();
    let Some(annots_obj) = annots_obj else {
        return Ok(Vec::new());
    };

    let items: Vec<Object> = match annots_obj {
        Object::Array(arr) => arr,
        Object::Reference(id) => doc
            .get_object(id)
            .ok()
            .and_then(|o| o.as_array().ok())
            .cloned()
            .unwrap_or_default(),
        _ => Vec::new(),
    };

    let mut out = Vec::with_capacity(items.len());
    for item in items {
        ctx.check()?;
        match item {
            Object::Reference(annot_id) => {
                let rect = doc
                    .get_dictionary(annot_id)
                    .ok()
                    .and_then(|d| d.get(b"Rect").ok())
                    .and_then(|r| parse_annot_rect(doc, r));
                if let Ok(annot_dict) = doc.get_object_mut(annot_id).and_then(Object::as_dict_mut) {
                    if let Some(r) = rect {
                        let mapped = matrix.transform_rect_bbox(r);
                        annot_dict.set("Rect", Object::Array(mapped.to_pdf_array()));
                    }
                    if let Some(pid) = new_parent_page
                        && annot_dict.has(b"P")
                    {
                        annot_dict.set("P", Object::Reference(pid));
                    }
                }
                out.push(Object::Reference(annot_id));
            }
            Object::Dictionary(mut annot_dict) => {
                if let Ok(r_obj) = annot_dict.get(b"Rect")
                    && let Some(r) = parse_annot_rect(doc, r_obj)
                {
                    let mapped = matrix.transform_rect_bbox(r);
                    annot_dict.set("Rect", Object::Array(mapped.to_pdf_array()));
                }
                if let Some(pid) = new_parent_page
                    && annot_dict.has(b"P")
                {
                    annot_dict.set("P", Object::Reference(pid));
                }
                out.push(Object::Dictionary(annot_dict));
            }
            other => out.push(other),
        }
    }

    if let Ok(page_dict) = doc.get_object_mut(page_id).and_then(Object::as_dict_mut) {
        if out.is_empty() {
            page_dict.remove(b"Annots");
        } else {
            page_dict.set("Annots", Object::Array(out.clone()));
        }
    }

    Ok(out)
}

fn parse_annot_rect(doc: &Document, obj: &Object) -> Option<PdfRect> {
    let arr = match obj {
        Object::Array(a) => a,
        Object::Reference(id) => doc.get_object(*id).ok()?.as_array().ok()?,
        _ => return None,
    };
    if arr.len() < 4 {
        return None;
    }
    let n = |o: &Object| -> Option<f64> {
        match o {
            Object::Integer(i) => Some(*i as f64),
            Object::Real(r) => Some(f64::from(*r)),
            Object::Reference(id) => match doc.get_object(*id).ok()? {
                Object::Integer(i) => Some(*i as f64),
                Object::Real(r) => Some(f64::from(*r)),
                _ => None,
            },
            _ => None,
        }
    };
    Some(PdfRect::new(
        n(&arr[0])?,
        n(&arr[1])?,
        n(&arr[2])?,
        n(&arr[3])?,
    ))
}
