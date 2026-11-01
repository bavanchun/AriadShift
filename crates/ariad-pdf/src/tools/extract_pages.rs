//! Implementation of the `pdf.extract_pages` tool and shared page-extraction helpers.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use ariad_core::tools::{page_range::DuplicatePolicy, params::extract_pages::ExtractPagesParams};
use lopdf::{Dictionary, Document, Object, ObjectId};

use crate::{
    document::{PdfDoc, ToolCtx},
    error::PdfError,
    naming,
    tools::OutputDoc,
};

const MAX_TREE_DEPTH: usize = 64;

/// Parsed outline item from a source PDF document.
#[derive(Debug, Clone)]
pub(crate) struct OutlineNode {
    pub title: Object,
    pub target_page_1_based: Option<u32>,
    pub dest_tail: Vec<Object>,
    pub children: Vec<OutlineNode>,
}

/// Outline item bound to a target page `ObjectId` in an output document.
#[derive(Debug, Clone)]
pub(crate) struct RenderedOutlineNode {
    pub title: Object,
    pub target_page_id: ObjectId,
    pub dest_tail: Vec<Object>,
    pub children: Vec<RenderedOutlineNode>,
}

/// Parsed `/PageLabels` rule for a starting 0-based page index.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PageLabelRule {
    pub style: Option<Vec<u8>>,
    pub prefix: Option<Object>,
    pub start: i64,
}

/// Returns the 1-based page count of `doc`.
#[must_use]
pub(crate) fn doc_page_count(doc: &PdfDoc) -> u32 {
    u32::try_from(doc.inner().get_pages().len()).unwrap_or(u32::MAX)
}

/// Extracts selected pages from a PDF document into a new PDF.
pub fn run(
    inputs: &[PdfDoc],
    params: &ExtractPagesParams,
    ctx: &ToolCtx,
) -> Result<Vec<OutputDoc>, PdfError> {
    ctx.check()?;
    if inputs.len() != 1 {
        return Err(PdfError::InvalidParams(format!(
            "pdf.extract_pages requires exactly 1 input document (got {})",
            inputs.len()
        )));
    }
    let source = &inputs[0];
    let page_count = doc_page_count(source);
    let selected = params
        .pages
        .expand(page_count, DuplicatePolicy::KeepInOrder)
        .map_err(|e| PdfError::InvalidParams(e.to_string()))?;
    if selected.is_empty() {
        return Err(PdfError::InvalidParams(
            "selected page range produced 0 pages".to_owned(),
        ));
    }

    let extracted = build_extracted_doc(source, &selected, ctx)?;
    Ok(vec![OutputDoc::unnamed_pdf(extracted)])
}

