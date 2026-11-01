//! Implementation of the `pdf.alternate_mix` tool.

use std::collections::{HashMap, HashSet};

use ariad_core::{reader::pdf::ReadError, tools::params::alternate_mix::AlternateMixParams};
use lopdf::{Document, Object, ObjectId};

use crate::{
    document::{PdfDoc, ToolCtx},
    error::PdfError,
    gc,
    tools::{
        OutputDoc,
        reorder::{self, OutlineNode},
    },
    xobject::{self, ObjectCopier},
};

/// Alternately interleaves pages from two PDF documents (`[A1, B1, A2, B2, ...]`), optionally
/// reversing the second document's page sequence for duplex-scan reassembly.
pub fn run(
    inputs: &[PdfDoc],
    params: &AlternateMixParams,
    ctx: &ToolCtx,
) -> Result<Vec<OutputDoc>, PdfError> {
    ctx.check()?;
    if inputs.len() != 2 {
        return Err(PdfError::InvalidParams(format!(
            "pdf.alternate_mix requires exactly 2 input documents, got {}",
            inputs.len()
        )));
    }
    if inputs[1].is_encrypted() {
        return Err(PdfError::Encrypted {
            needs_password: false,
        });
    }

    let mut out_doc = inputs[0].clone();
    let out_inner = out_doc.inner_mut()?;
    let doc1 = inputs[1].inner();

    let doc0_pages = reorder::collect_page_ids(out_inner, ctx)?;
    let doc1_pages = reorder::collect_page_ids(doc1, ctx)?;

    let total_pages = (doc0_pages.len() as u64).saturating_add(doc1_pages.len() as u64);
    let cap = super::page_set::output_page_cap(ctx.max_pages);
    if total_pages > cap {
        return Err(PdfError::LimitExceeded(ReadError::TooManyPages {
            actual: total_pages,
            limit: u32::try_from(cap).unwrap_or(u32::MAX),
        }));
    }

    let doc0_outlines = reorder::extract_outline_tree(out_inner, &doc0_pages, ctx)?;
    let doc1_outlines = reorder::extract_outline_tree(doc1, &doc1_pages, ctx)?;

    for &pid in &doc0_pages {
        reorder::materialize_page_attributes(out_inner, pid, ctx)?;
    }

    let mut copier = ObjectCopier::new();
    let mut doc1_page_map: HashMap<ObjectId, ObjectId> = HashMap::with_capacity(doc1_pages.len());
    let mut copied_doc1_pages: Vec<ObjectId> = Vec::with_capacity(doc1_pages.len());

    for &src_pid in &doc1_pages {
        ctx.check()?;
        let media_box = xobject::resolve_page_media_box(doc1, src_pid);
        let crop_box = xobject::resolve_page_box(doc1, src_pid, b"CropBox");
        let bleed_box = xobject::resolve_page_box(doc1, src_pid, b"BleedBox");
        let trim_box = xobject::resolve_page_box(doc1, src_pid, b"TrimBox");
        let art_box = xobject::resolve_page_box(doc1, src_pid, b"ArtBox");
        let rotation = reorder::effective_page_rotation(doc1, src_pid);
        let merged_res = xobject::resolve_page_resources(doc1, src_pid);

        let dst_pid = copier.copy_id(doc1, out_inner, src_pid, ctx)?;
        let copied_res = copier.copy_dictionary(doc1, out_inner, &merged_res, ctx)?;

        let Ok(page_dict) = out_inner.get_dictionary_mut(dst_pid) else {
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
        } else {
            page_dict.remove(b"Rotate");
        }
        page_dict.set("Resources", Object::Dictionary(copied_res));

        doc1_page_map.insert(src_pid, dst_pid);
        copied_doc1_pages.push(dst_pid);
    }

    if params.reverse_second {
        copied_doc1_pages.reverse();
    }

    let max_len = doc0_pages.len().max(copied_doc1_pages.len());
    let mut mixed_kids = Vec::with_capacity(doc0_pages.len() + copied_doc1_pages.len());
    for i in 0..max_len {
        ctx.check()?;
        if let Some(&p0) = doc0_pages.get(i) {
            mixed_kids.push(p0);
        }
        if let Some(&p1) = copied_doc1_pages.get(i) {
            mixed_kids.push(p1);
        }
    }

    let pages_root = reorder::pages_root_id(out_inner)?;
    for &kid_id in &mixed_kids {
        reorder::set_page_parent(out_inner, kid_id, pages_root);
    }
    reorder::rebuild_pages_root(out_inner, pages_root, &mixed_kids);

    let all_pages_set: HashSet<ObjectId> = mixed_kids.iter().copied().collect();
    let identity_map: HashMap<ObjectId, ObjectId> =
        mixed_kids.iter().copied().map(|id| (id, id)).collect();
    for &kid_id in &mixed_kids {
        ctx.check()?;
        reorder::update_page_annotations(out_inner, kid_id, &all_pages_set, &identity_map, ctx)?;
    }

    merge_second_acroform(doc1, out_inner, &mut copier, ctx)?;
    reorder::prune_acroform_for_kept_pages(out_inner, &mixed_kids, ctx)?;

    let mut combined_outlines =
        reorder::filter_and_remap_outlines(doc0_outlines, &identity_map, ctx)?;
    let mapped_doc1_outlines = copy_and_remap_outlines(
        doc1,
        out_inner,
        doc1_outlines,
        &doc1_page_map,
        &mut copier,
        ctx,
    )?;
    combined_outlines.extend(mapped_doc1_outlines);
    reorder::write_outline_tree(out_inner, &combined_outlines, ctx)?;

    if let Ok(cat_id) = reorder::catalog_id(out_inner)
        && let Ok(cat_dict) = out_inner.get_dictionary_mut(cat_id)
    {
        cat_dict.remove(b"PageLabels");
    }

    gc::collect_document(out_inner, ctx)?;

    Ok(vec![OutputDoc::unnamed_pdf(out_doc)])
}

