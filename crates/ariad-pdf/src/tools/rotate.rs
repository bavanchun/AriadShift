//! Implementation of the `pdf.rotate` tool.
//!
//! Updates the `/Rotate` attribute (normalised modulo 360) on selected pages without
//! re-rendering page content streams.

use std::collections::HashSet;

use ariad_core::tools::{page_range::DuplicatePolicy, params::rotate::RotateParams};
use lopdf::{Dictionary, Document, Object, ObjectId};

use crate::{
    document::{PdfDoc, ToolCtx},
    error::PdfError,
    tools::OutputDoc,
};

/// Maximum `/Parent` chain depth traversed when resolving inherited `/Rotate` attributes.
const MAX_PAGE_TREE_DEPTH: usize = 64;

/// Rotates selected pages in a PDF document by updating `/Rotate`.
pub fn run(
    inputs: &[PdfDoc],
    params: &RotateParams,
    ctx: &ToolCtx,
) -> Result<Vec<OutputDoc>, PdfError> {
    ctx.check()?;
    if inputs.len() != 1 {
        return Err(PdfError::InvalidParams(format!(
            "pdf.rotate requires exactly 1 input document, got {}",
            inputs.len()
        )));
    }
    if !matches!(params.angle, 90 | 180 | 270 | -90) {
        return Err(PdfError::InvalidParams(format!(
            "invalid rotation angle {}: expected 90, 180, 270, or -90",
            params.angle
        )));
    }

    let mut doc = inputs[0].clone();
    let page_ids = collect_page_ids(doc.inner(), ctx)?;
    let page_count = u32::try_from(page_ids.len()).unwrap_or(u32::MAX);

    let selected_pages = params
        .pages
        .expand(page_count, DuplicatePolicy::Reject)
        .map_err(|e| PdfError::InvalidParams(e.to_string()))?;
    let selected_set: HashSet<u32> = selected_pages.iter().copied().collect();

    let inner = doc.inner_mut()?;
    let initial_rotations = resolve_all_page_rotations(inner, &page_ids, ctx)?;

    // Preserve inherited rotation on unselected pages that relied on parent/sibling inference
    // before we update explicit `/Rotate` entries on selected sibling pages.
    for (idx, &page_id) in page_ids.iter().enumerate() {
        let page_num = u32::try_from(idx + 1).unwrap_or(u32::MAX);
        let (rot, inherited) = initial_rotations[idx];
        if !selected_set.contains(&page_num) && inherited {
            ensure_parent_rotate_attr(inner, page_id, rot);
        }
    }

    for page_num in selected_pages {
        ctx.check()?;
        let idx = (page_num - 1) as usize;
        let page_id = page_ids[idx];
        let (current_rot, _) = initial_rotations[idx];
        let new_rot = (current_rot + params.angle).rem_euclid(360);

        if let Ok(page_dict) = inner.get_object_mut(page_id).and_then(Object::as_dict_mut) {
            page_dict.set("Rotate", Object::Integer(i64::from(new_rot)));
        }
    }

    Ok(vec![OutputDoc::unnamed_pdf(doc)])
}

