//! Implementation of the `pdf.merge` tool.

use std::collections::{BTreeMap, BTreeSet};

use ariad_core::{
    reader::pdf::ReadError,
    tools::params::merge::{MergeOutlineMode, MergePageLabelsMode, MergeParams},
};
use lopdf::{Dictionary, Document, Object, ObjectId, StringFormat};

use crate::{
    document::{PdfDoc, ToolCtx},
    error::PdfError,
    naming,
    tools::{
        OutputDoc,
        page_set::{
            MAX_OUTPUT_PAGES, PageLabelRule, RenderedOutlineNode, SelectiveCopier,
            build_page_labels_object, collect_pages_tree_ids, default_xyz_tail, doc_page_count,
            filter_outlines_for_pages, inherited_page_attr, parse_outline_tree, parse_page_labels,
            resolve_dict, resolve_inherited_page_rotation, resolve_merged_resources,
            resolve_object, resolve_source_info_dict, write_outline_tree,
        },
    },
};

/// Merges multiple PDF documents into a single PDF.
pub fn run(
    inputs: &[PdfDoc],
    params: &MergeParams,
    ctx: &ToolCtx,
) -> Result<Vec<OutputDoc>, PdfError> {
    ctx.check()?;
    if !(2..=500).contains(&inputs.len()) {
        return Err(PdfError::InvalidParams(format!(
            "pdf.merge requires 2..=500 input documents (got {})",
            inputs.len()
        )));
    }

    let mut total_pages_u64 = 0_u64;
    for (idx, doc) in inputs.iter().enumerate() {
        ctx.check()?;
        if doc.is_encrypted() {
            return Err(PdfError::Encrypted {
                needs_password: false,
            });
        }
        let count = doc_page_count(doc);
        if count == 0 {
            return Err(PdfError::InvalidParams(format!(
                "input document {} has 0 pages",
                idx + 1
            )));
        }
        total_pages_u64 = total_pages_u64.saturating_add(u64::from(count));
    }
    if total_pages_u64 > MAX_OUTPUT_PAGES {
        return Err(PdfError::LimitExceeded(ReadError::TooManyPages {
            actual: total_pages_u64,
            limit: MAX_OUTPUT_PAGES as u32,
        }));
    }
    let total_pages = total_pages_u64 as u32;

    let version = inputs
        .iter()
        .map(|d| d.inner().version.as_str())
        .max()
        .unwrap_or("1.7")
        .to_owned();

    let mut target_doc = Document::with_version(version);
    let pages_root_id = target_doc.new_object_id();

    let mut all_new_page_ids: Vec<ObjectId> = Vec::with_capacity(total_pages as usize);
    let mut all_page_rotations: Vec<(ObjectId, i64, bool, bool)> =
        Vec::with_capacity(total_pages as usize);

    let mut merged_outlines: Vec<RenderedOutlineNode> = Vec::new();
    let mut merged_label_rules: BTreeMap<u32, PageLabelRule> = BTreeMap::new();
    let mut parsed_input_labels: Vec<BTreeMap<u32, PageLabelRule>> =
        Vec::with_capacity(inputs.len());
    for doc in inputs {
        parsed_input_labels.push(parse_page_labels(doc.inner(), ctx)?);
    }
    let any_input_has_labels = parsed_input_labels.iter().any(|m| !m.is_empty());

    let mut merged_acro_fields: Vec<Object> = Vec::new();
    let mut merged_acro_dr = Dictionary::new();
    let mut merged_need_appearances = false;
    let mut merged_acro_da: Option<Object> = None;
    let mut merged_acro_q: Option<Object> = None;
    let mut seen_field_names: BTreeSet<Vec<u8>> = BTreeSet::new();

    let mut merged_embedded_files: Vec<(Vec<u8>, Object)> = Vec::new();
    let mut seen_attachment_names: BTreeSet<Vec<u8>> = BTreeSet::new();
    let mut merged_af: Vec<Object> = Vec::new();

    let mut page_offset_0_based = 0_u32;

    for (doc_idx, source) in inputs.iter().enumerate() {
        ctx.check()?;
        let src_doc = source.inner();
        let page_map = src_doc.get_pages();
        let mut id_to_page_num = BTreeMap::new();
        for (&num, &id) in &page_map {
            id_to_page_num.insert(id, num);
        }
        let pages_tree_ids = collect_pages_tree_ids(src_doc, &page_map);

        let mut first_target_page_id: BTreeMap<ObjectId, ObjectId> = BTreeMap::new();
        let mut kept_page_num_to_id: BTreeMap<u32, ObjectId> = BTreeMap::new();
        let mut doc_new_page_ids: Vec<ObjectId> = Vec::with_capacity(page_map.len());

        for (&page_num, &src_page_id) in &page_map {
            let new_id = target_doc.new_object_id();
            doc_new_page_ids.push(new_id);
            all_new_page_ids.push(new_id);
            first_target_page_id.insert(src_page_id, new_id);
            kept_page_num_to_id.insert(page_num, new_id);
        }

        let mut copier = SelectiveCopier::new(
            src_doc,
            &mut target_doc,
            &pages_tree_ids,
            &first_target_page_id,
        );
        let mut kept_annot_ids: BTreeMap<ObjectId, Vec<ObjectId>> = BTreeMap::new();

        for (&page_num, &src_page_id) in &page_map {
            ctx.check()?;
            let new_page_id = kept_page_num_to_id[&page_num];
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
                let copied_val = copier.copy_object(value, ctx)?;
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
                    let copied_box = copier.copy_object(box_obj, ctx)?;
                    new_page_dict.set(box_key.to_vec(), copied_box);
                }
            }

            let merged_resources =
                resolve_merged_resources(src_doc, src_page_dict, &mut copier, ctx)?;
            new_page_dict.set(b"Resources".to_vec(), Object::Dictionary(merged_resources));

            let (rot_deg, is_inherited) = resolve_inherited_page_rotation(src_doc, src_page_dict);
            let had_explicit_rotate = src_page_dict.has(b"Rotate");
            all_page_rotations.push((new_page_id, rot_deg, is_inherited, had_explicit_rotate));

            if let Ok(annots_obj) = src_page_dict.get(b"Annots")
                && let Some(Object::Array(annots)) = resolve_object(src_doc, annots_obj)
            {
                let mut page_annots = Vec::new();
                for annot_entry in annots {
                    ctx.check()?;
                    if let Ok(src_annot_id) = annot_entry.as_reference() {
                        let new_annot_id = copier.copy_id(src_annot_id, ctx)?;
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
                        kept_annot_ids
                            .entry(src_annot_id)
                            .or_default()
                            .push(new_annot_id);
                        page_annots.push(Object::Reference(new_annot_id));
                    } else if let Some(annot_dict) = resolve_dict(src_doc, annot_entry) {
                        let mut copied_dict = copier.copy_dictionary(annot_dict, ctx)?;
                        if copied_dict.has(b"P") {
                            copied_dict.set(b"P".to_vec(), Object::Reference(new_page_id));
                        }
                        let new_annot_id =
                            copier.target.add_object(Object::Dictionary(copied_dict));
                        page_annots.push(Object::Reference(new_annot_id));
                    }
                }
                if !page_annots.is_empty() {
                    new_page_dict.set(b"Annots".to_vec(), Object::Array(page_annots));
                }
            }

            copier
                .target
                .objects
                .insert(new_page_id, Object::Dictionary(new_page_dict));
        }

        // Collect AcroForm fields and resources from this input.
        if let Ok(src_catalog) = src_doc.catalog() {
            collect_input_acroform(
                src_doc,
                src_catalog,
                &kept_annot_ids,
                &mut seen_field_names,
                &mut merged_acro_fields,
                &mut merged_acro_dr,
                &mut merged_need_appearances,
                &mut merged_acro_da,
                &mut merged_acro_q,
                &mut copier,
                ctx,
            )?;

            collect_input_attachments(
                src_doc,
                src_catalog,
                &mut seen_attachment_names,
                &mut merged_embedded_files,
                &mut merged_af,
                &mut copier,
                ctx,
            )?;
        }

        // Collect outlines according to `params.outline`.
        match params.outline {
            MergeOutlineMode::None => {}
            MergeOutlineMode::Keep => {
                let src_outlines = parse_outline_tree(src_doc, &id_to_page_num, ctx)?;
                let rendered = filter_outlines_for_pages(&src_outlines, &kept_page_num_to_id);
                merged_outlines.extend(rendered);
            }
            MergeOutlineMode::PerFile => {
                let first_page_id = doc_new_page_ids[0];
                let src_outlines = parse_outline_tree(src_doc, &id_to_page_num, ctx)?;
                let children = filter_outlines_for_pages(&src_outlines, &kept_page_num_to_id);
                let title_str = match naming::doc_stem(source) {
                    Some(s) if !s.is_empty() && s != "document" => s,
                    _ => format!("Document {}", doc_idx + 1),
                };
                merged_outlines.push(RenderedOutlineNode {
                    title: Object::String(title_str.into_bytes(), StringFormat::Literal),
                    target_page_id: first_page_id,
                    dest_tail: default_xyz_tail(),
                    children,
                });
            }
        }

        // Collect page labels according to `params.page_labels`.
        if params.page_labels == MergePageLabelsMode::Keep && any_input_has_labels {
            let src_labels = &parsed_input_labels[doc_idx];
            if !src_labels.contains_key(&0) {
                merged_label_rules.insert(
                    page_offset_0_based,
                    PageLabelRule {
                        style: Some(b"D".to_vec()),
                        prefix: None,
                        start: 1,
                    },
                );
            }
            for (&rel_idx, rule) in src_labels {
                merged_label_rules
                    .insert(page_offset_0_based.saturating_add(rel_idx), rule.clone());
            }
        }

        page_offset_0_based = page_offset_0_based.saturating_add(doc_page_count(source));
    }

    // Apply rotations to `/Pages` and `/Page` dictionaries.
    let inherited_angles: BTreeSet<i64> = all_page_rotations
        .iter()
        .filter(|(_, deg, is_inherited, _)| *is_inherited && *deg != 0)
        .map(|(_, deg, _, _)| *deg)
        .collect();
    let parent_rotate = if inherited_angles.len() == 1 {
        inherited_angles.into_iter().next()
    } else {
        None
    };

    for &(new_page_id, rot_deg, is_inherited, had_explicit_rotate) in &all_page_rotations {
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

    let kids: Vec<Object> = all_new_page_ids
        .iter()
        .copied()
        .map(Object::Reference)
        .collect();
    let mut pages_root_dict = Dictionary::new();
    pages_root_dict.set(b"Type".to_vec(), Object::Name(b"Pages".to_vec()));
    pages_root_dict.set(b"Kids".to_vec(), Object::Array(kids));
    pages_root_dict.set(b"Count".to_vec(), Object::Integer(i64::from(total_pages)));
    if let Some(p_rot) = parent_rotate {
        pages_root_dict.set(b"Rotate".to_vec(), Object::Integer(p_rot));
    }
    target_doc
        .objects
        .insert(pages_root_id, Object::Dictionary(pages_root_dict));

    // Build merged Catalog.
    let mut catalog_dict = Dictionary::new();
    catalog_dict.set(b"Type".to_vec(), Object::Name(b"Catalog".to_vec()));
    catalog_dict.set(b"Pages".to_vec(), Object::Reference(pages_root_id));

    // Copy document-level metadata/language/markinfo from first input.
    let first_src = inputs[0].inner();
    if let Ok(first_cat) = first_src.catalog() {
        let first_pages_ids = collect_pages_tree_ids(first_src, &first_src.get_pages());
        let empty_page_map = BTreeMap::new();
        let mut first_copier = SelectiveCopier::new(
            first_src,
            &mut target_doc,
            &first_pages_ids,
            &empty_page_map,
        );
        for key in [
            &b"Lang"[..],
            &b"Metadata"[..],
            &b"MarkInfo"[..],
            &b"ViewerPreferences"[..],
            &b"PageLayout"[..],
            &b"OutputIntents"[..],
        ] {
            if let Ok(val) = first_cat.get(key) {
                let copied = first_copier.copy_object(val, ctx)?;
                if !matches!(copied, Object::Null) {
                    catalog_dict.set(key.to_vec(), copied);
                }
            }
        }
    }

    // Attach merged `/AcroForm` if any fields survived.
    if !merged_acro_fields.is_empty() {
        let mut acro_dict = Dictionary::new();
        acro_dict.set(b"Fields".to_vec(), Object::Array(merged_acro_fields));
        acro_dict.set(
            b"NeedAppearances".to_vec(),
            Object::Boolean(merged_need_appearances),
        );
        if !merged_acro_dr.is_empty() {
            acro_dict.set(b"DR".to_vec(), Object::Dictionary(merged_acro_dr));
        }
        if let Some(da) = merged_acro_da {
            acro_dict.set(b"DA".to_vec(), da);
        }
        if let Some(q) = merged_acro_q {
            acro_dict.set(b"Q".to_vec(), q);
        }
        let acro_id = target_doc.add_object(Object::Dictionary(acro_dict));
        catalog_dict.set(b"AcroForm".to_vec(), Object::Reference(acro_id));
    }

    // Attach merged `/Names /EmbeddedFiles` and `/AF` if any attachments were present.
    if !merged_embedded_files.is_empty() {
        merged_embedded_files.sort_by(|a, b| a.0.cmp(&b.0));
        let mut names_arr = Vec::with_capacity(merged_embedded_files.len() * 2);
        for (name_bytes, filespec_ref) in merged_embedded_files {
            names_arr.push(Object::String(name_bytes, StringFormat::Literal));
            names_arr.push(filespec_ref);
        }
        let mut ef_tree = Dictionary::new();
        ef_tree.set(b"Names".to_vec(), Object::Array(names_arr));
        let ef_tree_id = target_doc.add_object(Object::Dictionary(ef_tree));

        let mut names_dict = Dictionary::new();
        names_dict.set(b"EmbeddedFiles".to_vec(), Object::Reference(ef_tree_id));
        catalog_dict.set(b"Names".to_vec(), Object::Dictionary(names_dict));
    }
    if !merged_af.is_empty() {
        catalog_dict.set(b"AF".to_vec(), Object::Array(merged_af));
    }

    // Attach merged `/Outlines`.
    if let Some(outlines_id) = write_outline_tree(&mut target_doc, &merged_outlines) {
        catalog_dict.set(b"Outlines".to_vec(), Object::Reference(outlines_id));
    }

    // Attach merged `/PageLabels`.
    match params.page_labels {
        MergePageLabelsMode::Renumber => {
            let entries = vec![(
                0_u32,
                PageLabelRule {
                    style: Some(b"D".to_vec()),
                    prefix: None,
                    start: 1,
                },
            )];
            catalog_dict.set(b"PageLabels".to_vec(), build_page_labels_object(&entries));
        }
        MergePageLabelsMode::Keep => {
            if !merged_label_rules.is_empty() {
                let entries: Vec<(u32, PageLabelRule)> = merged_label_rules.into_iter().collect();
                catalog_dict.set(b"PageLabels".to_vec(), build_page_labels_object(&entries));
            }
        }
    }

    let catalog_id = target_doc.add_object(Object::Dictionary(catalog_dict));
    target_doc
        .trailer
        .set(b"Root".to_vec(), Object::Reference(catalog_id));

    if let Some(info_dict) = resolve_source_info_dict(first_src) {
        let info_id = target_doc.add_object(Object::Dictionary(info_dict));
        target_doc
            .trailer
            .set(b"Info".to_vec(), Object::Reference(info_id));
    }

    let mut merged_pdf = inputs[0].clone();
    *merged_pdf.inner_mut()? = target_doc;
    if let Some(first_stem) = naming::doc_stem(&inputs[0]) {
        naming::set_doc_stem(&mut merged_pdf, &first_stem);
    }
    Ok(vec![OutputDoc::unnamed_pdf(merged_pdf)])
}