fn copy_and_remap_outlines(
    source_doc: &Document,
    target_doc: &mut Document,
    nodes: Vec<OutlineNode>,
    page_map: &HashMap<ObjectId, ObjectId>,
    copier: &mut ObjectCopier,
    ctx: &ToolCtx,
) -> Result<Vec<OutlineNode>, PdfError> {
    let mut out = Vec::with_capacity(nodes.len());
    for node in nodes {
        ctx.check()?;
        let children =
            copy_and_remap_outlines(source_doc, target_doc, node.children, page_map, copier, ctx)?;
        let mapped_dest = node.dest_page.and_then(|pid| page_map.get(&pid).copied());
        if let Some(new_pid) = mapped_dest {
            let copied_title = copier.copy_object(source_doc, target_doc, &node.title, ctx)?;
            let mut copied_tail = Vec::with_capacity(node.dest_tail.len());
            for item in &node.dest_tail {
                copied_tail.push(copier.copy_object(source_doc, target_doc, item, ctx)?);
            }
            out.push(OutlineNode {
                title: copied_title,
                dest_page: Some(new_pid),
                dest_tail: copied_tail,
                children,
            });
        } else if !children.is_empty() {
            out.extend(children);
        }
    }
    Ok(out)
}

fn merge_second_acroform(
    source_doc: &Document,
    target_doc: &mut Document,
    copier: &mut ObjectCopier,
    ctx: &ToolCtx,
) -> Result<(), PdfError> {
    ctx.check()?;
    let Ok(src_cat) = source_doc.catalog() else {
        return Ok(());
    };
    let Ok(src_acro_obj) = src_cat.get(b"AcroForm") else {
        return Ok(());
    };
    let src_acro_dict = match src_acro_obj {
        Object::Reference(id) => match source_doc.get_dictionary(*id) {
            Ok(d) => d,
            Err(_) => return Ok(()),
        },
        Object::Dictionary(d) => d,
        _ => return Ok(()),
    };

    let src_fields = match src_acro_dict.get(b"Fields") {
        Ok(Object::Reference(fid)) => {
            match source_doc.get_object(*fid).and_then(Object::as_array) {
                Ok(arr) => arr.clone(),
                Err(_) => return Ok(()),
            }
        }
        Ok(Object::Array(arr)) => arr.clone(),
        _ => return Ok(()),
    };
    if src_fields.is_empty() {
        return Ok(());
    }

    let mut copied_fields = Vec::with_capacity(src_fields.len());
    for field in &src_fields {
        ctx.check()?;
        copied_fields.push(copier.copy_object(source_doc, target_doc, field, ctx)?);
    }

    let cat_id = reorder::catalog_id(target_doc)?;
    let existing_acro = target_doc
        .get_dictionary(cat_id)
        .ok()
        .and_then(|c| c.get(b"AcroForm").ok().cloned());

    match existing_acro {
        Some(Object::Reference(acro_id)) => {
            if let Ok(mut acro_dict) = target_doc.get_dictionary(acro_id).cloned() {
                let mut combined = match acro_dict.get(b"Fields").cloned() {
                    Ok(Object::Reference(fid)) => target_doc
                        .get_object(fid)
                        .and_then(Object::as_array)
                        .cloned()
                        .unwrap_or_default(),
                    Ok(Object::Array(arr)) => arr,
                    _ => Vec::new(),
                };
                combined.extend(copied_fields);
                acro_dict.set("Fields", Object::Array(combined));
                target_doc
                    .objects
                    .insert(acro_id, Object::Dictionary(acro_dict));
            }
        }
        Some(Object::Dictionary(mut acro_dict)) => {
            let mut combined = match acro_dict.get(b"Fields").cloned() {
                Ok(Object::Reference(fid)) => target_doc
                    .get_object(fid)
                    .and_then(Object::as_array)
                    .cloned()
                    .unwrap_or_default(),
                Ok(Object::Array(arr)) => arr,
                _ => Vec::new(),
            };
            combined.extend(copied_fields);
            acro_dict.set("Fields", Object::Array(combined));
            if let Ok(cat_mut) = target_doc.get_dictionary_mut(cat_id) {
                cat_mut.set("AcroForm", Object::Dictionary(acro_dict));
            }
        }
        _ => {
            let copied_acro = copier.copy_dictionary(source_doc, target_doc, src_acro_dict, ctx)?;
            let new_acro_id = target_doc.add_object(Object::Dictionary(copied_acro));
            if let Ok(cat_mut) = target_doc.get_dictionary_mut(cat_id) {
                cat_mut.set("AcroForm", Object::Reference(new_acro_id));
            }
        }
    }

    Ok(())
}
