//! Implementation of the `pdf.reorder` tool and shared page-sequence manipulation helpers.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use ariad_core::{
    reader::pdf::ReadError,
    tools::{
        page_range::DuplicatePolicy,
        params::reorder::{ReorderParams, ReorderPreset, ReorderSpec},
    },
};
use lopdf::{Dictionary, Document, Object, ObjectId, dictionary};

pub(crate) use super::page_set::MAX_OUTPUT_PAGES;
use crate::{
    document::{PdfDoc, ToolCtx},
    error::PdfError,
    tools::{
        OutputDoc,
        page_set::{
            self, build_extracted_doc, collect_named_destinations, default_xyz_tail,
            doc_page_count, expand_page_range_bounded,
        },
    },
    xobject,
};

/// Maximum recursion depth when walking `/Outlines` or `/AcroForm` hierarchies.
const MAX_TREE_DEPTH: usize = 64;

/// Reorders pages in a PDF document using a preset or explicit page range sequence.
pub fn run(
    inputs: &[PdfDoc],
    params: &ReorderParams,
    ctx: &ToolCtx,
) -> Result<Vec<OutputDoc>, PdfError> {
    ctx.check()?;
    if inputs.len() != 1 {
        return Err(PdfError::InvalidParams(format!(
            "pdf.reorder requires exactly 1 input document, got {}",
            inputs.len()
        )));
    }

    let source = &inputs[0];
    if source.is_encrypted() {
        return Err(PdfError::Encrypted {
            needs_password: false,
        });
    }
    let page_count = doc_page_count(source);
    if page_count == 0 {
        return Err(PdfError::Malformed);
    }

    let target_seq = expand_reorder_sequence(&params.order, page_count)?;
    if target_seq.is_empty() {
        return Err(PdfError::InvalidParams(
            "reorder target page sequence must not be empty".to_owned(),
        ));
    }
    if target_seq.len() as u64 > MAX_OUTPUT_PAGES {
        return Err(PdfError::LimitExceeded(ReadError::TooManyPages {
            actual: target_seq.len() as u64,
            limit: MAX_OUTPUT_PAGES as u32,
        }));
    }

    let out_doc = build_extracted_doc(source, &target_seq, ctx)?;
    Ok(vec![OutputDoc::unnamed_pdf(out_doc)])
}

/// Expands a [`ReorderSpec`] against `page_count` into a 1-based page number sequence.
fn expand_reorder_sequence(spec: &ReorderSpec, page_count: u32) -> Result<Vec<u32>, PdfError> {
    match spec {
        ReorderSpec::Preset(ReorderPreset::Reverse) => {
            if u64::from(page_count) > MAX_OUTPUT_PAGES {
                return Err(PdfError::LimitExceeded(ReadError::TooManyPages {
                    actual: u64::from(page_count),
                    limit: MAX_OUTPUT_PAGES as u32,
                }));
            }
            Ok((1..=page_count).rev().collect())
        }
        ReorderSpec::Preset(ReorderPreset::DuplexInterleave) => {
            if u64::from(page_count) > MAX_OUTPUT_PAGES {
                return Err(PdfError::LimitExceeded(ReadError::TooManyPages {
                    actual: u64::from(page_count),
                    limit: MAX_OUTPUT_PAGES as u32,
                }));
            }
            let mut seq = Vec::with_capacity(page_count as usize);
            let mut front = 1_u32;
            let mut back = page_count;
            while front <= back {
                seq.push(front);
                if front < back {
                    seq.push(back);
                }
                front += 1;
                back = back.saturating_sub(1);
            }
            Ok(seq)
        }
        ReorderSpec::Preset(ReorderPreset::OddThenEven) => {
            if u64::from(page_count) > MAX_OUTPUT_PAGES {
                return Err(PdfError::LimitExceeded(ReadError::TooManyPages {
                    actual: u64::from(page_count),
                    limit: MAX_OUTPUT_PAGES as u32,
                }));
            }
            let mut seq = Vec::with_capacity(page_count as usize);
            let mut odd = 1_u32;
            while odd <= page_count {
                seq.push(odd);
                match odd.checked_add(2) {
                    Some(next) => odd = next,
                    None => break,
                }
            }
            let mut even = 2_u32;
            while even <= page_count {
                seq.push(even);
                match even.checked_add(2) {
                    Some(next) => even = next,
                    None => break,
                }
            }
            Ok(seq)
        }
        ReorderSpec::Pages(range) => {
            expand_page_range_bounded(range, page_count, DuplicatePolicy::KeepInOrder)
        }
    }
}

