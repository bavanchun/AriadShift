//! RGB mapping functions used by the PDF colour tools.

use ariad_core::tools::params::{AdjustColorsParams, HexColor, ReplaceColorParams};

fn hex_rgb(color: &HexColor) -> [u8; 3] {
    let bytes = color.as_str().as_bytes();
    let digit = |v: u8| match v {
        b'0'..=b'9' => v - b'0',
        b'a'..=b'f' => v - b'a' + 10,
        b'A'..=b'F' => v - b'A' + 10,
        _ => 0,
    };
    [
        digit(bytes[1]) * 16 + digit(bytes[2]),
        digit(bytes[3]) * 16 + digit(bytes[4]),
        digit(bytes[5]) * 16 + digit(bytes[6]),
    ]
}

/// Build a mapper which replaces colours within a per-channel 8-bit tolerance.
pub fn replace_mapper(
    params: &ReplaceColorParams,
) -> impl Fn([f32; 3]) -> Option<[f32; 3]> + use<> {
    let from = params.from.as_ref().map(hex_rgb);
    let to = hex_rgb(&params.to).map(|v| f32::from(v) / 255.0);
    let tolerance = params.tolerance;
    move |rgb| {
        let source = rgb.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
        if from.is_none_or(|expected| {
            source
                .into_iter()
                .zip(expected)
                .all(|(a, b)| a.abs_diff(b) <= tolerance)
        }) {
            Some(to)
        } else {
            None
        }
    }
}

