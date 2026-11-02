//! Pixel transformations used to prepare scans for recognition.
use image::{GrayImage, Luma};

/// Rotate in place around the image centre, keeping the canvas size and filling outside with white.
/// Positive angles rotate counter-clockwise in page coordinates.
#[must_use]
pub fn rotate(image: &GrayImage, angle_deg: f32) -> GrayImage {
    rotate_checked(image, angle_deg, || Ok::<(), std::convert::Infallible>(())).unwrap()
}

/// Rotate while checking cancellation and deadlines once per output row.
pub fn rotate_checked<E>(
    image: &GrayImage,
    angle_deg: f32,
    mut check: impl FnMut() -> Result<(), E>,
) -> Result<GrayImage, E> {
    check()?;
    let mut output = GrayImage::from_pixel(image.width(), image.height(), Luma([255]));
    let (sin, cos) = angle_deg.to_radians().sin_cos();
    let cx = (image.width() as f32 - 1.0) / 2.0;
    let cy = (image.height() as f32 - 1.0) / 2.0;
    for (x, y, pixel) in output.enumerate_pixels_mut() {
        if x == 0 {
            check()?;
        }
        let dx = x as f32 - cx;
        let dy = y as f32 - cy;
        // Inverse transform in image coordinates, whose y axis points down.
        let sx = cos * dx - sin * dy + cx;
        let sy = sin * dx + cos * dy + cy;
        if sx >= 0.0
            && sy >= 0.0
            && sx <= image.width().saturating_sub(1) as f32
            && sy <= image.height().saturating_sub(1) as f32
        {
            let x0 = sx.floor() as u32;
            let y0 = sy.floor() as u32;
            let x1 = (x0 + 1).min(image.width() - 1);
            let y1 = (y0 + 1).min(image.height() - 1);
            let fx = sx - x0 as f32;
            let fy = sy - y0 as f32;
            let top = f32::from(image.get_pixel(x0, y0)[0]) * (1.0 - fx)
                + f32::from(image.get_pixel(x1, y0)[0]) * fx;
            let bottom = f32::from(image.get_pixel(x0, y1)[0]) * (1.0 - fx)
                + f32::from(image.get_pixel(x1, y1)[0]) * fx;
            *pixel = Luma([(top * (1.0 - fy) + bottom * fy).round() as u8]);
        }
    }
    Ok(output)
}

/// Normalize a scan's background using a global histogram-derived white point.
/// Dark text remains dark and the operation is deterministic.
#[must_use]
pub fn clean(image: &GrayImage) -> GrayImage {
    clean_checked(image, || Ok::<(), std::convert::Infallible>(())).unwrap()
}

/// Normalize a scan while checking cancellation and deadlines once per row.
pub fn clean_checked<E>(
    image: &GrayImage,
    mut check: impl FnMut() -> Result<(), E>,
) -> Result<GrayImage, E> {
    check()?;
    let mut histogram = [0_u64; 256];
    for (x, _, pixel) in image.enumerate_pixels() {
        if x == 0 {
            check()?;
        }
        histogram[usize::from(pixel[0])] += 1;
    }
    let total = u64::from(image.width()) * u64::from(image.height());
    let target = total.saturating_mul(95) / 100;
    let mut cumulative = 0_u64;
    let mut white = 255_u8;
    for (value, count) in histogram.into_iter().enumerate() {
        cumulative += count;
        if cumulative >= target {
            white = (value as u8).max(1);
            break;
        }
    }
    let mut output = GrayImage::new(image.width(), image.height());
    for (x, y, pixel) in output.enumerate_pixels_mut() {
        if x == 0 {
            check()?;
        }
        let value = u32::from(image.get_pixel(x, y)[0]);
        *pixel = Luma([((value * 255 / u32::from(white)).min(255)) as u8]);
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identity_rotation_and_background_normalization() {
        let mut image = GrayImage::from_pixel(5, 5, Luma([200]));
        image.put_pixel(2, 2, Luma([0]));
        assert_eq!(rotate(&image, 0.0), image);
        let cleaned = clean(&image);
        assert_eq!(cleaned.get_pixel(0, 0)[0], 255);
        assert_eq!(cleaned.get_pixel(2, 2)[0], 0);
    }
    #[test]
    fn inverse_rotation_corrects_slanted_lines() {
        let mut image = GrayImage::from_pixel(400, 250, Luma([255]));
        for baseline in [45_i32, 90, 135, 180] {
            for x in 30..370_u32 {
                let y = baseline - ((x as f32 - 200.0) * 3.0_f32.to_radians().tan()).round() as i32;
                image.put_pixel(x, y as u32, Luma([0]));
            }
        }
        let corrected = rotate(&image, -3.0);
        let angle = crate::scan::skew::estimate(&corrected, 10.0)
            .unwrap()
            .angle_deg;
        assert!(angle.abs() <= 0.5, "{angle}");
    }
    #[test]
    fn pixel_operations_observe_cancellation() {
        let image = GrayImage::from_pixel(100, 100, Luma([200]));
        let mut checks = 0;
        let result = rotate_checked(&image, 1.0, || {
            checks += 1;
            if checks > 3 { Err(()) } else { Ok(()) }
        });
        assert!(result.is_err());
        let mut checks = 0;
        let result = clean_checked(&image, || {
            checks += 1;
            if checks > 3 { Err(()) } else { Ok(()) }
        });
        assert!(result.is_err());
    }
}