/// Collects all unique page [`ObjectId`]s in 1-based page order.
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

/// Returns the `/Root` catalog [`ObjectId`] from `doc.trailer`.
pub(crate) fn catalog_id(doc: &Document) -> Result<ObjectId, PdfError> {
    doc.trailer
        .get(b"Root")
        .and_then(Object::as_reference)
        .map_err(|_| PdfError::Malformed)
}

/// Returns the root `/Pages` [`ObjectId`] referenced by the document `/Catalog`.
pub(crate) fn pages_root_id(doc: &mut Document) -> Result<ObjectId, PdfError> {
    let cat_id = catalog_id(doc)?;
    if let Ok(cat_dict) = doc.get_dictionary(cat_id)
        && let Ok(pages_id) = cat_dict.get(b"Pages").and_then(Object::as_reference)
    {
        return Ok(pages_id);
    }
    let new_pages_id = doc.new_object_id();
    if let Ok(cat_dict) = doc.get_dictionary_mut(cat_id) {
        cat_dict.set("Pages", Object::Reference(new_pages_id));
    }
    Ok(new_pages_id)
}

/// Replaces the root `/Pages` dictionary with a flat `Kids` array and matching `Count`.
pub(crate) fn rebuild_pages_root(doc: &mut Document, pages_root_id: ObjectId, kids: &[ObjectId]) {
    let kids_arr: Vec<Object> = kids.iter().copied().map(Object::Reference).collect();
    let count = i64::try_from(kids.len()).unwrap_or(0);
    doc.objects.insert(
        pages_root_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => kids_arr,
            "Count" => count,
        }),
    );
}

/// Sets the `/Parent` entry on `page_id` to `pages_root_id`.
pub(crate) fn set_page_parent(doc: &mut Document, page_id: ObjectId, pages_root_id: ObjectId) {
    if let Ok(page_dict) = doc.get_dictionary_mut(page_id) {
        page_dict.set("Parent", Object::Reference(pages_root_id));
    }
}

/// Resolves the effective clockwise `/Rotate` (`0`, `90`, `180`, `270`) of `page_id`,
/// including values inherited from `/Pages` ancestors or sibling inference.
#[must_use]
pub(crate) fn effective_page_rotation(doc: &Document, page_id: ObjectId) -> i32 {
    let direct_or_ancestor = xobject::resolve_page_rotation(doc, page_id);
    if direct_or_ancestor != 0 {
        return direct_or_ancestor;
    }
    let Ok(page_dict) = doc.get_dictionary(page_id) else {
        return 0;
    };
    if page_dict.has(b"Rotate") {
        return 0;
    }

    // Match `info.rs` fallback when a `/Pages` parent originally had `/Rotate 90` and siblings
    // explicitly carry `{0, 180, 270}` without `90`.
    if let Ok(parent_id) = page_dict.get(b"Parent").and_then(Object::as_reference)
        && let Ok(parent_dict) = doc.get_dictionary(parent_id)
        && let Ok(kids) = parent_dict.get(b"Kids").and_then(Object::as_array)
    {
        let mut explicit_rotations = HashSet::new();
        for kid in kids {
            if let Ok(kid_id) = kid.as_reference()
                && let Ok(kid_dict) = doc.get_dictionary(kid_id)
                && let Ok(rot_obj) = kid_dict.get(b"Rotate")
                && let Ok(deg) = rot_obj.as_i64()
            {
                let norm = (deg as i32).rem_euclid(360);
                explicit_rotations.insert(norm);
            }
        }
        if explicit_rotations.contains(&0)
            && explicit_rotations.contains(&180)
            && explicit_rotations.contains(&270)
            && !explicit_rotations.contains(&90)
        {
            return 90;
        }
    }

    0
}

