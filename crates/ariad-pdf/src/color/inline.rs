use super::{ColorSpace, MappedColor, scalar, space};
use lopdf::{Dictionary, Document, Object, Stream};

pub(super) enum InlineError {
    Unsupported,
    Limit,
    Budget(crate::PdfError),
}

pub(super) fn rewrite<F>(
    doc: &Document,
    image: &mut Stream,
    resources: &Dictionary,
    mapper: &F,
    max_bytes: u64,
    max_depth: u16,
    budget: &mut crate::document::DecodeBudget,
) -> Result<(), InlineError>
where
    F: Fn(ColorSpace, &[f32]) -> Option<MappedColor>,
{
    let get =
        |long: &[u8], short: &[u8]| image.dict.get(long).or_else(|_| image.dict.get(short)).ok();
    if get(b"ImageMask", b"IM")
        .and_then(|v| v.as_bool().ok())
        .unwrap_or(false)
    {
        return Err(InlineError::Unsupported);
    }
    let dimension = |long: &[u8], short: &[u8]| -> Result<u32, InlineError> {
        let n = get(long, short)
            .and_then(|v| v.as_i64().ok())
            .ok_or(InlineError::Unsupported)?;
        if !(1..=65535).contains(&n) {
            return Err(InlineError::Unsupported);
        }
        Ok(n as u32)
    };
    let width = dimension(b"Width", b"W")?;
    let height = dimension(b"Height", b"H")?;
    crate::document::preflight_image_dimensions(u64::from(width), u64::from(height), 0, max_bytes)
        .map_err(|_| InlineError::Limit)?;
    let bpc = get(b"BitsPerComponent", b"BPC")
        .and_then(|v| v.as_i64().ok())
        .ok_or(InlineError::Unsupported)? as usize;
    if ![1, 2, 4, 8, 16].contains(&bpc) {
        return Err(InlineError::Unsupported);
    }
    let name = get(b"ColorSpace", b"CS")
        .and_then(|v| v.as_name().ok())
        .ok_or(InlineError::Unsupported)?;
    let source = space(doc, resources, name, max_depth).ok_or(InlineError::Unsupported)?;
    let count = source.count();
    let pixels = u64::from(width) * u64::from(height);
    if pixels.saturating_mul(count as u64) > max_bytes {
        return Err(InlineError::Limit);
    }
    let decode = get(b"Decode", b"D")
        .map(|v| v.as_array().map_err(|_| InlineError::Unsupported))
        .transpose()?;
    if let Some(a) = decode
        && (a.len() != count * 2 || a.iter().any(|v| scalar(v).is_none_or(|n| !n.is_finite())))
    {
        return Err(InlineError::Unsupported);
    }
    let mut normalized = image.clone();
    if let Some(filter) = get(b"Filter", b"F") {
        let normalize = |v: &Object| -> Result<Object, InlineError> {
            let name = v.as_name().map_err(|_| InlineError::Unsupported)?;
            Ok(Object::Name(match name {
                b"Fl" => b"FlateDecode".to_vec(),
                b"LZW" => b"LZWDecode".to_vec(),
                b"DCT" => b"DCTDecode".to_vec(),
                _ => name.to_vec(),
            }))
        };
        let f = match filter {
            Object::Array(a) => Object::Array(a.iter().map(normalize).collect::<Result<_, _>>()?),
            _ => normalize(filter)?,
        };
        normalized.dict.set("Filter", f);
    }
    if let Some(params) = get(b"DecodeParms", b"DP") {
        normalized.dict.set("DecodeParms", params.clone());
    }
    let filter = normalized.dict.get(b"Filter").ok().and_then(|f| match f {
        Object::Name(name) => Some(name.as_slice()),
        Object::Array(values) if values.len() == 1 => values[0].as_name().ok(),
        _ => None,
    });
    let row_bytes = (width as usize * count * bpc).div_ceil(8);
    let raw_size = row_bytes
        .checked_mul(height as usize)
        .ok_or(InlineError::Limit)?;
    if raw_size as u64 > max_bytes {
        return Err(InlineError::Limit);
    }
    budget.charge(raw_size).map_err(InlineError::Budget)?;
    let decoded_cmyk_jpeg = filter == Some(b"DCTDecode".as_slice()) && source == ColorSpace::Cmyk;
    let raw = if filter == Some(b"DCTDecode".as_slice()) {
        #[cfg(feature = "codecs")]
        {
            if bpc != 8 {
                return Err(InlineError::Unsupported);
            }
            let transform = crate::images::jpeg_transform(get(b"DecodeParms", b"DP"))
                .map_err(|_| InlineError::Unsupported)?;
            if source == ColorSpace::Cmyk {
                crate::images::decode_cmyk_jpeg(
                    &image.content,
                    width,
                    height,
                    get(b"Decode", b"D"),
                    transform,
                )
                .map_err(|_| InlineError::Unsupported)?
            } else if source == ColorSpace::Rgb && transform.is_some() {
                crate::images::decode_rgb_jpeg(
                    &image.content,
                    width,
                    height,
                    transform.unwrap_or(1),
                )
                .map_err(|_| InlineError::Unsupported)?
            } else {
                use std::io::Cursor;
                let mut reader = image::ImageReader::with_format(
                    Cursor::new(&image.content),
                    image::ImageFormat::Jpeg,
                );
                let mut limits = image::Limits::default();
                limits.max_image_width = Some(width);
                limits.max_image_height = Some(height);
                limits.max_alloc = Some(max_bytes);
                reader.limits(limits);
                let decoded = reader.decode().map_err(|_| InlineError::Unsupported)?;
                if decoded.width() != width || decoded.height() != height {
                    return Err(InlineError::Unsupported);
                }
                if source == ColorSpace::Gray {
                    decoded.to_luma8().into_raw()
                } else {
                    decoded.to_rgb8().into_raw()
                }
            }
        }
        #[cfg(not(feature = "codecs"))]
        {
            return Err(InlineError::Unsupported);
        }
    } else {
        normalized
            .get_plain_content_with_limit(
                max_bytes
                    .min((raw_size as u64).saturating_add(u64::from(height)))
                    .min((usize::MAX / 4) as u64) as usize,
            )
            .map_err(|e| match e {
                lopdf::Error::Decompress(lopdf::DecompressError::MemoryLimitExceeded {
                    ..
                }) => InlineError::Limit,
                _ => InlineError::Unsupported,
            })?
    };
    if Some(raw.len()) != row_bytes.checked_mul(height as usize) {
        return Err(InlineError::Unsupported);
    }
    let mut output = Vec::new();
    let remaining = max_bytes.saturating_sub(raw.len() as u64);
    let mut output_space = None;
    let max = ((1_u32 << bpc) - 1) as f32;
    for row in raw.chunks_exact(row_bytes) {
        for x in 0..width as usize {
            let mut values = Vec::with_capacity(count);
            for c in 0..count {
                let bit = (x * count + c) * bpc;
                let sample = if bpc == 16 {
                    u32::from(u16::from_be_bytes([row[bit / 8], row[bit / 8 + 1]]))
                } else {
                    u32::from((row[bit / 8] >> (8 - bpc - bit % 8)) & ((1_u16 << bpc) - 1) as u8)
                };
                let (lo, hi) = if decoded_cmyk_jpeg {
                    (0.0, 1.0)
                } else {
                    decode.map_or((0.0, 1.0), |a| {
                        (
                            scalar(&a[c * 2]).unwrap_or(0.0),
                            scalar(&a[c * 2 + 1]).unwrap_or(1.0),
                        )
                    })
                };
                values.push((lo + sample as f32 / max * (hi - lo)).clamp(0.0, 1.0));
            }
            let mapped = mapper(source, &values).ok_or(InlineError::Unsupported)?;
            if mapped.components.len() != mapped.space.count()
                || mapped.components.iter().any(|v| !v.is_finite())
                || output_space.is_some_and(|s| s != mapped.space)
            {
                return Err(InlineError::Unsupported);
            }
            let output_bytes = if bpc == 16 { 2 } else { 1 };
            if pixels
                .saturating_mul(mapped.space.count() as u64)
                .saturating_mul(output_bytes)
                > remaining
            {
                return Err(InlineError::Limit);
            }
            if output_space.is_none() {
                let output_size = pixels
                    .saturating_mul(mapped.space.count() as u64)
                    .saturating_mul(output_bytes);
                let output_size = usize::try_from(output_size).map_err(|_| InlineError::Limit)?;
                budget.charge(output_size).map_err(InlineError::Budget)?;
            }
            output_space = Some(mapped.space);
            for value in mapped.components {
                if bpc == 16 {
                    output.extend_from_slice(
                        &((value.clamp(0.0, 1.0) * 65535.0).round() as u16).to_be_bytes(),
                    );
                } else {
                    output.push((value.clamp(0.0, 1.0) * 255.0).round() as u8);
                }
            }
        }
    }
    let output_space = output_space.ok_or(InlineError::Unsupported)?;
    let mut new_dict = image.dict.clone();
    for key in [
        b"F".as_slice(),
        b"Filter",
        b"DP",
        b"DecodeParms",
        b"D",
        b"Decode",
        b"CS",
        b"ColorSpace",
        b"BPC",
        b"BitsPerComponent",
    ] {
        new_dict.remove(key);
    }
    new_dict.set("ColorSpace", Object::Name(output_space.name().to_vec()));
    new_dict.set("BitsPerComponent", if bpc == 16 { 16 } else { 8 });
    let mut result = Stream::new(new_dict, output);
    result.compress().map_err(|_| InlineError::Unsupported)?;
    *image = result;
    Ok(())
}
