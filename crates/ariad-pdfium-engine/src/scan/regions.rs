//! Detect distinct scanned regions against a largely uniform scanner bed.
use image::GrayImage;
use std::collections::VecDeque;

/// Inclusive-exclusive region in raster pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Region {
    pub x0: u32,
    pub y0: u32,
    pub x1: u32,
    pub y1: u32,
    /// Estimated counter-clockwise angle of the region's long edge.
    pub angle_deg: f32,
    /// Elongation-based confidence in the angle, between zero and one.
    pub angle_confidence: f32,
}

/// Locate regions whose luminance differs from the median border luminance.
///
/// A bounded occupancy grid suppresses individual speckles. A region's bounding box is
/// conservative to one grid cell; padding is applied after filtering by area.
#[must_use]
pub fn detect(image: &GrayImage, min_area_ratio: f32, padding: u32) -> Vec<Region> {
    detect_checked(image, min_area_ratio, padding, || {
        Ok::<(), std::convert::Infallible>(())
    })
    .unwrap()
}

/// Detect regions while polling the caller's cancellation and deadline check.
pub fn detect_checked<E>(
    image: &GrayImage,
    min_area_ratio: f32,
    padding: u32,
    mut check: impl FnMut() -> Result<(), E>,
) -> Result<Vec<Region>, E> {
    let (width, height) = image.dimensions();
    if width == 0
        || height == 0
        || !min_area_ratio.is_finite()
        || !(0.001..=0.9).contains(&min_area_ratio)
    {
        return Ok(Vec::new());
    }
    let mut border = [0_u64; 256];
    let step = (width.max(height) / 4096).max(1);
    for x in (0..width).step_by(step as usize) {
        border[usize::from(image.get_pixel(x, 0)[0])] += 1;
        border[usize::from(image.get_pixel(x, height - 1)[0])] += 1;
    }
    for y in (0..height).step_by(step as usize) {
        border[usize::from(image.get_pixel(0, y)[0])] += 1;
        border[usize::from(image.get_pixel(width - 1, y)[0])] += 1;
    }
    let half = border.iter().sum::<u64>() / 2;
    let mut total = 0;
    let background = border
        .iter()
        .position(|count| {
            total += count;
            total > half
        })
        .unwrap_or(255) as u8;
    // No more than 512 cells along either dimension, even for unusually large pages.
    let cell = width.max(height).div_ceil(512).max(4);
    let cols = width.div_ceil(cell);
    let rows = height.div_ceil(cell);
    let mut grid = vec![false; cols as usize * rows as usize];
    for gy in 0..rows {
        check()?;
        for gx in 0..cols {
            let x0 = gx * cell;
            let y0 = gy * cell;
            let x1 = x0.saturating_add(cell).min(width);
            let y1 = y0.saturating_add(cell).min(height);
            let mut different = 0_u64;
            for y in y0..y1 {
                for x in x0..x1 {
                    if image.get_pixel(x, y)[0].abs_diff(background) >= 24 {
                        different += 1;
                    }
                }
            }
            grid[(gy * cols + gx) as usize] =
                different * 5 >= u64::from(x1 - x0) * u64::from(y1 - y0);
        }
    }
    let mut regions = Vec::new();
    let mut queue = VecDeque::new();
    for gy in 0..rows {
        for gx in 0..cols {
            let index = (gy * cols + gx) as usize;
            if !grid[index] {
                continue;
            }
            grid[index] = false;
            queue.push_back((gx, gy));
            let (mut left, mut top, mut right, mut bottom) = (gx, gy, gx, gy);
            let (mut n, mut sx, mut sy, mut sxx, mut syy, mut sxy) =
                (0.0_f64, 0.0_f64, 0.0_f64, 0.0_f64, 0.0_f64, 0.0_f64);
            while let Some((x, y)) = queue.pop_front() {
                if (n as usize).is_multiple_of(4096) {
                    check()?;
                }
                let (xf, yf) = (f64::from(x), f64::from(y));
                n += 1.0;
                sx += xf;
                sy += yf;
                sxx += xf * xf;
                syy += yf * yf;
                sxy += xf * yf;
                left = left.min(x);
                top = top.min(y);
                right = right.max(x);
                bottom = bottom.max(y);
                for ny in y.saturating_sub(1)..=(y + 1).min(rows - 1) {
                    for nx in x.saturating_sub(1)..=(x + 1).min(cols - 1) {
                        let next = (ny * cols + nx) as usize;
                        if grid[next] {
                            grid[next] = false;
                            queue.push_back((nx, ny));
                        }
                    }
                }
            }
            let (x0, y0, x1, y1) = (
                left * cell,
                top * cell,
                right.saturating_add(1).saturating_mul(cell).min(width),
                bottom.saturating_add(1).saturating_mul(cell).min(height),
            );
            let ratio =
                f64::from(x1 - x0) * f64::from(y1 - y0) / (f64::from(width) * f64::from(height));
            if ratio >= f64::from(min_area_ratio) {
                let xx = sxx / n - (sx / n).powi(2);
                let yy = syy / n - (sy / n).powi(2);
                let xy = sxy / n - (sx / n) * (sy / n);
                let anisotropy = (xx - yy).hypot(2.0 * xy) / (xx + yy).max(f64::EPSILON);
                let axis = 0.5 * (2.0 * xy).atan2(xx - yy).to_degrees();
                let angle = if anisotropy < 0.1 {
                    0.0
                } else {
                    (-axis + 45.0).rem_euclid(90.0) - 45.0
                };
                regions.push(Region {
                    x0: x0.saturating_sub(padding),
                    y0: y0.saturating_sub(padding),
                    x1: x1.saturating_add(padding).min(width),
                    y1: y1.saturating_add(padding).min(height),
                    angle_deg: angle as f32,
                    angle_confidence: anisotropy.clamp(0.0, 1.0) as f32,
                });
            }
        }
    }
    regions.sort_by_key(|region| (region.y0, region.x0));
    Ok(regions)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Luma;
    #[test]
    fn separates_regions_and_keeps_edge_regions() {
        let mut image = GrayImage::from_pixel(200, 100, Luma([160]));
        for y in 10..90 {
            for x in 0..60 {
                image.put_pixel(x, y, Luma([250]));
            }
        }
        for y in 15..85 {
            for x in 120..180 {
                image.put_pixel(x, y, Luma([240]));
            }
        }
        let regions = detect(&image, 0.02, 2);
        assert_eq!(regions.len(), 2);
        assert_eq!(regions[0].x0, 0);
        assert!(regions.iter().any(|r| r.x0 <= 120 && r.x1 >= 180));
        assert!(detect(&GrayImage::from_pixel(50, 50, Luma([255])), 0.02, 0).is_empty());
    }
    #[test]
    fn detects_cancellation_during_grid_scan() {
        let image = GrayImage::from_pixel(200, 200, Luma([160]));
        let mut checks = 0;
        let result = detect_checked(&image, 0.02, 0, || {
            checks += 1;
            if checks > 3 { Err(()) } else { Ok(()) }
        });
        assert!(result.is_err());
    }
}
