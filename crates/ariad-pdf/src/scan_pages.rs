//! Rebuild pages from selected scan regions without re-encoding page images.
use crate::{
    PdfDoc, PdfError, ToolCtx,
    tools::{
        OutputDoc, OutputPayload,
        page_set::{PageSetOptions, build_page_set_output, output_page_cap},
    },
    xobject,
};
use lopdf::Object;

/// A source page and an optional rectangle in upright, normalized image coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScanPageRegion {
    /// One-based source page number.
    pub page: u32,
    /// Top-left and exclusive bottom-right coordinates, each in `[0, 1]`.
    /// `None` keeps the whole page.
    pub rect: Option<[f64; 4]>,
}

/// Duplicate and crop pages in the given order, preserving original image streams.
/// Page-set warnings, including those about tagged structure, remain attached to the result.
pub fn split_regions(
    source: &PdfDoc,
    regions: &[ScanPageRegion],
    ctx: &ToolCtx,
) -> Result<OutputDoc, PdfError> {
    ctx.check()?;
    let cap = output_page_cap(ctx.max_pages);
    if regions.len() as u64 > cap {
        return Err(PdfError::LimitExceeded(
            ariad_core::reader::pdf::ReadError::TooManyPages {
                actual: regions.len() as u64,
                limit: u32::try_from(cap).unwrap_or(u32::MAX),
            },
        ));
    }
    for region in regions {
        ctx.check()?;
        if let Some([x0, y0, x1, y1]) = region.rect
            && (![x0, y0, x1, y1]
                .iter()
                .all(|x| x.is_finite() && (0.0..=1.0).contains(x))
                || x0 >= x1
                || y0 >= y1)
        {
            return Err(PdfError::InvalidParams(
                "invalid normalized scan region".into(),
            ));
        }
    }
    let numbers: Vec<u32> = regions.iter().map(|region| region.page).collect();
    let mut result = build_page_set_output(source, &numbers, PageSetOptions::default(), ctx)?;
    let OutputPayload::Pdf(ref mut output) = result.payload else {
        return Err(PdfError::Malformed);
    };
    let page_ids = crate::tools::rotate::collect_page_ids(output.inner(), ctx)?;
    for (&page_id, region) in page_ids.iter().zip(regions) {
        ctx.check()?;
        let Some([x0, y0, x1, y1]) = region.rect else {
            continue;
        };
        let page_box = xobject::resolve_page_crop_box(output.inner(), page_id);
        let rotation = xobject::resolve_page_rotation(output.inner(), page_id).rem_euclid(360);
        if !page_box
            .to_array()
            .iter()
            .all(|v| v.is_finite() && v.abs() <= crate::paper::MAX_PAPER_SIDE_PT)
            || page_box.width() <= 0.0
            || page_box.height() <= 0.0
        {
            return Err(PdfError::Malformed);
        }
        let map = |u: f64, v: f64| match rotation {
            90 => (
                page_box.left + v * page_box.width(),
                page_box.bottom + u * page_box.height(),
            ),
            180 => (
                page_box.right - u * page_box.width(),
                page_box.bottom + v * page_box.height(),
            ),
            270 => (
                page_box.right - v * page_box.width(),
                page_box.top - u * page_box.height(),
            ),
            _ => (
                page_box.left + u * page_box.width(),
                page_box.top - v * page_box.height(),
            ),
        };
        let a = map(x0, y0);
        let b = map(x1, y1);
        let crop = xobject::PdfRect::new(a.0, a.1, b.0, b.1);
        if crop.width() < 1.0 || crop.height() < 1.0 {
            return Err(PdfError::InvalidParams(
                "scan region is smaller than one point".into(),
            ));
        }
        output
            .inner_mut()?
            .get_object_mut(page_id)
            .and_then(Object::as_dict_mut)
            .map_err(|_| PdfError::Malformed)?
            .set("CropBox", Object::Array(crop.to_pdf_array()));
    }
    Ok(result)
}
