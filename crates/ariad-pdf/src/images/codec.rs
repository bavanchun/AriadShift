//! Bounded image decoding and encoding primitives.

use std::io::{Cursor, Write};

use flate2::{Compression, write::ZlibEncoder};
use image::{ColorType, ImageEncoder, ImageReader, imageops::FilterType};
use lopdf::{Dictionary, Object, ObjectId, dictionary};

use crate::{PdfDoc, PdfError, ToolCtx, document::DecodeBudget};
use ariad_core::{limits::Limits, reader::pdf::ReadError};

/// Pixel representation of a decoded image.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ColorSpace {
    /// One luminance channel.
    Gray,
    /// Red, green and blue channels.
    Rgb,
    /// Cyan, magenta, yellow and black channels.
    Cmyk,
}

impl ColorSpace {
    const fn components(self) -> usize {
        match self {
            Self::Gray => 1,
            Self::Rgb => 3,
            Self::Cmyk => 4,
        }
    }
}

/// Unpacked 8-bit or big-endian 16-bit pixels. Masks remain separate.
#[derive(Clone, Debug, PartialEq)]
pub struct DecodedImage {
    pub width: u32,
    pub height: u32,
    pub color_space: ColorSpace,
    pub bits_per_component: u8,
    pub pixels: Vec<u8>,
    pub has_soft_mask: bool,
    pub has_mask: bool,
}