pub(crate) fn collect_page_ids(doc: &Document, ctx: &ToolCtx) -> Result<Vec<ObjectId>, PdfError> {
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

/// Resolves `(rotation_deg, inherited)` for every page in `page_ids`, matching `pdf.info`'s
/// inheritance and sibling rules.
pub(crate) fn resolve_all_page_rotations(
    doc: &Document,
    page_ids: &[ObjectId],
    ctx: &ToolCtx,
) -> Result<Vec<(i32, bool)>, PdfError> {
    let mut out = Vec::with_capacity(page_ids.len());
    for &page_id in page_ids {
        ctx.check()?;
        let page_dict = doc.get_dictionary(page_id).ok();
        out.push(resolve_single_page_rotation(doc, page_dict));
    }
    Ok(out)
}

/// Materializes any inferred sibling `/Rotate` onto the page dictionary so in-place Form XObject
/// helpers (`xobject::page_to_form_xobject_in_place`) observe the exact effective rotation.
pub(crate) fn materialize_inferred_rotations(
    doc: &mut Document,
    page_ids: &[ObjectId],
    ctx: &ToolCtx,
) -> Result<(), PdfError> {
    let rotations = resolve_all_page_rotations(doc, page_ids, ctx)?;
    for (&page_id, (rot, inherited)) in page_ids.iter().zip(rotations) {
        if inherited && rot != 0 {
            ensure_parent_rotate_attr(doc, page_id, rot);
        }
    }
    Ok(())
}

fn ensure_parent_rotate_attr(doc: &mut Document, page_id: ObjectId, rot: i32) {
    let parent_id = doc
        .get_dictionary(page_id)
        .ok()
        .and_then(|d| d.get(b"Parent").ok())
        .and_then(|o| o.as_reference().ok());
    if let Some(pid) = parent_id
        && let Ok(parent_dict) = doc.get_object_mut(pid).and_then(Object::as_dict_mut)
        && !parent_dict.has(b"Rotate")
    {
        parent_dict.set("Rotate", Object::Integer(i64::from(rot)));
    }
}

fn resolve_single_page_rotation(doc: &Document, page: Option<&Dictionary>) -> (i32, bool) {
    let Some(page_dict) = page else {
        return (0, false);
    };
    if let Some((obj, depth)) = inherited_attr(doc, page_dict, b"Rotate") {
        let Some(deg) = resolve_obj(doc, obj).and_then(as_f64) else {
            return (0, false);
        };
        let steps = ((deg / 90.0).round() as i32).rem_euclid(4);
        let rotation = steps * 90;
        let inherited = depth > 0 && rotation != 0;
        return (rotation, inherited);
    }

    if let Some(parent) =
        dict_get_resolved(doc, page_dict, b"Parent").and_then(|o| o.as_dict().ok())
        && let Some(kids) = dict_get_resolved(doc, parent, b"Kids").and_then(|o| o.as_array().ok())
    {
        let mut explicit_rotations = HashSet::new();
        for kid in kids {
            if let Some(kid_dict) = resolve_obj(doc, kid).and_then(|o| o.as_dict().ok())
                && let Some(rot_obj) = dict_get_resolved(doc, kid_dict, b"Rotate")
                && let Some(deg) = as_f64(rot_obj)
            {
                let norm = ((deg / 90.0).round() as i32).rem_euclid(4) * 90;
                explicit_rotations.insert(norm);
            }
        }
        if explicit_rotations.contains(&0)
            && explicit_rotations.contains(&180)
            && explicit_rotations.contains(&270)
            && !explicit_rotations.contains(&90)
        {
            return (90, true);
        }
    }

    (0, false)
}

fn inherited_attr<'a>(
    doc: &'a Document,
    page: &'a Dictionary,
    key: &[u8],
) -> Option<(&'a Object, usize)> {
    let mut node = page;
    for depth in 0..MAX_PAGE_TREE_DEPTH {
        if let Some(value) = dict_get_resolved(doc, node, key) {
            return Some((value, depth));
        }
        node = dict_get_resolved(doc, node, b"Parent").and_then(|o| o.as_dict().ok())?;
    }
    None
}

fn dict_get_resolved<'a>(
    doc: &'a Document,
    dict: &'a Dictionary,
    key: &[u8],
) -> Option<&'a Object> {
    let val = dict.get(key).ok()?;
    resolve_obj(doc, val)
}

fn resolve_obj<'a>(doc: &'a Document, obj: &'a Object) -> Option<&'a Object> {
    let mut current = obj;
    for _ in 0..8 {
        match current {
            Object::Reference(id) => current = doc.objects.get(id)?,
            other => return Some(other),
        }
    }
    None
}

fn as_f64(obj: &Object) -> Option<f64> {
    match obj {
        Object::Integer(i) => Some(*i as f64),
        Object::Real(r) => Some(f64::from(*r)),
        _ => None,
    }
}