/// Materializes inherited `/Resources`, `/MediaBox`, `/CropBox`, `/BleedBox`, `/TrimBox`,
/// `/ArtBox`, and `/Rotate` attributes directly onto `page_id` so the `/Page` is self-contained
/// when placed into a new `/Pages` root.
pub(crate) fn materialize_page_attributes(
    doc: &mut Document,
    page_id: ObjectId,
    ctx: &ToolCtx,
) -> Result<(), PdfError> {
    ctx.check()?;
    let media_box = xobject::resolve_page_media_box(doc, page_id);
    let crop_box = xobject::resolve_page_box(doc, page_id, b"CropBox");
    let bleed_box = xobject::resolve_page_box(doc, page_id, b"BleedBox");
    let trim_box = xobject::resolve_page_box(doc, page_id, b"TrimBox");
    let art_box = xobject::resolve_page_box(doc, page_id, b"ArtBox");
    let rotation = effective_page_rotation(doc, page_id);
    let merged_resources = xobject::resolve_page_resources(doc, page_id);

    let Ok(page_dict) = doc.get_dictionary_mut(page_id) else {
        return Err(PdfError::Malformed);
    };
    page_dict.set("Type", Object::Name(b"Page".to_vec()));
    if !page_dict.has(b"MediaBox") {
        page_dict.set("MediaBox", Object::Array(media_box.to_pdf_array()));
    }
    if !page_dict.has(b"CropBox")
        && let Some(rect) = crop_box
    {
        page_dict.set("CropBox", Object::Array(rect.to_pdf_array()));
    }
    if !page_dict.has(b"BleedBox")
        && let Some(rect) = bleed_box
    {
        page_dict.set("BleedBox", Object::Array(rect.to_pdf_array()));
    }
    if !page_dict.has(b"TrimBox")
        && let Some(rect) = trim_box
    {
        page_dict.set("TrimBox", Object::Array(rect.to_pdf_array()));
    }
    if !page_dict.has(b"ArtBox")
        && let Some(rect) = art_box
    {
        page_dict.set("ArtBox", Object::Array(rect.to_pdf_array()));
    }
    if rotation != 0 {
        page_dict.set("Rotate", Object::Integer(i64::from(rotation)));
    }
    page_dict.set("Resources", Object::Dictionary(merged_resources));
    Ok(())
}

