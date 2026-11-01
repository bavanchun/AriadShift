//! Implementation of the `image.to_pdf` tool (`crates/ariad-pdf/src/image_to_pdf.rs`).
//!
//! Combines one or more raster images into a single deterministic PDF document, one image per
//! page, with zero native dependencies so it runs identically on native targets and
//! `wasm32-unknown-unknown`.
//!
//! - JPEG inputs are embedded directly via `/Filter /DCTDecode` without re-encoding.
//! - PNG inputs are decoded within [`Limits`] decompression bounds and embedded via
//!   `/Filter /FlateDecode`, splitting any alpha channel (`RGBA`, `Gray+Alpha`, or `tRNS`) into a
//!   companion `/SMask` soft-mask Image XObject.

pub use ariad_core::tools::params::{ImageToPdfParams, Orientation, PageSize};
use ariad_core::{
    limits::Limits,
    reader::pdf::{CONTENT_BUDGET_BASE, CONTENT_BUDGET_PER_INPUT_BYTE, ReadError},
};
use lopdf::{Document, Object, Stream, dictionary};

use crate::{document::ToolCtx, error::PdfError};

/// Maximum pixel count accepted for a single input image (`16_384 * 16_384`).
pub const MAX_IMAGE_PIXELS: u64 = 16_384 * 16_384;

/// Maximum number of input images accepted in a single `image.to_pdf` invocation.
pub const MAX_INPUT_IMAGES: usize = 500;

const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";

/// Runs `image.to_pdf` on an sequence of raw image byte slices under `limits`.
pub fn run<I, B>(images: I, params: &ImageToPdfParams, limits: &Limits) -> Result<Vec<u8>, PdfError>
where
    I: IntoIterator<Item = B>,
    B: AsRef<[u8]>,
{
    run_with_ctx(images, params, limits, &ToolCtx::default())
}

