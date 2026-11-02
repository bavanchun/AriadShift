//! Bounded PDF compression primitives.

pub mod presets;
mod protected;
pub mod prune;

use crate::{
    PdfDoc, PdfError, ToolCtx, fonts, gc,
    images::{self, DecodeSession, DecodedImage, ImageError, ImageUse},
    writer,
};
use ariad_core::limits::Limits;
use flate2::{Compression, read::ZlibDecoder, write::ZlibEncoder};
use lopdf::{Object, ObjectId};
use presets::Profile;
use std::{
    collections::HashMap,
    io::{Read, Write},
    sync::Arc,
};

pub struct ImageCandidate {
    id: ObjectId,
    image: Arc<DecodedImage>,
    placement: ImageUse,
    original_len: usize,
    memory_limit: u64,
    flate: bool,
    mask: Option<(ObjectId, Arc<DecodedImage>)>,
}

/// Collect and deduplicate reachable objects, then recompress eligible streams.
pub fn structural(pdf: &mut PdfDoc, limits: &Limits, ctx: &ToolCtx) -> Result<(), PdfError> {
    gc::collect(pdf, ctx)?;
    writer::deduplicate_streams(pdf.inner_mut()?, ctx)?;
    let mut budget = pdf.decode_budget();
    let ids: Vec<_> = pdf.inner().objects.keys().copied().collect();
    for id in ids {
        ctx.check()?;
        let Some(Object::Stream(stream)) = pdf.inner().objects.get(&id) else {
            continue;
        };
        if stream
            .filters()
            .ok()
            .and_then(|filters| filters.first().copied())
            != Some(b"FlateDecode".as_slice())
            || stream.dict.has(b"F")
        {
            continue;
        }
        // Recompress the zlib payload before PDF predictors are reversed, preserving DecodeParms.
        let mut decoder = ZlibDecoder::new(stream.content.as_slice());
        let mut decoded = Vec::new();
        let mut chunk = [0_u8; 8192];
        let mut valid = true;
        loop {
            ctx.check()?;
            match decoder.read(&mut chunk) {
                Ok(0) => break,
                Ok(size) => {
                    budget.charge(size)?;
                    decoded.extend_from_slice(&chunk[..size]);
                }
                Err(_) => {
                    valid = false;
                    break;
                }
            }
        }
        if !valid {
            continue;
        }
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::best());
        encoder
            .write_all(&decoded)
            .map_err(|_| PdfError::Malformed)?;
        let compressed = encoder.finish().map_err(|_| PdfError::Malformed)?;
        if compressed.len() < stream.content.len()
            && let Some(Object::Stream(stream)) = pdf.inner_mut()?.objects.get_mut(&id)
        {
            stream.content = compressed;
            stream.dict.set("Length", stream.content.len() as i64);
        }
    }
    let usage = match fonts::glyph_usage(pdf, ctx, limits) {
        Ok(usage) => Some(usage),
        Err(fonts::FontError::Pdf(error)) => return Err(error),
        Err(_) => None,
    };
    if let Some(usage) = &usage {
        type Subsetter = fn(
            &mut PdfDoc,
            ObjectId,
            &fonts::GlyphUsage,
            &Limits,
        ) -> Result<fonts::SubsetOutcome, fonts::FontError>;
        let subsetters: [(&[u8], Subsetter); 2] = [
            (b"Type0", fonts::subset_type0),
            (b"TrueType", fonts::subset_simple_truetype),
        ];
        for (subtype, subset) in subsetters {
            let ids: Vec<_> = pdf
                .inner()
                .objects
                .iter()
                .filter_map(|(&id, object)| {
                    (object
                        .as_dict()
                        .ok()?
                        .get(b"Subtype")
                        .and_then(Object::as_name)
                        .ok()
                        == Some(subtype))
                    .then_some(id)
                })
                .collect();
            for id in ids {
                ctx.check()?;
                if let Err(fonts::FontError::Pdf(error)) = subset(pdf, id, usage, limits) {
                    return Err(error);
                }
            }
        }
    }
    // Newly subset font programs and originally plain streams also benefit from Flate.
    let ids: Vec<_> = pdf.inner().objects.keys().copied().collect();
    for id in ids {
        ctx.check()?;
        let Some(Object::Stream(stream)) = pdf.inner().objects.get(&id) else {
            continue;
        };
        if stream.dict.has(b"Filter") || stream.dict.has(b"DecodeParms") || stream.dict.has(b"F") {
            continue;
        }
        budget.charge(stream.content.len())?;
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::best());
        for chunk in stream.content.chunks(8192) {
            ctx.check()?;
            encoder.write_all(chunk).map_err(|_| PdfError::Malformed)?;
        }
        let compressed = encoder.finish().map_err(|_| PdfError::Malformed)?;
        if compressed.len() < stream.content.len()
            && let Some(Object::Stream(stream)) = pdf.inner_mut()?.objects.get_mut(&id)
        {
            stream.content = compressed;
            stream.dict.set("Filter", "FlateDecode");
            stream.dict.set("Length", stream.content.len() as i64);
        }
    }
    prune::prune(pdf, ctx)?;
    Ok(())
}