#[allow(clippy::too_many_arguments)]
fn collect_input_acroform(
    src_doc: &Document,
    src_catalog: &Dictionary,
    kept_annot_ids: &BTreeMap<ObjectId, Vec<ObjectId>>,
    seen_field_names: &mut BTreeSet<Vec<u8>>,
    merged_fields: &mut Vec<Object>,
    merged_dr: &mut Dictionary,
    merged_need_appearances: &mut bool,
    merged_da: &mut Option<Object>,
    merged_q: &mut Option<Object>,
    copier: &mut SelectiveCopier<'_>,
    ctx: &ToolCtx,
) -> Result<(), PdfError> {
    ctx.check()?;
    let Ok(acro_obj) = src_catalog.get(b"AcroForm") else {
        return Ok(());
    };
    let Some(acro_dict) = resolve_dict(src_doc, acro_obj) else {
        return Ok(());
    };

    if acro_dict
        .get(b"NeedAppearances")
        .and_then(Object::as_bool)
        .ok()
        == Some(true)
    {
        *merged_need_appearances = true;
    }
    if merged_da.is_none()
        && let Ok(da) = acro_dict.get(b"DA")
    {
        *merged_da = Some(copier.copy_object(da, ctx)?);
    }
    if merged_q.is_none()
        && let Ok(q) = acro_dict.get(b"Q")
    {
        *merged_q = Some(copier.copy_object(q, ctx)?);
    }

    if let Ok(dr_obj) = acro_dict.get(b"DR")
        && let Some(dr_dict) = resolve_dict(src_doc, dr_obj)
    {
        let copied_dr = copier.copy_dictionary(dr_dict, ctx)?;
        for (k, v) in copied_dr.iter() {
            if let (Ok(Object::Dictionary(existing_sub)), Object::Dictionary(new_sub)) =
                (merged_dr.get_mut(k.as_slice()), v)
            {
                for (sub_k, sub_v) in new_sub.iter() {
                    if !existing_sub.has(sub_k.as_slice()) {
                        existing_sub.set(sub_k.clone(), sub_v.clone());
                    }
                }
            } else if !merged_dr.has(k.as_slice()) {
                merged_dr.set(k.clone(), v.clone());
            }
        }
    }

    let Ok(fields_obj) = acro_dict.get(b"Fields") else {
        return Ok(());
    };
    let Some(Object::Array(fields)) = resolve_object(src_doc, fields_obj) else {
        return Ok(());
    };

    let mut visited_fields = BTreeSet::new();
    for field_entry in fields {
        ctx.check()?;
        let Ok(src_field_id) = field_entry.as_reference() else {
            continue;
        };
        if !visited_fields.insert(src_field_id) {
            continue;
        }
        let target_field_id = if let Some(mapped_ids) = kept_annot_ids.get(&src_field_id) {
            mapped_ids.first().copied()
        } else if let Some(field_dict) = resolve_dict(src_doc, field_entry)
            && let Ok(kids_obj) = field_dict.get(b"Kids")
            && let Some(Object::Array(kids)) = resolve_object(src_doc, kids_obj)
        {
            let mut surviving_kids = Vec::new();
            let mut seen_kids = BTreeSet::new();
            for kid in kids {
                ctx.check()?;
                if let Ok(src_kid_id) = kid.as_reference()
                    && seen_kids.insert(src_kid_id)
                    && let Some(mapped_kid_ids) = kept_annot_ids.get(&src_kid_id)
                    && let Some(&first_kid_id) = mapped_kid_ids.first()
                {
                    surviving_kids.push(Object::Reference(first_kid_id));
                }
            }
            if !surviving_kids.is_empty() {
                let new_parent_id = copier.copy_id(src_field_id, ctx)?;
                if let Some(Object::Dictionary(parent_dict)) =
                    copier.target.objects.get_mut(&new_parent_id)
                {
                    parent_dict.set(b"Kids".to_vec(), Object::Array(surviving_kids));
                }
                Some(new_parent_id)
            } else {
                None
            }
        } else {
            None
        };

        if let Some(fid) = target_field_id {
            disambiguate_field_name(copier.target, fid, seen_field_names);
            merged_fields.push(Object::Reference(fid));
        }
    }

    Ok(())
}