/// Builds a new [`PdfDoc`] containing the 1-based `selected_pages` from `source` in the
/// exact order given (supporting duplicate page selections).
pub(crate) fn build_extracted_doc(
    source: &PdfDoc,
    selected_pages: &[u32],
    ctx: &ToolCtx,
) -> Result<PdfDoc, PdfError> {
    ctx.check()?;
    if selected_pages.is_empty() {
        return Err(PdfError::InvalidParams(
            "selected page list must not be empty".to_owned(),
        ));
    }

    let src_doc = source.inner();
    let page_map = src_doc.get_pages();
    let mut id_to_page_num = BTreeMap::new();
    for (&num, &id) in &page_map {
        id_to_page_num.insert(id, num);
    }

    let pages_tree_ids = collect_pages_tree_ids(src_doc, &page_map);

    let mut target_doc = Document::with_version(src_doc.version.clone());
    let pages_root_id = target_doc.new_object_id();

    let mut new_page_ids = Vec::with_capacity(selected_pages.len());
    let mut first_target_page_id: BTreeMap<ObjectId, ObjectId> = BTreeMap::new();
    let mut kept_page_num_to_id: BTreeMap<u32, ObjectId> = BTreeMap::new();

    for &page_num in selected_pages {
        let &src_page_id = page_map.get(&page_num).ok_or_else(|| {
            PdfError::InvalidParams(format!(
                "page {page_num} does not exist in document with {} pages",
                page_map.len()
            ))
        })?;
        let new_id = target_doc.new_object_id();
        new_page_ids.push(new_id);
        first_target_page_id.entry(src_page_id).or_insert(new_id);
        kept_page_num_to_id.entry(page_num).or_insert(new_id);
    }

    let mut copier = SelectiveCopier::new(
        src_doc,
        &mut target_doc,
        &pages_tree_ids,
        &first_target_page_id,
    );

    let mut page_rotations: Vec<(ObjectId, i64, bool, bool)> =
        Vec::with_capacity(selected_pages.len());
    let mut kept_annot_ids: BTreeMap<ObjectId, Vec<ObjectId>> = BTreeMap::new();

    for (idx, &page_num) in selected_pages.iter().enumerate() {
        ctx.check()?;
        let src_page_id = page_map[&page_num];
        let new_page_id = new_page_ids[idx];
        let is_first_occurrence = first_target_page_id.get(&src_page_id) == Some(&new_page_id);

        let src_page_dict = src_doc
            .get_dictionary(src_page_id)
            .map_err(|_| PdfError::Malformed)?;

        let mut new_page_dict = Dictionary::new();
        for (key, value) in src_page_dict.iter() {
            if matches!(
                key.as_slice(),
                b"Type"
                    | b"Parent"
                    | b"Resources"
                    | b"MediaBox"
                    | b"CropBox"
                    | b"BleedBox"
                    | b"TrimBox"
                    | b"ArtBox"
                    | b"Rotate"
                    | b"Annots"
            ) {
                continue;
            }
            let copied_val = copier.copy_object(value)?;
            new_page_dict.set(key.clone(), copied_val);
        }

        new_page_dict.set(b"Type".to_vec(), Object::Name(b"Page".to_vec()));
        new_page_dict.set(b"Parent".to_vec(), Object::Reference(pages_root_id));

        for box_key in [
            &b"MediaBox"[..],
            &b"CropBox"[..],
            &b"BleedBox"[..],
            &b"TrimBox"[..],
            &b"ArtBox"[..],
        ] {
            if let Some(box_obj) = inherited_page_attr(src_doc, src_page_dict, box_key) {
                let copied_box = copier.copy_object(box_obj)?;
                new_page_dict.set(box_key.to_vec(), copied_box);
            }
        }

        let merged_resources = resolve_merged_resources(src_doc, src_page_dict, &mut copier)?;
        new_page_dict.set(b"Resources".to_vec(), Object::Dictionary(merged_resources));

        let (rot_deg, is_inherited) = resolve_inherited_page_rotation(src_doc, src_page_dict);
        let had_explicit_rotate = src_page_dict.has(b"Rotate");
        page_rotations.push((new_page_id, rot_deg, is_inherited, had_explicit_rotate));

        let page_annots = copy_page_annotations(
            src_doc,
            src_page_dict,
            new_page_id,
            is_first_occurrence,
            &id_to_page_num,
            &first_target_page_id,
            &mut kept_annot_ids,
            &mut copier,
        )?;
        if !page_annots.is_empty() {
            new_page_dict.set(b"Annots".to_vec(), Object::Array(page_annots));
        }

        copier
            .target
            .objects
            .insert(new_page_id, Object::Dictionary(new_page_dict));
    }

    let copier_mapping = copier.mapping;

    // Determine whether we should set `/Rotate` on the `/Pages` root dictionary to preserve
    // `inherited_rotation` semantics for pages that inherited rotation from `/Pages`.
    let inherited_angles: BTreeSet<i64> = page_rotations
        .iter()
        .filter(|(_, deg, is_inherited, _)| *is_inherited && *deg != 0)
        .map(|(_, deg, _, _)| *deg)
        .collect();
    let parent_rotate = if inherited_angles.len() == 1 {
        inherited_angles.into_iter().next()
    } else {
        None
    };

    for &(new_page_id, rot_deg, is_inherited, had_explicit_rotate) in &page_rotations {
        let Some(Object::Dictionary(page_dict)) = target_doc.objects.get_mut(&new_page_id) else {
            continue;
        };
        if let Some(p_rot) = parent_rotate {
            if is_inherited && rot_deg == p_rot {
                page_dict.remove(b"Rotate");
            } else {
                page_dict.set(b"Rotate".to_vec(), Object::Integer(rot_deg));
            }
        } else if rot_deg != 0 || had_explicit_rotate {
            page_dict.set(b"Rotate".to_vec(), Object::Integer(rot_deg));
        }
    }

    let kids: Vec<Object> = new_page_ids
        .iter()
        .copied()
        .map(Object::Reference)
        .collect();
    let mut pages_root_dict = Dictionary::new();
    pages_root_dict.set(b"Type".to_vec(), Object::Name(b"Pages".to_vec()));
    pages_root_dict.set(b"Kids".to_vec(), Object::Array(kids));
    pages_root_dict.set(
        b"Count".to_vec(),
        Object::Integer(i64::try_from(new_page_ids.len()).unwrap_or(i64::MAX)),
    );
    if let Some(p_rot) = parent_rotate {
        pages_root_dict.set(b"Rotate".to_vec(), Object::Integer(p_rot));
    }
    target_doc
        .objects
        .insert(pages_root_id, Object::Dictionary(pages_root_dict));

    // Build Catalog and copy document-level structures.
    let mut catalog_dict = Dictionary::new();
    catalog_dict.set(b"Type".to_vec(), Object::Name(b"Catalog".to_vec()));
    catalog_dict.set(b"Pages".to_vec(), Object::Reference(pages_root_id));

    if let Ok(src_catalog) = src_doc.catalog() {
        let mut copier = SelectiveCopier::with_existing_mapping(
            src_doc,
            &mut target_doc,
            &pages_tree_ids,
            &first_target_page_id,
            copier_mapping,
        );

        for key in [
            &b"Lang"[..],
            &b"Metadata"[..],
            &b"Names"[..],
            &b"AF"[..],
            &b"MarkInfo"[..],
            &b"ViewerPreferences"[..],
            &b"PageLayout"[..],
            &b"OutputIntents"[..],
            &b"Dests"[..],
        ] {
            if let Ok(val) = src_catalog.get(key) {
                let copied = copier.copy_object(val)?;
                if !matches!(copied, Object::Null) {
                    catalog_dict.set(key.to_vec(), copied);
                }
            }
        }

        if let Some(acroform_obj) =
            copy_filtered_acroform(src_doc, src_catalog, &kept_annot_ids, &mut copier)?
        {
            catalog_dict.set(b"AcroForm".to_vec(), acroform_obj);
        }
    }

    // Rebuild `/Outlines` pointing to kept pages.
    let src_outlines = parse_outline_tree(src_doc, &id_to_page_num);
    let rendered_outlines = filter_outlines_for_pages(&src_outlines, &kept_page_num_to_id);
    if let Some(outlines_id) = write_outline_tree(&mut target_doc, &rendered_outlines) {
        catalog_dict.set(b"Outlines".to_vec(), Object::Reference(outlines_id));
    }

    // Rebuild `/PageLabels` mapped across the selected pages.
    let src_labels = parse_page_labels(src_doc);
    if !src_labels.is_empty() {
        let remapped_labels = remap_page_labels_for_selection(&src_labels, selected_pages);
        if !remapped_labels.is_empty() {
            let labels_obj = build_page_labels_object(&remapped_labels);
            catalog_dict.set(b"PageLabels".to_vec(), labels_obj);
        }
    }

    let catalog_id = target_doc.add_object(Object::Dictionary(catalog_dict));
    target_doc
        .trailer
        .set(b"Root".to_vec(), Object::Reference(catalog_id));

    // Preserve `/Info` dictionary in trailer.
    if let Some(info_dict) = resolve_source_info_dict(src_doc) {
        let info_id = target_doc.add_object(Object::Dictionary(info_dict));
        target_doc
            .trailer
            .set(b"Info".to_vec(), Object::Reference(info_id));
    }

    let mut out_pdf = source.clone();
    *out_pdf.inner_mut()? = target_doc;
    if let Some(stem) = naming::doc_stem(source) {
        naming::set_doc_stem(&mut out_pdf, &stem);
    }
    Ok(out_pdf)
}

