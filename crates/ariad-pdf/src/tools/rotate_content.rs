//! Implementation of the `pdf.rotate_content` tool.
//!
//! Rotates selected page content streams by an arbitrary clockwise angle `angle_deg` (`-360..=360`)
//! using a Form XObject and a 2D affine transformation matrix, expanding the `/MediaBox` when
//! `expand == true` so rotated corners are not clipped.

use ariad_core::tools::{
    page_range::DuplicatePolicy,
    params::{rotate_content::RotateContentParams, scale::ScaleFit},
};
use lopdf::{Object, Stream, dictionary};

use crate::{
    document::{PdfDoc, ToolCtx},
    error::PdfError,
    paper::MAX_PAPER_SIDE_PT,
    tools::{
        OutputDoc,
        rotate::{collect_page_ids, materialize_inferred_rotations},
        scale::{apply_transformed_page_state, charge_page_contents, transform_page_annotations},
    },
    xobject::{self, PdfRect},
};

/// Rotates page content streams by an arbitrary angle via Form XObject placement matrices.
pub fn run(
    inputs: &[PdfDoc],
    params: &RotateContentParams,
    ctx: &ToolCtx,
) -> Result<Vec<OutputDoc>, PdfError> {
    ctx.check()?;
    if inputs.len() != 1 {
        return Err(PdfError::InvalidParams(format!(
            "pdf.rotate_content requires exactly 1 input document, got {}",
            inputs.len()
        )));
    }
    if !params.angle_deg.is_finite() || !(-360.0..=360.0).contains(&params.angle_deg) {
        return Err(PdfError::InvalidParams(format!(
            "angle_deg ({}) must be a finite number in [-360.0, 360.0]",
            params.angle_deg
        )));
    }

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

    let angle_deg = f64::from(params.angle_deg);

    for page_num in selected_pages {
        ctx.check()?;
        let page_id = page_ids[(page_num - 1) as usize];
        charge_page_contents(inner, page_id, &mut budget, ctx)?;

        let form = xobject::page_to_form_xobject_in_place(inner, page_id, ctx)?;
        let rot = form.rotation.rem_euclid(360);

        let (matrix, new_box) = if rot == 0 {
            xobject::content_rotation_matrix(form.bbox, angle_deg, params.expand)
        } else {
            let (visual_w, visual_h) = if matches!(rot, 90 | 270) {
                (form.bbox.height(), form.bbox.width())
            } else {
                (form.bbox.width(), form.bbox.height())
            };
            let upright_rect = PdfRect::from_size(visual_w, visual_h);
            let upright_matrix =
                xobject::placement_matrix(form.bbox, rot, upright_rect, ScaleFit::Stretch);
            let (rot_matrix, rotated_box) =
                xobject::content_rotation_matrix(upright_rect, angle_deg, params.expand);
            (upright_matrix.then(rot_matrix), rotated_box)
        };

        if !new_box.width().is_finite()
            || !new_box.height().is_finite()
            || new_box.width() <= 0.0
            || new_box.height() <= 0.0
            || new_box.width() > MAX_PAPER_SIDE_PT
            || new_box.height() > MAX_PAPER_SIDE_PT
        {
            return Err(PdfError::InvalidParams(format!(
                "rotated page box ({} x {} pt) exceeds allowed bounds",
                new_box.width(),
                new_box.height()
            )));
        }

        let content_bytes = xobject::render_place_xobject(b"Fm0", matrix);
        let mut content_stream = Stream::new(dictionary! {}, content_bytes);
        let _ = content_stream.compress();
        let content_id = inner.add_object(Object::Stream(content_stream));

        transform_page_annotations(inner, page_id, matrix, None, ctx)?;
        apply_transformed_page_state(inner, page_id, new_box, form.id, content_id);
    }

    Ok(vec![OutputDoc::unnamed_pdf(doc)])
}