/// Decode each eligible, placed image once for all target-size trials.
pub fn images(
    pdf: &PdfDoc,
    limits: &Limits,
    ctx: &ToolCtx,
) -> Result<Vec<ImageCandidate>, PdfError> {
    let placements: HashMap<ObjectId, ImageUse> =
        images::placements(pdf, ctx, limits).map_err(image_error)?;
    let protected = protected::soft_mask_images(pdf.inner(), ctx)?;
    let mut session = DecodeSession::for_cached_images(pdf, limits);
    let mut result = Vec::new();
    let mut decoded: HashMap<ObjectId, Arc<DecodedImage>> = HashMap::new();
    let memory_limit = image_memory_limit(limits);
    let mut cached_bytes = 0_u64;
    let mut ids: Vec<_> = placements.keys().copied().collect();
    ids.sort_unstable();
    for id in ids {
        ctx.check()?;
        if protected.contains(&id) {
            continue;
        }
        let stream = pdf
            .inner()
            .get_object(id)
            .and_then(Object::as_stream)
            .map_err(|_| PdfError::Malformed)?;
        if stream.dict.has(b"Mask")
            || stream
                .dict
                .get(b"ImageMask")
                .and_then(Object::as_bool)
                .unwrap_or(false)
            || stream
                .dict
                .get(b"BitsPerComponent")
                .and_then(Object::as_i64)
                .ok()
                == Some(1)
        {
            continue;
        }
        let filters = stream.filters().unwrap_or_default();
        if !(filters.is_empty()
            || (filters.len() == 1 && (filters[0] == b"DCTDecode" || filters[0] == b"FlateDecode")))
        {
            continue;
        }
        let Some(image) = decode_cached(
            pdf,
            id,
            &mut session,
            &mut decoded,
            &mut cached_bytes,
            memory_limit,
            ctx,
        )?
        else {
            continue;
        };
        let mask = if let Ok(value) = stream.dict.get(b"SMask") {
            let Ok(mask_id) = value.as_reference() else {
                continue;
            };
            let Ok(mask_stream) = pdf.inner().get_object(mask_id).and_then(Object::as_stream)
            else {
                continue;
            };
            if mask_stream.dict.has(b"SMask")
                || mask_stream.dict.has(b"Mask")
                || mask_stream.dict.has(b"Matte")
            {
                continue;
            }
            let Some(mask_image) = decode_cached(
                pdf,
                mask_id,
                &mut session,
                &mut decoded,
                &mut cached_bytes,
                memory_limit,
                ctx,
            )?
            else {
                continue;
            };
            if mask_image.width != image.width
                || mask_image.height != image.height
                || mask_image.color_space != images::ColorSpace::Gray
            {
                continue;
            }
            Some((mask_id, mask_image))
        } else {
            None
        };
        result.push(ImageCandidate {
            id,
            image,
            placement: placements[&id],
            original_len: stream.content.len(),
            memory_limit,
            flate: filters.len() == 1 && filters[0] == b"FlateDecode",
            mask,
        });
    }
    Ok(result)
}

pub(crate) fn image_memory_limit(limits: &Limits) -> u64 {
    limits
        .max_memory_mb
        .map_or(limits.max_decompressed_bytes, |mb| {
            limits
                .max_decompressed_bytes
                .min(u64::from(mb) * 1024 * 1024)
        })
}