/// Updates annotations on `page_id` so `/P` references `page_id` and any internal page destinations
/// referencing dropped pages are cleaned up.
pub(crate) fn update_page_annotations(
    doc: &mut Document,
    page_id: ObjectId,
    orig_page_set: &HashSet<ObjectId>,
    old_to_new_page: &HashMap<ObjectId, ObjectId>,
    ctx: &ToolCtx,
) -> Result<(), PdfError> {
    ctx.check()?;
    let Ok(page_dict) = doc.get_dictionary(page_id) else {
        return Ok(());
    };
    let Ok(annots_obj) = page_dict.get(b"Annots").cloned() else {
        return Ok(());
    };
    let annot_items = match annots_obj {
        Object::Reference(id) => doc.get_object(id).and_then(Object::as_array).cloned().ok(),
        Object::Array(arr) => Some(arr),
        _ => None,
    };
    let Some(items) = annot_items else {
        return Ok(());
    };

    for item in items {
        ctx.check()?;
        let Ok(annot_id) = item.as_reference() else {
            continue;
        };
        let mut action_id_to_update = None;
        let mut drop_action = false;

        if let Ok(annot_dict) = doc.get_dictionary_mut(annot_id) {
            if annot_dict.has(b"P") {
                annot_dict.set("P", Object::Reference(page_id));
            }
            if let Ok(dest) = annot_dict.get(b"Dest").cloned() {
                match remap_dest_object(&dest, orig_page_set, old_to_new_page) {
                    DestRemap::Updated(new_dest) => {
                        annot_dict.set("Dest", new_dest);
                    }
                    DestRemap::Dropped => {
                        annot_dict.remove(b"Dest");
                    }
                    DestRemap::Unchanged => {}
                }
            }
            if let Ok(action_obj) = annot_dict.get(b"A").cloned() {
                match action_obj {
                    Object::Reference(aid) => action_id_to_update = Some(aid),
                    Object::Dictionary(mut adict) => {
                        if remap_goto_action_dict(&mut adict, orig_page_set, old_to_new_page) {
                            annot_dict.set("A", Object::Dictionary(adict));
                        } else {
                            annot_dict.remove(b"A");
                        }
                    }
                    _ => {}
                }
            }
        }

        if let Some(aid) = action_id_to_update
            && let Ok(adict) = doc.get_dictionary_mut(aid)
            && !remap_goto_action_dict(adict, orig_page_set, old_to_new_page)
        {
            drop_action = true;
        }
        if drop_action && let Ok(annot_dict) = doc.get_dictionary_mut(annot_id) {
            annot_dict.remove(b"A");
        }
    }
    Ok(())
}

enum DestRemap {
    Unchanged,
    Updated(Object),
    Dropped,
}

fn remap_dest_object(
    dest: &Object,
    orig_page_set: &HashSet<ObjectId>,
    old_to_new_page: &HashMap<ObjectId, ObjectId>,
) -> DestRemap {
    let Object::Array(arr) = dest else {
        return DestRemap::Unchanged;
    };
    let Some(Object::Reference(target_id)) = arr.first() else {
        return DestRemap::Unchanged;
    };
    if !orig_page_set.contains(target_id) {
        return DestRemap::Unchanged;
    };
    if let Some(&new_id) = old_to_new_page.get(target_id) {
        if new_id == *target_id {
            DestRemap::Unchanged
        } else {
            let mut updated = arr.clone();
            updated[0] = Object::Reference(new_id);
            DestRemap::Updated(Object::Array(updated))
        }
    } else {
        DestRemap::Dropped
    }
}

/// Returns `true` if the action should be kept, or `false` if it was a `/GoTo` action pointing to
/// a dropped page.
fn remap_goto_action_dict(
    adict: &mut Dictionary,
    orig_page_set: &HashSet<ObjectId>,
    old_to_new_page: &HashMap<ObjectId, ObjectId>,
) -> bool {
    let is_goto = adict
        .get(b"S")
        .and_then(Object::as_name)
        .is_ok_and(|name| name == b"GoTo");
    if !is_goto {
        return true;
    }
    let Ok(d_obj) = adict.get(b"D").cloned() else {
        return true;
    };
    match remap_dest_object(&d_obj, orig_page_set, old_to_new_page) {
        DestRemap::Unchanged => true,
        DestRemap::Updated(new_d) => {
            adict.set("D", new_d);
            true
        }
        DestRemap::Dropped => false,
    }
}