/// Collects all `/Pages` tree node IDs and `/Page` object IDs in `doc`.
pub(crate) fn collect_pages_tree_ids(
    doc: &Document,
    page_map: &BTreeMap<u32, ObjectId>,
) -> BTreeSet<ObjectId> {
    let mut ids: BTreeSet<ObjectId> = page_map.values().copied().collect();
    if let Ok(catalog) = doc.catalog()
        && let Ok(pages_id) = catalog.get(b"Pages").and_then(Object::as_reference)
    {
        let mut stack = vec![pages_id];
        while let Some(id) = stack.pop() {
            if !ids.insert(id) {
                continue;
            }
            if let Ok(dict) = doc.get_dictionary(id)
                && let Ok(Object::Array(kids)) = dict.get(b"Kids")
            {
                for kid in kids {
                    if let Ok(kid_id) = kid.as_reference() {
                        stack.push(kid_id);
                    }
                }
            }
        }
    }
    for (&id, obj) in &doc.objects {
        if let Ok(dict) = obj.as_dict()
            && let Ok(t) = dict.get(b"Type").and_then(Object::as_name)
            && (t == b"Pages" || t == b"Page")
        {
            ids.insert(id);
        }
    }
    ids
}

/// Object copier that remaps kept `/Page` references to their new target page IDs and refuses to
/// traverse into removed `/Page` objects or old `/Pages` tree nodes.
pub(crate) struct SelectiveCopier<'a> {
    pub source: &'a Document,
    pub target: &'a mut Document,
    pub pages_tree_ids: &'a BTreeSet<ObjectId>,
    pub first_target_page_id: &'a BTreeMap<ObjectId, ObjectId>,
    pub mapping: BTreeMap<ObjectId, ObjectId>,
}

impl<'a> SelectiveCopier<'a> {
    pub fn new(
        source: &'a Document,
        target: &'a mut Document,
        pages_tree_ids: &'a BTreeSet<ObjectId>,
        first_target_page_id: &'a BTreeMap<ObjectId, ObjectId>,
    ) -> Self {
        Self {
            source,
            target,
            pages_tree_ids,
            first_target_page_id,
            mapping: BTreeMap::new(),
        }
    }

    pub fn with_existing_mapping(
        source: &'a Document,
        target: &'a mut Document,
        pages_tree_ids: &'a BTreeSet<ObjectId>,
        first_target_page_id: &'a BTreeMap<ObjectId, ObjectId>,
        mapping: BTreeMap<ObjectId, ObjectId>,
    ) -> Self {
        Self {
            source,
            target,
            pages_tree_ids,
            first_target_page_id,
            mapping,
        }
    }

    pub fn copy_id(&mut self, source_id: ObjectId) -> Result<ObjectId, PdfError> {
        if let Some(&mapped_page) = self.first_target_page_id.get(&source_id) {
            return Ok(mapped_page);
        }
        if let Some(&existing) = self.mapping.get(&source_id) {
            return Ok(existing);
        }
        let new_id = self.target.new_object_id();
        self.mapping.insert(source_id, new_id);

        let obj = self
            .source
            .get_object(source_id)
            .map_err(|_| PdfError::Malformed)?;
        let copied = self.copy_object(obj)?;
        self.target.objects.insert(new_id, copied);
        Ok(new_id)
    }

