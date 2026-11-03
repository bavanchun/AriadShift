//! Grayscale raster fallback for pages without usable vector content.

use super::geometry::Candidate;
use crate::{output::ToolContext, render};
use ariad_core::tools::params::form_model::FieldType;
use ariad_engine_io::EngineFailure;
use pdfium_render::prelude::*;

#[derive(Clone, Copy)]
struct Run {
    x1: u32,
    x2: u32,
    y: u32,
}
fn dark(pixel: &image::Rgba<u8>) -> bool {
    let [r, g, b, _] = pixel.0;
    u16::from(r) * 3 + u16::from(g) * 6 + u16::from(b) < 1050
}
fn pdf_point(x: f64, y: f64, width: f64, height: f64, crop: [f64; 4], rotation: u16) -> [f64; 2] {
    let u = x / width;
    let v = 1.0 - y / height;
    let w = crop[2] - crop[0];
    let h = crop[3] - crop[1];
    match rotation {
        90 => [crop[0] + (1.0 - v) * w, crop[1] + u * h],
        180 => [crop[0] + (1.0 - u) * w, crop[1] + (1.0 - v) * h],
        270 => [crop[0] + v * w, crop[1] + (1.0 - u) * h],
        _ => [crop[0] + u * w, crop[1] + v * h],
    }
}
fn rect(a: [f64; 2], b: [f64; 2]) -> [f64; 4] {
    [
        a[0].min(b[0]),
        a[1].min(b[1]),
        a[0].max(b[0]),
        a[1].max(b[1]),
    ]
}

pub(super) fn detect(
    ctx: &ToolContext<'_>,
    page: &PdfPage<'_>,
    page_number: usize,
) -> Result<Vec<Candidate>, EngineFailure> {
    let (width, height) = render::compute_page_pixels(page, 100)?;
    ctx.check_pixels(width, height)?;
    let image = render::render_page_rgba(
        page,
        page_number,
        width,
        height,
        PdfColor::WHITE,
        false,
        ctx.limits().max_memory_mb,
    )?;
    let w = image.width();
    let h = image.height();
    let mut runs: Vec<Run> = Vec::new();
    for y in 0..h {
        if y % 32 == 0 {
            ctx.check()?;
        }
        let mut start = None;
        for x in 0..=w {
            let is_dark = x < w && dark(image.get_pixel(x, y));
            if is_dark && start.is_none() {
                start = Some(x);
            }
            if !is_dark {
                if let Some(x1) = start.take() {
                    if x - x1 >= 8 {
                        runs.push(Run { x1, x2: x - 1, y });
                        if runs.len() > 50_000 {
                            return Err(EngineFailure::limit_exceeded());
                        }
                    }
                }
            }
        }
    }
    let crop = page
        .boundaries()
        .crop()
        .or_else(|_| page.boundaries().media())
        .map_err(crate::text_layout::map_pdfium_error)?
        .bounds;
    let bounds = [
        f64::from(crop.left().value),
        f64::from(crop.bottom().value),
        f64::from(crop.right().value),
        f64::from(crop.top().value),
    ];
    let rotation = page
        .rotation()
        .map_or(0, crate::text_layout::rotation_degrees);
    let transform = |x: u32, y: u32| {
        pdf_point(
            f64::from(x),
            f64::from(y),
            f64::from(w),
            f64::from(h),
            bounds,
            rotation,
        )
    };
    let mut candidates: Vec<Candidate> = Vec::new();
    let mut work = 0usize;
    for (i, run) in runs.iter().enumerate() {
        if i % 256 == 0 {
            ctx.check()?;
        }
        // Ignore near-identical scanlines from a single thick stroke.
        if i > 0
            && runs[i - 1].y + 1 == run.y
            && runs[i - 1].x1.abs_diff(run.x1) <= 2
            && runs[i - 1].x2.abs_diff(run.x2) <= 2
        {
            continue;
        }
        let length = run.x2 - run.x1 + 1;
        if (8..=30).contains(&length) {
            for other in runs.iter().skip(i + 1) {
                work += 1;
                if work % 1024 == 0 {
                    ctx.check()?;
                }
                if work > 1_000_000 {
                    break;
                }
                let dy = other.y.saturating_sub(run.y);
                if dy > 30 {
                    break;
                }
                if dy < 8
                    || run.x1.abs_diff(other.x1) > 2
                    || run.x2.abs_diff(other.x2) > 2
                    || length.abs_diff(dy) > 5
                {
                    continue;
                }
                let span = dy.max(1);
                let left = (run.y..=other.y)
                    .filter(|y| dark(image.get_pixel(run.x1, *y)))
                    .count();
                let right = (run.y..=other.y)
                    .filter(|y| dark(image.get_pixel(run.x2, *y)))
                    .count();
                if left * 4 >= span as usize * 3 && right * 4 >= span as usize * 3 {
                    let box_rect = rect(transform(run.x1, run.y), transform(run.x2, other.y));
                    candidates.push(Candidate {
                        rect: box_rect,
                        kind: FieldType::Checkbox,
                        confidence: 0.58,
                        reason: "raster_square",
                    });
                    break;
                }
            }
        }
        if length >= 50 {
            let a = transform(run.x1, run.y);
            let b = transform(run.x2, run.y);
            let line = rect(a, b);
            if line[2] - line[0] >= 36.0 && line[3] - line[1] < 2.0 {
                candidates.push(Candidate {
                    rect: [line[0], line[1], line[2], line[1] + 14.0],
                    kind: FieldType::Text,
                    confidence: 0.55,
                    reason: "raster_rule",
                });
            }
        }
        if candidates.len() > 1000 {
            break;
        }
    }
    Ok(candidates)
}
