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

/// Returns true only for a page consisting of image draws and graphics transforms.
/// Unknown operators, forms and annotations prevent raster replacement.
pub fn is_image_only(pdf: &PdfDoc, page_number: u32, ctx: &ToolCtx) -> Result<bool, PdfError> {
    ctx.check()?;
    let ids = crate::tools::rotate::collect_page_ids(pdf.inner(), ctx)?;
    let index = page_number.checked_sub(1).ok_or(PdfError::Malformed)? as usize;
    let id = *ids.get(index).ok_or(PdfError::Malformed)?;
    let doc = pdf.inner();
    let page = doc.get_dictionary(id).map_err(|_| PdfError::Malformed)?;
    if page.has(b"Annots") || page.has(b"StructParents") || page.has(b"AA") {
        return Ok(false);
    }
    let Ok(contents) = page.get(b"Contents") else {
        return Ok(false);
    };
    let object = doc
        .dereference(contents)
        .map_err(|_| PdfError::Malformed)?
        .1;
    let pieces: Vec<Object> = match object {
        Object::Array(items) => items.clone(),
        Object::Stream(_) => vec![contents.clone()],
        _ => return Err(PdfError::Malformed),
    };
    let resources = xobject::resolve_page_resources(doc, id);
    let objects = match resources
        .get(b"XObject")
        .ok()
        .and_then(|o| doc.dereference(o).ok())
        .and_then(|(_, o)| o.as_dict().ok())
    {
        Some(objects) => objects,
        None => return Ok(false),
    };
    let mut budget = pdf.decode_budget();
    let mut bytes = Vec::new();
    for piece in &pieces {
        ctx.check()?;
        let stream = doc
            .dereference(piece)
            .map_err(|_| PdfError::Malformed)?
            .1
            .as_stream()
            .map_err(|_| PdfError::Malformed)?;
        let data = budget.decode(stream)?.ok_or(PdfError::Malformed)?;
        bytes.extend_from_slice(&data);
        bytes.push(b'\n');
    }
    let mut depth = 0usize;
    let mut images = 0usize;
    for operation in crate::scan_text::decode_checked(&bytes, ctx)?.operations {
        ctx.check()?;
        match operation.operator.as_str() {
            "q" if operation.operands.is_empty() => {
                depth = depth.checked_add(1).ok_or(PdfError::Malformed)?
            }
            "Q" if operation.operands.is_empty() && depth > 0 => depth -= 1,
            "cm" if operation.operands.len() == 6
                && operation
                    .operands
                    .iter()
                    .all(|o| matches!(o, Object::Integer(_) | Object::Real(_))) => {}
            "Do" if operation.operands.len() == 1 => {
                let Ok(name) = operation.operands[0].as_name() else {
                    return Ok(false);
                };
                let Some(stream) = objects
                    .get(name)
                    .ok()
                    .and_then(|o| doc.dereference(o).ok())
                    .and_then(|(_, o)| o.as_stream().ok())
                else {
                    return Ok(false);
                };
                if stream.dict.has(b"OC") || stream.dict.has(b"Alternates") {
                    return Ok(false);
                }
                if stream
                    .dict
                    .get(b"Subtype")
                    .ok()
                    .and_then(|o| o.as_name().ok())
                    != Some(b"Image".as_slice())
                {
                    return Ok(false);
                }
                images += 1;
            }
            _ => return Ok(false),
        }
    }
    Ok(depth == 0 && images > 0)
}

/// Replace a proven image-only page with an upright cleaned grayscale raster.
/// Returns false without changing the document when the page is not eligible.
pub fn replace_with_cleaned_image(
    pdf: &mut PdfDoc,
    page_number: u32,
    width: u32,
    height: u32,
    pixels: &[u8],
    ctx: &ToolCtx,
) -> Result<bool, PdfError> {
    use lopdf::{Stream, dictionary};
    if !is_image_only(pdf, page_number, ctx)? {
        return Ok(false);
    }
    let expected = u64::from(width)
        .checked_mul(u64::from(height))
        .ok_or(PdfError::Malformed)?;
    if width == 0 || height == 0 || expected != pixels.len() as u64 {
        return Err(PdfError::Malformed);
    }
    let mut budget = pdf.decode_budget();
    budget.charge(pixels.len())?;
    let ids = crate::tools::rotate::collect_page_ids(pdf.inner(), ctx)?;
    let id = *ids
        .get((page_number - 1) as usize)
        .ok_or(PdfError::Malformed)?;
    let rect = xobject::resolve_page_crop_box(pdf.inner(), id);
    let rotation = xobject::resolve_page_rotation(pdf.inner(), id).rem_euclid(360);
    let [l, b, r, t] = rect.to_array();
    if ![l, b, r, t]
        .iter()
        .all(|n| n.is_finite() && n.abs() <= crate::paper::MAX_PAPER_SIDE_PT)
        || rect.width() <= 0.0
        || rect.height() <= 0.0
    {
        return Err(PdfError::Malformed);
    }
    let (w, h) = (rect.width(), rect.height());
    let matrix = match rotation {
        0 => [w, 0.0, 0.0, h, l, b],
        90 => [0.0, h, -w, 0.0, r, b],
        180 => [-w, 0.0, 0.0, -h, r, t],
        270 => [0.0, -h, w, 0.0, l, t],
        _ => return Err(PdfError::Malformed),
    };
    ctx.check()?;
    let mut image = Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Image", "Width" => i64::from(width),
            "Height" => i64::from(height), "ColorSpace" => "DeviceGray", "BitsPerComponent" => 8,
        },
        pixels.to_vec(),
    );
    image.compress().map_err(|_| PdfError::Malformed)?;
    let doc = pdf.inner_mut()?;
    let image_id = doc.add_object(image);
    let [a, b, c, d, e, f] = matrix;
    let content = format!("q {a} {b} {c} {d} {e} {f} cm /CleanImage Do Q\n");
    let content_id = doc.add_object(Stream::new(dictionary! {}, content.into_bytes()));
    let page = doc
        .get_object_mut(id)
        .and_then(Object::as_dict_mut)
        .map_err(|_| PdfError::Malformed)?;
    page.set(
        "Resources",
        dictionary! { "XObject" => dictionary! { "CleanImage" => Object::Reference(image_id) } },
    );
    page.set("Contents", Object::Reference(content_id));
    page.remove(b"Thumb");
    Ok(true)
}

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