/// Prunes `/AcroForm` fields whose widget annotations belong exclusively to dropped pages.
pub(crate) fn prune_acroform_for_kept_pages(
    doc: &mut Document,
    kept_pages: &[ObjectId],
    ctx: &ToolCtx,
) -> Result<(), PdfError> {
    ctx.check()?;
    let kept_page_set: HashSet<ObjectId> = kept_pages.iter().copied().collect();
    let mut kept_annot_ids = HashSet::new();
    for &page_id in kept_pages {
        ctx.check()?;
        let Ok(page_dict) = doc.get_dictionary(page_id) else {
            continue;
        };
        let Ok(annots_obj) = page_dict.get(b"Annots") else {
            continue;
        };
        let arr_opt = match annots_obj {
            Object::Reference(id) => doc.get_object(*id).and_then(Object::as_array).ok(),
            Object::Array(arr) => Some(arr),
            _ => None,
        };
        if let Some(arr) = arr_opt {
            for item in arr {
                if let Ok(aid) = item.as_reference() {
                    kept_annot_ids.insert(aid);
                }
            }
        }
    }

    let cat_id = catalog_id(doc)?;
    let Ok(cat_dict) = doc.get_dictionary(cat_id) else {
        return Ok(());
    };
    let Ok(acro_obj) = cat_dict.get(b"AcroForm").cloned() else {
        return Ok(());
    };

    let (acro_id_opt, mut acro_dict) = match acro_obj {
        Object::Reference(id) => match doc.get_dictionary(id).cloned() {
            Ok(d) => (Some(id), d),
            Err(_) => return Ok(()),
        },
        Object::Dictionary(d) => (None, d),
        _ => return Ok(()),
    };

    let fields_arr = match acro_dict.get(b"Fields").cloned() {
        Ok(Object::Reference(fid)) => doc.get_object(fid).and_then(Object::as_array).cloned().ok(),
        Ok(Object::Array(arr)) => Some(arr),
        _ => None,
    };
    let Some(fields) = fields_arr else {
        return Ok(());
    };

    let mut visited = HashMap::new();
    let mut surviving_fields = Vec::new();
    let mut seen_fields = BTreeSet::new();
    for field_ref in fields {
        ctx.check()?;
        let Ok(field_id) = field_ref.as_reference() else {
            surviving_fields.push(field_ref);
            continue;
        };
        if seen_fields.insert(field_id)
            && retain_acroform_field(
                doc,
                field_id,
                &kept_page_set,
                &kept_annot_ids,
                &mut visited,
                0,
                ctx,
            )?
        {
            surviving_fields.push(Object::Reference(field_id));
        }
    }

    if surviving_fields.is_empty() {
        if let Ok(cat_mut) = doc.get_dictionary_mut(cat_id) {
            cat_mut.remove(b"AcroForm");
        }
    } else {
        acro_dict.set("Fields", Object::Array(surviving_fields));
        if let Some(acro_id) = acro_id_opt {
            doc.objects.insert(acro_id, Object::Dictionary(acro_dict));
        } else if let Ok(cat_mut) = doc.get_dictionary_mut(cat_id) {
            cat_mut.set("AcroForm", Object::Dictionary(acro_dict));
        }
    }
    Ok(())
}

fn retain_acroform_field(
    doc: &mut Document,
    field_id: ObjectId,
    kept_page_set: &HashSet<ObjectId>,
    kept_annot_ids: &HashSet<ObjectId>,
    visited: &mut HashMap<ObjectId, bool>,
    depth: usize,
    ctx: &ToolCtx,
) -> Result<bool, PdfError> {
    ctx.check()?;
    if depth >= MAX_TREE_DEPTH {
        return Ok(false);
    }
    if let Some(&cached) = visited.get(&field_id) {
        return Ok(cached);
    }
    visited.insert(field_id, false);

    let Ok(field_dict) = doc.get_dictionary(field_id).cloned() else {
        return Ok(false);
    };

    if let Ok(kids) = field_dict.get(b"Kids").and_then(Object::as_array).cloned() {
        let mut surviving_kids = Vec::new();
        let mut seen_kids = BTreeSet::new();
        for kid in kids {
            if let Ok(kid_id) = kid.as_reference()
                && seen_kids.insert(kid_id)
                && retain_acroform_field(
                    doc,
                    kid_id,
                    kept_page_set,
                    kept_annot_ids,
                    visited,
                    depth + 1,
                    ctx,
                )?
            {
                surviving_kids.push(Object::Reference(kid_id));
            }
        }
        if surviving_kids.is_empty() {
            return Ok(false);
        }
        if let Ok(dict_mut) = doc.get_dictionary_mut(field_id) {
            dict_mut.set("Kids", Object::Array(surviving_kids));
        }
        visited.insert(field_id, true);
        return Ok(true);
    }

    let kept = if kept_annot_ids.contains(&field_id) {
        true
    } else if let Ok(p_id) = field_dict.get(b"P").and_then(Object::as_reference) {
        kept_page_set.contains(&p_id)
    } else {
        let is_widget = field_dict
            .get(b"Subtype")
            .and_then(Object::as_name)
            .is_ok_and(|name| name == b"Widget");
        !is_widget
    };
    visited.insert(field_id, kept);
    Ok(kept)
}

