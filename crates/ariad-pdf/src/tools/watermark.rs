//! Add removable text or image watermarks to selected pages.

use crate::{
    PdfDoc, PdfError, ToolCtx,
    draw::{
        font::FontPlan,
        geometry::{PageGeometry, tiled_grid},
        image::{self, ImagePlacement},
        layer::Marking,
        text::{self, TextAlign, TextLayout, TextStyle},
    },
    tools::{InputDoc, OutputDoc, ToolWarning, mark_image, marks_common},
};
use ariad_core::tools::params::{
    WatermarkFontSize, WatermarkKind, WatermarkLayout, WatermarkParams,
};

fn placements(
    geometry: &PageGeometry,
    params: &WatermarkParams,
    width: f64,
    height: f64,
    rotation: f64,
) -> Result<Vec<(f64, f64)>, PdfError> {
    if params.layout != WatermarkLayout::Tiled {
        return Ok(vec![marks_common::position(
            geometry,
            params.anchor,
            width,
            height,
            params.margin_pt,
            params.offset_x_pt,
            params.offset_y_pt,
            params.x_pt,
            params.y_pt,
            rotation,
        )?]);
    }
    let (left, bottom, right, top) = marks_common::rotated_bounds(width, height, rotation);
    let grid = tiled_grid(
        geometry.width_pt,
        geometry.height_pt,
        right - left + params.tile_gap_pt,
        top - bottom + params.tile_gap_pt,
    )?;
    let x = params.x_pt.unwrap_or(params.margin_pt) + params.offset_x_pt - left;
    let y = params.y_pt.unwrap_or(params.margin_pt) + params.offset_y_pt - bottom;
    Ok(grid.into_iter().map(|(gx, gy)| (x + gx, y + gy)).collect())
}
fn warn_hidden(
    original: &PdfDoc,
    pages: &[lopdf::ObjectId],
    params: &WatermarkParams,
    limits: &ariad_core::limits::Limits,
    ctx: &ToolCtx,
) -> Result<bool, PdfError> {
    if params.layer != ariad_core::tools::params::overlay_marks_common::MarkLayer::Under {
        return Ok(false);
    }
    let mut budget = original.decode_budget();
    for page in pages {
        if mark_image::hidden_under_scan(original, *page, limits, &mut budget, ctx)? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Add one removable watermark layer, using a second input only for an image mark.
pub fn run_inputs(
    inputs: &[InputDoc],
    params: &WatermarkParams,
    ctx: &ToolCtx,
) -> Result<(Vec<PdfDoc>, Vec<OutputDoc>), PdfError> {
    params
        .validate()
        .map_err(|e| PdfError::InvalidParams(e.into()))?;
    let expected = match params.kind {
        WatermarkKind::Text => 1,
        WatermarkKind::Image => 2,
    };
    if inputs.len() != expected {
        return Err(PdfError::InvalidParams(format!(
            "watermark kind requires exactly {expected} inputs"
        )));
    }
    let (original, image_bytes) = if params.kind == WatermarkKind::Image {
        let (pdf, bytes) = mark_image::inputs(inputs, ctx)?;
        (pdf, Some(bytes))
    } else {
        (mark_image::pdf_input(&inputs[0], ctx)?, None)
    };
    let mut pdf = original.clone();
    let pages = marks_common::pages(&pdf, &params.pages, &inputs[0].limits, ctx)?;
    let hidden = warn_hidden(&original, &pages, params, &inputs[0].limits, ctx)?;
    match params.kind {
        WatermarkKind::Text => {
            let text_value = params
                .text
                .as_deref()
                .ok_or_else(|| PdfError::InvalidParams("text watermark requires text".into()))?;
            let mut plan = FontPlan::new(params.font);
            let normalized = plan.add_text(text_value)?;
            let unit_width = normalized
                .split('\n')
                .map(|line| plan.measure_line(line, 1.0))
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .fold(0.0_f64, f64::max);
            if unit_width <= 0.0 {
                return Err(PdfError::InvalidParams(
                    "watermark has no visible width".into(),
                ));
            }
            let mut layouts = Vec::with_capacity(pages.len());
            for page in pages {
                ctx.check()?;
                let geometry = PageGeometry::from_page(pdf.inner(), page)?;
                let size = match params.font_size_pt {
                    WatermarkFontSize::Points(size) => size,
                    WatermarkFontSize::Auto(_) => {
                        if params.layout == WatermarkLayout::Diagonal {
                            0.7 * geometry.width_pt.hypot(geometry.height_pt) / unit_width
                        } else {
                            48.0
                        }
                    }
                };
                let layout = TextLayout::prepare(&mut plan, &normalized, size, None, 1.2)?;
                let angle = if params.layout == WatermarkLayout::Diagonal {
                    geometry.diagonal_angle()
                } else {
                    params.rotation_deg
                };
                let positions = placements(&geometry, params, layout.width, layout.height, angle)?;
                layouts.push((page, geometry, size, angle, layout, positions));
            }
            let font = plan.embed(pdf.inner_mut()?)?;
            for (page, geometry, size, angle, layout, positions) in layouts {
                let style = TextStyle {
                    size_pt: size,
                    color: params.color.clone(),
                    opacity: params.opacity,
                    stroke: None,
                    align: TextAlign::Center,
                    rotation_deg: angle,
                };
                text::draw_many(
                    &mut pdf,
                    page,
                    &geometry,
                    &font,
                    &layout,
                    &style,
                    &positions,
                    Marking::Watermark,
                    marks_common::layer(params.layer),
                    ctx,
                )?;
            }
        }
        WatermarkKind::Image => {
            let bytes = image_bytes.ok_or(PdfError::Malformed)?;
            let image = mark_image::embed(&mut pdf, bytes, &inputs[1].limits, ctx)?;
            let (native_width, native_height) = mark_image::natural_size(bytes, image, ctx)?;
            let width = params.image_width_pt.unwrap_or(native_width);
            let height = width * native_height / native_width;
            for page in pages {
                ctx.check()?;
                let geometry = PageGeometry::from_page(pdf.inner(), page)?;
                let angle = if params.layout == WatermarkLayout::Diagonal {
                    geometry.diagonal_angle()
                } else {
                    params.rotation_deg
                };
                let positions = placements(&geometry, params, width, height, angle)?;
                image::draw_many(
                    &mut pdf,
                    page,
                    &geometry,
                    image,
                    ImagePlacement {
                        x: 0.0,
                        y: 0.0,
                        width,
                        height,
                        rotation_deg: angle,
                        opacity: params.opacity,
                    },
                    &positions,
                    Marking::Watermark,
                    marks_common::layer(params.layer),
                    ctx,
                )?;
            }
        }
    }
    let mut result = OutputDoc::unnamed_pdf(pdf);
    if hidden {
        result.coded_warnings.push(ToolWarning {
            code: "watermark_hidden_under_content".into(),
            message: "an opaque page image may hide the watermark placed underneath it".into(),
        });
    }
    Ok((vec![original], vec![result]))
}
