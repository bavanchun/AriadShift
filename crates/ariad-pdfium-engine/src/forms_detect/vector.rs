//! Bounded extraction of vector path geometry from PDFium's safe wrappers.

use super::geometry::Edge;
use crate::output::ToolContext;
use ariad_engine_io::EngineFailure;
use pdfium_render::prelude::*;

const MAX_OBJECTS: usize = 50_000;
const MAX_FORM_DEPTH: usize = 8;

#[derive(Default)]
pub(super) struct VectorSignals {
    pub edges: Vec<Edge>,
    pub boxes: Vec<[f64; 4]>,
    pub visits: usize,
    pub truncated: bool,
}

pub(super) fn collect<'a>(
    ctx: &ToolContext<'_>,
    iter: PdfPageObjectsIterator<'a>,
    matrix: PdfMatrix,
    depth: usize,
    output: &mut VectorSignals,
) -> Result<(), EngineFailure> {
    if depth > usize::from(ctx.limits().max_nesting_depth) {
        return Err(EngineFailure::limit_exceeded());
    }
    if depth > MAX_FORM_DEPTH {
        output.truncated = true;
        return Ok(());
    }
    for object in iter {
        output.visits += 1;
        if output.visits % 1024 == 0 {
            ctx.check()?;
        }
        if output.visits > MAX_OBJECTS || output.visits > ctx.limits().max_blocks as usize {
            output.truncated = true;
            break;
        }
        match object {
            PdfPageObject::Path(path) => {
                let edge_start = output.edges.len();
                let stroked = path.is_stroked().unwrap_or(false);
                let transform = path
                    .matrix()
                    .unwrap_or(PdfMatrix::IDENTITY)
                    .multiply(matrix);
                let segments = path.segments().transform(transform);
                let mut first = None;
                let mut previous = None;
                let mut polygon = Vec::new();
                let mut closed = false;
                let mut linear = true;
                for segment in segments.iter() {
                    output.visits += 1;
                    if output.visits % 1024 == 0 {
                        ctx.check()?;
                    }
                    if output.visits > MAX_OBJECTS
                        || output.visits > ctx.limits().max_blocks as usize
                    {
                        output.truncated = true;
                        break;
                    }
                    let (x, y) = segment.point();
                    let point = [f64::from(x.value), f64::from(y.value)];
                    if point
                        .iter()
                        .any(|n| !n.is_finite() || n.abs() > 1_000_000.0)
                    {
                        linear = false;
                        continue;
                    }
                    match segment.segment_type() {
                        PdfPathSegmentType::MoveTo => {
                            first = Some(point);
                            previous = Some(point);
                            polygon.clear();
                            polygon.push(point);
                        }
                        PdfPathSegmentType::LineTo => {
                            if let Some(p) = previous {
                                output.edges.push(Edge { a: p, b: point });
                            }
                            previous = Some(point);
                            polygon.push(point);
                        }
                        _ => {
                            linear = false;
                            previous = Some(point);
                        }
                    }
                    if segment.is_close() {
                        if let (Some(p), Some(f)) = (previous, first) {
                            if p != f {
                                output.edges.push(Edge { a: p, b: f });
                            }
                            closed = true;
                        }
                    }
                }
                if linear && closed && polygon.len() >= 4 && polygon.len() <= 5 {
                    let rect = polygon.iter().fold(
                        [
                            f64::INFINITY,
                            f64::INFINITY,
                            f64::NEG_INFINITY,
                            f64::NEG_INFINITY,
                        ],
                        |r, p| {
                            [
                                r[0].min(p[0]),
                                r[1].min(p[1]),
                                r[2].max(p[0]),
                                r[3].max(p[1]),
                            ]
                        },
                    );
                    let width = rect[2] - rect[0];
                    let height = rect[3] - rect[1];
                    if !stroked && width <= 20.0 && height <= 20.0 {
                        output.edges.truncate(edge_start);
                    } else if polygon
                        .iter()
                        .all(|p| (p[0] - rect[0]).abs() < 1.0 || (p[0] - rect[2]).abs() < 1.0)
                        && polygon
                            .iter()
                            .all(|p| (p[1] - rect[1]).abs() < 1.0 || (p[1] - rect[3]).abs() < 1.0)
                    {
                        output.boxes.push(rect);
                    }
                }
            }
            PdfPageObject::XObjectForm(form) => {
                let next = form
                    .matrix()
                    .unwrap_or(PdfMatrix::IDENTITY)
                    .multiply(matrix);
                collect(ctx, form.iter(), next, depth + 1, output)?;
            }
            _ => {}
        }
        if output.truncated {
            break;
        }
    }
    Ok(())
}