/// In-memory representation of a bookmark entry in `/Outlines`.
#[derive(Clone, Debug)]
pub(crate) struct OutlineNode {
    /// Raw `/Title` object from the source outline item.
    pub title: Object,
    /// Resolved target page `ObjectId` in the source (or remapped) document.
    pub dest_page: Option<ObjectId>,
    /// Remaining elements of the destination array after the page reference (e.g. `[/XYZ, left, top, zoom]`).
    pub dest_tail: Vec<Object>,
    /// Nested child outline entries.
    pub children: Vec<OutlineNode>,
}

/// Extracts the `/Outlines` bookmark tree from `doc`, resolving page destinations against `orig_pages`.
pub(crate) fn extract_outline_tree(
    doc: &Document,
    orig_pages: &[ObjectId],
    ctx: &ToolCtx,
) -> Result<Vec<OutlineNode>, PdfError> {
    ctx.check()?;
    let Ok(catalog) = doc.catalog() else {
        return Ok(Vec::new());
    };
    let Some(outlines_dict) = resolve_dict(doc, catalog.get(b"Outlines").ok()) else {
        return Ok(Vec::new());
    };
    let Ok(first_id) = outlines_dict.get(b"First").and_then(Object::as_reference) else {
        return Ok(Vec::new());
    };

    let named_dests = collect_named_destinations(doc, ctx)?;
    let orig_page_set: HashSet<ObjectId> = orig_pages.iter().copied().collect();
    let mut visited = HashSet::new();
    walk_outline_siblings(
        doc,
        first_id,
        orig_pages,
        &orig_page_set,
        &named_dests,
        &mut visited,
        0,
        ctx,
    )
}

#[allow(clippy::too_many_arguments)]
fn walk_outline_siblings(
    doc: &Document,
    first_id: ObjectId,
    orig_pages: &[ObjectId],
    orig_page_set: &HashSet<ObjectId>,
    named_dests: &BTreeMap<Vec<u8>, Vec<Object>>,
    visited: &mut HashSet<ObjectId>,
    depth: usize,
    ctx: &ToolCtx,
) -> Result<Vec<OutlineNode>, PdfError> {
    if depth >= MAX_TREE_DEPTH {
        return Ok(Vec::new());
    }
    let mut nodes = Vec::new();
    let mut curr = Some(first_id);

    while let Some(item_id) = curr {
        ctx.check()?;
        if !visited.insert(item_id) {
            break;
        }
        let Ok(dict) = doc.get_dictionary(item_id) else {
            break;
        };
        let title = dict
            .get(b"Title")
            .ok()
            .and_then(|o| page_set::resolve_object(doc, o))
            .cloned()
            .unwrap_or_else(|| Object::string_literal("Bookmark"));
        let (dest_page, dest_tail) =
            resolve_outline_item_dest(doc, dict, orig_pages, orig_page_set, named_dests);

        let children = if let Ok(child_first) = dict.get(b"First").and_then(Object::as_reference) {
            walk_outline_siblings(
                doc,
                child_first,
                orig_pages,
                orig_page_set,
                named_dests,
                visited,
                depth + 1,
                ctx,
            )?
        } else {
            Vec::new()
        };

        nodes.push(OutlineNode {
            title,
            dest_page,
            dest_tail,
            children,
        });

        curr = dict.get(b"Next").ok().and_then(|o| o.as_reference().ok());
    }

    Ok(nodes)
}