/// Runs `image.to_pdf` on a sequence of raw image byte slices under `limits` and cooperative `ctx`.
pub fn run_with_ctx<I, B>(
    images: I,
    params: &ImageToPdfParams,
    limits: &Limits,
    ctx: &ToolCtx,
) -> Result<Vec<u8>, PdfError>
where
    I: IntoIterator<Item = B>,
    B: AsRef<[u8]>,
{
    ctx.check()?;
    limits.validate().map_err(ReadError::from)?;
    params
        .validate()
        .map_err(|err| PdfError::InvalidParams(err.to_string()))?;

    let inputs: Vec<B> = images.into_iter().collect();
    if inputs.is_empty() {
        return Err(PdfError::InvalidParams(
            "image.to_pdf requires at least one input image".to_owned(),
        ));
    }

    let page_limit = limits
        .max_pages
        .map_or(MAX_INPUT_IMAGES, |max| (max as usize).min(MAX_INPUT_IMAGES));
    if inputs.len() > page_limit {
        return Err(PdfError::LimitExceeded(ReadError::TooManyPages {
            actual: inputs.len() as u64,
            limit: u32::try_from(page_limit).unwrap_or(u32::MAX),
        }));
    }

    let mut total_input_bytes = 0_u64;
    for item in &inputs {
        let len = item.as_ref().len() as u64;
        total_input_bytes = total_input_bytes.saturating_add(len);
        if let Some(max_input) = limits.max_input_bytes
            && (len > max_input || total_input_bytes > max_input)
        {
            return Err(PdfError::LimitExceeded(ReadError::InputTooLarge {
                actual: total_input_bytes,
                limit: max_input,
            }));
        }
    }

    let amplification = CONTENT_BUDGET_BASE
        .saturating_add(CONTENT_BUDGET_PER_INPUT_BYTE.saturating_mul(total_input_bytes))
        .min((usize::MAX / 4) as u64);
    let max_decoded_bytes = limits.max_decompressed_bytes.min(amplification);
    let mut decoded_so_far = 0_u64;

    let mut doc = Document::with_version("1.7");
    let catalog_id = doc.new_object_id();
    let pages_id = doc.new_object_id();
    let mut page_ids = Vec::with_capacity(inputs.len());

    for item in &inputs {
        ctx.check()?;
        let raw = item.as_ref();
        let prepared = prepare_image(raw, max_decoded_bytes, &mut decoded_so_far, ctx)?;
        let (page_w, page_h, draw_x, draw_y, draw_w, draw_h) =
            compute_page_placement(prepared.width, prepared.height, params);

        let smask_ref = if let Some(alpha_bytes) = prepared.smask {
            let smask_stream = build_flate_stream(
                dictionary! {
                    "Type" => "XObject",
                    "Subtype" => "Image",
                    "Width" => i64::from(prepared.width),
                    "Height" => i64::from(prepared.height),
                    "ColorSpace" => "DeviceGray",
                    "BitsPerComponent" => 8_i64,
                },
                alpha_bytes,
            )?;
            Some(doc.add_object(smask_stream))
        } else {
            None
        };

        let mut img_dict = dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => i64::from(prepared.width),
            "Height" => i64::from(prepared.height),
            "ColorSpace" => prepared.color_space.as_pdf_name(),
            "BitsPerComponent" => i64::from(prepared.bits_per_component),
        };
        if let Some(smask_id) = smask_ref {
            img_dict.set("SMask", Object::Reference(smask_id));
        }

        let img_stream = match prepared.encoding {
            ImageEncoding::JpegPassthrough => {
                img_dict.set("Filter", "DCTDecode");
                Stream::new(img_dict, prepared.data)
            }
            ImageEncoding::FlateRaw => build_flate_stream(img_dict, prepared.data)?,
        };
        let img_id = doc.add_object(img_stream);

        let content_ops = format!(
            "q {} 0 0 {} {} {} cm /Im0 Do Q\n",
            fmt_pt(draw_w),
            fmt_pt(draw_h),
            fmt_pt(draw_x),
            fmt_pt(draw_y),
        );
        let content_stream = Stream::new(dictionary! {}, content_ops.into_bytes());
        let content_id = doc.add_object(content_stream);

        let page_dict = dictionary! {
            "Type" => "Page",
            "Parent" => Object::Reference(pages_id),
            "MediaBox" => vec![
                Object::Integer(0),
                Object::Integer(0),
                pt_object(page_w),
                pt_object(page_h),
            ],
            "Resources" => dictionary! {
                "XObject" => dictionary! {
                    "Im0" => Object::Reference(img_id),
                },
            },
            "Contents" => Object::Reference(content_id),
        };
        let page_id = doc.add_object(page_dict);
        page_ids.push(Object::Reference(page_id));
    }

    let page_count = i64::try_from(page_ids.len()).unwrap_or(i64::MAX);
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => page_ids,
            "Count" => page_count,
        }),
    );
    doc.objects.insert(
        catalog_id,
        Object::Dictionary(dictionary! {
            "Type" => "Catalog",
            "Pages" => Object::Reference(pages_id),
        }),
    );
    doc.trailer.set("Root", Object::Reference(catalog_id));

    let mut out = Vec::new();
    doc.save_to(&mut out).map_err(|_| PdfError::Malformed)?;
    Ok(out)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(clippy::enum_variant_names)]
enum ColorSpace {
    DeviceGray,
    DeviceRgb,
    DeviceCmyk,
}