pub(crate) fn preflight_image_memory(
    stream: &lopdf::Stream,
    retained_bytes: u64,
    memory_limit: u64,
) -> Result<(), PdfError> {
    let width = stream
        .dict
        .get(b"Width")
        .and_then(Object::as_i64)
        .ok()
        .and_then(|value| u64::try_from(value).ok())
        .unwrap_or(0);
    let height = stream
        .dict
        .get(b"Height")
        .and_then(Object::as_i64)
        .ok()
        .and_then(|value| u64::try_from(value).ok())
        .unwrap_or(0);
    // The image decoder may hold a decoded surface plus intermediate codec buffers.
    let estimate = width
        .saturating_mul(height)
        .saturating_mul(72)
        .saturating_add(65536);
    if retained_bytes.saturating_add(estimate) > memory_limit {
        return Err(PdfError::LimitExceeded(
            ariad_core::reader::pdf::ReadError::DecompressedTooLarge {
                limit: memory_limit,
            },
        ));
    }
    Ok(())
}

fn decode_cached(
    pdf: &PdfDoc,
    id: ObjectId,
    session: &mut DecodeSession<'_>,
    cache: &mut HashMap<ObjectId, Arc<DecodedImage>>,
    cached_bytes: &mut u64,
    memory_limit: u64,
    ctx: &ToolCtx,
) -> Result<Option<Arc<DecodedImage>>, PdfError> {
    if let Some(image) = cache.get(&id) {
        return Ok(Some(Arc::clone(image)));
    }
    let stream = pdf
        .inner()
        .get_object(id)
        .and_then(Object::as_stream)
        .map_err(|_| PdfError::Malformed)?;
    preflight_image_memory(stream, *cached_bytes, memory_limit)?;
    let image = match session.decode(id, ctx) {
        Ok(image) => image,
        Err(ImageError::Pdf(error)) => return Err(error),
        Err(_) => return Ok(None),
    };
    *cached_bytes = cached_bytes.saturating_add(image.pixels.len() as u64);
    let image = Arc::new(image);
    cache.insert(id, Arc::clone(&image));
    Ok(Some(image))
}

