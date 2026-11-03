//! Image input validation and bounded intrinsic resolution metadata.

use crate::{
    PdfDoc, PdfError, ToolCtx,
    image_to_pdf::{self, ImageXObject},
    tools::{InputDoc, InputSource},
};
use ariad_core::limits::Limits;

/// Open a PDF input after checking its byte signature when raw bytes are supplied.
pub(super) fn pdf_input(input: &InputDoc, ctx: &ToolCtx) -> Result<PdfDoc, PdfError> {
    if let InputSource::Bytes(bytes) = &input.source
        && !bytes.starts_with(b"%PDF-")
    {
        return Err(PdfError::InvalidParams("input 0 must be a PDF".into()));
    }
    input.open_for_mutation(ctx)
}

/// Require a PDF in slot zero and a supported raster image in slot one.
pub(super) fn inputs<'a>(
    inputs: &'a [InputDoc],
    ctx: &ToolCtx,
) -> Result<(PdfDoc, &'a [u8]), PdfError> {
    if inputs.len() != 2 {
        return Err(PdfError::InvalidParams(
            "this tool requires a PDF input 0 and image input 1".into(),
        ));
    }
    let bytes = match &inputs[1].source {
        InputSource::Bytes(bytes) => bytes.as_slice(),
        InputSource::Doc(_) => {
            return Err(PdfError::InvalidParams("input 1 must be an image".into()));
        }
    };
    match image::guess_format(bytes) {
        Ok(
            image::ImageFormat::Png
            | image::ImageFormat::Jpeg
            | image::ImageFormat::WebP
            | image::ImageFormat::Tiff
            | image::ImageFormat::Bmp,
        ) => {}
        _ => {
            return Err(PdfError::InvalidParams(
                "input 1 must be PNG, JPEG, WebP, TIFF or BMP".into(),
            ));
        }
    }
    Ok((pdf_input(&inputs[0], ctx)?, bytes))
}

pub(super) fn embed(
    pdf: &mut PdfDoc,
    bytes: &[u8],
    limits: &Limits,
    ctx: &ToolCtx,
) -> Result<ImageXObject, PdfError> {
    image_to_pdf::embed_image(pdf.inner_mut()?, bytes, limits, ctx)
}