fn disambiguate_field_name(
    target_doc: &mut Document,
    field_id: ObjectId,
    seen_field_names: &mut BTreeSet<Vec<u8>>,
) {
    let Some(Object::Dictionary(dict)) = target_doc.objects.get_mut(&field_id) else {
        return;
    };
    let Ok(Object::String(name_bytes, fmt)) = dict.get(b"T").cloned() else {
        return;
    };
    if seen_field_names.insert(name_bytes.clone()) {
        return;
    }
    let base = String::from_utf8_lossy(&name_bytes);
    let mut counter = 2_u32;
    loop {
        let candidate = format!("{base}_{counter}").into_bytes();
        if seen_field_names.insert(candidate.clone()) {
            dict.set(b"T".to_vec(), Object::String(candidate, fmt));
            break;
        }
        counter = counter.saturating_add(1);
    }
}

fn collect_input_attachments(
    src_doc: &Document,
    src_catalog: &Dictionary,
    seen_attachment_names: &mut BTreeSet<Vec<u8>>,
    merged_embedded_files: &mut Vec<(Vec<u8>, Object)>,
    merged_af: &mut Vec<Object>,
    copier: &mut SelectiveCopier<'_>,
    ctx: &ToolCtx,
) -> Result<(), PdfError> {
    if let Ok(names_obj) = src_catalog.get(b"Names")
        && let Some(names_dict) = resolve_dict(src_doc, names_obj)
        && let Ok(ef_obj) = names_dict.get(b"EmbeddedFiles")
        && let Some(ef_dict) = resolve_dict(src_doc, ef_obj)
    {
        let mut visited = BTreeSet::new();
        if let Ok(root_id) = ef_obj.as_reference() {
            visited.insert(root_id);
        }
        collect_name_tree_entries(
            src_doc,
            ef_dict,
            &mut visited,
            seen_attachment_names,
            merged_embedded_files,
            copier,
            0,
            ctx,
        )?;
    }

    if let Ok(af_obj) = src_catalog.get(b"AF")
        && let Some(Object::Array(af_items)) = resolve_object(src_doc, af_obj)
    {
        for item in af_items {
            ctx.check()?;
            let copied = copier.copy_object(item, ctx)?;
            if !matches!(copied, Object::Null) && !merged_af.contains(&copied) {
                merged_af.push(copied);
            }
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn collect_name_tree_entries(
    src_doc: &Document,
    node_dict: &Dictionary,
    visited: &mut BTreeSet<ObjectId>,
    seen_names: &mut BTreeSet<Vec<u8>>,
    out: &mut Vec<(Vec<u8>, Object)>,
    copier: &mut SelectiveCopier<'_>,
    depth: usize,
    ctx: &ToolCtx,
) -> Result<(), PdfError> {
    ctx.check()?;
    if depth >= 32 {
        return Ok(());
    }
    if let Ok(names_obj) = node_dict.get(b"Names")
        && let Some(Object::Array(pairs)) = resolve_object(src_doc, names_obj)
    {
        for pair in pairs.as_chunks::<2>().0 {
            ctx.check()?;
            let Some(name_bytes) = resolve_object(src_doc, &pair[0])
                .and_then(|o| o.as_str().ok())
                .map(<[u8]>::to_vec)
            else {
                continue;
            };
            let unique_name = disambiguate_attachment_name(&name_bytes, seen_names);
            let copied_filespec = copier.copy_object(&pair[1], ctx)?;
            out.push((unique_name, copied_filespec));
        }
    }
    if let Ok(kids_obj) = node_dict.get(b"Kids")
        && let Some(Object::Array(kids)) = resolve_object(src_doc, kids_obj)
    {
        for kid in kids {
            ctx.check()?;
            if let Ok(kid_id) = kid.as_reference()
                && !visited.insert(kid_id)
            {
                continue;
            }
            if let Some(kid_dict) = resolve_dict(src_doc, kid) {
                collect_name_tree_entries(
                    src_doc,
                    kid_dict,
                    visited,
                    seen_names,
                    out,
                    copier,
                    depth + 1,
                    ctx,
                )?;
            }
        }
    }
    Ok(())
}

fn disambiguate_attachment_name(raw: &[u8], seen: &mut BTreeSet<Vec<u8>>) -> Vec<u8> {
    if seen.insert(raw.to_vec()) {
        return raw.to_vec();
    }
    let name_str = String::from_utf8_lossy(raw);
    let (stem, ext) = match name_str.rfind('.') {
        Some(pos) if pos > 0 => (&name_str[..pos], &name_str[pos..]),
        _ => (name_str.as_ref(), ""),
    };
    let mut counter = 2_u32;
    loop {
        let candidate = format!("{stem}_{counter}{ext}").into_bytes();
        if seen.insert(candidate.clone()) {
            return candidate;
        }
        counter = counter.saturating_add(1);
    }
}
