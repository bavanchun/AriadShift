//! Deterministic pixel treatment for simulated scanned pages.
use ariad_core::tools::params::ScannerEffectParams;
use image::{Rgba, RgbaImage};

/// Apply grayscale, tonal changes, noise, blur and seeded rotation to an image.
/// Callers enforce the pixel and memory limits before invoking this operation.
#[must_use]
pub fn apply(image: &RgbaImage, params: &ScannerEffectParams) -> RgbaImage {
    apply_checked(image, params, || Ok::<(), std::convert::Infallible>(())).unwrap()
}

/// Apply the effect while polling cancellation and deadlines between rows and stages.
pub fn apply_checked<E>(
    image: &RgbaImage,
    params: &ScannerEffectParams,
    mut check: impl FnMut() -> Result<(), E>,
) -> Result<RgbaImage, E> {
    check()?;
    let mut state = params.seed ^ 0x9e37_79b9_7f4a_7c15;
    if state == 0 {
        state = 0x2545_f491_4f6c_dd1d;
    }
    let mut next = || {
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        (state.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 40) as f32 / 16_777_215.0
    };
    let angle = (next() * 2.0 - 1.0) * params.skew_deg;
    let mut treated = image.clone();
    let contrast = 1.0 + params.contrast;
    for (x, _, pixel) in treated.enumerate_pixels_mut() {
        if x == 0 {
            check()?;
        }
        let gray = (0.2126 * f32::from(pixel[0])
            + 0.7152 * f32::from(pixel[1])
            + 0.0722 * f32::from(pixel[2]))
        .round();
        let noise = (next() * 2.0 - 1.0) * params.noise * 127.5;
        for channel in &mut pixel.0[..3] {
            let input = if params.grayscale {
                gray
            } else {
                f32::from(*channel)
            };
            *channel = (((input - 127.5) * contrast + 127.5 + params.brightness * 255.0 + noise)
                .round())
            .clamp(0.0, 255.0) as u8;
        }
    }
    check()?;
    if params.blur > 0.0 {
        treated = image::imageops::blur(&treated, params.blur);
    }
    check()?;
    if angle.abs() < f32::EPSILON {
        return Ok(treated);
    }
    let (sin, cos) = angle.to_radians().sin_cos();
    let cx = (treated.width() as f32 - 1.0) / 2.0;
    let cy = (treated.height() as f32 - 1.0) / 2.0;
    let mut result = RgbaImage::from_pixel(
        treated.width(),
        treated.height(),
        Rgba([255, 255, 255, 255]),
    );
    for (x, y, pixel) in result.enumerate_pixels_mut() {
        if x == 0 {
            check()?;
        }
        let dx = x as f32 - cx;
        let dy = y as f32 - cy;
        let sx = (cos * dx - sin * dy + cx).round();
        let sy = (sin * dx + cos * dy + cy).round();
        if sx >= 0.0 && sy >= 0.0 && sx < treated.width() as f32 && sy < treated.height() as f32 {
            *pixel = *treated.get_pixel(sx as u32, sy as u32);
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn seed_repeats_and_parameters_affect_pixels() {
        let input = RgbaImage::from_pixel(40, 40, Rgba([100, 150, 200, 255]));
        let params: ScannerEffectParams = serde_json::from_str("{}").unwrap();
        let first = apply(&input, &params);
        assert_eq!(first, apply(&input, &params));
        let mut changed = params;
        changed.seed = 9;
        assert_ne!(first, apply(&input, &changed));
    }
}
