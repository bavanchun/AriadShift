//! Rasterized pixel comparison and connected-box grouping.

use std::collections::VecDeque;

use image::{DynamicImage, GenericImageView};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Maximum allowed visual change bounding boxes per page.
pub const MAX_VISUAL_BOXES_PER_PAGE: usize = 200;

/// Grid cell dimension in pixels for connected-component grouping.
pub const GRID_CELL_SIZE: u32 = 16;

/// Minimum pixel difference in RGB space to count as a visual change.
pub const PIXEL_DIFF_THRESHOLD: u32 = 35;

/// Minimum number of diff pixels in a connected component to report a change.
pub const MIN_COMPONENT_DIFF_PIXELS: usize = 5;

/// Maximum total pixels across an image before returning an error (50 megapixels).
pub const MAX_IMAGE_PIXELS: u64 = 50_000_000;

/// A visual difference region on a page.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct VisualChange {
    /// Bounding box `[left, bottom, right, top]` in PDF points (bottom-left origin).
    pub bbox: [f64; 4],
    /// Number of differing pixels inside this bounding box.
    pub diff_pixels: usize,
}

/// Compares two rendered page images, grouping pixel differences into bounding boxes.
///
/// Images are compared at the specified `dpi` (used to convert pixel coordinates into PDF points).
pub fn visual_diff_images(
    before_img: &DynamicImage,
    after_img: &DynamicImage,
    dpi: f64,
) -> Result<Vec<VisualChange>, String> {
    if dpi <= 0.0 || !dpi.is_finite() {
        return Err("invalid dpi for visual diff".into());
    }

    let (w1, h1) = before_img.dimensions();
    let (w2, h2) = after_img.dimensions();

    let width = w1.max(w2);
    let height = h1.max(h2);

    let total_pixels = (width as u64) * (height as u64);
    if total_pixels > MAX_IMAGE_PIXELS {
        return Err(format!(
            "image dimension {width}x{height} exceeds pixel limit {MAX_IMAGE_PIXELS}"
        ));
    }

    if width == 0 || height == 0 {
        return Ok(Vec::new());
    }

    let before_rgba = before_img.to_rgba8();
    let after_rgba = after_img.to_rgba8();

    let grid_w = width.div_ceil(GRID_CELL_SIZE);
    let grid_h = height.div_ceil(GRID_CELL_SIZE);
    let grid_len = (grid_w as usize) * (grid_h as usize);

    let mut grid_counts = vec![0usize; grid_len];

    for y in 0..height {
        let gy = y / GRID_CELL_SIZE;
        let gy_offset = (gy as usize) * (grid_w as usize);

        for x in 0..width {
            let gx = x / GRID_CELL_SIZE;
            let cell_idx = gy_offset + (gx as usize);

            let is_diff = if x < w1 && y < h1 && x < w2 && y < h2 {
                let p1 = before_rgba.get_pixel(x, y);
                let p2 = after_rgba.get_pixel(x, y);

                let dr = (p1[0] as i32 - p2[0] as i32).unsigned_abs();
                let dg = (p1[1] as i32 - p2[1] as i32).unsigned_abs();
                let db = (p1[2] as i32 - p2[2] as i32).unsigned_abs();
                let da = (p1[3] as i32 - p2[3] as i32).unsigned_abs();

                (dr + dg + db + da) > PIXEL_DIFF_THRESHOLD
            } else {
                // Pixel exists in one image but not the other
                true
            };

            if is_diff {
                grid_counts[cell_idx] = grid_counts[cell_idx].saturating_add(1);
            }
        }
    }

    // Connected component analysis on active grid cells (8-way connected)
    let mut visited = vec![false; grid_len];
    let mut components = Vec::new();

    for gy in 0..grid_h {
        for gx in 0..grid_w {
            let start_idx = (gy as usize) * (grid_w as usize) + (gx as usize);
            if visited[start_idx] || grid_counts[start_idx] == 0 {
                continue;
            }

            visited[start_idx] = true;
            let mut queue = VecDeque::new();
            queue.push_back((gx, gy));

            let mut min_gx = gx;
            let mut max_gx = gx;
            let mut min_gy = gy;
            let mut max_gy = gy;
            let mut comp_pixels = 0usize;

            while let Some((cx, cy)) = queue.pop_front() {
                let c_idx = (cy as usize) * (grid_w as usize) + (cx as usize);
                comp_pixels = comp_pixels.saturating_add(grid_counts[c_idx]);

                min_gx = min_gx.min(cx);
                max_gx = max_gx.max(cx);
                min_gy = min_gy.min(cy);
                max_gy = max_gy.max(cy);

                // 8-way neighbors
                for dy in [-1i32, 0, 1] {
                    for dx in [-1i32, 0, 1] {
                        if dx == 0 && dy == 0 {
                            continue;
                        }
                        let nx = cx as i32 + dx;
                        let ny = cy as i32 + dy;
                        if nx >= 0 && nx < grid_w as i32 && ny >= 0 && ny < grid_h as i32 {
                            let unx = nx as u32;
                            let uny = ny as u32;
                            let n_idx = (uny as usize) * (grid_w as usize) + (unx as usize);
                            if !visited[n_idx] && grid_counts[n_idx] > 0 {
                                visited[n_idx] = true;
                                queue.push_back((unx, uny));
                            }
                        }
                    }
                }
            }

            if comp_pixels >= MIN_COMPONENT_DIFF_PIXELS {
                components.push((min_gx, min_gy, max_gx, max_gy, comp_pixels));
            }
        }
    }

    let scale = 72.0 / dpi;
    let mut changes = Vec::with_capacity(components.len().min(MAX_VISUAL_BOXES_PER_PAGE));

    for (min_gx, min_gy, max_gx, max_gy, comp_pixels) in components {
        let px0 = (min_gx * GRID_CELL_SIZE) as f64;
        let py0 = (min_gy * GRID_CELL_SIZE) as f64;
        let px1 = (((max_gx + 1) * GRID_CELL_SIZE).min(width)) as f64;
        let py1 = (((max_gy + 1) * GRID_CELL_SIZE).min(height)) as f64;

        let left = px0 * scale;
        let right = px1 * scale;
        let bottom = ((height as f64) - py1) * scale;
        let top = ((height as f64) - py0) * scale;

        changes.push(VisualChange {
            bbox: [left, bottom, right, top],
            diff_pixels: comp_pixels,
        });

        if changes.len() >= MAX_VISUAL_BOXES_PER_PAGE {
            break;
        }
    }

    // Sort deterministically: top-to-bottom (highest top first), then left-to-right
    changes.sort_by(|a, b| {
        b.bbox[3]
            .partial_cmp(&a.bbox[3])
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                a.bbox[0]
                    .partial_cmp(&b.bbox[0])
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    });

    Ok(changes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    #[test]
    fn identical_images_report_zero_changes() {
        let img =
            DynamicImage::ImageRgba8(RgbaImage::from_pixel(100, 100, Rgba([255, 255, 255, 255])));
        let changes = visual_diff_images(&img, &img, 72.0).unwrap();
        assert!(changes.is_empty());
    }

    #[test]
    fn detects_tint_change_in_subregion() {
        let mut before = RgbaImage::from_pixel(100, 100, Rgba([255, 255, 255, 255]));
        let mut after = RgbaImage::from_pixel(100, 100, Rgba([255, 255, 255, 255]));

        // Paint a 20x20 blue box in before and green box in after at (30, 30)
        for y in 30..50 {
            for x in 30..50 {
                before.put_pixel(x, y, Rgba([0, 0, 255, 255]));
                after.put_pixel(x, y, Rgba([0, 255, 0, 255]));
            }
        }

        let changes = visual_diff_images(
            &DynamicImage::ImageRgba8(before),
            &DynamicImage::ImageRgba8(after),
            72.0,
        )
        .unwrap();

        assert_eq!(changes.len(), 1);
        assert!(changes[0].diff_pixels >= 400);
        // At 72 DPI, 1 pixel = 1 pt. Height is 100, so y in 30..50 has bottom around 100-50=50, top around 100-30=70.
        assert!(changes[0].bbox[0] <= 32.0);
        assert!(changes[0].bbox[2] >= 48.0);
    }
}
