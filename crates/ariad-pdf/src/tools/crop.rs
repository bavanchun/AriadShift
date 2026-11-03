//! Implementation of the `pdf.crop` tool.
//!
//! Sets `/CropBox` or `/MediaBox` on selected pages of a PDF document from an explicit
//! `{ left, bottom, right, top }` rectangle in PDF points.

use ariad_core::tools::{
    page_range::DuplicatePolicy,
    params::crop::{CropAutoMode, CropBoxSpec, CropParams, CropRect, CropTarget},
};
use lopdf::{Document, Object, ObjectId};

use crate::{
    document::{PdfDoc, ToolCtx},
    error::PdfError,
    paper::MAX_PAPER_SIDE_PT,
    tools::{OutputDoc, rotate::collect_page_ids},
    xobject::{self, PdfRect},
};

/// Minimum cropped page width or height in PDF points.
const MIN_CROP_SIDE_PT: f64 = 1.0;

/// Sets `/CropBox` or `/MediaBox` on selected pages of a PDF document.
pub fn run(
    inputs: &[PdfDoc],
    params: &CropParams,
    ctx: &ToolCtx,
) -> Result<Vec<OutputDoc>, PdfError> {
    ctx.check()?;
    if inputs.len() != 1 {
        return Err(PdfError::InvalidParams(format!(
            "pdf.crop requires exactly 1 input document, got {}",
            inputs.len()
        )));
    }

    let CropRect {
        left,
        bottom,
        right,
        top,
    } = match params.r#box {
        CropBoxSpec::Box(rect) => rect,
        CropBoxSpec::Auto(CropAutoMode::TrimWhitespaceLater) => {
            return Err(PdfError::InvalidParams(
                "whitespace auto-crop (`trim-whitespace-later`) requires rendering; supply explicit `{left, bottom, right, top}` coordinates in points".to_owned(),
            ));
        }
    };

    if !left.is_finite()
        || !bottom.is_finite()
        || !right.is_finite()
        || !top.is_finite()
        || left.abs() > MAX_PAPER_SIDE_PT
        || bottom.abs() > MAX_PAPER_SIDE_PT
        || right.abs() > MAX_PAPER_SIDE_PT
        || top.abs() > MAX_PAPER_SIDE_PT
    {
        return Err(PdfError::InvalidParams(
            "crop box coordinates must be finite numbers in [-1000000, 1000000] points".to_owned(),
        ));
    }

    let width = right - left;
    let height = top - bottom;
    if width < MIN_CROP_SIDE_PT
        || height < MIN_CROP_SIDE_PT
        || width > MAX_PAPER_SIDE_PT
        || height > MAX_PAPER_SIDE_PT
    {
        return Err(PdfError::InvalidParams(format!(
            "invalid crop box [{left}, {bottom}, {right}, {top}]: right must exceed left and top must exceed bottom by at least 1 pt"
        )));
    }

    let crop_rect = PdfRect {
        left,
        bottom,
        right,
        top,
    };

    let mut doc = inputs[0].clone();
    let page_ids = collect_page_ids(doc.inner(), ctx)?;
    let page_count = u32::try_from(page_ids.len()).unwrap_or(u32::MAX);

    let selected_pages = params
        .pages
        .expand(page_count, DuplicatePolicy::Reject)
        .map_err(|e| PdfError::InvalidParams(e.to_string()))?;

    let inner = doc.inner_mut()?;
    let box_array = Object::Array(crop_rect.to_pdf_array());

    for page_num in selected_pages {
        ctx.check()?;
        let page_id = page_ids[(page_num - 1) as usize];
        match params.target {
            CropTarget::Crop => {
                let media = xobject::resolve_page_media_box(inner, page_id);
                let ix_left = crop_rect.left.max(media.left);
                let ix_bottom = crop_rect.bottom.max(media.bottom);
                let ix_right = crop_rect.right.min(media.right);
                let ix_top = crop_rect.top.min(media.top);
                if ix_right - ix_left < MIN_CROP_SIDE_PT || ix_top - ix_bottom < MIN_CROP_SIDE_PT {
                    return Err(PdfError::InvalidParams(format!(
                        "crop box [{left}, {bottom}, {right}, {top}] has empty or sub-point intersection with page {page_num} MediaBox [{}, {}, {}, {}]",
                        media.left, media.bottom, media.right, media.top
                    )));
                }
                if let Ok(page_dict) = inner.get_object_mut(page_id).and_then(Object::as_dict_mut) {
                    page_dict.set("CropBox", box_array.clone());
                }
            }
            CropTarget::Media => {
                let ancestor_has_crop = ancestor_has_key(inner, page_id, b"CropBox");
                if let Ok(page_dict) = inner.get_object_mut(page_id).and_then(Object::as_dict_mut) {
                    page_dict.set("MediaBox", box_array.clone());
                    if ancestor_has_crop || page_dict.has(b"CropBox") {
                        page_dict.set("CropBox", box_array.clone());
                    }
                    page_dict.remove(b"TrimBox");
                    page_dict.remove(b"BleedBox");
                    page_dict.remove(b"ArtBox");
                }
            }
        }
    }

    Ok(vec![OutputDoc::unnamed_pdf(doc)])
}

pub(crate) fn ancestor_has_key(doc: &Document, page_id: ObjectId, key: &[u8]) -> bool {
    let mut current = doc
        .get_dictionary(page_id)
        .ok()
        .and_then(|d| d.get(b"Parent").ok())
        .and_then(|o| o.as_reference().ok());
    for _ in 0..64 {
        let Some(id) = current else {
            break;
        };
        let Ok(dict) = doc.get_dictionary(id) else {
            break;
        };
        if dict.has(key) {
            return true;
        }
        current = dict.get(b"Parent").ok().and_then(|o| o.as_reference().ok());
    }
    false
}
