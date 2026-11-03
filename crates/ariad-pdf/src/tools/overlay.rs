//! Implementation of the `pdf.overlay` tool.

use std::collections::HashMap;

use ariad_core::tools::{
    page_range::DuplicatePolicy,
    params::{
        overlay::{OverlayPagesMode, OverlayParams, OverlayPosition},
        scale::ScaleFit,
    },
};
use lopdf::{Dictionary, Object, ObjectId, Stream, dictionary};

use crate::{
    document::{PdfDoc, ToolCtx},
    error::PdfError,
    tools::{
        OutputDoc,
        crop::ancestor_has_key,
        nup::{
            append_clipped_xobject, collect_ordered_page_ids, convert_page_with_budget,
            effective_version,
        },
        rotate::materialize_inferred_rotations,
        scale::transform_page_annotations,
    },
    xobject::{self, FormXObject, Matrix, ObjectCopier, PdfRect},
};

/// Overlays or underlays pages from a second PDF onto selected pages of a base PDF.
pub fn run(
    inputs: &[PdfDoc],
    params: &OverlayParams,
    ctx: &ToolCtx,
) -> Result<Vec<OutputDoc>, PdfError> {
    ctx.check()?;
    if inputs.len() != 2 {
        return Err(PdfError::InvalidParams(format!(
            "pdf.overlay requires exactly 2 input documents (base and overlay), got {}",
            inputs.len()
        )));
    }

    let base_pdf = &inputs[0];
    let mut ov_pdf = inputs[1].clone();

    let base_pages = collect_ordered_page_ids(base_pdf.inner(), ctx)?;
    let ov_pages = collect_ordered_page_ids(ov_pdf.inner(), ctx)?;

    let base_count = u32::try_from(base_pages.len()).unwrap_or(u32::MAX);
    let selected = params
        .pages
        .expand(base_count, DuplicatePolicy::Reject)
        .map_err(|e| PdfError::InvalidParams(e.to_string()))?;

    let mut out_pdf = base_pdf.clone();
    let mut base_budget = base_pdf.decode_budget();
    let mut ov_budget = ov_pdf.decode_budget();
    let mut copier = ObjectCopier::new();
    let mut ov_form_cache: HashMap<ObjectId, FormXObject> = HashMap::new();

    let base_ver = effective_version(base_pdf.inner());
    let ov_ver = effective_version(ov_pdf.inner());
    let merged_ver = if ov_ver > base_ver { ov_ver } else { base_ver };

    let ov_doc_mut = ov_pdf.inner_mut()?;
    materialize_inferred_rotations(ov_doc_mut, &ov_pages, ctx)?;
    let ov_doc = ov_pdf.inner();

    let target_doc = out_pdf.inner_mut()?;
    target_doc.version = merged_ver;
    materialize_inferred_rotations(target_doc, &base_pages, ctx)?;

    for (sel_idx, page_num) in selected.iter().copied().enumerate() {
        ctx.check()?;
        let base_idx = (page_num as usize) - 1;
        let ov_idx_opt = match params.overlay_pages {
            OverlayPagesMode::First => Some(0),
            OverlayPagesMode::Cycle => Some(sel_idx % ov_pages.len()),
            OverlayPagesMode::Matching => (base_idx < ov_pages.len()).then_some(base_idx),
        };
        let Some(ov_idx) = ov_idx_opt else {
            continue;
        };

        let base_page_id = base_pages[base_idx];
        let ov_page_id = ov_pages[ov_idx];

        let mut new_resources = xobject::resolve_page_resources(target_doc, base_page_id);
        let base_form = xobject::page_to_form_xobject_in_place(
            target_doc,
            base_page_id,
            &mut base_budget,
            ctx,
        )?;

        let ov_form = if let Some(cached) = ov_form_cache.get(&ov_page_id).copied() {
            cached
        } else {
            let converted = convert_page_with_budget(
                ov_doc,
                ov_page_id,
                target_doc,
                &mut copier,
                &mut ov_budget,
                ctx,
            )?;
            ov_form_cache.insert(ov_page_id, converted);
            converted
        };

        let base_rot = base_form.rotation.rem_euclid(360);
        let (target_rect, base_matrix) = if base_rot == 0 {
            (base_form.crop_box, Matrix::identity())
        } else {
            let (visual_w, visual_h) = if matches!(base_rot, 90 | 270) {
                (base_form.crop_box.height(), base_form.crop_box.width())
            } else {
                (base_form.crop_box.width(), base_form.crop_box.height())
            };
            let upright_rect = PdfRect::from_size(visual_w, visual_h);
            let mat = xobject::placement_matrix(
                base_form.crop_box,
                base_rot,
                upright_rect,
                ScaleFit::Stretch,
            );
            (upright_rect, mat)
        };

        let ov_matrix = xobject::placement_matrix(
            ov_form.crop_box,
            ov_form.rotation,
            target_rect,
            ScaleFit::Contain,
        );

        let mut content_buf = Vec::new();
        match params.position {
            OverlayPosition::Over => {
                append_clipped_xobject(&mut content_buf, target_rect, "FmBase", base_matrix);
                append_clipped_xobject(&mut content_buf, target_rect, "FmOverlay", ov_matrix);
            }
            OverlayPosition::Under => {
                append_clipped_xobject(&mut content_buf, target_rect, "FmOverlay", ov_matrix);
                append_clipped_xobject(&mut content_buf, target_rect, "FmBase", base_matrix);
            }
        }

        let mut content_stream = Stream::new(Dictionary::new(), content_buf);
        let _ = content_stream.compress();
        let content_id = target_doc.add_object(Object::Stream(content_stream));

        if base_rot != 0 {
            transform_page_annotations(target_doc, base_page_id, base_matrix, None, ctx)?;
        }
        let ancestor_has_crop = ancestor_has_key(target_doc, base_page_id, b"CropBox");
        let ancestor_has_rotate = ancestor_has_key(target_doc, base_page_id, b"Rotate");

        let xobjects = dictionary! {
            "FmBase" => Object::Reference(base_form.id),
            "FmOverlay" => Object::Reference(ov_form.id),
        };
        new_resources.set("XObject", Object::Dictionary(xobjects));

        let Ok(page_dict) = target_doc.get_dictionary_mut(base_page_id) else {
            return Err(PdfError::Malformed);
        };
        if base_rot != 0 {
            let media_arr = Object::Array(target_rect.to_pdf_array());
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
        }
        page_dict.set("Resources", Object::Dictionary(new_resources));
        page_dict.set("Contents", Object::Reference(content_id));
    }

    Ok(vec![OutputDoc::unnamed_pdf(out_pdf)])
}