/// Build an RGB adjustment mapper. Gamma uses `x^(1/gamma)`, then contrast
/// scales about 0.5, brightness is added, and saturation mixes the result with
/// Rec. 709 luminance. Components are clamped after the final step.
pub fn adjust_mapper(params: &AdjustColorsParams) -> impl Fn([f32; 3]) -> Option<[f32; 3]> + use<> {
    let (brightness, contrast, saturation, gamma) = (
        params.brightness,
        params.contrast,
        params.saturation,
        params.gamma,
    );
    move |rgb| {
        let rgb = rgb.map(|value| {
            (value.clamp(0.0, 1.0).powf(1.0 / gamma) - 0.5) * (1.0 + contrast) + 0.5 + brightness
        });
        let luminance = 0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2];
        Some(
            rgb.map(|value| (luminance + (value - luminance) * (1.0 + saturation)).clamp(0.0, 1.0)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn replacement_respects_each_channel_tolerance() {
        let params =
            serde_json::from_value(json!({"from":"#102030", "to":"#B00020", "tolerance":2}))
                .unwrap();
        let map = replace_mapper(&params);
        assert_eq!(
            map([16.0 / 255.0, 34.0 / 255.0, 48.0 / 255.0]),
            Some([176.0 / 255.0, 0.0, 32.0 / 255.0])
        );
        assert_eq!(map([16.0 / 255.0, 35.0 / 255.0, 48.0 / 255.0]), None);
    }
    #[test]
    fn adjustment_order_and_limits_are_stable() {
        let params = serde_json::from_value(
            json!({"gamma":2.0, "contrast":-1.0, "brightness":0.1, "saturation":-1.0}),
        )
        .unwrap();
        let mapped = adjust_mapper(&params)([0.0, 0.25, 1.0]).unwrap();
        assert!(mapped.into_iter().all(|c| (c - 0.6).abs() < 1e-6));
    }
}

/// Transform a supported image XObject, leaving its mask objects attached.
#[allow(clippy::too_many_arguments)]
pub(crate) fn adjust_image<F>(
    source: &crate::PdfDoc,
    doc: &mut lopdf::Document,
    id: lopdf::ObjectId,
    budget: &mut crate::document::DecodeBudget,
    limits: &ariad_core::limits::Limits,
    ctx: &crate::ToolCtx,
    mapper: &F,
    retained: &mut u64,
) -> Result<crate::color::targeted_document::ImageChange, crate::PdfError>
where
    F: Fn([f32; 3]) -> Option<[f32; 3]>,
{
    use crate::{
        color::targeted_document::ImageChange,
        images::{self, ColorSpace, DecodedImage, ImageError},
    };
    use lopdf::Object;
    let stream = source
        .inner()
        .get_object(id)
        .and_then(Object::as_stream)
        .map_err(|_| crate::PdfError::Malformed)?;
    if stream
        .dict
        .get(b"ImageMask")
        .and_then(Object::as_bool)
        .unwrap_or(false)
    {
        return Ok(ImageChange::Unsupported("image mask unchanged"));
    }
    let filters = match stream.filters() {
        Ok(filters) => filters,
        Err(_) if !stream.dict.has(b"Filter") => Vec::new(),
        Err(_) => return Ok(ImageChange::Unsupported("unsupported image filter")),
    };
    let jpeg = filters.len() == 1 && filters[0] == b"DCTDecode";
    if !(filters.is_empty() || jpeg || (filters.len() == 1 && filters[0] == b"FlateDecode")) {
        return Ok(ImageChange::Unsupported("unsupported image filter"));
    }
    let indexed = stream
        .dict
        .get(b"ColorSpace")
        .ok()
        .and_then(|v| super::color::resolve(source.inner(), v, limits.max_nesting_depth))
        .and_then(|v| v.as_array().ok())
        .and_then(|v| v.first())
        .and_then(|v| v.as_name().ok())
        .is_some_and(|n| matches!(n, b"Indexed" | b"I"));
    if !jpeg
        && !indexed
        && stream
            .dict
            .get(b"BitsPerComponent")
            .and_then(Object::as_i64)
            .ok()
            != Some(8)
    {
        return Ok(ImageChange::Unsupported("unsupported sample depth"));
    }
    let width = stream
        .dict
        .get(b"Width")
        .and_then(Object::as_i64)
        .map_err(|_| crate::PdfError::Malformed)?;
    let height = stream
        .dict
        .get(b"Height")
        .and_then(Object::as_i64)
        .map_err(|_| crate::PdfError::Malformed)?;
    if width <= 0 || height <= 0 {
        return Err(crate::PdfError::Malformed);
    }
    let memory = crate::document::image_memory_limit(limits);
    crate::document::preflight_image_dimensions(width as u64, height as u64, *retained, memory)?;
    let decoded = match images::decode_with_budget(
        source,
        id,
        ctx,
        budget,
        limits.max_nesting_depth,
        false,
        false,
    ) {
        Ok(image) => image,
        Err(ImageError::Pdf(error)) => return Err(error),
        Err(_) => return Ok(ImageChange::Unsupported("image decoding unsupported")),
    };
    if decoded.bits_per_component != 8 {
        return Ok(ImageChange::Unsupported("unsupported sample depth"));
    }
    let count = usize::try_from(u64::from(decoded.width) * u64::from(decoded.height))
        .map_err(|_| crate::PdfError::Malformed)?;
    let preblend = match prepare_matte(source, stream, &decoded, budget, limits, ctx, mapper) {
        Ok(matte) => matte,
        Err(ImageError::Pdf(error)) => return Err(error),
        Err(_) => return Ok(ImageChange::Unsupported("preblended soft mask unsupported")),
    };
    let mask = stream
        .dict
        .get(b"Mask")
        .ok()
        .and_then(|v| super::color::resolve(source.inner(), v, limits.max_nesting_depth))
        .and_then(|v| v.as_array().ok());
    let alpha = if !stream.dict.has(b"SMask") {
        if let Some(mask) = mask {
            match color_key_alpha(
                source, id, stream, mask, &decoded, jpeg, indexed, budget, limits, ctx,
            ) {
                Ok(alpha) => Some(alpha),
                Err(ImageError::Pdf(error)) => return Err(error),
                Err(_) => return Ok(ImageChange::Unsupported("colour-key mask unsupported")),
            }
        } else {
            None
        }
    } else {
        None
    };
    budget.charge(count.checked_mul(3).ok_or(crate::PdfError::Malformed)?)?;
    let mut pixels = Vec::with_capacity(count * 3);
    let channels = match decoded.color_space {
        ColorSpace::Gray => 1,
        ColorSpace::Rgb => 3,
        ColorSpace::Cmyk => 4,
    };
    for (index, pixel) in decoded.pixels.chunks_exact(channels).enumerate() {
        if index.is_multiple_of(65_536) {
            ctx.check()?;
        }
        let alpha = match &preblend {
            Some(matte) => *matte.alpha.get(index).ok_or(crate::PdfError::Malformed)?,
            None => 1.0,
        };
        let p = |n: usize| {
            let value = f32::from(pixel[n]) / 255.0;
            if let Some(matte) = &preblend {
                if alpha > 0.0 {
                    ((value - matte.original[n] * (1.0 - alpha)) / alpha).clamp(0.0, 1.0)
                } else {
                    matte.original[n]
                }
            } else {
                value
            }
        };
        let rgb = match decoded.color_space {
            ColorSpace::Gray => [p(0); 3],
            ColorSpace::Rgb => [p(0), p(1), p(2)],
            ColorSpace::Cmyk => [
                (1.0 - p(0)) * (1.0 - p(3)),
                (1.0 - p(1)) * (1.0 - p(3)),
                (1.0 - p(2)) * (1.0 - p(3)),
            ],
        };
        let mut mapped = mapper(rgb).ok_or(crate::PdfError::Malformed)?;
        if let Some(matte) = &preblend {
            for (value, &background) in mapped.iter_mut().zip(&matte.mapped) {
                *value = *value * alpha + background * (1.0 - alpha);
            }
        }
        pixels.extend(mapped.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8));
    }
    if pixels.len() != count * 3 {
        return Err(crate::PdfError::Malformed);
    }
    let output = DecodedImage {
        width: decoded.width,
        height: decoded.height,
        color_space: ColorSpace::Rgb,
        bits_per_component: 8,
        pixels,
        has_soft_mask: decoded.has_soft_mask,
        has_mask: decoded.has_mask,
    };
    let (data, params) = if jpeg {
        (
            images::encode_jpeg(&output, 95).map_err(|_| crate::PdfError::Malformed)?,
            None,
        )
    } else {
        let (data, params) =
            images::encode_flate(&output, 9).map_err(|_| crate::PdfError::Malformed)?;
        (data, Some(params))
    };
    budget.charge(data.len())?;
    *retained = retained.saturating_add(data.len() as u64);
    let mut updated = stream.clone();
    updated.content = data;
    updated.dict.set("ColorSpace", "DeviceRGB");
    updated.dict.set("BitsPerComponent", 8);
    updated
        .dict
        .set("Filter", if jpeg { "DCTDecode" } else { "FlateDecode" });
    updated.dict.remove(b"Decode");
    updated.dict.remove(b"DecodeParms");
    if let Some(params) = params {
        updated.dict.set("DecodeParms", params);
    }
    if mask.is_some() {
        updated.dict.remove(b"Mask");
    }
    if let Some(alpha) = alpha {
        let mask_stream = crate::image_to_pdf::build_flate_stream(
            lopdf::dictionary! { "Type" => "XObject", "Subtype" => "Image", "Width" => i64::from(decoded.width), "Height" => i64::from(decoded.height), "ColorSpace" => "DeviceGray", "BitsPerComponent" => 8 },
            alpha,
        )?;
        budget.charge(mask_stream.content.len())?;
        *retained = retained.saturating_add(mask_stream.content.len() as u64);
        let mask_id = doc.add_object(mask_stream);
        updated.dict.set("SMask", mask_id);
        crate::draw::layer::require_transparency(doc);
    }
    if let Some(matte) = preblend {
        let mut soft_mask = matte.stream;
        soft_mask.dict.set(
            "Matte",
            matte
                .mapped
                .into_iter()
                .map(Object::Real)
                .collect::<Vec<_>>(),
        );
        *retained = retained.saturating_add(soft_mask.content.len() as u64);
        updated.dict.set("SMask", doc.add_object(soft_mask));
    }
    updated.dict.set("Length", updated.content.len() as i64);
    Ok(ImageChange::Replaced(doc.add_object(updated)))
}

pub(crate) fn output(
    pdf: crate::PdfDoc,
    counts: crate::color::targeted::Warnings,
) -> crate::tools::OutputDoc {
    let mut output = crate::tools::OutputDoc::unnamed_pdf(pdf);
    if !counts.is_empty() {
        let message = counts
            .into_iter()
            .map(|((op, reason), count)| format!("{op}: {reason} ({count})"))
            .collect::<Vec<_>>()
            .join(", ");
        output.coded_warnings.push(crate::tools::ToolWarning {
            code: "color_not_converted".into(),
            message,
        });
    }
    output
}

// Colour-key ranges describe samples before the image Decode array (ISO 32000-1,
// 8.9.6.4). Convert the key to an alpha image before changing colour space.
#[allow(clippy::too_many_arguments)]
fn color_key_alpha(
    source: &crate::PdfDoc,
    id: lopdf::ObjectId,
    stream: &lopdf::Stream,
    ranges: &[lopdf::Object],
    decoded: &crate::images::DecodedImage,
    jpeg: bool,
    indexed: bool,
    budget: &mut crate::document::DecodeBudget,
    limits: &ariad_core::limits::Limits,
    ctx: &crate::ToolCtx,
) -> Result<Vec<u8>, crate::images::ImageError> {
    use crate::images::{self, ColorSpace, ImageError};
    let channels = if indexed {
        1
    } else {
        match decoded.color_space {
            ColorSpace::Gray => 1,
            ColorSpace::Rgb => 3,
            ColorSpace::Cmyk => 4,
        }
    };
    let bits = if jpeg {
        8
    } else {
        stream
            .dict
            .get(b"BitsPerComponent")
            .and_then(lopdf::Object::as_i64)
            .map_err(|_| ImageError::Malformed("bits per component"))? as usize
    };
    if !matches!(bits, 1 | 2 | 4 | 8) || ranges.len() != channels * 2 {
        return Err(ImageError::Unsupported("colour-key mask"));
    }
    let max = (1u32 << bits) - 1;
    let mut bounds = Vec::with_capacity(channels);
    for pair in ranges.as_chunks::<2>().0 {
        let low = super::color::resolve(source.inner(), &pair[0], limits.max_nesting_depth)
            .and_then(|v| v.as_i64().ok())
            .ok_or(ImageError::Malformed("mask range"))?;
        let high = super::color::resolve(source.inner(), &pair[1], limits.max_nesting_depth)
            .and_then(|v| v.as_i64().ok())
            .ok_or(ImageError::Malformed("mask range"))?;
        if low < 0 || high < low || high > i64::from(max) {
            return Err(ImageError::Malformed("mask range"));
        }
        bounds.push((low as u32, high as u32));
    }
    let width = decoded.width as usize;
    let height = decoded.height as usize;
    let count = width
        .checked_mul(height)
        .ok_or(ImageError::Malformed("image dimensions"))?;
    budget.charge(count)?;
    let data = if jpeg {
        images::decode_with_budget(
            source,
            id,
            ctx,
            budget,
            limits.max_nesting_depth,
            false,
            true,
        )?
        .pixels
    } else if stream.dict.has(b"Filter") {
        budget
            .decode(stream)?
            .ok_or(ImageError::Malformed("image compression"))?
    } else {
        budget.charge(stream.content.len())?;
        stream.content.clone()
    };
    let row_samples = width
        .checked_mul(channels)
        .ok_or(ImageError::Malformed("image dimensions"))?;
    let row_bytes = row_samples
        .checked_mul(bits)
        .ok_or(ImageError::Malformed("image dimensions"))?
        .div_ceil(8);
    if data.len()
        != row_bytes
            .checked_mul(height)
            .ok_or(ImageError::Malformed("image dimensions"))?
    {
        return Err(ImageError::Malformed("image data length"));
    }
    let mut alpha = Vec::with_capacity(count);
    for row in data.chunks_exact(row_bytes) {
        ctx.check()?;
        for x in 0..width {
            let transparent = bounds.iter().enumerate().all(|(channel, &(low, high))| {
                let bit = (x * channels + channel) * bits;
                let value = u32::from((row[bit / 8] >> (8 - bits - bit % 8)) & (max as u8));
                (low..=high).contains(&value)
            });
            alpha.push(if transparent { 0 } else { 255 });
        }
    }
    Ok(alpha)
}

#[cfg(test)]
mod image_tests {
    use super::*;
    use crate::{PdfDoc, ToolCtx, color::targeted_document::ImageChange};
    use ariad_core::limits::Limits;
    use lopdf::{Object, Stream, dictionary};

    fn source_with_image(stream: Stream) -> (PdfDoc, lopdf::ObjectId) {
        let bytes = std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../fixtures/pdf-tools/multipage-20.pdf"),
        )
        .unwrap();
        let mut pdf = crate::document::open(&bytes, &Limits::local()).unwrap();
        let id = pdf.inner_mut().unwrap().add_object(stream);
        (pdf, id)
    }
    fn apply(source: &PdfDoc, id: lopdf::ObjectId) -> (lopdf::Document, lopdf::ObjectId) {
        let mut output = source.inner().clone();
        let mut budget = source.decode_budget();
        let change = adjust_image(
            source,
            &mut output,
            id,
            &mut budget,
            &Limits::local(),
            &ToolCtx::new(),
            &|rgb| Some(rgb.map(|v| (v + 0.1).min(1.0))),
            &mut 0,
        )
        .unwrap();
        match change {
            ImageChange::Replaced(id) => (output, id),
            other => panic!("expected adjusted image: {other:?}"),
        }
    }
    #[test]
    fn rgb_color_key_uses_samples_before_decode() {
        let stream = Stream::new(
            dictionary! {
                "Type" => "XObject", "Subtype" => "Image", "Width" => 2, "Height" => 1,
                "ColorSpace" => "DeviceRGB", "BitsPerComponent" => 8,
                "Decode" => vec![1.into(), 0.into(), 0.into(), 1.into(), 0.into(), 1.into()],
                "Mask" => vec![255.into(), 255.into(), 0.into(), 0.into(), 0.into(), 0.into()],
            },
            vec![255, 0, 0, 0, 255, 0],
        );
        let (source, id) = source_with_image(stream);
        let (output, new_id) = apply(&source, id);
        let image = output.get_object(new_id).unwrap().as_stream().unwrap();
        assert!(!image.dict.has(b"Mask"));
        let alpha = output
            .get_object(image.dict.get(b"SMask").unwrap().as_reference().unwrap())
            .unwrap()
            .as_stream()
            .unwrap();
        assert_eq!(alpha.decompressed_content().unwrap(), vec![0, 255]);
        assert!(
            source
                .inner()
                .get_object(id)
                .unwrap()
                .as_stream()
                .unwrap()
                .dict
                .has(b"Mask")
        );
    }
    #[test]
    fn indexed_color_key_preserves_transparency_after_palette_expansion() {
        let palette = Object::string_literal(vec![0, 0, 0, 255, 0, 0]);
        let stream = Stream::new(
            dictionary! {
                "Type" => "XObject", "Subtype" => "Image", "Width" => 2, "Height" => 1,
                "ColorSpace" => vec![Object::Name(b"Indexed".to_vec()), Object::Name(b"DeviceRGB".to_vec()), 1.into(), palette],
                "BitsPerComponent" => 1, "Mask" => vec![0.into(), 0.into()],
            },
            vec![0b0100_0000],
        );
        let (source, id) = source_with_image(stream);
        let (output, new_id) = apply(&source, id);
        let image = output.get_object(new_id).unwrap().as_stream().unwrap();
        let alpha = output
            .get_object(image.dict.get(b"SMask").unwrap().as_reference().unwrap())
            .unwrap()
            .as_stream()
            .unwrap();
        assert_eq!(alpha.decompressed_content().unwrap(), vec![0, 255]);
    }

    #[test]
    fn preblended_soft_mask_is_copied_and_matte_is_adjusted() {
        let (mut source, _) = source_with_image(Stream::new(dictionary! {}, vec![]));
        let doc = source.inner_mut().unwrap();
        let mask = doc.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject", "Subtype" => "Image", "Width" => 1, "Height" => 1,
                "ColorSpace" => "DeviceGray", "BitsPerComponent" => 8,
                "Matte" => vec![1.into(), 1.into(), 1.into()],
            },
            vec![128],
        ));
        let id = doc.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject", "Subtype" => "Image", "Width" => 1, "Height" => 1,
                "ColorSpace" => "DeviceRGB", "BitsPerComponent" => 8, "SMask" => mask,
            },
            vec![255, 127, 127],
        ));
        let mut output = source.inner().clone();
        let mut budget = source.decode_budget();
        let change = adjust_image(
            &source,
            &mut output,
            id,
            &mut budget,
            &Limits::local(),
            &ToolCtx::new(),
            &|rgb| Some(rgb.map(|v| v * 0.5)),
            &mut 0,
        )
        .unwrap();
        let ImageChange::Replaced(id) = change else {
            panic!("image should be adjusted")
        };
        let image = output.get_object(id).unwrap().as_stream().unwrap();
        assert_eq!(image.decompressed_content().unwrap(), vec![128, 64, 64]);
        let new_mask = image.dict.get(b"SMask").unwrap().as_reference().unwrap();
        assert_ne!(new_mask, mask);
        let matte = output
            .get_object(new_mask)
            .unwrap()
            .as_stream()
            .unwrap()
            .dict
            .get(b"Matte")
            .unwrap()
            .as_array()
            .unwrap();
        assert_eq!(matte, &vec![Object::Real(0.5); 3]);
        assert_eq!(
            source
                .inner()
                .get_object(mask)
                .unwrap()
                .as_stream()
                .unwrap()
                .dict
                .get(b"Matte")
                .unwrap()
                .as_array()
                .unwrap(),
            &vec![1.into(), 1.into(), 1.into()]
        );
    }

    #[test]
    fn jpeg_is_reencoded_and_soft_mask_is_preserved() {
        use image::ImageEncoder;
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 95)
            .write_image(
                &[200, 80, 30, 30, 100, 200],
                2,
                1,
                image::ExtendedColorType::Rgb8,
            )
            .unwrap();
        let (mut source, _) = source_with_image(Stream::new(dictionary! {}, vec![]));
        let doc = source.inner_mut().unwrap();
        let mask = doc.add_object(Stream::new(dictionary! { "Type" => "XObject", "Subtype" => "Image", "Width" => 2, "Height" => 1, "ColorSpace" => "DeviceGray", "BitsPerComponent" => 8 }, vec![128, 255]));
        let id = doc.add_object(Stream::new(dictionary! { "Type" => "XObject", "Subtype" => "Image", "Width" => 2, "Height" => 1, "ColorSpace" => "DeviceRGB", "BitsPerComponent" => 8, "Filter" => "DCTDecode", "SMask" => mask }, jpeg.clone()));
        let (output, adjusted) = apply(&source, id);
        let image = output.get_object(adjusted).unwrap().as_stream().unwrap();
        assert_eq!(
            image.dict.get(b"Filter").unwrap().as_name().unwrap(),
            b"DCTDecode"
        );
        assert_eq!(
            image.dict.get(b"SMask").unwrap().as_reference().unwrap(),
            mask
        );
        assert_ne!(image.content, jpeg);
        assert_eq!(
            source
                .inner()
                .get_object(id)
                .unwrap()
                .as_stream()
                .unwrap()
                .content,
            jpeg
        );
    }

    #[test]
    fn indexed_preblended_palette_is_transformed_in_base_color_space() {
        let (mut source, _) = source_with_image(Stream::new(dictionary! {}, vec![]));
        let doc = source.inner_mut().unwrap();
        let mask = doc.add_object(Stream::new(dictionary! { "Type" => "XObject", "Subtype" => "Image", "Width" => 1, "Height" => 1, "ColorSpace" => "DeviceGray", "BitsPerComponent" => 8, "Matte" => vec![1.into(), 1.into(), 1.into()] }, vec![128]));
        let palette = Object::string_literal(vec![255, 127, 127]);
        let id = doc.add_object(Stream::new(dictionary! { "Type" => "XObject", "Subtype" => "Image", "Width" => 1, "Height" => 1, "ColorSpace" => vec![Object::Name(b"Indexed".to_vec()), Object::Name(b"DeviceRGB".to_vec()), 0.into(), palette], "BitsPerComponent" => 8, "SMask" => mask }, vec![0]));
        let mut output = source.inner().clone();
        let mut budget = source.decode_budget();
        let change = adjust_image(
            &source,
            &mut output,
            id,
            &mut budget,
            &Limits::local(),
            &ToolCtx::new(),
            &|rgb| Some(rgb.map(|v| v * 0.5)),
            &mut 0,
        )
        .unwrap();
        let ImageChange::Replaced(id) = change else {
            panic!("image should be adjusted")
        };
        assert_eq!(
            output
                .get_object(id)
                .unwrap()
                .as_stream()
                .unwrap()
                .decompressed_content()
                .unwrap(),
            vec![128, 64, 64]
        );
    }
}

