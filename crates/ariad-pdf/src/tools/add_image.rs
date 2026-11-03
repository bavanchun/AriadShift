//! Place a raster image on selected PDF pages.

use crate::{
    PdfDoc, PdfError, ToolCtx,
    draw::{
        geometry::PageGeometry,
        image::{self, ImagePlacement},
        layer::Marking,
    },
    tools::{InputDoc, OutputDoc, mark_image, marks_common},
};
use ariad_core::tools::params::AddImageParams;

/// Validate a PDF and image input, then place one embedded image on selected pages.
pub fn run_inputs(
    inputs: &[InputDoc],
    params: &AddImageParams,
    ctx: &ToolCtx,
) -> Result<(Vec<PdfDoc>, Vec<OutputDoc>), PdfError> {
    params
        .validate()
        .map_err(|e| PdfError::InvalidParams(e.into()))?;
    let (original, bytes) = mark_image::inputs(inputs, ctx)?;
    let mut pdf = original.clone();
    let pages = marks_common::pages(&pdf, &params.pages, &inputs[0].limits, ctx)?;
    let embedded = mark_image::embed(&mut pdf, bytes, &inputs[1].limits, ctx)?;
    let (native_width, native_height) = mark_image::natural_size(bytes, embedded, ctx)?;
    let (width, height) = match (params.width_pt, params.height_pt) {
        (Some(w), Some(h)) => (w, h),
        (Some(w), None) => (w, w * native_height / native_width),
        (None, Some(h)) => (h * native_width / native_height, h),
        (None, None) => (native_width, native_height),
    };
    for page in pages {
        ctx.check()?;
        let geometry = PageGeometry::from_page(pdf.inner(), page)?;
        let (x, y) = marks_common::position(
            &geometry,
            params.anchor,
            width,
            height,
            params.margin_pt,
            params.offset_x_pt,
            params.offset_y_pt,
            params.x_pt,
            params.y_pt,
            params.rotation_deg,
        )?;
        image::draw(
            &mut pdf,
            page,
            &geometry,
            embedded,
            ImagePlacement {
                x,
                y,
                width,
                height,
                rotation_deg: params.rotation_deg,
                opacity: params.opacity,
            },
            Marking::Content,
            marks_common::layer(params.layer),
            ctx,
        )?;
    }
    Ok((vec![original], vec![OutputDoc::unnamed_pdf(pdf)]))
}