    pub fn copy_object(&mut self, obj: &Object) -> Result<Object, PdfError> {
        match obj {
            Object::Null => Ok(Object::Null),
            Object::Boolean(b) => Ok(Object::Boolean(*b)),
            Object::Integer(i) => Ok(Object::Integer(*i)),
            Object::Real(r) => Ok(Object::Real(*r)),
            Object::Name(n) => Ok(Object::Name(n.clone())),
            Object::String(s, fmt) => Ok(Object::String(s.clone(), *fmt)),
            Object::Array(items) => {
                let mut out = Vec::with_capacity(items.len());
                for item in items {
                    out.push(self.copy_object(item)?);
                }
                Ok(Object::Array(out))
            }
            Object::Dictionary(dict) => Ok(Object::Dictionary(self.copy_dictionary(dict)?)),
            Object::Stream(stream) => {
                let dict = self.copy_dictionary(&stream.dict)?;
                let mut copied_stream = lopdf::Stream::new(dict, stream.content.clone());
                copied_stream.allows_compression = stream.allows_compression;
                Ok(Object::Stream(copied_stream))
            }
            Object::Reference(id) => {
                if let Some(&mapped_page) = self.first_target_page_id.get(id) {
                    return Ok(Object::Reference(mapped_page));
                }
                if self.pages_tree_ids.contains(id) {
                    return Ok(Object::Null);
                }
                let new_id = self.copy_id(*id)?;
                Ok(Object::Reference(new_id))
            }
        }
    }

    pub fn copy_dictionary(&mut self, dict: &Dictionary) -> Result<Dictionary, PdfError> {
        let mut out = Dictionary::new();
        for (k, v) in dict.iter() {
            let copied_v = self.copy_object(v)?;
            if !matches!(copied_v, Object::Null) || matches!(v, Object::Null) {
                out.set(k.clone(), copied_v);
            }
        }
        Ok(out)
    }
}

/// Resolves an inheritable attribute (`MediaBox`, `CropBox`, `Resources`, `Rotate`, etc.) by
/// walking `/Parent` up to `MAX_TREE_DEPTH`.
pub(crate) fn inherited_page_attr<'a>(
    doc: &'a Document,
    page_dict: &'a Dictionary,
    key: &[u8],
) -> Option<&'a Object> {
    let mut current = page_dict;
    for _ in 0..MAX_TREE_DEPTH {
        if let Ok(val) = current.get(key) {
            return Some(val);
        }
        let parent_obj = current.get(b"Parent").ok()?;
        current = resolve_dict(doc, parent_obj)?;
    }
    None
}

/// Resolves and merges `/Resources` from ancestor `/Pages` nodes down to `page_dict`.
pub(crate) fn resolve_merged_resources(
    src_doc: &Document,
    page_dict: &Dictionary,
    copier: &mut SelectiveCopier<'_>,
) -> Result<Dictionary, PdfError> {
    let mut chain: Vec<&Dictionary> = Vec::new();
    let mut current = Some(page_dict);
    for _ in 0..MAX_TREE_DEPTH {
        let Some(node) = current else {
            break;
        };
        if let Ok(res_obj) = node.get(b"Resources")
            && let Some(res_dict) = resolve_dict(src_doc, res_obj)
        {
            chain.push(res_dict);
        }
        current = node
            .get(b"Parent")
            .ok()
            .and_then(|p| resolve_dict(src_doc, p));
    }

    // Apply from furthest ancestor down to the page itself so page-local resources win.
    let mut merged = Dictionary::new();
    for res_dict in chain.into_iter().rev() {
        let copied_res = copier.copy_dictionary(res_dict)?;
        for (k, v) in copied_res.iter() {
            if let (Ok(Object::Dictionary(existing_sub)), Object::Dictionary(new_sub)) =
                (merged.get_mut(k.as_slice()), v)
            {
                for (sub_k, sub_v) in new_sub.iter() {
                    existing_sub.set(sub_k.clone(), sub_v.clone());
                }
            } else {
                merged.set(k.clone(), v.clone());
            }
        }
    }
    Ok(merged)
}