/// Errors encountered while decoding or encoding PDF images.
#[derive(Debug, thiserror::Error)]
pub enum ImageError {
    #[error("unsupported image: {0}")]
    Unsupported(&'static str),
    #[error("malformed image: {0}")]
    Malformed(&'static str),
    #[error(transparent)]
    Pdf(#[from] PdfError),
    #[error(transparent)]
    Image(#[from] image::ImageError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Jpeg(#[from] oxiarc_jpeg::JpegError),
}

fn resolve<'a>(
    doc: &'a lopdf::Document,
    mut obj: &'a Object,
    limit: u16,
) -> Result<&'a Object, ImageError> {
    for _ in 0..=limit {
        match obj {
            Object::Reference(id) => {
                obj = doc
                    .get_object(*id)
                    .map_err(|_| ImageError::Malformed("reference"))?
            }
            _ => return Ok(obj),
        }
    }
    Err(PdfError::LimitExceeded(ReadError::NestingTooDeep { limit }).into())
}

fn number(obj: &Object) -> Option<f64> {
    match obj {
        Object::Integer(v) => Some(*v as f64),
        Object::Real(v) => Some(f64::from(*v)),
        _ => None,
    }
}

fn has_adobe_marker(bytes: &[u8]) -> bool {
    if !bytes.starts_with(&[0xff, 0xd8]) {
        return false;
    }
    let mut pos = 2;
    while pos + 4 <= bytes.len() {
        if bytes[pos] != 0xff {
            return false;
        }
        while pos < bytes.len() && bytes[pos] == 0xff {
            pos += 1;
        }
        if pos >= bytes.len() {
            return false;
        }
        let marker = bytes[pos];
        pos += 1;
        if marker == 0xda || marker == 0xd9 {
            break;
        }
        if marker == 0x01 || (0xd0..=0xd7).contains(&marker) {
            continue;
        }
        if pos + 2 > bytes.len() {
            break;
        }
        let size = usize::from(u16::from_be_bytes([bytes[pos], bytes[pos + 1]]));
        if size < 2 || pos.checked_add(size).is_none_or(|end| end > bytes.len()) {
            break;
        }
        if marker == 0xee && size >= 14 && &bytes[pos + 2..pos + 7] == b"Adobe" {
            return true;
        }
        pos += size;
    }
    false
}

pub(crate) fn jpeg_transform(params: Option<&Object>) -> Result<Option<i64>, ImageError> {
    let Some(mut params) = params else {
        return Ok(None);
    };
    if let Object::Array(values) = params {
        params = values
            .first()
            .ok_or(ImageError::Malformed("JPEG decode parameters"))?;
    }
    if matches!(params, Object::Null) {
        return Ok(None);
    }
    let dict = params
        .as_dict()
        .map_err(|_| ImageError::Malformed("JPEG decode parameters"))?;
    let Ok(value) = dict.get(b"ColorTransform") else {
        return Ok(None);
    };
    let value = value
        .as_i64()
        .map_err(|_| ImageError::Malformed("JPEG color transform"))?;
    if !matches!(value, 0 | 1) {
        return Err(ImageError::Unsupported("JPEG color transform"));
    }
    Ok(Some(value))
}

pub(crate) fn decode_rgb_jpeg(
    bytes: &[u8],
    width: u32,
    height: u32,
    transform: i64,
) -> Result<Vec<u8>, ImageError> {
    use zune_core::bytestream::ZCursor;
    let mut decoder = zune_jpeg::JpegDecoder::new(ZCursor::new(bytes));
    decoder.set_options(
        decoder
            .options()
            .set_max_width(width as usize)
            .set_max_height(height as usize),
    );
    decoder
        .decode_headers()
        .map_err(|_| ImageError::Malformed("JPEG header"))?;
    if decoder.dimensions() != Some((width as usize, height as usize)) {
        return Err(ImageError::Malformed("JPEG dimensions differ"));
    }
    let input = decoder
        .input_colorspace()
        .ok_or(ImageError::Malformed("JPEG colour space"))?;
    if input.num_components() != 3 {
        return Err(ImageError::Malformed("JPEG component count"));
    }
    decoder.set_options(decoder.options().jpeg_set_out_colorspace(input));
    let size = (width as usize)
        .checked_mul(height as usize)
        .and_then(|n| n.checked_mul(3))
        .ok_or(ImageError::Malformed("dimensions overflow"))?;
    if decoder.output_buffer_size() != Some(size) {
        return Err(ImageError::Malformed("JPEG component count"));
    }
    let mut pixels = vec![0; size];
    decoder
        .decode_into(&mut pixels)
        .map_err(|_| ImageError::Malformed("JPEG data"))?;
    if transform == 1 {
        for p in pixels.as_chunks_mut::<3>().0 {
            let y = f64::from(p[0]);
            let cb = f64::from(p[1]) - 128.0;
            let cr = f64::from(p[2]) - 128.0;
            let rgb = [
                y + 1.402 * cr,
                y - 0.344136 * cb - 0.714136 * cr,
                y + 1.772 * cb,
            ];
            for i in 0..3 {
                p[i] = rgb[i].round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    Ok(pixels)
}

pub(crate) fn decode_cmyk_jpeg(
    bytes: &[u8],
    width: u32,
    height: u32,
    decode: Option<&Object>,
    transform: Option<i64>,
) -> Result<Vec<u8>, ImageError> {
    use zune_core::{bytestream::ZCursor, colorspace::ColorSpace as JpegSpace};
    let mut decoder = zune_jpeg::JpegDecoder::new(ZCursor::new(bytes));
    decoder.set_options(
        decoder
            .options()
            .set_max_width(width as usize)
            .set_max_height(height as usize),
    );
    decoder
        .decode_headers()
        .map_err(|_| ImageError::Malformed("JPEG header"))?;
    if decoder.dimensions() != Some((width as usize, height as usize)) {
        return Err(ImageError::Malformed("JPEG dimensions differ"));
    }
    let input = decoder
        .input_colorspace()
        .ok_or(ImageError::Malformed("JPEG colour space"))?;
    if !matches!(input, JpegSpace::CMYK | JpegSpace::YCCK) {
        return Err(ImageError::Malformed("JPEG component count"));
    }
    decoder.set_options(
        decoder
            .options()
            .set_max_width(width as usize)
            .set_max_height(height as usize)
            .jpeg_set_out_colorspace(input),
    );
    let size = (width as usize)
        .checked_mul(height as usize)
        .and_then(|n| n.checked_mul(4))
        .ok_or(ImageError::Malformed("dimensions overflow"))?;
    if decoder.output_buffer_size() != Some(size) {
        return Err(ImageError::Malformed("JPEG component count"));
    }
    let mut pixels = vec![0; size];
    decoder
        .decode_into(&mut pixels)
        .map_err(|_| ImageError::Malformed("JPEG data"))?;
    if transform.map_or(input == JpegSpace::YCCK, |value| value == 1) {
        for p in pixels.as_chunks_mut::<4>().0 {
            let y = f64::from(p[0]);
            let cb = f64::from(p[1]) - 128.0;
            let cr = f64::from(p[2]) - 128.0;
            let rgb = [
                y + 1.402 * cr,
                y - 0.344136 * cb - 0.714136 * cr,
                y + 1.772 * cb,
            ];
            for i in 0..3 {
                p[i] = 255 - (rgb[i].round().clamp(0.0, 255.0) as u8);
            }
        }
    }
    let default = if has_adobe_marker(bytes) {
        (1.0, 0.0)
    } else {
        (0.0, 1.0)
    };
    let array = decode
        .map(|o| {
            o.as_array()
                .map_err(|_| ImageError::Malformed("decode array"))
        })
        .transpose()?;
    if let Some(a) = array
        && (a.len() != 8 || a.iter().any(|o| number(o).is_none_or(|n| !n.is_finite())))
    {
        return Err(ImageError::Malformed("decode array"));
    }
    for p in pixels.as_chunks_mut::<4>().0 {
        for i in 0..4 {
            let (lo, hi) = array.map_or(default, |a| {
                (
                    number(&a[i * 2]).unwrap_or(default.0),
                    number(&a[i * 2 + 1]).unwrap_or(default.1),
                )
            });
            p[i] =
                ((lo + f64::from(p[i]) / 255.0 * (hi - lo)).clamp(0.0, 1.0) * 255.0).round() as u8;
        }
    }
    Ok(pixels)
}

type Palette = Option<(usize, Vec<u8>)>;

fn color_space(
    doc: &lopdf::Document,
    obj: &Object,
    limit: u16,
    depth: u16,
    budget: &mut DecodeBudget,
) -> Result<(ColorSpace, Palette), ImageError> {
    if depth > limit {
        return Err(PdfError::LimitExceeded(ReadError::NestingTooDeep { limit }).into());
    }
    let obj = resolve(doc, obj, limit.saturating_sub(depth))?;
    let (name, rest) = match obj {
        Object::Name(n) => (n.as_slice(), None),
        Object::Array(a) if !a.is_empty() => (
            a[0].as_name()
                .map_err(|_| ImageError::Malformed("colour space"))?,
            Some(a),
        ),
        _ => return Err(ImageError::Malformed("colour space")),
    };
    match name {
        b"DeviceGray" | b"G" | b"CalGray" => Ok((ColorSpace::Gray, None)),
        b"DeviceRGB" | b"RGB" | b"CalRGB" => Ok((ColorSpace::Rgb, None)),
        b"DeviceCMYK" | b"CMYK" => Ok((ColorSpace::Cmyk, None)),
        b"ICCBased" => {
            let a = rest.ok_or(ImageError::Malformed("ICC profile"))?;
            let profile = resolve(
                doc,
                a.get(1).ok_or(ImageError::Malformed("ICC profile"))?,
                limit.saturating_sub(depth),
            )?
            .as_stream()
            .map_err(|_| ImageError::Malformed("ICC profile"))?;
            match profile.dict.get(b"N").and_then(Object::as_i64) {
                Ok(1) => Ok((ColorSpace::Gray, None)),
                Ok(3) => Ok((ColorSpace::Rgb, None)),
                Ok(4) => Ok((ColorSpace::Cmyk, None)),
                _ => Err(ImageError::Unsupported("ICC component count")),
            }
        }
        b"Indexed" | b"I" => {
            let a = rest.ok_or(ImageError::Malformed("indexed colour space"))?;
            if a.len() != 4 {
                return Err(ImageError::Malformed("indexed colour space"));
            }
            let (base, nested) = color_space(doc, &a[1], limit, depth.saturating_add(1), budget)?;
            if nested.is_some() {
                return Err(ImageError::Unsupported("nested indexed colour space"));
            }
            let high = usize::try_from(
                a[2].as_i64()
                    .map_err(|_| ImageError::Malformed("indexed high value"))?,
            )
            .map_err(|_| ImageError::Malformed("indexed high value"))?;
            if high > 255 {
                return Err(ImageError::Malformed("indexed high value"));
            }
            let lookup = resolve(doc, &a[3], limit.saturating_sub(depth))?;
            let palette = match lookup {
                Object::String(data, _) => {
                    let size = (high + 1) * base.components();
                    if data.len() < size {
                        return Err(ImageError::Malformed("short indexed palette"));
                    }
                    budget.charge(size)?;
                    data[..size].to_vec()
                }
                Object::Stream(stream) => {
                    budget.charge(1024)?;
                    stream
                        .get_plain_content_with_limit(1024)
                        .map_err(|_| ImageError::Malformed("indexed palette"))?
                }
                _ => return Err(ImageError::Malformed("indexed palette")),
            };
            if palette.len() < (high + 1) * base.components() {
                return Err(ImageError::Malformed("short indexed palette"));
            }
            Ok((base, Some((high, palette))))
        }
        _ => Err(ImageError::Unsupported("colour space")),
    }
}

/// Decode one image XObject under the document's remaining stream budget.
pub fn decode(
    doc: &PdfDoc,
    stream_id: ObjectId,
    ctx: &ToolCtx,
    limits: &Limits,
) -> Result<DecodedImage, ImageError> {
    DecodeSession::new(doc, limits).decode(stream_id, ctx)
}

/// Shared cumulative decode budget for a sequence of images in one document.
pub struct DecodeSession<'a> {
    doc: &'a PdfDoc,
    budget: DecodeBudget,
    max_depth: u16,
    release_temporary: bool,
}

impl<'a> DecodeSession<'a> {
    /// Starts a batch with the limits used when opening the document.
    #[must_use]
    pub fn new(doc: &'a PdfDoc, limits: &Limits) -> Self {
        Self {
            doc,
            budget: doc.decode_budget(),
            max_depth: limits.max_nesting_depth,
            release_temporary: false,
        }
    }

    /// Starts a session retaining decoded pixels while releasing temporary stream buffers.
    /// The caller must retain every returned image until the session is dropped.
    pub fn for_cached_images(doc: &'a PdfDoc, limits: &Limits) -> Self {
        let mut session = Self::new(doc, limits);
        session.release_temporary = true;
        session
    }

    pub fn decode(
        &mut self,
        stream_id: ObjectId,
        ctx: &ToolCtx,
    ) -> Result<DecodedImage, ImageError> {
        decode_with_budget(
            self.doc,
            stream_id,
            ctx,
            &mut self.budget,
            self.max_depth,
            self.release_temporary,
        )
    }
}

fn decode_with_budget(
    doc: &PdfDoc,
    stream_id: ObjectId,
    ctx: &ToolCtx,
    budget: &mut DecodeBudget,
    max_depth: u16,
    release_temporary: bool,
) -> Result<DecodedImage, ImageError> {
    ctx.check()?;
    let pdf = doc.inner();
    let stream = pdf
        .get_object(stream_id)
        .and_then(Object::as_stream)
        .map_err(|_| ImageError::Malformed("image stream"))?;
    let d = &stream.dict;
    if d.get(b"ImageMask")
        .and_then(Object::as_bool)
        .unwrap_or(false)
    {
        return Err(ImageError::Unsupported("image mask"));
    }
    let dimension = |name: &[u8]| -> Result<u32, ImageError> {
        let n = d
            .get(name)
            .and_then(Object::as_i64)
            .map_err(|_| ImageError::Malformed("dimension"))?;
        if !(1..=65535).contains(&n) {
            return Err(ImageError::Malformed("dimension"));
        }
        Ok(n as u32)
    };
    let width = dimension(b"Width")?;
    let height = dimension(b"Height")?;
    let (space, palette) = color_space(
        pdf,
        d.get(b"ColorSpace")
            .map_err(|_| ImageError::Malformed("colour space"))?,
        max_depth,
        0,
        budget,
    )?;
    let components = space.components();
    let decode_object = d
        .get(b"Decode")
        .ok()
        .map(|o| resolve(pdf, o, max_depth))
        .transpose()?;
    let filters = if d.has(b"Filter") {
        stream
            .filters()
            .map_err(|_| ImageError::Malformed("image filter"))?
    } else {
        Vec::new()
    };
    let jpeg = filters.len() == 1 && filters[0] == b"DCTDecode";
    if jpeg
        && d.get(b"BitsPerComponent")
            .is_ok_and(|value| value.as_i64().ok() != Some(8))
    {
        return Err(ImageError::Unsupported("JPEG bits per component"));
    }
    let bpc = if jpeg {
        8
    } else {
        d.get(b"BitsPerComponent")
            .and_then(Object::as_i64)
            .map_err(|_| ImageError::Malformed("bits per component"))? as usize
    };
    if ![1, 2, 4, 8, 16].contains(&bpc) {
        return Err(ImageError::Unsupported("bits per component"));
    }
    let output_bits = if bpc == 16 && palette.is_none() {
        16
    } else {
        8
    };
    let pixel_count = (width as usize)
        .checked_mul(height as usize)
        .ok_or(ImageError::Malformed("dimensions overflow"))?;
    let out_len = pixel_count
        .checked_mul(components)
        .and_then(|n| n.checked_mul(usize::from(output_bits / 8)))
        .ok_or(ImageError::Malformed("dimensions overflow"))?;
    budget.charge(out_len)?;
    if jpeg {
        let params = d
            .get(b"DecodeParms")
            .ok()
            .map(|p| resolve(pdf, p, max_depth))
            .transpose()?;
        let transform = jpeg_transform(params)?;
        if palette.is_some() {
            return Err(ImageError::Unsupported("indexed JPEG"));
        }
        if space == ColorSpace::Cmyk {
            let pixels =
                decode_cmyk_jpeg(&stream.content, width, height, decode_object, transform)?;
            return Ok(DecodedImage {
                width,
                height,
                color_space: ColorSpace::Cmyk,
                bits_per_component: 8,
                pixels,
                has_soft_mask: d.has(b"SMask"),
                has_mask: d.has(b"Mask"),
            });
        }
        let (actual_space, mut data) = if space == ColorSpace::Rgb && transform.is_some() {
            (
                ColorSpace::Rgb,
                decode_rgb_jpeg(&stream.content, width, height, transform.unwrap_or(1))?,
            )
        } else {
            let mut reader =
                ImageReader::with_format(Cursor::new(&stream.content), image::ImageFormat::Jpeg);
            let mut limits = image::Limits::default();
            limits.max_image_width = Some(width);
            limits.max_image_height = Some(height);
            limits.max_alloc = Some((out_len as u64).saturating_mul(8));
            reader.limits(limits);
            let image = reader.decode()?;
            if image.width() != width || image.height() != height {
                return Err(ImageError::Malformed("JPEG dimensions differ"));
            }
            if space == ColorSpace::Gray {
                (ColorSpace::Gray, image.to_luma8().into_raw())
            } else {
                (ColorSpace::Rgb, image.to_rgb8().into_raw())
            }
        };
        if let Some(decode) = decode_object {
            let array = decode
                .as_array()
                .map_err(|_| ImageError::Malformed("decode array"))?;
            if array.len() != components * 2
                || array
                    .iter()
                    .any(|o| number(o).is_none_or(|v| !v.is_finite()))
            {
                return Err(ImageError::Malformed("decode array"));
            }
            for pixel in data.chunks_exact_mut(components) {
                for (i, value) in pixel.iter_mut().enumerate() {
                    let lo = number(&array[2 * i]).ok_or(ImageError::Malformed("decode array"))?;
                    let hi =
                        number(&array[2 * i + 1]).ok_or(ImageError::Malformed("decode array"))?;
                    *value = ((lo + f64::from(*value) / 255.0 * (hi - lo)).clamp(0.0, 1.0) * 255.0)
                        .round() as u8;
                }
            }
        }
        return Ok(DecodedImage {
            width,
            height,
            color_space: actual_space,
            bits_per_component: 8,
            pixels: data,
            has_soft_mask: d.has(b"SMask"),
            has_mask: d.has(b"Mask"),
        });
    }
    if filters
        .iter()
        .any(|f| *f != b"FlateDecode" && *f != b"LZWDecode")
    {
        return Err(ImageError::Unsupported("image filter"));
    }
    if let Ok(params) = d.get(b"DecodeParms") {
        let check = |value: &Object| -> Result<(), ImageError> {
            if matches!(value, Object::Null) {
                return Ok(());
            }
            let dict = value
                .as_dict()
                .map_err(|_| ImageError::Malformed("decode parameters"))?;
            if let Ok(predictor) = dict.get(b"Predictor") {
                let predictor = predictor
                    .as_i64()
                    .map_err(|_| ImageError::Malformed("predictor"))?;
                if predictor != 1 && predictor != 2 && !(10..=15).contains(&predictor) {
                    return Err(ImageError::Unsupported("predictor"));
                }
            }
            Ok(())
        };
        if let Object::Array(values) = params {
            for value in values {
                check(value)?;
            }
        } else {
            check(params)?;
        }
    }
    let sample_components = if palette.is_some() { 1 } else { components };
    let row_samples = width as usize * sample_components;
    let row_bytes = (row_samples * bpc).div_ceil(8);
    let data = if filters.is_empty() {
        budget.charge(stream.content.len())?;
        stream.content.clone()
    } else {
        budget
            .decode(stream)?
            .ok_or(ImageError::Malformed("compressed image"))?
    };
    if Some(data.len()) != row_bytes.checked_mul(height as usize) {
        return Err(ImageError::Malformed("image data length"));
    }
    let decode = decode_object
        .map(|o| {
            o.as_array()
                .map_err(|_| ImageError::Malformed("decode array"))
        })
        .transpose()?;
    if let Some(a) = decode
        && (a.len() != sample_components * 2
            || a.iter().any(|v| number(v).is_none_or(|n| !n.is_finite())))
    {
        return Err(ImageError::Malformed("decode array"));
    }
    let mut pixels = Vec::with_capacity(out_len);
    let max = ((1_u32 << bpc) - 1) as f64;
    let default_max = if palette.is_some() { max } else { 1.0 };
    for row in data.chunks_exact(row_bytes) {
        for x in 0..row_samples {
            let bit = x * bpc;
            let sample = if bpc == 16 {
                u32::from(u16::from_be_bytes([row[bit / 8], row[bit / 8 + 1]]))
            } else {
                u32::from((row[bit / 8] >> (8 - bpc - bit % 8)) & ((1_u16 << bpc) - 1) as u8)
            };
            let (lo, hi) = decode.map_or((0.0, default_max), |a| {
                (
                    number(&a[(x % sample_components) * 2]).unwrap_or(0.0),
                    number(&a[(x % sample_components) * 2 + 1]).unwrap_or(default_max),
                )
            });
            let value = lo + (sample as f64 / max) * (hi - lo);
            if let Some((high, table)) = &palette {
                let index = value.round().clamp(0.0, *high as f64) as usize;
                pixels.extend_from_slice(&table[index * components..(index + 1) * components]);
            } else if output_bits == 16 {
                pixels.extend_from_slice(
                    &((value.clamp(0.0, 1.0) * 65535.0).round() as u16).to_be_bytes(),
                );
            } else {
                pixels.push((value.clamp(0.0, 1.0) * 255.0).round() as u8);
            }
        }
    }
    if release_temporary {
        budget.release_temporary(data.len());
    }
    Ok(DecodedImage {
        width,
        height,
        color_space: space,
        bits_per_component: output_bits,
        pixels,
        has_soft_mask: d.has(b"SMask"),
        has_mask: d.has(b"Mask"),
    })
}

fn validate(img: &DecodedImage) -> Result<(), ImageError> {
    let len = (img.width as usize)
        .checked_mul(img.height as usize)
        .and_then(|n| n.checked_mul(img.color_space.components()))
        .and_then(|n| n.checked_mul(usize::from(img.bits_per_component / 8)));
    if img.width == 0
        || img.height == 0
        || img.width > 65535
        || img.height > 65535
        || !matches!(img.bits_per_component, 8 | 16)
        || len != Some(img.pixels.len())
    {
        return Err(ImageError::Malformed("pixel dimensions"));
    }
    Ok(())
}

/// Convert pixels to grayscale using device RGB or CMYK approximations.
pub fn to_gray(img: &DecodedImage) -> Result<DecodedImage, ImageError> {
    validate(img)?;
    let bytes_per_component = usize::from(img.bits_per_component / 8);
    let max = if bytes_per_component == 1 {
        255_u32
    } else {
        65535
    };
    let mut pixels =
        Vec::with_capacity(img.width as usize * img.height as usize * bytes_per_component);
    for p in img
        .pixels
        .chunks_exact(img.color_space.components() * bytes_per_component)
    {
        let sample = |i: usize| -> u32 {
            if bytes_per_component == 1 {
                u32::from(p[i])
            } else {
                u32::from(u16::from_be_bytes([p[2 * i], p[2 * i + 1]]))
            }
        };
        let (r, g, b) = match img.color_space {
            ColorSpace::Gray => (sample(0), sample(0), sample(0)),
            ColorSpace::Rgb => (sample(0), sample(1), sample(2)),
            ColorSpace::Cmyk => (
                ((max - sample(0)) * (max - sample(3)) + max / 2) / max,
                ((max - sample(1)) * (max - sample(3)) + max / 2) / max,
                ((max - sample(2)) * (max - sample(3)) + max / 2) / max,
            ),
        };
        let gray = (299 * r + 587 * g + 114 * b + 500) / 1000;
        if bytes_per_component == 1 {
            pixels.push(gray as u8)
        } else {
            pixels.extend_from_slice(&(gray as u16).to_be_bytes())
        }
    }
    Ok(DecodedImage {
        width: img.width,
        height: img.height,
        color_space: ColorSpace::Gray,
        bits_per_component: img.bits_per_component,
        pixels,
        has_soft_mask: img.has_soft_mask,
        has_mask: img.has_mask,
    })
}

/// Resize grayscale or RGB pixels with Lanczos3.
pub fn resample(img: &DecodedImage, width: u32, height: u32) -> Result<DecodedImage, ImageError> {
    validate(img)?;
    if width == 0
        || height == 0
        || width > 65535
        || height > 65535
        || u64::from(width)
            * u64::from(height)
            * img.color_space.components() as u64
            * u64::from(img.bits_per_component / 8)
            > 256 * 1024 * 1024
        || u64::from(img.width) * u64::from(height) * 16 > 256 * 1024 * 1024
    {
        return Err(ImageError::Malformed("resample dimensions"));
    }
    let pixels = if img.bits_per_component == 16 {
        let samples = img
            .pixels
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_be_bytes(*pair))
            .collect::<Vec<_>>();
        let values = match img.color_space {
            ColorSpace::Gray => image::imageops::resize(
                &image::ImageBuffer::<image::Luma<u16>, Vec<u16>>::from_raw(
                    img.width, img.height, samples,
                )
                .ok_or(ImageError::Malformed("pixels"))?,
                width,
                height,
                FilterType::Lanczos3,
            )
            .into_raw(),
            ColorSpace::Rgb => image::imageops::resize(
                &image::ImageBuffer::<image::Rgb<u16>, Vec<u16>>::from_raw(
                    img.width, img.height, samples,
                )
                .ok_or(ImageError::Malformed("pixels"))?,
                width,
                height,
                FilterType::Lanczos3,
            )
            .into_raw(),
            ColorSpace::Cmyk => image::imageops::resize(
                &image::ImageBuffer::<image::Rgba<u16>, Vec<u16>>::from_raw(
                    img.width, img.height, samples,
                )
                .ok_or(ImageError::Malformed("pixels"))?,
                width,
                height,
                FilterType::Lanczos3,
            )
            .into_raw(),
        };
        values.into_iter().flat_map(u16::to_be_bytes).collect()
    } else {
        match img.color_space {
            ColorSpace::Gray => image::imageops::resize(
                &image::ImageBuffer::<image::Luma<u8>, &[u8]>::from_raw(
                    img.width,
                    img.height,
                    img.pixels.as_slice(),
                )
                .ok_or(ImageError::Malformed("pixels"))?,
                width,
                height,
                FilterType::Lanczos3,
            )
            .into_raw(),
            ColorSpace::Rgb => image::imageops::resize(
                &image::ImageBuffer::<image::Rgb<u8>, &[u8]>::from_raw(
                    img.width,
                    img.height,
                    img.pixels.as_slice(),
                )
                .ok_or(ImageError::Malformed("pixels"))?,
                width,
                height,
                FilterType::Lanczos3,
            )
            .into_raw(),
            ColorSpace::Cmyk => image::imageops::resize(
                &image::ImageBuffer::<image::Rgba<u8>, &[u8]>::from_raw(
                    img.width,
                    img.height,
                    img.pixels.as_slice(),
                )
                .ok_or(ImageError::Malformed("pixels"))?,
                width,
                height,
                FilterType::Lanczos3,
            )
            .into_raw(),
        }
    };
    Ok(DecodedImage {
        width,
        height,
        color_space: img.color_space,
        bits_per_component: img.bits_per_component,
        pixels,
        has_soft_mask: img.has_soft_mask,
        has_mask: img.has_mask,
    })
}

/// Encode pixels to JPEG, reducing 16-bit samples and converting CMYK to RGB.
pub fn encode_jpeg(img: &DecodedImage, quality: u8) -> Result<Vec<u8>, ImageError> {
    encode_jpeg_with_sampling(img, quality, false)
}

/// Encode a lossy optimization image using 4:2:0 chroma subsampling.
pub fn encode_jpeg_420(img: &DecodedImage, quality: u8) -> Result<Vec<u8>, ImageError> {
    encode_jpeg_with_sampling(img, quality, true)
}

fn encode_jpeg_with_sampling(
    img: &DecodedImage,
    quality: u8,
    subsample: bool,
) -> Result<Vec<u8>, ImageError> {
    validate(img)?;
    if !(1..=100).contains(&quality) {
        return Err(ImageError::Malformed("JPEG quality"));
    }
    let eight_bit = (img.bits_per_component == 16).then(|| {
        img.pixels
            .as_chunks::<2>()
            .0
            .iter()
            .map(|p| p[0])
            .collect::<Vec<_>>()
    });
    let input = eight_bit.as_deref().unwrap_or(&img.pixels);
    let converted;
    let (color, pixels) = match img.color_space {
        ColorSpace::Gray => (ColorType::L8, input),
        ColorSpace::Rgb => (ColorType::Rgb8, input),
        ColorSpace::Cmyk => {
            converted = input
                .as_chunks::<4>()
                .0
                .iter()
                .flat_map(|p| {
                    let k = u16::from(p[3]);
                    [p[0], p[1], p[2]]
                        .map(|v| (((255 - u16::from(v)) * (255 - k) + 127) / 255) as u8)
                })
                .collect::<Vec<_>>();
            (ColorType::Rgb8, converted.as_slice())
        }
    };
    let mut bytes = Vec::new();
    if subsample {
        let width =
            u16::try_from(img.width).map_err(|_| ImageError::Malformed("JPEG dimensions"))?;
        let height =
            u16::try_from(img.height).map_err(|_| ImageError::Malformed("JPEG dimensions"))?;
        let color = match color {
            ColorType::L8 => oxiarc_jpeg::InputColor::Luma,
            _ => oxiarc_jpeg::InputColor::Rgb,
        };
        bytes = oxiarc_jpeg::encode_to_vec(pixels, width, height, color, quality)?;
    } else {
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, quality).write_image(
            pixels,
            img.width,
            img.height,
            color.into(),
        )?;
    }
    Ok(bytes)
}

fn paeth_predictor(left: u8, up: u8, upper_left: u8) -> u8 {
    let estimate = i32::from(left) + i32::from(up) - i32::from(upper_left);
    let left_distance = (estimate - i32::from(left)).abs();
    let up_distance = (estimate - i32::from(up)).abs();
    let upper_left_distance = (estimate - i32::from(upper_left)).abs();

    if left_distance <= up_distance && left_distance <= upper_left_distance {
        left
    } else if up_distance <= upper_left_distance {
        up
    } else {
        upper_left
    }
}

fn filter_row(row: &[u8], previous: &[u8], bytes_per_pixel: usize, kind: u8) -> (u64, Vec<u8>) {
    let mut filtered = Vec::with_capacity(row.len() + 1);
    filtered.push(kind);
    let mut score = 0_u64;

    for (index, &value) in row.iter().enumerate() {
        let up = previous[index];
        let (left, upper_left) = if index >= bytes_per_pixel {
            (
                row[index - bytes_per_pixel],
                previous[index - bytes_per_pixel],
            )
        } else {
            (0, 0)
        };
        let predictor = match kind {
            0 => 0,
            1 => left,
            2 => up,
            3 => ((u16::from(left) + u16::from(up)) / 2) as u8,
            _ => paeth_predictor(left, up, upper_left),
        };
        let filtered_value = value.wrapping_sub(predictor);
        score += i64::from(filtered_value as i8).unsigned_abs();
        filtered.push(filtered_value);
    }

    (score, filtered)
}

/// Encode pixels with zlib and per-row PNG predictor selection.
pub fn encode_flate(img: &DecodedImage, level: u32) -> Result<(Vec<u8>, Dictionary), ImageError> {
    validate(img)?;
    if level > 9 {
        return Err(ImageError::Malformed("compression level"));
    }
    let bpp = img.color_space.components() * usize::from(img.bits_per_component / 8);
    let row_len = img.width as usize * bpp;
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::new(level));
    let mut prev = vec![0; row_len];
    for row in img.pixels.chunks_exact(row_len) {
        let mut best_score = u64::MAX;
        let mut best = Vec::new();
        for kind in 0..=4_u8 {
            let (score, filtered) = filter_row(row, &prev, bpp, kind);
            if score < best_score {
                best_score = score;
                best = filtered;
            }
        }
        encoder.write_all(&best)?;
        prev.copy_from_slice(row);
    }
    Ok((
        encoder.finish()?,
        dictionary! { "Predictor" => 15, "Colors" => img.color_space.components() as i64, "BitsPerComponent" => i64::from(img.bits_per_component), "Columns" => i64::from(img.width) },
    ))
}