fn u16_at(bytes: &[u8], offset: usize, little: bool) -> Option<u16> {
    let data: [u8; 2] = bytes.get(offset..offset.checked_add(2)?)?.try_into().ok()?;
    Some(if little {
        u16::from_le_bytes(data)
    } else {
        u16::from_be_bytes(data)
    })
}
fn u32_at(bytes: &[u8], offset: usize, little: bool) -> Option<u32> {
    let data: [u8; 4] = bytes.get(offset..offset.checked_add(4)?)?.try_into().ok()?;
    Some(if little {
        u32::from_le_bytes(data)
    } else {
        u32::from_be_bytes(data)
    })
}
fn sane(x: f64, y: f64) -> Option<(f64, f64)> {
    (x.is_finite()
        && y.is_finite()
        && x > 0.0
        && y > 0.0
        && x <= 1_000_000_000.0
        && y <= 1_000_000_000.0)
        .then_some((x, y))
}
fn tiff_dpi(mut data: &[u8]) -> Option<(f64, f64)> {
    if data.starts_with(b"Exif\0\0") {
        data = &data[6..];
    }
    let little = match data.get(..2)? {
        b"II" => true,
        b"MM" => false,
        _ => return None,
    };
    if u16_at(data, 2, little)? != 42 {
        return None;
    }
    let offset = u32_at(data, 4, little)? as usize;
    let count = usize::from(u16_at(data, offset, little)?);
    if count > 4096 {
        return None;
    }
    let mut x = None;
    let mut y = None;
    let mut unit = 2u16;
    for index in 0..count {
        let entry = offset.checked_add(2)?.checked_add(index.checked_mul(12)?)?;
        let tag = u16_at(data, entry, little)?;
        let kind = u16_at(data, entry.checked_add(2)?, little)?;
        let elements = u32_at(data, entry.checked_add(4)?, little)?;
        match (tag, kind, elements) {
            (0x011a | 0x011b, 5, 1) => {
                let pos = u32_at(data, entry.checked_add(8)?, little)? as usize;
                let numerator = u32_at(data, pos, little)?;
                let denominator = u32_at(data, pos.checked_add(4)?, little)?;
                if denominator == 0 {
                    return None;
                }
                let value = f64::from(numerator) / f64::from(denominator);
                if tag == 0x011a {
                    x = Some(value);
                } else {
                    y = Some(value);
                }
            }
            (0x0128, 3, 1) => unit = u16_at(data, entry.checked_add(8)?, little)?,
            _ => {}
        }
    }
    let factor = match unit {
        2 => 1.0,
        3 => 2.54,
        _ => return None,
    };
    sane(x? * factor, y? * factor)
}
fn png_dpi(bytes: &[u8], ctx: &ToolCtx) -> Result<Option<(f64, f64)>, PdfError> {
    let mut offset = 8usize;
    let mut found = None;
    for _ in 0..4096 {
        ctx.check()?;
        let Some(length) = u32_at(bytes, offset, false).map(|n| n as usize) else {
            break;
        };
        let Some(kind_start) = offset.checked_add(4) else {
            break;
        };
        let Some(start) = offset.checked_add(8) else {
            break;
        };
        let Some(kind) = bytes.get(kind_start..start) else {
            break;
        };
        let Some(end) = start.checked_add(length) else {
            break;
        };
        let Some(data) = bytes.get(start..end) else {
            break;
        };
        if kind == b"pHYs" && data.len() == 9 && data[8] == 1 {
            found = sane(
                f64::from(u32_at(data, 0, false).unwrap_or(0)) * 0.0254,
                f64::from(u32_at(data, 4, false).unwrap_or(0)) * 0.0254,
            );
        }
        if kind == b"eXIf"
            && let Some(dpi) = tiff_dpi(data)
        {
            found = Some(dpi);
        }
        if kind == b"IEND" {
            break;
        }
        let Some(next) = end.checked_add(4) else {
            break;
        };
        offset = next;
    }
    Ok(found)
}
fn jpeg_dpi(bytes: &[u8], ctx: &ToolCtx) -> Result<Option<(f64, f64)>, PdfError> {
    let mut pos = 2usize;
    let mut jfif = None;
    let mut exif = None;
    for _ in 0..4096 {
        ctx.check()?;
        if bytes.get(pos) != Some(&0xff) {
            break;
        }
        while bytes.get(pos) == Some(&0xff) {
            pos += 1;
            if pos.is_multiple_of(65_536) {
                ctx.check()?;
            }
        }
        let Some(&tag) = bytes.get(pos) else {
            break;
        };
        pos += 1;
        if tag == 0xda || tag == 0xd9 {
            break;
        }
        if tag == 0x01 || (0xd0..=0xd8).contains(&tag) {
            continue;
        }
        let Some(length) = u16_at(bytes, pos, false).map(usize::from) else {
            break;
        };
        if length < 2 {
            break;
        }
        let Some(end) = pos.checked_add(length) else {
            break;
        };
        let Some(payload) = bytes.get(pos + 2..end) else {
            break;
        };
        if tag == 0xe0 && payload.starts_with(b"JFIF\0") && payload.len() >= 12 {
            let factor = match payload[7] {
                1 => Some(1.0),
                2 => Some(2.54),
                _ => None,
            };
            if let Some(factor) = factor {
                jfif = sane(
                    f64::from(u16_at(payload, 8, false).unwrap_or(0)) * factor,
                    f64::from(u16_at(payload, 10, false).unwrap_or(0)) * factor,
                );
            }
        }
        if tag == 0xe1 && payload.starts_with(b"Exif\0\0") {
            exif = tiff_dpi(payload).or(exif);
        }
        pos = end;
    }
    Ok(exif.or(jfif))
}
fn webp_dpi(bytes: &[u8], ctx: &ToolCtx) -> Result<Option<(f64, f64)>, PdfError> {
    let mut offset = 12usize;
    for _ in 0..4096 {
        ctx.check()?;
        let Some(length_offset) = offset.checked_add(4) else {
            break;
        };
        let Some(start) = offset.checked_add(8) else {
            break;
        };
        let Some(kind) = bytes.get(offset..length_offset) else {
            break;
        };
        let Some(length) = u32_at(bytes, length_offset, true).map(|n| n as usize) else {
            break;
        };
        let Some(end) = start.checked_add(length) else {
            break;
        };
        let Some(data) = bytes.get(start..end) else {
            break;
        };
        if kind == b"EXIF"
            && let Some(dpi) = tiff_dpi(data)
        {
            return Ok(Some(dpi));
        }
        let Some(next) = end.checked_add(length % 2) else {
            break;
        };
        offset = next;
    }
    Ok(None)
}
/// Physical width and height from available resolution metadata, falling back to 96 DPI.
pub(super) fn natural_size(
    bytes: &[u8],
    image: ImageXObject,
    ctx: &ToolCtx,
) -> Result<(f64, f64), PdfError> {
    let dpi = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        png_dpi(bytes, ctx)?
    } else if bytes.starts_with(b"\xff\xd8") {
        jpeg_dpi(bytes, ctx)?
    } else if bytes.starts_with(b"II*\0") || bytes.starts_with(b"MM\0*") {
        tiff_dpi(bytes)
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        webp_dpi(bytes, ctx)?
    } else if bytes.starts_with(b"BM") && u32_at(bytes, 14, true).is_some_and(|size| size >= 40) {
        let x = u32_at(bytes, 38, true).unwrap_or(0) as i32;
        let y = u32_at(bytes, 42, true).unwrap_or(0) as i32;
        sane(f64::from(x) * 0.0254, f64::from(y) * 0.0254)
    } else {
        None
    }
    .unwrap_or((96.0, 96.0));
    let (width, height) = crate::draw::image::oriented_dimensions(image);
    let (dx, dy) = if matches!(image.orientation, 5..=8) {
        (dpi.1, dpi.0)
    } else {
        dpi
    };
    Ok((f64::from(width) * 72.0 / dx, f64::from(height) * 72.0 / dy))
}

