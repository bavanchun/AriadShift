//! Bounded single-frame raster decoding for OCR.
use super::common::EngineFailure;
use ariad_core::limits::Limits;
use image::{GenericImageView, GrayImage, ImageReader};
use std::{
    io::{Cursor, Read},
    path::Path,
};

const MAX_IMAGE_BYTES: u64 = 128 * 1024 * 1024;
const MAX_PIXELS: u64 = 100_000_000;

fn image_error(error: image::ImageError) -> EngineFailure {
    match error {
        image::ImageError::Limits(_) => EngineFailure::limit_exceeded(),
        _ => EngineFailure::invalid_request(),
    }
}

// image decodes the first TIFF frame; reject a chain of frames instead of silently losing pages.
fn single_frame_tiff(bytes: &[u8]) -> Result<(), EngineFailure> {
    if bytes.len() < 8 {
        return Err(EngineFailure::invalid_request());
    }
    let little = match &bytes[..4] {
        b"II*\0" => true,
        b"MM\0*" => false,
        _ => return Err(EngineFailure::invalid_request()),
    };
    let word = |offset: usize| -> Result<u16, EngineFailure> {
        let raw: [u8; 2] = bytes
            .get(
                offset
                    ..offset
                        .checked_add(2)
                        .ok_or_else(EngineFailure::invalid_request)?,
            )
            .ok_or_else(EngineFailure::invalid_request)?
            .try_into()
            .map_err(|_| EngineFailure::invalid_request())?;
        Ok(if little {
            u16::from_le_bytes(raw)
        } else {
            u16::from_be_bytes(raw)
        })
    };
    let dword = |offset: usize| -> Result<u32, EngineFailure> {
        let raw: [u8; 4] = bytes
            .get(
                offset
                    ..offset
                        .checked_add(4)
                        .ok_or_else(EngineFailure::invalid_request)?,
            )
            .ok_or_else(EngineFailure::invalid_request)?
            .try_into()
            .map_err(|_| EngineFailure::invalid_request())?;
        Ok(if little {
            u32::from_le_bytes(raw)
        } else {
            u32::from_be_bytes(raw)
        })
    };
    let start = usize::try_from(dword(4)?).map_err(|_| EngineFailure::invalid_request())?;
    let count = usize::from(word(start)?);
    let end = start
        .checked_add(2)
        .and_then(|n| count.checked_mul(12).and_then(|c| n.checked_add(c)))
        .ok_or_else(EngineFailure::invalid_request)?;
    if dword(end)? != 0 {
        return Err(EngineFailure::invalid_request());
    }
    Ok(())
}

pub(crate) fn decode(
    path: &Path,
    limits: &Limits,
    deadline: std::time::Instant,
) -> Result<GrayImage, EngineFailure> {
    let check = || {
        if std::time::Instant::now() >= deadline {
            Err(EngineFailure::limit_exceeded())
        } else {
            Ok(())
        }
    };
    check()?;
    let cap = limits
        .max_input_bytes
        .unwrap_or(MAX_IMAGE_BYTES)
        .min(MAX_IMAGE_BYTES);
    let metadata = std::fs::symlink_metadata(path).map_err(|_| EngineFailure::io())?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(EngineFailure::invalid_request());
    }
    if metadata.len() > cap {
        return Err(EngineFailure::limit_exceeded());
    }
    let file = std::fs::File::open(path).map_err(|_| EngineFailure::io())?;
    let mut bytes = Vec::new();
    file.take(cap.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|_| EngineFailure::io())?;
    if bytes.len() as u64 > cap {
        return Err(EngineFailure::limit_exceeded());
    }
    check()?;
    let format = image::guess_format(&bytes).map_err(|_| EngineFailure::invalid_request())?;
    if !matches!(
        format,
        image::ImageFormat::Png
            | image::ImageFormat::Jpeg
            | image::ImageFormat::WebP
            | image::ImageFormat::Bmp
            | image::ImageFormat::Tiff
    ) {
        return Err(EngineFailure::invalid_request());
    }
    if format == image::ImageFormat::Tiff {
        single_frame_tiff(&bytes)?;
    }
    let alloc = limits
        .max_memory_mb
        .map(|mb| u64::from(mb) * 1024 * 1024)
        .unwrap_or(512 * 1024 * 1024)
        .min(limits.max_decompressed_bytes)
        .saturating_sub(bytes.len() as u64);
    let mut image_limits = image::Limits::default();
    image_limits.max_image_width = Some(16_384);
    image_limits.max_image_height = Some(16_384);
    image_limits.max_alloc = Some(alloc);
    let mut reader = ImageReader::with_format(Cursor::new(&bytes), format);
    reader.limits(image_limits.clone());
    let (width, height) = reader.into_dimensions().map_err(image_error)?;
    let pixels = u64::from(width) * u64::from(height);
    if pixels == 0 || pixels > MAX_PIXELS || pixels.saturating_mul(8) > alloc {
        return Err(EngineFailure::limit_exceeded());
    }
    let mut reader = ImageReader::with_format(Cursor::new(&bytes), format);
    reader.limits(image_limits);
    let decoded = reader.decode().map_err(image_error)?;
    check()?;
    if decoded.dimensions() != (width, height) {
        return Err(EngineFailure::invalid_request());
    }
    let mut gray = GrayImage::new(width, height);
    for (y, target_row) in gray.as_mut().chunks_exact_mut(width as usize).enumerate() {
        check()?;
        for (x, target) in target_row.iter_mut().enumerate() {
            let source = decoded.get_pixel(x as u32, y as u32).0;
            let alpha = u32::from(source[3]);
            let luma = (77 * u32::from(source[0])
                + 150 * u32::from(source[1])
                + 29 * u32::from(source[2]))
                >> 8;
            *target = ((luma * alpha + 255 * (255 - alpha) + 127) / 255) as u8;
        }
    }
    Ok(gray)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_sixteen_bit_transparency_and_reports_dimension_limits() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("rgba.png");
        let rgba =
            image::ImageBuffer::from_raw(2, 1, vec![0u16, 0, 0, 0, 65535, 0, 0, 65535]).unwrap();
        image::DynamicImage::ImageRgba16(rgba).save(&path).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        let gray = decode(&path, &Limits::local(), deadline).unwrap();
        assert_eq!(gray.as_raw(), &[255, 76]);
        image::GrayImage::new(16_385, 1).save(&path).unwrap();
        assert_eq!(
            decode(&path, &Limits::local(), deadline).unwrap_err().code,
            ariad_core::protocol::ErrorCode::LimitExceeded
        );
    }
    #[test]
    fn rejects_multiple_or_truncated_tiff_directories() {
        let mut bytes = b"II*\0\x08\0\0\0\0\0\0\0\0\0".to_vec();
        assert!(single_frame_tiff(&bytes).is_ok());
        bytes[10] = 1;
        assert!(single_frame_tiff(&bytes).is_err());
        assert!(single_frame_tiff(&bytes[..10]).is_err());
    }

    #[test]
    fn decodes_fixture_and_honors_input_budget() {
        let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/scan/vi-garden-300dpi.png");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        let image = decode(&fixture, &Limits::local(), deadline).unwrap();
        assert!(image.width() > 100 && image.height() > 100);
        let mut limits = Limits::local();
        limits.max_input_bytes = Some(16);
        assert!(decode(&fixture, &limits, deadline).is_err());
        assert!(decode(&fixture, &Limits::local(), std::time::Instant::now()).is_err());
    }
}
