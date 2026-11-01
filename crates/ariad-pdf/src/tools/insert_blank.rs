//! Implementation of the `pdf.insert_blank` tool.

use std::collections::{HashMap, HashSet};

use ariad_core::{
    reader::pdf::ReadError,
    tools::{
        page_range::DuplicatePolicy,
        params::insert_blank::{BlankPageSize, InsertBlankParams},
    },
};
use lopdf::{Document, Object, ObjectId, Stream, dictionary};

use crate::{
    document::{PdfDoc, ToolCtx},
    error::PdfError,
    gc,
    paper::resolve_paper_size,
    tools::{
        OutputDoc,
        page_set::{PageLabelRule, build_page_labels_object, parse_page_labels},
        reorder,
    },
    xobject::{self, PdfRect},
};

/// Inserts `params.count` blank pages after every page selected by `params.after`.
pub fn run(
    inputs: &[PdfDoc],
    params: &InsertBlankParams,
    ctx: &ToolCtx,
) -> Result<Vec<OutputDoc>, PdfError> {
    ctx.check()?;
    if inputs.len() != 1 {
        return Err(PdfError::InvalidParams(format!(
            "pdf.insert_blank requires exactly 1 input document, got {}",
            inputs.len()
        )));
    }
    if params.count == 0 {
        return Err(PdfError::InvalidParams(
            "insert_blank count must be at least 1".to_owned(),
        ));
    }

    let explicit_paper = match &params.size {
        BlankPageSize::Same(_) => None,
        BlankPageSize::Paper(spec) => Some(resolve_paper_size(spec)?),
    };

    let mut out_doc = inputs[0].clone();
    let inner = out_doc.inner_mut()?;
    let orig_pages = reorder::collect_page_ids(inner, ctx)?;
    let page_count = u32::try_from(orig_pages.len()).map_err(|_| PdfError::Malformed)?;

    let after_pages = params
        .after
        .expand(page_count, DuplicatePolicy::Reject)
        .map_err(|e| PdfError::InvalidParams(e.to_string()))?;
    let after_set: HashSet<u32> = after_pages.into_iter().collect();

    let inserted_total = (after_set.len() as u64).saturating_mul(u64::from(params.count));
    let total_output_pages = (orig_pages.len() as u64).saturating_add(inserted_total);
    if total_output_pages > reorder::MAX_OUTPUT_PAGES {
        return Err(PdfError::LimitExceeded(ReadError::TooManyPages {
            actual: total_output_pages,
            limit: reorder::MAX_OUTPUT_PAGES as u32,
        }));
    }

    let outline_tree = reorder::extract_outline_tree(inner, &orig_pages, ctx)?;
    let pages_root = reorder::pages_root_id(inner)?;

    for &pid in &orig_pages {
        reorder::materialize_page_attributes(inner, pid, ctx)?;
    }

    let blank_stream_id = inner.add_object(Object::Stream(Stream::new(dictionary! {}, Vec::new())));

    let mut new_kids: Vec<ObjectId> = Vec::with_capacity(total_output_pages as usize);
    let mut orig_to_new_idx: Vec<usize> = Vec::with_capacity(orig_pages.len());

    for (idx, &page_id) in orig_pages.iter().enumerate() {
        ctx.check()?;
        let page_num = (idx as u32) + 1;
        orig_to_new_idx.push(new_kids.len());
        reorder::set_page_parent(inner, page_id, pages_root);
        new_kids.push(page_id);

        if after_set.contains(&page_num) {
            let (media_box, rotation) = match explicit_paper {
                Some(dims) => (PdfRect::from_size(dims.width_pt, dims.height_pt), 0),
                None => {
                    let crop = xobject::resolve_page_crop_box(inner, page_id);
                    let rot = reorder::effective_page_rotation(inner, page_id);
                    (PdfRect::from_size(crop.width(), crop.height()), rot)
                }
            };

            for _ in 0..params.count {
                ctx.check()?;
                let mut blank_dict = dictionary! {
                    "Type" => "Page",
                    "Parent" => Object::Reference(pages_root),
                    "MediaBox" => media_box.to_pdf_array(),
                    "Resources" => dictionary! {},
                    "Contents" => Object::Reference(blank_stream_id),
                };
                if rotation != 0 {
                    blank_dict.set("Rotate", Object::Integer(i64::from(rotation)));
                }
                let blank_id = inner.add_object(Object::Dictionary(blank_dict));
                new_kids.push(blank_id);
            }
        }
    }

    reorder::rebuild_pages_root(inner, pages_root, &new_kids);

    let orig_page_set: HashSet<ObjectId> = orig_pages.iter().copied().collect();
    let identity_map: HashMap<ObjectId, ObjectId> =
        orig_pages.iter().copied().map(|id| (id, id)).collect();
    for &pid in &orig_pages {
        ctx.check()?;
        reorder::update_page_annotations(inner, pid, &orig_page_set, &identity_map, ctx)?;
    }

    let remapped_outlines = reorder::filter_and_remap_outlines(outline_tree, &identity_map, ctx)?;
    reorder::write_outline_tree(inner, &remapped_outlines, ctx)?;

    shift_page_labels(inner, &orig_to_new_idx, ctx)?;

    gc::collect_document(inner, ctx)?;

    Ok(vec![OutputDoc::unnamed_pdf(out_doc)])
}

fn shift_page_labels(
    doc: &mut Document,
    orig_to_new_idx: &[usize],
    ctx: &ToolCtx,
) -> Result<(), PdfError> {
    let src_labels = parse_page_labels(doc, ctx)?;
    if src_labels.is_empty() {
        return Ok(());
    }
    let mut shifted: Vec<(u32, PageLabelRule)> = Vec::with_capacity(src_labels.len());
    for (old_idx, rule) in src_labels {
        if let Some(&new_idx) = orig_to_new_idx.get(old_idx as usize)
            && let Ok(new_idx_u32) = u32::try_from(new_idx)
        {
            shifted.push((new_idx_u32, rule));
        }
    }
    let Ok(cat_id) = reorder::catalog_id(doc) else {
        return Ok(());
    };
    if shifted.is_empty() {
        if let Ok(cat_mut) = doc.get_dictionary_mut(cat_id) {
            cat_mut.remove(b"PageLabels");
        }
        return Ok(());
    }
    let pl_obj = build_page_labels_object(&shifted);
    let existing_pl_id = doc
        .get_dictionary(cat_id)
        .ok()
        .and_then(|c| c.get(b"PageLabels").ok())
        .and_then(|o| o.as_reference().ok());
    if let Some(pl_id) = existing_pl_id {
        doc.objects.insert(pl_id, pl_obj);
    } else if let Ok(cat_mut) = doc.get_dictionary_mut(cat_id) {
        cat_mut.set("PageLabels", pl_obj);
    }
    Ok(())
}