impl ColorSpace {
    const fn as_pdf_name(self) -> &'static str {
        match self {
            Self::DeviceGray => "DeviceGray",
            Self::DeviceRgb => "DeviceRGB",
            Self::DeviceCmyk => "DeviceCMYK",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ImageEncoding {
    JpegPassthrough,
    FlateRaw,
}

struct PreparedImage {
    width: u32,
    height: u32,
    color_space: ColorSpace,
    bits_per_component: u8,
    encoding: ImageEncoding,
    data: Vec<u8>,
    smask: Option<Vec<u8>>,
}

fn prepare_image(
    bytes: &[u8],
    max_decoded_bytes: u64,
    decoded_so_far: &mut u64,
    ctx: &ToolCtx,
) -> Result<PreparedImage, PdfError> {
    if bytes.starts_with(PNG_SIGNATURE) {
        return prepare_png(bytes, max_decoded_bytes, decoded_so_far, ctx);
    }
    if bytes.starts_with(b"\xFF\xD8\xFF") {
        return prepare_jpeg(bytes, max_decoded_bytes, decoded_so_far, ctx);
    }
    if is_heic_or_avif(bytes) {
        return Err(PdfError::InvalidParams(
            "unsupported image format `heic`: HEIC/AVIF is not supported".to_owned(),
        ));
    }
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Err(PdfError::InvalidParams(
            "unsupported image format `webp`: requires the `image` crate".to_owned(),
        ));
    }
    if bytes.starts_with(b"II*\x00") || bytes.starts_with(b"MM\x00*") {
        return Err(PdfError::InvalidParams(
            "unsupported image format `tiff`: requires the `image` crate".to_owned(),
        ));
    }
    if bytes.starts_with(b"BM") {
        return Err(PdfError::InvalidParams(
            "unsupported image format `bmp`: requires the `image` crate".to_owned(),
        ));
    }
    Err(PdfError::InvalidParams(
        "unsupported or unrecognized image format".to_owned(),
    ))
}

fn is_heic_or_avif(bytes: &[u8]) -> bool {
    if bytes.len() < 12 || &bytes[4..8] != b"ftyp" {
        return false;
    }
    matches!(
        &bytes[8..12],
        b"heic" | b"heix" | b"hevc" | b"hevx" | b"mif1" | b"msf1" | b"avif" | b"avis"
    )
}

fn check_dimensions_and_budget(
    width: u32,
    height: u32,
    channels: u64,
    max_decoded_bytes: u64,
    decoded_so_far: &mut u64,
) -> Result<(), PdfError> {
    if width == 0 || height == 0 {
        return Err(PdfError::InvalidParams(
            "image dimensions must be non-zero".to_owned(),
        ));
    }
    let pixels = u64::from(width)
        .checked_mul(u64::from(height))
        .ok_or(PdfError::LimitExceeded(ReadError::DecompressedTooLarge {
            limit: max_decoded_bytes,
        }))?;
    if pixels > MAX_IMAGE_PIXELS {
        return Err(PdfError::LimitExceeded(ReadError::DecompressedTooLarge {
            limit: MAX_IMAGE_PIXELS,
        }));
    }
    let byte_cost = pixels.saturating_mul(channels);
    *decoded_so_far = decoded_so_far.saturating_add(byte_cost);
    if *decoded_so_far > max_decoded_bytes {
        return Err(PdfError::LimitExceeded(ReadError::DecompressedTooLarge {
            limit: max_decoded_bytes,
        }));
    }
    Ok(())
}

fn prepare_jpeg(
    bytes: &[u8],
    max_decoded_bytes: u64,
    decoded_so_far: &mut u64,
    ctx: &ToolCtx,
) -> Result<PreparedImage, PdfError> {
    if bytes.len() < 4 || &bytes[0..2] != b"\xFF\xD8" {
        return Err(PdfError::InvalidParams("corrupt JPEG header".to_owned()));
    }

    let mut pos = 2_usize;
    let mut sof_info: Option<(u32, u32, ColorSpace)> = None;
    let mut saw_sos_or_eoi = false;

    while pos < bytes.len() {
        ctx.check()?;
        if bytes[pos] != 0xFF {
            return Err(PdfError::InvalidParams(
                "corrupt JPEG marker structure".to_owned(),
            ));
        }
        while pos < bytes.len() && bytes[pos] == 0xFF {
            pos += 1;
        }
        if pos >= bytes.len() {
            break;
        }
        let marker = bytes[pos];
        pos += 1;

        match marker {
            0x00 => {
                return Err(PdfError::InvalidParams(
                    "unexpected stuffed byte outside JPEG scan".to_owned(),
                ));
            }
            0xD8 | 0x01 | 0xD0..=0xD7 => continue,
            0xD9 => {
                saw_sos_or_eoi = true;
                break;
            }
            _ => {}
        }

        if pos + 2 > bytes.len() {
            return Err(PdfError::InvalidParams(
                "truncated JPEG marker length".to_owned(),
            ));
        }
        let seg_len = u16::from_be_bytes([bytes[pos], bytes[pos + 1]]) as usize;
        if seg_len < 2 || pos + seg_len > bytes.len() {
            return Err(PdfError::InvalidParams(
                "truncated JPEG marker segment".to_owned(),
            ));
        }
        let payload = &bytes[pos + 2..pos + seg_len];

        match marker {
            0xC0..=0xC2 => {
                if payload.len() < 6 {
                    return Err(PdfError::InvalidParams(
                        "truncated JPEG SOF segment".to_owned(),
                    ));
                }
                let precision = payload[0];
                if precision != 8 {
                    return Err(PdfError::InvalidParams(
                        "only 8-bit JPEG precision is supported".to_owned(),
                    ));
                }
                let height = u32::from(u16::from_be_bytes([payload[1], payload[2]]));
                let width = u32::from(u16::from_be_bytes([payload[3], payload[4]]));
                let components = payload[5];
                let color_space = match components {
                    1 => ColorSpace::DeviceGray,
                    3 => ColorSpace::DeviceRgb,
                    4 => ColorSpace::DeviceCmyk,
                    _ => {
                        return Err(PdfError::InvalidParams(
                            "unsupported JPEG component count".to_owned(),
                        ));
                    }
                };
                check_dimensions_and_budget(
                    width,
                    height,
                    u64::from(components),
                    max_decoded_bytes,
                    decoded_so_far,
                )?;
                sof_info = Some((width, height, color_space));
            }
            0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF => {
                return Err(PdfError::InvalidParams(
                    "unsupported non-DCT or arithmetic-coded JPEG".to_owned(),
                ));
            }
            0xDA => {
                // Start of Scan: verify image data follows and EOI marker exists.
                let scan_data = &bytes[pos + seg_len..];
                if scan_data.len() < 2 || !scan_data.windows(2).any(|w| w == b"\xFF\xD9") {
                    return Err(PdfError::InvalidParams(
                        "truncated JPEG entropy scan (missing EOI)".to_owned(),
                    ));
                }
                saw_sos_or_eoi = true;
                break;
            }
            _ => {}
        }

        pos += seg_len;
    }

    let Some((width, height, color_space)) = sof_info else {
        return Err(PdfError::InvalidParams(
            "JPEG is missing a valid SOF frame header".to_owned(),
        ));
    };
    if !saw_sos_or_eoi {
        return Err(PdfError::InvalidParams(
            "JPEG is missing scan data".to_owned(),
        ));
    }

    Ok(PreparedImage {
        width,
        height,
        color_space,
        bits_per_component: 8,
        encoding: ImageEncoding::JpegPassthrough,
        data: bytes.to_vec(),
        smask: None,
    })
}

#[allow(clippy::too_many_lines)]
fn prepare_png(
    bytes: &[u8],
    max_decoded_bytes: u64,
    decoded_so_far: &mut u64,
    ctx: &ToolCtx,
) -> Result<PreparedImage, PdfError> {
    if bytes.len() < 33 || !bytes.starts_with(PNG_SIGNATURE) {
        return Err(PdfError::InvalidParams("corrupt PNG header".to_owned()));
    }

    let mut pos = PNG_SIGNATURE.len();
    let mut ihdr: Option<(u32, u32, u8, u8)> = None;
    let mut palette: Option<Vec<[u8; 3]>> = None;
    let mut trns: Option<Vec<u8>> = None;
    let mut idat = Vec::new();
    let mut saw_iend = false;

    while pos < bytes.len() {
        ctx.check()?;
        if pos + 12 > bytes.len() {
            return Err(PdfError::InvalidParams(
                "truncated PNG chunk header".to_owned(),
            ));
        }
        let chunk_len =
            u32::from_be_bytes([bytes[pos], bytes[pos + 1], bytes[pos + 2], bytes[pos + 3]])
                as usize;
        let chunk_type = &bytes[pos + 4..pos + 8];
        let data_start = pos + 8;
        let Some(data_end) = data_start.checked_add(chunk_len) else {
            return Err(PdfError::InvalidParams(
                "overflow in PNG chunk length".to_owned(),
            ));
        };
        let Some(chunk_end) = data_end.checked_add(4) else {
            return Err(PdfError::InvalidParams(
                "overflow in PNG chunk CRC".to_owned(),
            ));
        };
        if chunk_end > bytes.len() {
            return Err(PdfError::InvalidParams(
                "truncated PNG chunk payload".to_owned(),
            ));
        }
        let chunk_data = &bytes[data_start..data_end];

        match chunk_type {
            b"IHDR" => {
                if ihdr.is_some() || chunk_len != 13 {
                    return Err(PdfError::InvalidParams("invalid PNG IHDR chunk".to_owned()));
                }
                let width = u32::from_be_bytes([
                    chunk_data[0],
                    chunk_data[1],
                    chunk_data[2],
                    chunk_data[3],
                ]);
                let height = u32::from_be_bytes([
                    chunk_data[4],
                    chunk_data[5],
                    chunk_data[6],
                    chunk_data[7],
                ]);
                let bit_depth = chunk_data[8];
                let color_type = chunk_data[9];
                let compression = chunk_data[10];
                let filter = chunk_data[11];
                let interlace = chunk_data[12];

                if compression != 0 || filter != 0 {
                    return Err(PdfError::InvalidParams(
                        "unsupported PNG compression or filter method".to_owned(),
                    ));
                }
                if interlace != 0 {
                    return Err(PdfError::InvalidParams(
                        "interlaced PNG is not supported".to_owned(),
                    ));
                }
                let valid_depth = match color_type {
                    0 => matches!(bit_depth, 1 | 2 | 4 | 8 | 16),
                    2 | 4 | 6 => matches!(bit_depth, 8 | 16),
                    3 => matches!(bit_depth, 1 | 2 | 4 | 8),
                    _ => false,
                };
                if !valid_depth {
                    return Err(PdfError::InvalidParams(
                        "invalid PNG color type or bit depth".to_owned(),
                    ));
                }
                let channels: u64 = match color_type {
                    0 => 1,
                    2 | 3 => 3,
                    4 => 2,
                    6 => 4,
                    _ => unreachable!(),
                };
                check_dimensions_and_budget(
                    width,
                    height,
                    channels,
                    max_decoded_bytes,
                    decoded_so_far,
                )?;
                ihdr = Some((width, height, bit_depth, color_type));
            }
            b"PLTE" => {
                if chunk_len == 0 || !chunk_len.is_multiple_of(3) || chunk_len > 256 * 3 {
                    return Err(PdfError::InvalidParams("invalid PNG PLTE chunk".to_owned()));
                }
                let entries = chunk_data.as_chunks::<3>().0.to_vec();
                palette = Some(entries);
            }
            b"tRNS" => {
                trns = Some(chunk_data.to_vec());
            }
            b"IDAT" => {
                if ihdr.is_none() {
                    return Err(PdfError::InvalidParams(
                        "PNG IDAT appeared before IHDR".to_owned(),
                    ));
                }
                idat.extend_from_slice(chunk_data);
            }
            b"IEND" => {
                saw_iend = true;
                break;
            }
            _ => {}
        }

        pos = chunk_end;
    }

    let Some((width, height, bit_depth, color_type)) = ihdr else {
        return Err(PdfError::InvalidParams("PNG is missing IHDR".to_owned()));
    };
    if !saw_iend || idat.is_empty() {
        return Err(PdfError::InvalidParams(
            "PNG is missing IDAT or IEND chunk".to_owned(),
        ));
    }

    let samples_per_pixel: usize = match color_type {
        0 | 3 => 1,
        2 => 3,
        4 => 2,
        6 => 4,
        _ => unreachable!(),
    };
    let bits_per_pixel = samples_per_pixel * (bit_depth as usize);
    let bpp = bits_per_pixel.div_ceil(8);
    let row_bytes = (width as usize)
        .checked_mul(bits_per_pixel)
        .ok_or_else(|| PdfError::InvalidParams("PNG scanline width overflow".to_owned()))?
        .div_ceil(8);
    let expected_raw_len = (height as usize)
        .checked_mul(row_bytes.saturating_add(1))
        .ok_or_else(|| PdfError::InvalidParams("PNG image size overflow".to_owned()))?;

    let budget_cap = (max_decoded_bytes as usize).max(expected_raw_len);
    let zlib_stream = Stream::new(dictionary! { "Filter" => "FlateDecode" }, idat);
    let decompressed = match zlib_stream.decompressed_content_with_limit(budget_cap) {
        Ok(data) => data,
        Err(lopdf::Error::Decompress(lopdf::DecompressError::MemoryLimitExceeded { .. })) => {
            return Err(PdfError::LimitExceeded(ReadError::DecompressedTooLarge {
                limit: max_decoded_bytes,
            }));
        }
        Err(_) => {
            return Err(PdfError::InvalidParams(
                "corrupt PNG zlib stream".to_owned(),
            ));
        }
    };
    if decompressed.len() != expected_raw_len {
        return Err(PdfError::InvalidParams(
            "PNG decompressed scanline length mismatch".to_owned(),
        ));
    }

    let unfiltered = unfilter_scanlines(&decompressed, height as usize, row_bytes, bpp, ctx)?;
    decode_png_pixels(
        width,
        height,
        bit_depth,
        color_type,
        row_bytes,
        palette.as_deref(),
        trns.as_deref(),
        &unfiltered,
        ctx,
    )
}

fn unfilter_scanlines(
    raw: &[u8],
    height: usize,
    row_bytes: usize,
    bpp: usize,
    ctx: &ToolCtx,
) -> Result<Vec<u8>, PdfError> {
    let mut out = vec![0_u8; height * row_bytes];
    let stride = row_bytes + 1;

    for y in 0..height {
        if y & 0x3f == 0 {
            ctx.check()?;
        }
        let filter = raw[y * stride];
        let src = &raw[y * stride + 1..(y + 1) * stride];
        let (prev_rows, cur_rows) = out.split_at_mut(y * row_bytes);
        let cur = &mut cur_rows[..row_bytes];
        let prev = if y > 0 {
            Some(&prev_rows[(y - 1) * row_bytes..y * row_bytes])
        } else {
            None
        };

        for i in 0..row_bytes {
            let x = src[i];
            let a = if i >= bpp { cur[i - bpp] } else { 0 };
            let b = prev.map_or(0, |p| p[i]);
            let c = match prev {
                Some(p) if i >= bpp => p[i - bpp],
                _ => 0,
            };
            cur[i] = match filter {
                0 => x,
                1 => x.wrapping_add(a),
                2 => x.wrapping_add(b),
                3 => x.wrapping_add(((u16::from(a) + u16::from(b)) / 2) as u8),
                4 => x.wrapping_add(paeth_predictor(a, b, c)),
                _ => {
                    return Err(PdfError::InvalidParams(
                        "invalid PNG scanline filter type".to_owned(),
                    ));
                }
            };
        }
    }

    Ok(out)
}

fn paeth_predictor(a: u8, b: u8, c: u8) -> u8 {
    let a_i = i32::from(a);
    let b_i = i32::from(b);
    let c_i = i32::from(c);
    let p = a_i + b_i - c_i;
    let pa = (p - a_i).abs();
    let pb = (p - b_i).abs();
    let pc = (p - c_i).abs();
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn decode_png_pixels(
    width: u32,
    height: u32,
    bit_depth: u8,
    color_type: u8,
    row_bytes: usize,
    palette: Option<&[[u8; 3]]>,
    trns: Option<&[u8]>,
    unfiltered: &[u8],
    ctx: &ToolCtx,
) -> Result<PreparedImage, PdfError> {
    let pixel_count = (width as usize) * (height as usize);
    match color_type {
        // Grayscale
        0 => {
            let gray8 = unpack_gray_samples(
                width as usize,
                height as usize,
                bit_depth,
                row_bytes,
                unfiltered,
            )?;
            let smask = if let Some(trns_bytes) = trns {
                if trns_bytes.len() < 2 {
                    return Err(PdfError::InvalidParams(
                        "invalid PNG grayscale tRNS chunk".to_owned(),
                    ));
                }
                let key_val = u16::from_be_bytes([trns_bytes[0], trns_bytes[1]]);
                let key_u8 = scale_sample_to_u8(key_val, bit_depth);
                Some(
                    gray8
                        .iter()
                        .map(|&g| if g == key_u8 { 0 } else { 255 })
                        .collect(),
                )
            } else {
                None
            };
            Ok(PreparedImage {
                width,
                height,
                color_space: ColorSpace::DeviceGray,
                bits_per_component: 8,
                encoding: ImageEncoding::FlateRaw,
                data: gray8,
                smask,
            })
        }
        // Truecolor RGB
        2 => {
            let rgb8 = if bit_depth == 8 {
                unfiltered.to_vec()
            } else {
                // 16-bit big-endian samples: take high byte of each channel
                unfiltered.as_chunks::<2>().0.iter().map(|c| c[0]).collect()
            };
            let smask = if let Some(trns_bytes) = trns {
                if trns_bytes.len() < 6 {
                    return Err(PdfError::InvalidParams(
                        "invalid PNG RGB tRNS chunk".to_owned(),
                    ));
                }
                let kr = scale_sample_to_u8(
                    u16::from_be_bytes([trns_bytes[0], trns_bytes[1]]),
                    bit_depth,
                );
                let kg = scale_sample_to_u8(
                    u16::from_be_bytes([trns_bytes[2], trns_bytes[3]]),
                    bit_depth,
                );
                let kb = scale_sample_to_u8(
                    u16::from_be_bytes([trns_bytes[4], trns_bytes[5]]),
                    bit_depth,
                );
                Some(
                    rgb8.as_chunks::<3>()
                        .0
                        .iter()
                        .map(|px| {
                            if px[0] == kr && px[1] == kg && px[2] == kb {
                                0
                            } else {
                                255
                            }
                        })
                        .collect(),
                )
            } else {
                None
            };
            Ok(PreparedImage {
                width,
                height,
                color_space: ColorSpace::DeviceRgb,
                bits_per_component: 8,
                encoding: ImageEncoding::FlateRaw,
                data: rgb8,
                smask,
            })
        }
        // Indexed palette
        3 => {
            let Some(pal) = palette else {
                return Err(PdfError::InvalidParams(
                    "indexed PNG is missing PLTE chunk".to_owned(),
                ));
            };
            let indices = unpack_palette_indices(
                width as usize,
                height as usize,
                bit_depth,
                row_bytes,
                unfiltered,
            )?;
            let mut rgb = Vec::with_capacity(pixel_count * 3);
            let mut alpha = trns.map(|_| Vec::with_capacity(pixel_count));
            for &idx in &indices {
                let Some(&[r, g, b]) = pal.get(idx as usize) else {
                    return Err(PdfError::InvalidParams(
                        "PNG palette index out of bounds".to_owned(),
                    ));
                };
                rgb.extend_from_slice(&[r, g, b]);
                if let (Some(a_vec), Some(trns_slice)) = (alpha.as_mut(), trns) {
                    let a = trns_slice.get(idx as usize).copied().unwrap_or(255);
                    a_vec.push(a);
                }
            }
            Ok(PreparedImage {
                width,
                height,
                color_space: ColorSpace::DeviceRgb,
                bits_per_component: 8,
                encoding: ImageEncoding::FlateRaw,
                data: rgb,
                smask: alpha,
            })
        }
        // Grayscale + Alpha
        4 => {
            let mut gray = Vec::with_capacity(pixel_count);
            let mut alpha = Vec::with_capacity(pixel_count);
            let step = if bit_depth == 8 { 2 } else { 4 };
            for (i, px) in unfiltered.chunks_exact(step).enumerate() {
                if i & 0x3fff == 0 {
                    ctx.check()?;
                }
                if bit_depth == 8 {
                    gray.push(px[0]);
                    alpha.push(px[1]);
                } else {
                    gray.push(px[0]);
                    alpha.push(px[2]);
                }
            }
            Ok(PreparedImage {
                width,
                height,
                color_space: ColorSpace::DeviceGray,
                bits_per_component: 8,
                encoding: ImageEncoding::FlateRaw,
                data: gray,
                smask: Some(alpha),
            })
        }
        // RGBA
        6 => {
            let mut rgb = Vec::with_capacity(pixel_count * 3);
            let mut alpha = Vec::with_capacity(pixel_count);
            let step = if bit_depth == 8 { 4 } else { 8 };
            for (i, px) in unfiltered.chunks_exact(step).enumerate() {
                if i & 0x3fff == 0 {
                    ctx.check()?;
                }
                if bit_depth == 8 {
                    rgb.extend_from_slice(&[px[0], px[1], px[2]]);
                    alpha.push(px[3]);
                } else {
                    rgb.extend_from_slice(&[px[0], px[2], px[4]]);
                    alpha.push(px[6]);
                }
            }
            Ok(PreparedImage {
                width,
                height,
                color_space: ColorSpace::DeviceRgb,
                bits_per_component: 8,
                encoding: ImageEncoding::FlateRaw,
                data: rgb,
                smask: Some(alpha),
            })
        }
        _ => Err(PdfError::InvalidParams(
            "unsupported PNG color type".to_owned(),
        )),
    }
}

fn unpack_gray_samples(
    width: usize,
    height: usize,
    bit_depth: u8,
    row_bytes: usize,
    unfiltered: &[u8],
) -> Result<Vec<u8>, PdfError> {
    if bit_depth == 8 {
        return Ok(unfiltered.to_vec());
    }
    if bit_depth == 16 {
        return Ok(unfiltered.as_chunks::<2>().0.iter().map(|c| c[0]).collect());
    }
    let indices = unpack_palette_indices(width, height, bit_depth, row_bytes, unfiltered)?;
    let max_val = (1_u16 << bit_depth) - 1;
    Ok(indices
        .into_iter()
        .map(|v| ((u16::from(v) * 255 + max_val / 2) / max_val) as u8)
        .collect())
}

fn unpack_palette_indices(
    width: usize,
    height: usize,
    bit_depth: u8,
    row_bytes: usize,
    unfiltered: &[u8],
) -> Result<Vec<u8>, PdfError> {
    if bit_depth == 8 {
        return Ok(unfiltered.to_vec());
    }
    let mut out = Vec::with_capacity(width * height);
    let mask = (1_u8 << bit_depth) - 1;
    let samples_per_byte = 8 / (bit_depth as usize);
    for y in 0..height {
        let row = &unfiltered[y * row_bytes..(y + 1) * row_bytes];
        let mut x = 0;
        for &byte in row {
            for s in (0..samples_per_byte).rev() {
                if x >= width {
                    break;
                }
                let shift = s * (bit_depth as usize);
                out.push((byte >> shift) & mask);
                x += 1;
            }
        }
    }
    Ok(out)
}

fn scale_sample_to_u8(val: u16, bit_depth: u8) -> u8 {
    match bit_depth {
        16 => (val >> 8) as u8,
        8 => (val & 0xFF) as u8,
        1 | 2 | 4 => {
            let max_val = (1_u16 << bit_depth) - 1;
            (((val & max_val) * 255 + max_val / 2) / max_val) as u8
        }
        _ => (val & 0xFF) as u8,
    }
}

/// Computes `(page_w, page_h, draw_x, draw_y, draw_w, draw_h)` in PDF points.
fn compute_page_placement(
    img_width: u32,
    img_height: u32,
    params: &ImageToPdfParams,
) -> (f64, f64, f64, f64, f64, f64) {
    let img_w = f64::from(img_width);
    let img_h = f64::from(img_height);
    let margin = params.margin_pt;

    match params.page.portrait_pt() {
        None => {
            let page_w = img_w + 2.0 * margin;
            let page_h = img_h + 2.0 * margin;
            (page_w, page_h, margin, margin, img_w, img_h)
        }
        Some((short_pt, long_pt)) => {
            let (page_w, page_h) = match params.orientation {
                Orientation::Portrait => (short_pt, long_pt),
                Orientation::Landscape => (long_pt, short_pt),
                Orientation::Auto => {
                    if img_width > img_height {
                        (long_pt, short_pt)
                    } else {
                        (short_pt, long_pt)
                    }
                }
            };
            let avail_w = (page_w - 2.0 * margin).max(1.0);
            let avail_h = (page_h - 2.0 * margin).max(1.0);
            let scale = (avail_w / img_w).min(avail_h / img_h);
            let draw_w = img_w * scale;
            let draw_h = img_h * scale;
            let draw_x = (page_w - draw_w) / 2.0;
            let draw_y = (page_h - draw_h) / 2.0;
            (page_w, page_h, draw_x, draw_y, draw_w, draw_h)
        }
    }
}

fn round_4(v: f64) -> f64 {
    (v * 10_000.0).round() / 10_000.0
}

fn pt_object(v: f64) -> Object {
    let r = round_4(v);
    if (r - r.round()).abs() < 1e-6 {
        Object::Integer(r.round() as i64)
    } else {
        Object::Real(r as f32)
    }
}

fn fmt_pt(v: f64) -> String {
    let r = round_4(v);
    if (r - r.round()).abs() < 1e-6 {
        format!("{}", r.round() as i64)
    } else {
        let s = format!("{r:.4}");
        s.trim_end_matches('0').trim_end_matches('.').to_owned()
    }
}

fn build_flate_stream(mut dict: lopdf::Dictionary, raw: Vec<u8>) -> Result<Stream, PdfError> {
    let mut stream = Stream::new(dict.clone(), raw.clone());
    stream.compress().map_err(|_| PdfError::Malformed)?;
    if stream.dict.has(b"Filter") {
        return Ok(stream);
    }
    // `lopdf::Stream::compress` skips setting `/Filter /FlateDecode` when the compressed buffer is
    // not at least 20 bytes smaller than `raw` (common on tiny images). Wrap `raw` in a valid
    // RFC 1950 zlib stream so `/Filter /FlateDecode` is always present.
    dict.set("Filter", "FlateDecode");
    Ok(Stream::new(dict, zlib_stored(&raw)))
}

fn zlib_stored(raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(raw.len() + (raw.len() / 65_535 + 1) * 5 + 6);
    out.extend_from_slice(&[0x78, 0x01]);
    if raw.is_empty() {
        out.extend_from_slice(&[0x01, 0x00, 0x00, 0xFF, 0xFF]);
    } else {
        let chunks: Vec<&[u8]> = raw.chunks(65_535).collect();
        for (idx, chunk) in chunks.iter().enumerate() {
            let is_last = idx + 1 == chunks.len();
            out.push(u8::from(is_last));
            let len = u16::try_from(chunk.len()).unwrap_or(u16::MAX);
            out.extend_from_slice(&len.to_le_bytes());
            out.extend_from_slice(&(!len).to_le_bytes());
            out.extend_from_slice(chunk);
        }
    }
    let mut a: u32 = 1;
    let mut b: u32 = 0;
    for &byte in raw {
        a = (a + u32::from(byte)) % 65_521;
        b = (b + a) % 65_521;
    }
    let adler = (b << 16) | a;
    out.extend_from_slice(&adler.to_be_bytes());
    out
}