/// Resolves page rotation in degrees (`0`, `90`, `180`, `270`) and whether it was inherited from
/// an ancestor `/Pages` dictionary (including sibling-inferred parent rotation).
pub(crate) fn resolve_inherited_page_rotation(
    doc: &Document,
    page_dict: &Dictionary,
) -> (i64, bool) {
    let mut current = page_dict;
    for depth in 0..MAX_TREE_DEPTH {
        if let Ok(rot_obj) = current.get(b"Rotate")
            && let Some(deg) = resolve_object(doc, rot_obj).and_then(object_as_f64)
        {
            let norm = i64::from(((deg / 90.0).round() as i32).rem_euclid(4) * 90);
            return (norm, depth > 0);
        }
        let Some(parent_dict) = current
            .get(b"Parent")
            .ok()
            .and_then(|p| resolve_dict(doc, p))
        else {
            break;
        };
        current = parent_dict;
    }

    if let Some(parent) = page_dict
        .get(b"Parent")
        .ok()
        .and_then(|p| resolve_dict(doc, p))
        && let Ok(kids_obj) = parent.get(b"Kids")
        && let Some(Object::Array(kids)) = resolve_object(doc, kids_obj)
    {
        let mut explicit_rotations = HashSet::new();
        for kid in kids {
            if let Some(kid_dict) = resolve_dict(doc, kid)
                && let Ok(rot_obj) = kid_dict.get(b"Rotate")
                && let Some(deg) = resolve_object(doc, rot_obj).and_then(object_as_f64)
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

#[allow(clippy::too_many_arguments)]
fn copy_page_annotations(
    src_doc: &Document,
    src_page_dict: &Dictionary,
    new_page_id: ObjectId,
    is_first_occurrence: bool,
    id_to_page_num: &BTreeMap<ObjectId, u32>,
    first_target_page_id: &BTreeMap<ObjectId, ObjectId>,
    kept_annot_ids: &mut BTreeMap<ObjectId, Vec<ObjectId>>,
    copier: &mut SelectiveCopier<'_>,
) -> Result<Vec<Object>, PdfError> {
    let Ok(annots_obj) = src_page_dict.get(b"Annots") else {
        return Ok(Vec::new());
    };
    let Some(Object::Array(annots)) = resolve_object(src_doc, annots_obj) else {
        return Ok(Vec::new());
    };

    let mut out_annots = Vec::new();
    for annot_entry in annots {
        let src_annot_id = annot_entry.as_reference().ok();
        let Some(annot_dict) = resolve_dict(src_doc, annot_entry) else {
            continue;
        };

        if annot_targets_removed_page(src_doc, annot_dict, id_to_page_num, first_target_page_id) {
            continue;
        }

        if let Some(src_id) = src_annot_id {
            let new_annot_id = if is_first_occurrence {
                copier.copy_id(src_id)?
            } else {
                let copied_dict = copier.copy_dictionary(annot_dict)?;
                copier.target.add_object(Object::Dictionary(copied_dict))
            };
            if let Some(Object::Dictionary(target_annot_dict)) =
                copier.target.objects.get_mut(&new_annot_id)
                && (target_annot_dict.has(b"P")
                    || target_annot_dict
                        .get(b"Subtype")
                        .and_then(Object::as_name)
                        .ok()
                        == Some(b"Widget"))
            {
                target_annot_dict.set(b"P".to_vec(), Object::Reference(new_page_id));
            }
            kept_annot_ids.entry(src_id).or_default().push(new_annot_id);
            out_annots.push(Object::Reference(new_annot_id));
        } else {
            let mut copied_dict = copier.copy_dictionary(annot_dict)?;
            if copied_dict.has(b"P") {
                copied_dict.set(b"P".to_vec(), Object::Reference(new_page_id));
            }
            let new_annot_id = copier.target.add_object(Object::Dictionary(copied_dict));
            out_annots.push(Object::Reference(new_annot_id));
        }
    }
    Ok(out_annots)
}

/// Returns `true` if `annot_dict` has an internal page destination (`/Dest` or `/A /GoTo /D`)
/// pointing to a `/Page` in `src_doc` that is NOT in `first_target_page_id`.
fn annot_targets_removed_page(
    src_doc: &Document,
    annot_dict: &Dictionary,
    id_to_page_num: &BTreeMap<ObjectId, u32>,
    first_target_page_id: &BTreeMap<ObjectId, ObjectId>,
) -> bool {
    if let Ok(dest_obj) = annot_dict.get(b"Dest")
        && let Some(target_page_id) = extract_dest_page_id(src_doc, dest_obj, id_to_page_num)
    {
        return !first_target_page_id.contains_key(&target_page_id);
    }
    if let Ok(action_obj) = annot_dict.get(b"A")
        && let Some(action_dict) = resolve_dict(src_doc, action_obj)
        && action_dict.get(b"S").and_then(Object::as_name).ok() == Some(b"GoTo")
        && let Ok(d_obj) = action_dict.get(b"D")
        && let Some(target_page_id) = extract_dest_page_id(src_doc, d_obj, id_to_page_num)
    {
        return !first_target_page_id.contains_key(&target_page_id);
    }
    false
}

fn extract_dest_page_id(
    doc: &Document,
    dest_obj: &Object,
    id_to_page_num: &BTreeMap<ObjectId, u32>,
) -> Option<ObjectId> {
    let Object::Array(items) = resolve_object(doc, dest_obj)? else {
        return None;
    };
    let page_id = items.first()?.as_reference().ok()?;
    id_to_page_num.contains_key(&page_id).then_some(page_id)
}

/// Copies `/AcroForm`, keeping only fields whose widget annotations belong to kept pages.
fn copy_filtered_acroform(
    src_doc: &Document,
    src_catalog: &Dictionary,
    kept_annot_ids: &BTreeMap<ObjectId, Vec<ObjectId>>,
    copier: &mut SelectiveCopier<'_>,
) -> Result<Option<Object>, PdfError> {
    let Ok(acro_obj) = src_catalog.get(b"AcroForm") else {
        return Ok(None);
    };
    let Some(acro_dict) = resolve_dict(src_doc, acro_obj) else {
        return Ok(None);
    };

    let mut out_fields = Vec::new();
    if let Ok(fields_obj) = acro_dict.get(b"Fields")
        && let Some(Object::Array(fields)) = resolve_object(src_doc, fields_obj)
    {
        for field_entry in fields {
            let Ok(src_field_id) = field_entry.as_reference() else {
                continue;
            };
            if let Some(mapped_ids) = kept_annot_ids.get(&src_field_id) {
                if let Some(&first_id) = mapped_ids.first() {
                    out_fields.push(Object::Reference(first_id));
                }
                continue;
            }
            let Some(field_dict) = resolve_dict(src_doc, field_entry) else {
                continue;
            };
            if let Ok(kids_obj) = field_dict.get(b"Kids")
                && let Some(Object::Array(kids)) = resolve_object(src_doc, kids_obj)
            {
                let mut surviving_kids = Vec::new();
                for kid in kids {
                    if let Ok(src_kid_id) = kid.as_reference()
                        && let Some(mapped_kid_ids) = kept_annot_ids.get(&src_kid_id)
                        && let Some(&first_kid_id) = mapped_kid_ids.first()
                    {
                        surviving_kids.push(Object::Reference(first_kid_id));
                    }
                }
                if !surviving_kids.is_empty() {
                    let new_parent_id = copier.copy_id(src_field_id)?;
                    if let Some(Object::Dictionary(parent_dict)) =
                        copier.target.objects.get_mut(&new_parent_id)
                    {
                        parent_dict.set(b"Kids".to_vec(), Object::Array(surviving_kids));
                    }
                    out_fields.push(Object::Reference(new_parent_id));
                }
            }
        }
    }

    if out_fields.is_empty() {
        return Ok(None);
    }

    let mut new_acro = Dictionary::new();
    for (k, v) in acro_dict.iter() {
        if k.as_slice() == b"Fields" {
            continue;
        }
        let copied = copier.copy_object(v)?;
        if !matches!(copied, Object::Null) {
            new_acro.set(k.clone(), copied);
        }
    }
    new_acro.set(b"Fields".to_vec(), Object::Array(out_fields));
    let acro_id = copier.target.add_object(Object::Dictionary(new_acro));
    Ok(Some(Object::Reference(acro_id)))
}

/// Parses the `/Outlines` hierarchy from `doc`, mapping destination page references to 1-based
/// page numbers via `id_to_page_num`.
pub(crate) fn parse_outline_tree(
    doc: &Document,
    id_to_page_num: &BTreeMap<ObjectId, u32>,
) -> Vec<OutlineNode> {
    let Ok(catalog) = doc.catalog() else {
        return Vec::new();
    };
    let Ok(outlines_obj) = catalog.get(b"Outlines") else {
        return Vec::new();
    };
    let Some(outlines_dict) = resolve_dict(doc, outlines_obj) else {
        return Vec::new();
    };
    let Ok(first_id) = outlines_dict.get(b"First").and_then(Object::as_reference) else {
        return Vec::new();
    };

    let mut visited = BTreeSet::new();
    parse_outline_siblings(doc, first_id, id_to_page_num, &mut visited, 0)
}

fn parse_outline_siblings(
    doc: &Document,
    first_id: ObjectId,
    id_to_page_num: &BTreeMap<ObjectId, u32>,
    visited: &mut BTreeSet<ObjectId>,
    depth: usize,
) -> Vec<OutlineNode> {
    if depth >= MAX_TREE_DEPTH {
        return Vec::new();
    }
    let mut nodes = Vec::new();
    let mut cur_opt = Some(first_id);
    while let Some(item_id) = cur_opt {
        if !visited.insert(item_id) {
            break;
        }
        let Ok(dict) = doc.get_dictionary(item_id) else {
            break;
        };
        let title = dict
            .get(b"Title")
            .ok()
            .and_then(|o| resolve_object(doc, o))
            .cloned()
            .unwrap_or_else(|| Object::string_literal("Bookmark"));
        let (target_page_1_based, dest_tail) = extract_outline_dest(doc, dict, id_to_page_num);
        let children = if let Ok(child_first) = dict.get(b"First").and_then(Object::as_reference) {
            parse_outline_siblings(doc, child_first, id_to_page_num, visited, depth + 1)
        } else {
            Vec::new()
        };
        nodes.push(OutlineNode {
            title,
            target_page_1_based,
            dest_tail,
            children,
        });
        cur_opt = dict.get(b"Next").and_then(Object::as_reference).ok();
    }
    nodes
}

fn extract_outline_dest(
    doc: &Document,
    item_dict: &Dictionary,
    id_to_page_num: &BTreeMap<ObjectId, u32>,
) -> (Option<u32>, Vec<Object>) {
    let dest_arr = if let Ok(dest_obj) = item_dict.get(b"Dest") {
        resolve_object(doc, dest_obj).and_then(|o| match o {
            Object::Array(arr) => Some(arr),
            _ => None,
        })
    } else if let Ok(action_obj) = item_dict.get(b"A")
        && let Some(action_dict) = resolve_dict(doc, action_obj)
        && action_dict.get(b"S").and_then(Object::as_name).ok() == Some(b"GoTo")
        && let Ok(d_obj) = action_dict.get(b"D")
    {
        resolve_object(doc, d_obj).and_then(|o| match o {
            Object::Array(arr) => Some(arr),
            _ => None,
        })
    } else {
        None
    };

    if let Some(arr) = dest_arr
        && let Some(first) = arr.first()
        && let Ok(page_id) = first.as_reference()
        && let Some(&page_num) = id_to_page_num.get(&page_id)
    {
        let tail: Vec<Object> = arr
            .iter()
            .skip(1)
            .filter(|o| !matches!(o, Object::Reference(_)))
            .cloned()
            .collect();
        let tail = if tail.is_empty() {
            default_xyz_tail()
        } else {
            tail
        };
        return (Some(page_num), tail);
    }
    (None, default_xyz_tail())
}

pub(crate) fn default_xyz_tail() -> Vec<Object> {
    vec![
        Object::Name(b"XYZ".to_vec()),
        Object::Null,
        Object::Null,
        Object::Null,
    ]
}

/// Filters outline nodes to those targeting pages in `kept_page_num_to_id`, promoting surviving
/// child bookmarks when a parent bookmark's page was removed.
pub(crate) fn filter_outlines_for_pages(
    nodes: &[OutlineNode],
    kept_page_num_to_id: &BTreeMap<u32, ObjectId>,
) -> Vec<RenderedOutlineNode> {
    let mut out = Vec::new();
    for node in nodes {
        let filtered_children = filter_outlines_for_pages(&node.children, kept_page_num_to_id);
        if let Some(page_num) = node.target_page_1_based
            && let Some(&target_page_id) = kept_page_num_to_id.get(&page_num)
        {
            out.push(RenderedOutlineNode {
                title: node.title.clone(),
                target_page_id,
                dest_tail: node.dest_tail.clone(),
                children: filtered_children,
            });
        } else {
            out.extend(filtered_children);
        }
    }
    out
}

/// Writes `roots` as a PDF `/Outlines` object hierarchy into `doc`, returning the `/Outlines`
/// root `ObjectId` when `roots` is non-empty.
pub(crate) fn write_outline_tree(
    doc: &mut Document,
    roots: &[RenderedOutlineNode],
) -> Option<ObjectId> {
    if roots.is_empty() {
        return None;
    }
    let outlines_root_id = doc.new_object_id();
    let (first_id, last_id, total_count) = write_outline_level(doc, outlines_root_id, roots);

    let mut root_dict = Dictionary::new();
    root_dict.set(b"Type".to_vec(), Object::Name(b"Outlines".to_vec()));
    root_dict.set(b"First".to_vec(), Object::Reference(first_id));
    root_dict.set(b"Last".to_vec(), Object::Reference(last_id));
    root_dict.set(b"Count".to_vec(), Object::Integer(total_count));
    doc.objects
        .insert(outlines_root_id, Object::Dictionary(root_dict));
    Some(outlines_root_id)
}

fn write_outline_level(
    doc: &mut Document,
    parent_id: ObjectId,
    siblings: &[RenderedOutlineNode],
) -> (ObjectId, ObjectId, i64) {
    let item_ids: Vec<ObjectId> = (0..siblings.len()).map(|_| doc.new_object_id()).collect();
    let mut total_count = 0_i64;

    for (idx, node) in siblings.iter().enumerate() {
        let item_id = item_ids[idx];
        let mut dict = Dictionary::new();
        dict.set(b"Title".to_vec(), node.title.clone());
        dict.set(b"Parent".to_vec(), Object::Reference(parent_id));
        if idx > 0 {
            dict.set(b"Prev".to_vec(), Object::Reference(item_ids[idx - 1]));
        }
        if idx + 1 < item_ids.len() {
            dict.set(b"Next".to_vec(), Object::Reference(item_ids[idx + 1]));
        }

        let mut dest = Vec::with_capacity(1 + node.dest_tail.len());
        dest.push(Object::Reference(node.target_page_id));
        dest.extend(node.dest_tail.iter().cloned());
        dict.set(b"Dest".to_vec(), Object::Array(dest));

        total_count = total_count.saturating_add(1);
        if !node.children.is_empty() {
            let (first_child, last_child, child_count) =
                write_outline_level(doc, item_id, &node.children);
            dict.set(b"First".to_vec(), Object::Reference(first_child));
            dict.set(b"Last".to_vec(), Object::Reference(last_child));
            dict.set(b"Count".to_vec(), Object::Integer(child_count));
            total_count = total_count.saturating_add(child_count);
        }

        doc.objects.insert(item_id, Object::Dictionary(dict));
    }

    (
        item_ids[0],
        *item_ids.last().expect("siblings is non-empty"),
        total_count,
    )
}

/// Parses `/PageLabels` from `doc`'s Catalog into a map of 0-based page index -> [`PageLabelRule`].
pub(crate) fn parse_page_labels(doc: &Document) -> BTreeMap<u32, PageLabelRule> {
    let mut out = BTreeMap::new();
    let Ok(catalog) = doc.catalog() else {
        return out;
    };
    let Ok(labels_obj) = catalog.get(b"PageLabels") else {
        return out;
    };
    let Some(labels_dict) = resolve_dict(doc, labels_obj) else {
        return out;
    };
    collect_page_label_nums(doc, labels_dict, &mut out, 0);
    out
}

fn collect_page_label_nums(
    doc: &Document,
    node_dict: &Dictionary,
    out: &mut BTreeMap<u32, PageLabelRule>,
    depth: usize,
) {
    if depth >= MAX_TREE_DEPTH {
        return;
    }
    if let Ok(nums_obj) = node_dict.get(b"Nums")
        && let Some(Object::Array(nums)) = resolve_object(doc, nums_obj)
    {
        for pair in nums.as_chunks::<2>().0 {
            let Some(idx_i64) = resolve_object(doc, &pair[0]).and_then(|o| o.as_i64().ok()) else {
                continue;
            };
            let Ok(page_idx) = u32::try_from(idx_i64) else {
                continue;
            };
            let Some(rule_dict) = resolve_dict(doc, &pair[1]) else {
                continue;
            };
            let style = rule_dict
                .get(b"S")
                .ok()
                .and_then(|o| resolve_object(doc, o))
                .and_then(|o| o.as_name().ok())
                .map(<[u8]>::to_vec);
            let prefix = rule_dict
                .get(b"P")
                .ok()
                .and_then(|o| resolve_object(doc, o))
                .cloned();
            let start = rule_dict
                .get(b"St")
                .ok()
                .and_then(|o| resolve_object(doc, o))
                .and_then(|o| o.as_i64().ok())
                .unwrap_or(1)
                .max(1);
            out.insert(
                page_idx,
                PageLabelRule {
                    style,
                    prefix,
                    start,
                },
            );
        }
    }
    if let Ok(kids_obj) = node_dict.get(b"Kids")
        && let Some(Object::Array(kids)) = resolve_object(doc, kids_obj)
    {
        for kid in kids {
            if let Some(kid_dict) = resolve_dict(doc, kid) {
                collect_page_label_nums(doc, kid_dict, out, depth + 1);
            }
        }
    }
}

/// Remaps source `/PageLabels` across the 1-based `selected_pages` sequence.
pub(crate) fn remap_page_labels_for_selection(
    src_labels: &BTreeMap<u32, PageLabelRule>,
    selected_pages: &[u32],
) -> Vec<(u32, PageLabelRule)> {
    if src_labels.is_empty() || selected_pages.is_empty() {
        return Vec::new();
    }

    let mut result: Vec<(u32, PageLabelRule)> = Vec::new();
    let mut prev_resolved: Option<(Option<Vec<u8>>, Option<Object>, i64)> = None;

    for (out_idx_usize, &page_num_1_based) in selected_pages.iter().enumerate() {
        let Ok(out_idx) = u32::try_from(out_idx_usize) else {
            break;
        };
        let src_idx = page_num_1_based.saturating_sub(1);
        let resolved =
            if let Some((&rule_start_idx, rule)) = src_labels.range(..=src_idx).next_back() {
                let offset = i64::from(src_idx.saturating_sub(rule_start_idx));
                (
                    rule.style.clone(),
                    rule.prefix.clone(),
                    rule.start.saturating_add(offset),
                )
            } else {
                (
                    Some(b"D".to_vec()),
                    None,
                    i64::from(src_idx).saturating_add(1),
                )
            };

        let continues_prev = prev_resolved
            .as_ref()
            .is_some_and(|(p_style, p_prefix, p_num)| {
                *p_style == resolved.0
                    && *p_prefix == resolved.1
                    && p_num.saturating_add(1) == resolved.2
            });

        if !continues_prev {
            result.push((
                out_idx,
                PageLabelRule {
                    style: resolved.0.clone(),
                    prefix: resolved.1.clone(),
                    start: resolved.2,
                },
            ));
        }
        prev_resolved = Some(resolved);
    }

    result
}

/// Builds a `/PageLabels` number-tree dictionary [`Object`] from `(0_based_page_idx, rule)` pairs.
pub(crate) fn build_page_labels_object(entries: &[(u32, PageLabelRule)]) -> Object {
    let mut nums = Vec::with_capacity(entries.len() * 2);
    for (page_idx, rule) in entries {
        nums.push(Object::Integer(i64::from(*page_idx)));
        let mut dict = Dictionary::new();
        if let Some(style) = &rule.style {
            dict.set(b"S".to_vec(), Object::Name(style.clone()));
        }
        if let Some(prefix) = &rule.prefix {
            dict.set(b"P".to_vec(), prefix.clone());
        }
        dict.set(b"St".to_vec(), Object::Integer(rule.start.max(1)));
        nums.push(Object::Dictionary(dict));
    }
    let mut tree = Dictionary::new();
    tree.set(b"Nums".to_vec(), Object::Array(nums));
    Object::Dictionary(tree)
}

/// Resolves the document `/Info` dictionary from `src_doc` (cleaning any indirect references into
/// direct scalar values).
pub(crate) fn resolve_source_info_dict(src_doc: &Document) -> Option<Dictionary> {
    let info_obj = src_doc.trailer.get(b"Info").ok()?;
    let info_dict = resolve_dict(src_doc, info_obj)?;
    let mut out = Dictionary::new();
    for (k, v) in info_dict.iter() {
        if let Some(resolved) = resolve_object(src_doc, v)
            && !matches!(resolved, Object::Reference(_))
        {
            out.set(k.clone(), resolved.clone());
        }
    }
    Some(out)
}

pub(crate) fn resolve_object<'a>(doc: &'a Document, mut obj: &'a Object) -> Option<&'a Object> {
    for _ in 0..16 {
        match *obj {
            Object::Reference(id) => obj = doc.objects.get(&id)?,
            _ => return Some(obj),
        }
    }
    None
}

pub(crate) fn resolve_dict<'a>(doc: &'a Document, obj: &'a Object) -> Option<&'a Dictionary> {
    resolve_object(doc, obj)?.as_dict().ok()
}

fn object_as_f64(obj: &Object) -> Option<f64> {
    match *obj {
        Object::Integer(n) => Some(n as f64),
        Object::Real(n) => Some(f64::from(n)),
        _ => None,
    }
}
