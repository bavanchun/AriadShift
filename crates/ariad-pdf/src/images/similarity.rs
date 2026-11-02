//! Grayscale structural similarity for rendered pages.

use std::io::Cursor;

use image::{ImageFormat, ImageReader, Limits};

/// Maximum decoded allocation accepted by the PNG comparison helper.
const MAX_PNG_BYTES: u64 = 256 * 1024 * 1024;
const MAX_DIMENSION: u32 = 65_535;

/// Compare two 8-bit grayscale images using non-overlapping windows of up to 8×8 pixels.
///
/// Returns NaN if either buffer length does not match the non-zero dimensions.
#[must_use]
pub fn ssim_gray(a: &[u8], b: &[u8], width: u32, height: u32) -> f64 {
    let Some(len) = (width as usize).checked_mul(height as usize) else {
        return f64::NAN;
    };
    if len == 0 || a.len() != len || b.len() != len {
        return f64::NAN;
    }
    let mut total = 0.0;
    let mut windows = 0_u64;
    for y in (0..height).step_by(8) {
        for x in (0..width).step_by(8) {
            let mut sum_a = 0.0;
            let mut sum_b = 0.0;
            let mut sum_aa = 0.0;
            let mut sum_bb = 0.0;
            let mut sum_ab = 0.0;
            let mut count = 0.0;
            for yy in y..height.min(y.saturating_add(8)) {
                for xx in x..width.min(x.saturating_add(8)) {
                    let i = yy as usize * width as usize + xx as usize;
                    let va = f64::from(a[i]);
                    let vb = f64::from(b[i]);
                    sum_a += va;
                    sum_b += vb;
                    sum_aa += va * va;
                    sum_bb += vb * vb;
                    sum_ab += va * vb;
                    count += 1.0;
                }
            }
            let mean_a = sum_a / count;
            let mean_b = sum_b / count;
            let var_a = (sum_aa / count - mean_a * mean_a).max(0.0);
            let var_b = (sum_bb / count - mean_b * mean_b).max(0.0);
            let covariance = sum_ab / count - mean_a * mean_b;
            let c1 = (0.01_f64 * 255.0).powi(2);
            let c2 = (0.03_f64 * 255.0).powi(2);
            total += ((2.0 * mean_a * mean_b + c1) * (2.0 * covariance + c2))
                / ((mean_a * mean_a + mean_b * mean_b + c1) * (var_a + var_b + c2));
            windows += 1;
        }
    }
    total / windows as f64
}

/// Decode two equally sized PNG images and compare their grayscale pixels.
pub fn similarity_png(a_png: &[u8], b_png: &[u8]) -> Result<f64, image::ImageError> {
    fn decode(bytes: &[u8]) -> Result<image::GrayImage, image::ImageError> {
        let mut reader = ImageReader::with_format(Cursor::new(bytes), ImageFormat::Png);
        let mut limits = Limits::default();
        limits.max_image_width = Some(MAX_DIMENSION);
        limits.max_image_height = Some(MAX_DIMENSION);
        limits.max_alloc = Some(MAX_PNG_BYTES);
        reader.limits(limits);
        Ok(reader.decode()?.to_luma8())
    }
    let a = decode(a_png)?;
    let b = decode(b_png)?;
    if a.dimensions() != b.dimensions() {
        return Err(image::ImageError::Parameter(
            image::error::ParameterError::from_kind(
                image::error::ParameterErrorKind::DimensionMismatch,
            ),
        ));
    }
    Ok(ssim_gray(a.as_raw(), b.as_raw(), a.width(), a.height()))
}