struct Preblend {
    stream: lopdf::Stream,
    original: Vec<f32>,
    mapped: [f32; 3],
    alpha: Vec<f32>,
}

#[allow(clippy::too_many_arguments)]
fn prepare_matte<F>(
    source: &crate::PdfDoc,
    image: &lopdf::Stream,
    decoded: &crate::images::DecodedImage,
    budget: &mut crate::document::DecodeBudget,
    limits: &ariad_core::limits::Limits,
    ctx: &crate::ToolCtx,
    mapper: &F,
) -> Result<Option<Preblend>, crate::images::ImageError>
where
    F: Fn([f32; 3]) -> Option<[f32; 3]>,
{
    use crate::images::{self, ColorSpace, ImageError};
    use lopdf::Object;
    let Ok(value) = image.dict.get(b"SMask") else {
        return Ok(None);
    };
    let mut id = value
        .as_reference()
        .map_err(|_| ImageError::Unsupported("soft mask"))?;
    let mut visited = std::collections::HashSet::new();
    let mask = loop {
        if !visited.insert(id) || visited.len() > usize::from(limits.max_nesting_depth) {
            return Err(ImageError::Malformed("soft mask reference cycle or depth"));
        }
        let value = source
            .inner()
            .get_object(id)
            .map_err(|_| ImageError::Malformed("soft mask reference"))?;
        match value {
            Object::Reference(next) => {
                id = *next;
                ctx.check()?;
            }
            Object::Stream(stream) => break stream,
            _ => return Err(ImageError::Malformed("soft mask")),
        }
    };
    let Ok(matte) = mask.dict.get(b"Matte") else {
        return Ok(None);
    };
    // For Indexed images the palette entries, not the indexes, are preblended.
    // The shared decoder has already expanded those entries to the base space.
    let matte = super::color::resolve(source.inner(), matte, limits.max_nesting_depth)
        .and_then(|v| v.as_array().ok())
        .ok_or(ImageError::Malformed("matte"))?;
    let count = match decoded.color_space {
        ColorSpace::Gray => 1,
        ColorSpace::Rgb => 3,
        ColorSpace::Cmyk => 4,
    };
    if matte.len() != count {
        return Err(ImageError::Malformed("matte components"));
    }
    let original: Vec<f32> = matte
        .iter()
        .map(|v| match v {
            Object::Integer(n) => Some(*n as f32),
            Object::Real(n) => Some(*n),
            _ => None,
        })
        .collect::<Option<_>>()
        .ok_or(ImageError::Malformed("matte components"))?;
    if original
        .iter()
        .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
    {
        return Err(ImageError::Malformed("matte components"));
    }
    let rgb = match decoded.color_space {
        ColorSpace::Gray => [original[0]; 3],
        ColorSpace::Rgb => [original[0], original[1], original[2]],
        ColorSpace::Cmyk => [
            (1.0 - original[0]) * (1.0 - original[3]),
            (1.0 - original[1]) * (1.0 - original[3]),
            (1.0 - original[2]) * (1.0 - original[3]),
        ],
    };
    let mapped = mapper(rgb).ok_or(ImageError::Malformed("matte mapping"))?;
    let decoded_mask = images::decode_with_budget(
        source,
        id,
        ctx,
        budget,
        limits.max_nesting_depth,
        false,
        false,
    )?;
    if decoded_mask.width != decoded.width
        || decoded_mask.height != decoded.height
        || decoded_mask.color_space != ColorSpace::Gray
    {
        return Err(ImageError::Malformed(
            "soft mask dimensions or colour space",
        ));
    }
    let count = usize::try_from(u64::from(decoded.width) * u64::from(decoded.height))
        .map_err(|_| ImageError::Malformed("soft mask size"))?;
    budget.charge(
        count
            .checked_mul(std::mem::size_of::<f32>())
            .ok_or(ImageError::Malformed("soft mask size"))?,
    )?;
    let alpha = match decoded_mask.bits_per_component {
        8 if decoded_mask.pixels.len() == count => decoded_mask
            .pixels
            .iter()
            .map(|&value| f32::from(value) / 255.0)
            .collect(),
        16 if decoded_mask.pixels.len() == count.saturating_mul(2) => decoded_mask
            .pixels
            .as_chunks::<2>()
            .0
            .iter()
            .map(|value| f32::from(u16::from_be_bytes([value[0], value[1]])) / 65535.0)
            .collect(),
        _ => return Err(ImageError::Unsupported("soft mask samples")),
    };
    budget.charge(mask.content.len())?;
    Ok(Some(Preblend {
        stream: mask.clone(),
        original,
        mapped,
        alpha,
    }))
}