fn resolve_outline_item_dest(
    doc: &Document,
    item_dict: &Dictionary,
    orig_pages: &[ObjectId],
    orig_page_set: &HashSet<ObjectId>,
    named_dests: &BTreeMap<Vec<u8>, Vec<Object>>,
) -> (Option<ObjectId>, Vec<Object>) {
    let raw_dest = item_dict.get(b"Dest").ok().or_else(|| {
        let action_dict = resolve_dict(doc, item_dict.get(b"A").ok())?;
        let subtype = resolve_obj(doc, action_dict.get(b"S").ok()?)?
            .as_name()
            .ok()?;
        if subtype == b"GoTo" {
            action_dict.get(b"D").ok()
        } else {
            None
        }
    });

    let Some(dest_obj) = raw_dest.and_then(|o| resolve_obj(doc, o)) else {
        return (None, default_xyz_tail());
    };

    if let Some(arr) = resolve_dest_array(doc, dest_obj, named_dests)
        && let Some(first) = arr.first()
    {
        let resolved_pid = if let Ok(pid) = first.as_reference()
            && orig_page_set.contains(&pid)
        {
            Some(pid)
        } else if let Some(Object::Integer(idx)) = resolve_obj(doc, first)
            && *idx >= 0
            && let Some(&pid) = orig_pages.get(*idx as usize)
        {
            Some(pid)
        } else {
            None
        };

        if let Some(pid) = resolved_pid {
            let tail: Vec<Object> = arr
                .iter()
                .skip(1)
                .filter_map(|o| {
                    let resolved = resolve_obj(doc, o)?;
                    (!matches!(resolved, Object::Reference(_))).then(|| resolved.clone())
                })
                .collect();
            let tail = if tail.is_empty() {
                default_xyz_tail()
            } else {
                tail
            };
            return (Some(pid), tail);
        }
    }

    (None, default_xyz_tail())
}

fn resolve_dest_array<'a>(
    doc: &'a Document,
    dest_obj: &'a Object,
    named_dests: &'a BTreeMap<Vec<u8>, Vec<Object>>,
) -> Option<&'a [Object]> {
    match dest_obj {
        Object::Array(arr) => Some(arr.as_slice()),
        Object::Dictionary(d) => resolve_obj(doc, d.get(b"D").ok()?)?
            .as_array()
            .ok()
            .map(Vec::as_slice),
        Object::Name(name) => named_dests.get(name.as_slice()).map(Vec::as_slice),
        Object::String(name, _) => named_dests.get(name.as_slice()).map(Vec::as_slice),
        _ => None,
    }
}

/// Filters `nodes` to keep only entries targeting pages in `page_map` (promoting surviving
/// children if a parent's page was dropped) and remaps `dest_page` through `page_map`.
pub(crate) fn filter_and_remap_outlines(
    nodes: Vec<OutlineNode>,
    page_map: &HashMap<ObjectId, ObjectId>,
    ctx: &ToolCtx,
) -> Result<Vec<OutlineNode>, PdfError> {
    let mut out = Vec::new();
    for node in nodes {
        ctx.check()?;
        let children = filter_and_remap_outlines(node.children, page_map, ctx)?;
        let mapped_dest = node.dest_page.and_then(|pid| page_map.get(&pid).copied());
        if let Some(new_pid) = mapped_dest {
            out.push(OutlineNode {
                title: node.title,
                dest_page: Some(new_pid),
                dest_tail: node.dest_tail,
                children,
            });
        } else if !children.is_empty() {
            // Promote surviving children when the parent's target page was removed.
            out.extend(children);
        }
    }
    Ok(out)
}