/// Detect the common scan-page case only when an initial, unmasked image is provably opaque
/// and covers the crop box. Unsupported state conservatively produces no warning.
pub(super) fn hidden_under_scan(
    pdf: &PdfDoc,
    page: lopdf::ObjectId,
    limits: &Limits,
    budget: &mut crate::document::DecodeBudget,
    ctx: &ToolCtx,
) -> Result<bool, PdfError> {
    use crate::{
        color::{page_resources, resolve},
        xobject::{Matrix, PdfRect},
    };
    use lopdf::{Object, content::Content};
    let doc = pdf.inner();
    let resources = page_resources(doc, page, limits)?;
    let crop = crate::xobject::resolve_page_crop_box(doc, page);
    let mut ctm = Matrix::identity();
    let mut alpha = 1.0_f64;
    let mut stack = Vec::new();
    let mut count = 0usize;
    let num = |v: &Object| {
        match v {
            Object::Integer(n) => Some(*n as f64),
            Object::Real(n) => Some(f64::from(*n)),
            _ => None,
        }
        .filter(|v| v.is_finite())
    };
    for id in doc.get_page_contents(page) {
        ctx.check()?;
        let stream = doc
            .get_object(id)
            .and_then(Object::as_stream)
            .map_err(|_| PdfError::Malformed)?;
        let data = if stream.dict.has(b"Filter") {
            budget.decode(stream)?.ok_or(PdfError::Malformed)?
        } else {
            budget.charge(stream.content.len())?;
            stream.content.clone()
        };
        let content = Content::decode(&data).map_err(|_| PdfError::Malformed)?;
        for op in content.operations {
            count += 1;
            if count > 4_000_000 {
                return Err(PdfError::LimitExceeded(
                    ariad_core::reader::pdf::ReadError::TooManyObjects { limit: 4_000_000 },
                ));
            }
            if count.is_multiple_of(1024) {
                ctx.check()?;
            }
            match op.operator.as_str() {
                "q" => {
                    if stack.len() >= 4096 {
                        return Ok(false);
                    }
                    stack.push((ctm, alpha));
                }
                "Q" => {
                    if let Some(state) = stack.pop() {
                        (ctm, alpha) = state;
                    } else {
                        return Ok(false);
                    }
                }
                "cm" => {
                    if op.operands.len() != 6 {
                        return Ok(false);
                    }
                    let mut values = [0.0; 6];
                    for (i, v) in op.operands.iter().enumerate() {
                        let Some(n) = num(v) else {
                            return Ok(false);
                        };
                        values[i] = n;
                    }
                    ctm = Matrix(values).then(ctm);
                }
                "gs" => {
                    let Some(name) = op.operands.first().and_then(|o| o.as_name().ok()) else {
                        return Ok(false);
                    };
                    let Some(gs) = resources
                        .get(b"ExtGState")
                        .ok()
                        .and_then(|v| resolve(doc, v, limits.max_nesting_depth))
                        .and_then(|v| v.as_dict().ok())
                        .and_then(|d| d.get(name).ok())
                        .and_then(|v| resolve(doc, v, limits.max_nesting_depth))
                        .and_then(|v| v.as_dict().ok())
                    else {
                        return Ok(false);
                    };
                    if let Ok(v) = gs.get(b"ca") {
                        let Some(v) = resolve(doc, v, limits.max_nesting_depth).and_then(num)
                        else {
                            return Ok(false);
                        };
                        alpha = v;
                    }
                    if gs
                        .get(b"op")
                        .ok()
                        .and_then(|v| resolve(doc, v, limits.max_nesting_depth))
                        .is_some_and(|v| v.as_bool().ok() != Some(false))
                    {
                        return Ok(false);
                    }
                    if gs
                        .get(b"BM")
                        .ok()
                        .and_then(|v| resolve(doc, v, limits.max_nesting_depth))
                        .is_some_and(|v| v.as_name().ok() != Some(b"Normal"))
                        || gs
                            .get(b"SMask")
                            .ok()
                            .and_then(|v| resolve(doc, v, limits.max_nesting_depth))
                            .is_some_and(|v| v.as_name().ok() != Some(b"None"))
                    {
                        return Ok(false);
                    }
                }
                "Do" => {
                    if (alpha - 1.0).abs() > 1e-9 {
                        return Ok(false);
                    }
                    let Some(name) = op.operands.first().and_then(|o| o.as_name().ok()) else {
                        return Ok(false);
                    };
                    let Some(stream) = resources
                        .get(b"XObject")
                        .ok()
                        .and_then(|v| resolve(doc, v, limits.max_nesting_depth))
                        .and_then(|v| v.as_dict().ok())
                        .and_then(|d| d.get(name).ok())
                        .and_then(|v| resolve(doc, v, limits.max_nesting_depth))
                        .and_then(|v| v.as_stream().ok())
                    else {
                        return Ok(false);
                    };
                    if stream.dict.get(b"Subtype").and_then(Object::as_name).ok() != Some(b"Image")
                        || stream.dict.has(b"Mask")
                        || stream.dict.has(b"SMask")
                        || stream
                            .dict
                            .get(b"ImageMask")
                            .ok()
                            .is_some_and(|v| v.as_bool().ok() != Some(false))
                        || stream
                            .dict
                            .get(b"SMaskInData")
                            .ok()
                            .is_some_and(|v| v.as_i64().ok() != Some(0))
                    {
                        return Ok(false);
                    }
                    let [a, b, c, d, _, _] = ctm.0;
                    let axis_aligned =
                        (b.abs() < 1e-7 && c.abs() < 1e-7) || (a.abs() < 1e-7 && d.abs() < 1e-7);
                    let bounds = ctm.transform_rect_bbox(PdfRect::from_size(1.0, 1.0));
                    return Ok(axis_aligned
                        && bounds.left <= crop.left + 0.01
                        && bounds.bottom <= crop.bottom + 0.01
                        && bounds.right >= crop.right - 0.01
                        && bounds.top >= crop.top - 0.01);
                }
                "S" | "s" | "f" | "F" | "f*" | "B" | "B*" | "b" | "b*" | "sh" | "Tj" | "TJ"
                | "'" | "\"" | "BI" | "W" | "W*" | "m" | "l" | "c" | "v" | "y" | "h" | "re" => {
                    return Ok(false);
                }
                _ => {}
            }
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn tiff() -> Vec<u8> {
        let mut data = b"II\x2a\0\x08\0\0\0\x03\0".to_vec();
        for (tag, kind, offset) in [(0x011au16, 5u16, 50u32), (0x011b, 5, 58), (0x0128, 3, 2)] {
            data.extend_from_slice(&tag.to_le_bytes());
            data.extend_from_slice(&kind.to_le_bytes());
            data.extend_from_slice(&1u32.to_le_bytes());
            data.extend_from_slice(&offset.to_le_bytes());
        }
        data.extend_from_slice(&0u32.to_le_bytes());
        data.extend_from_slice(&300u32.to_le_bytes());
        data.extend_from_slice(&1u32.to_le_bytes());
        data.extend_from_slice(&150u32.to_le_bytes());
        data.extend_from_slice(&1u32.to_le_bytes());
        data
    }
    #[test]
    fn resolution_metadata_and_orientation() {
        let ctx = ToolCtx::new();
        let data = tiff();
        assert_eq!(tiff_dpi(&data), Some((300.0, 150.0)));
        let mut webp = b"RIFF\0\0\0\0WEBPEXIF".to_vec();
        webp.extend_from_slice(&(data.len() as u32).to_le_bytes());
        webp.extend_from_slice(&data);
        assert_eq!(webp_dpi(&webp, &ctx).unwrap(), Some((300.0, 150.0)));
        let jpeg = b"\xff\xd8\xff\xe0\0\x10JFIF\0\x01\x02\x01\x01\x2c\0\x96\0\0\xff\xd9";
        assert_eq!(jpeg_dpi(jpeg, &ctx).unwrap(), Some((300.0, 150.0)));
        let image = ImageXObject {
            id: (1, 0),
            width: 600,
            height: 150,
            orientation: 6,
        };
        assert_eq!(natural_size(&data, image, &ctx).unwrap(), (72.0, 144.0));
        assert!(tiff_dpi(b"II\x2a\0\xff\xff\xff\xff").is_none());
    }
}