pub fn apply_images(
    pdf: &mut PdfDoc,
    candidates: &[ImageCandidate],
    profile: Profile,
    ctx: &ToolCtx,
) -> Result<(), PdfError> {
    if profile.lossless {
        return Ok(());
    }
    let mut unique = std::collections::HashSet::new();
    let mut cached_bytes = 0_u64;
    for candidate in candidates {
        if unique.insert(Arc::as_ptr(&candidate.image)) {
            cached_bytes = cached_bytes.saturating_add(candidate.image.pixels.len() as u64);
        }
        if let Some((_, mask)) = &candidate.mask
            && unique.insert(Arc::as_ptr(mask))
        {
            cached_bytes = cached_bytes.saturating_add(mask.pixels.len() as u64);
        }
    }
    for candidate in candidates {
        ctx.check()?;
        let dpi = candidate
            .placement
            .min_dpi_x
            .min(candidate.placement.min_dpi_y);
        let dimensions = if dpi.is_finite() && dpi > f64::from(profile.dpi) * 1.5 {
            let scale = f64::from(profile.dpi) / dpi;
            Some((
                (f64::from(candidate.image.width) * scale).round().max(1.0) as u32,
                (f64::from(candidate.image.height) * scale).round().max(1.0) as u32,
            ))
        } else {
            None
        };
        check_image_memory(candidate, dimensions, cached_bytes)?;
        let resized = dimensions
            .map(|(width, height)| {
                images::resample(&candidate.image, width, height).map_err(image_error)
            })
            .transpose()?;
        let image = resized.as_ref().unwrap_or(candidate.image.as_ref());
        let jpeg = images::encode_jpeg_420(image, profile.quality).map_err(image_error)?;
        let (encoded, flate_params) = if candidate.flate && resized.is_some() {
            let (flate, params) = images::encode_flate(image, 9).map_err(image_error)?;
            if flate.len() < jpeg.len() {
                (flate, Some(params))
            } else {
                (jpeg, None)
            }
        } else {
            (jpeg, None)
        };
        let mask_update = if resized.is_some() {
            if let Some((mask_id, mask)) = &candidate.mask {
                let scaled =
                    images::resample(mask, image.width, image.height).map_err(image_error)?;
                let (mask_bytes, params) = images::encode_flate(&scaled, 9).map_err(image_error)?;
                let mut mask_stream = pdf
                    .inner()
                    .get_object(*mask_id)
                    .and_then(Object::as_stream)
                    .map_err(|_| PdfError::Malformed)?
                    .clone();
                mask_stream.content = mask_bytes;
                mask_stream.dict.set("Width", i64::from(image.width));
                mask_stream.dict.set("Height", i64::from(image.height));
                mask_stream.dict.set("ColorSpace", "DeviceGray");
                mask_stream
                    .dict
                    .set("BitsPerComponent", i64::from(scaled.bits_per_component));
                mask_stream.dict.set("Filter", "FlateDecode");
                mask_stream.dict.remove(b"Decode");
                mask_stream.dict.set("DecodeParms", params);
                mask_stream
                    .dict
                    .set("Length", mask_stream.content.len() as i64);
                Some(mask_stream)
            } else {
                None
            }
        } else {
            None
        };
        if encoded
            .len()
            .saturating_add(mask_update.as_ref().map_or(0, |mask| mask.content.len()))
            >= candidate.original_len
        {
            continue;
        }
        let new_mask = mask_update
            .map(|mask| pdf.inner_mut().map(|doc| doc.add_object(mask)))
            .transpose()?;
        let stream = pdf
            .inner_mut()?
            .get_object_mut(candidate.id)
            .and_then(Object::as_stream_mut)
            .map_err(|_| PdfError::Malformed)?;
        stream.content = encoded;
        stream.dict.set("Width", i64::from(image.width));
        stream.dict.set("Height", i64::from(image.height));
        stream.dict.set(
            "BitsPerComponent",
            i64::from(if flate_params.is_some() {
                image.bits_per_component
            } else {
                8
            }),
        );
        stream.dict.set(
            "ColorSpace",
            match image.color_space {
                images::ColorSpace::Gray => "DeviceGray",
                images::ColorSpace::Cmyk if flate_params.is_some() => "DeviceCMYK",
                _ => "DeviceRGB",
            },
        );
        stream.dict.set(
            "Filter",
            if flate_params.is_some() {
                "FlateDecode"
            } else {
                "DCTDecode"
            },
        );
        if let Some(mask_id) = new_mask {
            stream.dict.set("SMask", Object::Reference(mask_id));
        }
        stream.dict.remove(b"Decode");
        stream.dict.remove(b"DecodeParms");
        if let Some(params) = flate_params {
            stream.dict.set("DecodeParms", params);
        }
        stream.dict.set("Length", stream.content.len() as i64);
    }
    Ok(())
}

fn check_image_memory(
    candidate: &ImageCandidate,
    dimensions: Option<(u32, u32)>,
    cached: u64,
) -> Result<(), PdfError> {
    let (width, height) = dimensions.unwrap_or((candidate.image.width, candidate.image.height));
    let pixels = u64::from(width).saturating_mul(u64::from(height));
    // Account conservatively for resize intermediates, encoders, and the aligned mask.
    let mut temporary = pixels.saturating_mul(32).saturating_add(65536);
    if dimensions.is_some() {
        temporary = temporary.saturating_add(
            u64::from(candidate.image.width)
                .saturating_mul(u64::from(height))
                .saturating_mul(16),
        );
        if candidate.image.bits_per_component == 16 {
            temporary = temporary.saturating_add(candidate.image.pixels.len() as u64);
        }
        if let Some((_, mask)) = &candidate.mask {
            temporary = temporary
                .saturating_add(
                    u64::from(mask.width)
                        .saturating_mul(u64::from(height))
                        .saturating_mul(16),
                )
                .saturating_add(pixels.saturating_mul(4));
            if mask.bits_per_component == 16 {
                temporary = temporary.saturating_add(mask.pixels.len() as u64);
            }
        }
    }
    if cached.saturating_add(temporary) > candidate.memory_limit {
        return Err(PdfError::LimitExceeded(
            ariad_core::reader::pdf::ReadError::DecompressedTooLarge {
                limit: candidate.memory_limit,
            },
        ));
    }
    Ok(())
}

fn image_error(error: ImageError) -> PdfError {
    match error {
        ImageError::Pdf(error) => error,
        ImageError::Malformed("resample dimensions") => {
            PdfError::LimitExceeded(ariad_core::reader::pdf::ReadError::DecompressedTooLarge {
                limit: 256 * 1024 * 1024,
            })
        }
        _ => PdfError::Malformed,
    }
}
