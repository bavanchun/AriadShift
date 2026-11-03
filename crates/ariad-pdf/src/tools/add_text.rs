//! Add text to selected PDF pages.

use crate::{
    PdfDoc, PdfError, ToolCtx,
    draw::{
        font::FontPlan,
        geometry::PageGeometry,
        layer::Marking,
        text::{self, TextAlign, TextLayout, TextStyle},
    },
    tools::{OutputDoc, marks_common},
};
use ariad_core::{
    limits::Limits,
    tools::params::{AddTextParams, TextAlignment},
};

/// Draw a text layout on every selected page.
pub fn run(
    inputs: &[PdfDoc],
    params: &AddTextParams,
    limits: &Limits,
    ctx: &ToolCtx,
) -> Result<Vec<OutputDoc>, PdfError> {
    if inputs.len() != 1 {
        return Err(PdfError::InvalidParams(
            "pdf.add_text requires exactly one input".into(),
        ));
    }
    params
        .validate()
        .map_err(|e| PdfError::InvalidParams(e.into()))?;
    let mut pdf = inputs[0].clone();
    let pages = marks_common::pages(&pdf, &params.pages, limits, ctx)?;
    let mut plan = FontPlan::new(params.font);
    let layout = TextLayout::prepare(
        &mut plan,
        &params.text,
        params.font_size_pt,
        params.max_width_pt,
        params.line_height,
    )?;
    let font = plan.embed(pdf.inner_mut()?)?;
    let style = TextStyle {
        size_pt: params.font_size_pt,
        color: params.color.clone(),
        opacity: params.opacity,
        stroke: None,
        align: match params.align {
            TextAlignment::Left => TextAlign::Left,
            TextAlignment::Center => TextAlign::Center,
            TextAlignment::Right => TextAlign::Right,
        },
        rotation_deg: params.rotation_deg,
    };
    for page in pages {
        ctx.check()?;
        let geometry = PageGeometry::from_page(pdf.inner(), page)?;
        let (x, y) = marks_common::position(
            &geometry,
            params.anchor,
            layout.width,
            layout.height,
            params.margin_pt,
            params.offset_x_pt,
            params.offset_y_pt,
            params.x_pt,
            params.y_pt,
            params.rotation_deg,
        )?;
        text::draw(
            &mut pdf,
            page,
            &geometry,
            &font,
            &layout,
            &style,
            x,
            y,
            Marking::Content,
            marks_common::layer(params.layer),
            ctx,
        )?;
    }
    Ok(vec![OutputDoc::unnamed_pdf(pdf)])
}
