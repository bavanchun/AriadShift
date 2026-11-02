//! Bounded projection-profile estimation of text-line skew.
use image::{GrayImage, imageops::FilterType};

/// Estimated counter-clockwise baseline angle and relative peak strength.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SkewEstimate {
    /// Baseline angle in degrees, counter-clockwise in page coordinates.
    pub angle_deg: f32,
    /// Relative improvement over the average projection score, from zero to one.
    pub confidence: f32,
}

/// Estimate skew from dark pixels using a projection profile at quarter-degree intervals.
///
/// Images are reduced to at most 800 pixels on their longest edge, bounding work and memory.
/// Blank images have no estimate. The angle describes the input, not the correction.
#[must_use]
pub fn estimate(image: &GrayImage, max_angle_deg: f32) -> Option<SkewEstimate> {
    estimate_checked(image, max_angle_deg, || {
        Ok::<(), std::convert::Infallible>(())
    })
    .unwrap()
}

/// Estimate skew while polling the caller's cancellation and deadline check.
pub fn estimate_checked<E>(
    image: &GrayImage,
    max_angle_deg: f32,
    mut check: impl FnMut() -> Result<(), E>,
) -> Result<Option<SkewEstimate>, E> {
    check()?;
    if image.width() == 0
        || image.height() == 0
        || !max_angle_deg.is_finite()
        || !(0.0..=45.0).contains(&max_angle_deg)
    {
        return Ok(None);
    }
    let scale = (800.0 / image.width().max(image.height()) as f32).min(1.0);
    let width = ((image.width() as f32 * scale).round() as u32).max(1);
    let height = ((image.height() as f32 * scale).round() as u32).max(1);
    let reduced = image::imageops::resize(image, width, height, FilterType::Triangle);
    let points: Vec<(f32, f32)> = reduced
        .enumerate_pixels()
        .filter_map(|(x, y, p)| {
            (p[0] < 160).then_some((
                x as f32 - width as f32 / 2.0,
                y as f32 - height as f32 / 2.0,
            ))
        })
        .collect();
    if points.len() < 20 || points.len() * 100 > (width as usize * height as usize) * 35 {
        return Ok(None);
    }
    let bins = ((width as f32).hypot(height as f32).ceil() as usize) + 3;
    let mut profile = vec![0_u32; bins];
    let steps = (max_angle_deg * 4.0).floor() as i32;
    let mut best = (f64::NEG_INFINITY, 0_i32);
    let mut sum = 0.0_f64;
    let mut n = 0_u32;
    for step in -steps..=steps {
        check()?;
        profile.fill(0);
        let angle = (step as f32 * 0.25).to_radians();
        let (sin, cos) = angle.sin_cos();
        for &(x, y) in &points {
            let row = (y * cos + x * sin + bins as f32 / 2.0).round() as usize;
            if let Some(bin) = profile.get_mut(row) {
                *bin += 1;
            }
        }
        let score: f64 = profile.iter().map(|&v| f64::from(v).powi(2)).sum();
        if score > best.0 {
            best = (score, step);
        }
        sum += score;
        n += 1;
    }
    let confidence = if best.0 > 0.0 {
        ((best.0 - sum / f64::from(n)) / best.0).clamp(0.0, 1.0) as f32
    } else {
        0.0
    };
    Ok(Some(SkewEstimate {
        angle_deg: best.1 as f32 * 0.25,
        confidence,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Luma;
    #[test]
    fn blank_and_horizontal_lines() {
        let mut image = GrayImage::from_pixel(300, 200, Luma([255]));
        assert!(estimate(&image, 10.0).is_none());
        for row in [40, 80, 120, 160] {
            for x in 20..280 {
                image.put_pixel(x, row, Luma([0]));
            }
        }
        let result = estimate(&image, 10.0).unwrap();
        assert!(result.angle_deg.abs() <= 0.25);
        assert!(result.confidence > 0.0);
    }
    #[test]
    fn detects_slanted_baselines() {
        let mut image = GrayImage::from_pixel(400, 250, Luma([255]));
        for baseline in [45_i32, 90, 135, 180] {
            for x in 30..370_u32 {
                let y = baseline - ((x as f32 - 200.0) * 3.0_f32.to_radians().tan()).round() as i32;
                image.put_pixel(x, y as u32, Luma([0]));
            }
        }
        let result = estimate(&image, 10.0).unwrap();
        assert!((result.angle_deg - 3.0).abs() <= 0.5, "{result:?}");
    }
    #[test]
    fn detects_cancellation_during_projection() {
        let mut image = GrayImage::from_pixel(100, 100, Luma([255]));
        for x in 10..90 {
            image.put_pixel(x, 50, Luma([0]));
        }
        let mut checks = 0;
        let result = estimate_checked(&image, 10.0, || {
            checks += 1;
            if checks > 3 { Err(()) } else { Ok(()) }
        });
        assert!(result.is_err());
    }
}
