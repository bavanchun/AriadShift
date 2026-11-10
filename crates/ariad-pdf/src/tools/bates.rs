//! Add single-document Bates numbers as pagination artifacts.
use crate::{
    PdfDoc, PdfError, ToolCtx,
    draw::{
        font::FontPlan,
        geometry::PageGeometry,
        layer::LayerPosition,
        text::{self, TextAlign, TextLayout, TextStyle},
    },
    tools::{OutputDoc, furniture_common, marks_common},
};
use ariad_core::{
    limits::Limits,
    tools::params::{BatesParams, Opacity},
};
/// Add a bounded Bates sequence to selected pages.
pub fn run(
    inputs: &[PdfDoc],
    params: &BatesParams,
    limits: &Limits,
    ctx: &ToolCtx,
) -> Result<Vec<OutputDoc>, PdfError> {
    if inputs.len() != 1 {
        return Err(PdfError::InvalidParams(
            "pdf.bates requires exactly one input".into(),
        ));
    }
    params
        .validate()
        .map_err(|e| PdfError::InvalidParams(e.into()))?;
    let mut pdf = inputs[0].clone();
    let pages = marks_common::pages(&pdf, &params.pages, limits, ctx)?;
    if pages.is_empty() {
        return Ok(vec![OutputDoc::unnamed_pdf(pdf)]);
    }
    let last = params
        .start
        .checked_add((pages.len() - 1) as u64)
        .ok_or_else(|| PdfError::InvalidParams("Bates sequence overflows".into()))?;
    if last >= 10_u64.pow(u32::from(params.digits)) {
        return Err(PdfError::InvalidParams(
            "Bates sequence exceeds its digit width".into(),
        ));
    }
    let mut plan = FontPlan::new(params.font);
    let mut layouts = Vec::with_capacity(pages.len());
    for (index, page) in pages.into_iter().enumerate() {
        ctx.check()?;
        let value = params.start + index as u64;
        let label = format!(
            "{}{:0width$}{}",
            params.prefix,
            value,
            params.suffix,
            width = usize::from(params.digits)
        );
        let layout = TextLayout::prepare(&mut plan, &label, params.font_size_pt, None, 1.2)?;
        layouts.push((page, layout));
    }
    let font = plan.embed(pdf.inner_mut()?)?;
    let opacity: Opacity =
        serde_json::from_value(serde_json::json!(1.0)).map_err(|_| PdfError::Malformed)?;
    let style = TextStyle {
        size_pt: params.font_size_pt,
        color: params.color.clone(),
        opacity,
        stroke: None,
        align: TextAlign::Center,
        rotation_deg: 0.0,
    };
    for (page, layout) in layouts {
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
            None,
            None,
            0.0,
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
            furniture_common::pagination(params.anchor),
            LayerPosition::Over,
            ctx,
        )?;
    }
    Ok(vec![OutputDoc::unnamed_pdf(pdf)])
}
