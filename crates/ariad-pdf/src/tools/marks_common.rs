//! Shared validation and visual placement for overlay tools.

use crate::{
    PdfDoc, PdfError, ToolCtx,
    color::page_ids,
    draw::{self, geometry::PageGeometry, layer::LayerPosition},
};
use ariad_core::{
    limits::Limits,
    tools::{
        page_range::PageRange,
        params::{Anchor, overlay_marks_common::MarkLayer},
    },
};
use lopdf::ObjectId;

pub(super) fn numbered_pages(
    pdf: &PdfDoc,
    range: &PageRange,
    limits: &Limits,
    ctx: &ToolCtx,
) -> Result<Vec<(u32, ObjectId)>, PdfError> {
    let all = page_ids(pdf.inner(), limits, ctx)?;
    draw::check_page_count(all.len(), limits, ctx)?;
    let selected = crate::recolor::selected_pages(pdf, range, limits, ctx)?;
    selected
        .into_iter()
        .map(|index| {
            Ok((
                index,
                *all.get((index - 1) as usize).ok_or(PdfError::Malformed)?,
            ))
        })
        .collect()
}
pub(super) fn pages(
    pdf: &PdfDoc,
    range: &PageRange,
    limits: &Limits,
    ctx: &ToolCtx,
) -> Result<Vec<ObjectId>, PdfError> {
    Ok(numbered_pages(pdf, range, limits, ctx)?
        .into_iter()
        .map(|(_, id)| id)
        .collect())
}
/// Axis-aligned bounds of a rectangle rotated about its lower-left corner.
pub(super) fn rotated_bounds(width: f64, height: f64, rotation: f64) -> (f64, f64, f64, f64) {
    let angle = rotation.to_radians();
    let (sin, cos) = angle.sin_cos();
    let xs = [0.0, width * cos, -height * sin, width * cos - height * sin];
    let ys = [0.0, width * sin, height * cos, width * sin + height * cos];
    (
        xs.into_iter().fold(f64::INFINITY, f64::min),
        ys.into_iter().fold(f64::INFINITY, f64::min),
        xs.into_iter().fold(f64::NEG_INFINITY, f64::max),
        ys.into_iter().fold(f64::NEG_INFINITY, f64::max),
    )
}
#[allow(clippy::too_many_arguments)]
pub(super) fn position(
    geometry: &PageGeometry,
    anchor: Anchor,
    width: f64,
    height: f64,
    margin: f64,
    offset_x: f64,
    offset_y: f64,
    x: Option<f64>,
    y: Option<f64>,
    rotation: f64,
) -> Result<(f64, f64), PdfError> {
    let (left, bottom, right, top) = rotated_bounds(width, height, rotation);
    let (ax, ay) = geometry.place(
        anchor,
        right - left,
        top - bottom,
        margin,
        offset_x,
        offset_y,
    )?;
    Ok((
        x.map_or(ax, |v| v + offset_x) - left,
        y.map_or(ay, |v| v + offset_y) - bottom,
    ))
}
pub(super) fn layer(value: MarkLayer) -> LayerPosition {
    match value {
        MarkLayer::Over => LayerPosition::Over,
        MarkLayer::Under => LayerPosition::Under,
    }
}