/// Writes `nodes` as the `/Outlines` hierarchy on `doc`, or removes `/Outlines` if `nodes` is empty.
pub(crate) fn write_outline_tree(
    doc: &mut Document,
    nodes: &[OutlineNode],
    ctx: &ToolCtx,
) -> Result<(), PdfError> {
    ctx.check()?;
    let cat_id = catalog_id(doc)?;
    if nodes.is_empty() {
        if let Ok(cat_dict) = doc.get_dictionary_mut(cat_id) {
            cat_dict.remove(b"Outlines");
        }
        return Ok(());
    }

    let outlines_root_id = doc.new_object_id();
    let (first_id, last_id, total_count) = build_outline_level(doc, outlines_root_id, nodes, ctx)?;
    doc.objects.insert(
        outlines_root_id,
        Object::Dictionary(dictionary! {
            "Type" => "Outlines",
            "First" => Object::Reference(first_id),
            "Last" => Object::Reference(last_id),
            "Count" => Object::Integer(total_count),
        }),
    );
    if let Ok(cat_dict) = doc.get_dictionary_mut(cat_id) {
        cat_dict.set("Outlines", Object::Reference(outlines_root_id));
    }
    Ok(())
}

fn build_outline_level(
    doc: &mut Document,
    parent_id: ObjectId,
    nodes: &[OutlineNode],
    ctx: &ToolCtx,
) -> Result<(ObjectId, ObjectId, i64), PdfError> {
    let item_ids: Vec<ObjectId> = (0..nodes.len()).map(|_| doc.new_object_id()).collect();
    let mut total_count = 0_i64;

    for (i, node) in nodes.iter().enumerate() {
        ctx.check()?;
        let item_id = item_ids[i];
        let mut dict = Dictionary::new();
        dict.set("Title", node.title.clone());
        dict.set("Parent", Object::Reference(parent_id));
        if i > 0 {
            dict.set("Prev", Object::Reference(item_ids[i - 1]));
        }
        if i + 1 < nodes.len() {
            dict.set("Next", Object::Reference(item_ids[i + 1]));
        }
        if let Some(pid) = node.dest_page {
            let mut dest_arr = Vec::with_capacity(1 + node.dest_tail.len());
            dest_arr.push(Object::Reference(pid));
            if node.dest_tail.is_empty() {
                dest_arr.extend(default_xyz_tail());
            } else {
                dest_arr.extend(node.dest_tail.iter().cloned());
            }
            dict.set("Dest", Object::Array(dest_arr));
        }
        if !node.children.is_empty() {
            let (first_child, last_child, child_count) =
                build_outline_level(doc, item_id, &node.children, ctx)?;
            dict.set("First", Object::Reference(first_child));
            dict.set("Last", Object::Reference(last_child));
            dict.set("Count", Object::Integer(child_count));
            total_count = total_count.saturating_add(1).saturating_add(child_count);
        } else {
            total_count = total_count.saturating_add(1);
        }
        doc.objects.insert(item_id, Object::Dictionary(dict));
    }

    Ok((item_ids[0], *item_ids.last().unwrap(), total_count))
}

fn resolve_obj<'a>(doc: &'a Document, mut obj: &'a Object) -> Option<&'a Object> {
    for _ in 0..16 {
        match *obj {
            Object::Reference(id) => obj = doc.objects.get(&id)?,
            _ => return Some(obj),
        }
    }
    None
}

fn resolve_dict<'a>(doc: &'a Document, obj: Option<&'a Object>) -> Option<&'a Dictionary> {
    resolve_obj(doc, obj?)?.as_dict().ok()
}
