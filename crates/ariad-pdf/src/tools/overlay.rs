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
        nup::{
            append_clipped_xobject, charge_page_streams, collect_ordered_page_ids,
            convert_page_with_budget, effective_version,
        },
    },
    xobject::{self, FormXObject, Matrix, ObjectCopier},
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
    let ov_pdf = &inputs[1];
    let ov_doc = ov_pdf.inner();

    let base_pages = collect_ordered_page_ids(base_pdf.inner(), ctx)?;
    let ov_pages = collect_ordered_page_ids(ov_doc, ctx)?;

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
    let ov_ver = effective_version(ov_doc);
    let merged_ver = if ov_ver > base_ver { ov_ver } else { base_ver };

    let target_doc = out_pdf.inner_mut()?;
    target_doc.version = merged_ver;

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

        charge_page_streams(target_doc, base_page_id, &mut base_budget, ctx)?;
        let base_form = xobject::page_to_form_xobject_in_place(target_doc, base_page_id, ctx)?;

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

        let relative_rotation = (ov_form.rotation - base_form.rotation).rem_euclid(360);
        let ov_matrix = xobject::placement_matrix(
            ov_form.crop_box,
            relative_rotation,
            base_form.crop_box,
            ScaleFit::Contain,
        );

        let mut content_buf = Vec::new();
        match params.position {
            OverlayPosition::Over => {
                append_clipped_xobject(
                    &mut content_buf,
                    base_form.crop_box,
                    "FmBase",
                    Matrix::identity(),
                );
                append_clipped_xobject(
                    &mut content_buf,
                    base_form.crop_box,
                    "FmOverlay",
                    ov_matrix,
                );
            }
            OverlayPosition::Under => {
                append_clipped_xobject(
                    &mut content_buf,
                    base_form.crop_box,
                    "FmOverlay",
                    ov_matrix,
                );
                append_clipped_xobject(
                    &mut content_buf,
                    base_form.crop_box,
                    "FmBase",
                    Matrix::identity(),
                );
            }
        }

        let mut content_stream = Stream::new(Dictionary::new(), content_buf);
        let _ = content_stream.compress();
        let content_id = target_doc.add_object(Object::Stream(content_stream));

        let xobjects = dictionary! {
            "FmBase" => Object::Reference(base_form.id),
            "FmOverlay" => Object::Reference(ov_form.id),
        };
        let new_resources = dictionary! {
            "XObject" => Object::Dictionary(xobjects),
        };

        let Ok(page_dict) = target_doc.get_dictionary_mut(base_page_id) else {
            return Err(PdfError::Malformed);
        };
        page_dict.set("Resources", Object::Dictionary(new_resources));
        page_dict.set("Contents", Object::Reference(content_id));
    }

    Ok(vec![OutputDoc::unnamed_pdf(out_pdf)])
}
