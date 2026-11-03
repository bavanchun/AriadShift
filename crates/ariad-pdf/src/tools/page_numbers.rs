//! Add page-number pagination artifacts.
use crate::{
    PdfDoc, PdfError, ToolCtx,
    draw::{
        font::FontPlan,
        geometry::PageGeometry,
        layer::LayerPosition,
        text::{self, TextAlign, TextLayout, TextStyle},
        tokens::{self, TokenContext},
    },
    tools::{OutputDoc, furniture_common, marks_common},
};
use ariad_core::{
    limits::Limits,
    tools::params::{Opacity, PageNumbersParams},
};
/// Add numbered pagination to selected pages.
pub fn run(
    inputs: &[PdfDoc],
    params: &PageNumbersParams,
    limits: &Limits,
    ctx: &ToolCtx,
) -> Result<Vec<OutputDoc>, PdfError> {
    if inputs.len() != 1 {
        return Err(PdfError::InvalidParams(
            "pdf.page_numbers requires exactly one input".into(),
        ));
    }
    params
        .validate()
        .map_err(|e| PdfError::InvalidParams(e.into()))?;
    furniture_common::check_tokens(&params.format, &["n", "total", "label"])?;
    let mut pdf = inputs[0].clone();
    let selected = marks_common::numbered_pages(&pdf, &params.pages, limits, ctx)?;
    let selected: Vec<_> = selected
        .into_iter()
        .skip(params.skip_first as usize)
        .collect();
    if selected.is_empty() {
        return Ok(vec![OutputDoc::unnamed_pdf(pdf)]);
    }
    let total = params
        .start
        .checked_add(u32::try_from(selected.len() - 1).map_err(|_| PdfError::Malformed)?)
        .ok_or_else(|| PdfError::InvalidParams("page-number sequence overflows".into()))?;
    let rules = furniture_common::labels(pdf.inner(), ctx)?;
    let mut plan = FontPlan::new(params.font);
    let mut prepared = Vec::with_capacity(selected.len());
    for (index, (number, page)) in selected.into_iter().enumerate() {
        ctx.check()?;
        let label = furniture_common::label(&rules, number)?;
        let n = params
            .start
            .checked_add(index as u32)
            .ok_or_else(|| PdfError::InvalidParams("page-number sequence overflows".into()))?;
        let value = tokens::expand(
            &params.format,
            &TokenContext {
                number: Some(n),
                total: Some(total),
                label: Some(&label),
                ..TokenContext::default()
            },
            ctx,
        )?;
        if value.is_empty() {
            continue;
        }
        let layout = TextLayout::prepare(&mut plan, &value, params.font_size_pt, None, 1.2)?;
        prepared.push((page, layout));
    }
    if prepared.is_empty() {
        return Ok(vec![OutputDoc::unnamed_pdf(pdf)]);
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
    for (page, layout) in prepared {
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
