//! Add pagination headers and footers using the caller's original filename.
use crate::{
    PdfDoc, PdfError, ToolCtx,
    draw::{
        font::FontPlan,
        geometry::PageGeometry,
        layer::{LayerPosition, Marking},
        text::{self, TextAlign, TextLayout, TextStyle},
        tokens::{self, TokenContext},
    },
    tools::{InputDoc, OutputDoc, furniture_common, marks_common},
};
use ariad_core::tools::params::{Anchor, HeaderFooterParams, Opacity};
use unicode_normalization::UnicodeNormalization;

/// Render header and footer templates on selected pages.
pub fn run_inputs(
    inputs: &[InputDoc],
    params: &HeaderFooterParams,
    ctx: &ToolCtx,
) -> Result<(Vec<PdfDoc>, Vec<OutputDoc>), PdfError> {
    if inputs.len() != 1 {
        return Err(PdfError::InvalidParams(
            "pdf.header_footer requires exactly one input".into(),
        ));
    }
    params
        .validate()
        .map_err(|e| PdfError::InvalidParams(e.into()))?;
    let original = inputs[0].open_for_mutation(ctx)?;
    let mut pdf = original.clone();
    let pages = marks_common::numbered_pages(&pdf, &params.pages, &inputs[0].limits, ctx)?;
    let pages: Vec<_> = pages.into_iter().skip(params.skip_first as usize).collect();
    if pages.is_empty() {
        return Ok((vec![original], vec![OutputDoc::unnamed_pdf(pdf)]));
    }
    let filename = inputs[0]
        .stem
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
        .chars()
        .filter(|ch| !ch.is_control())
        .collect::<String>()
        .nfc()
        .collect::<String>();
    if filename.chars().count() > 4096 {
        return Err(PdfError::InvalidParams(
            "filename is too long for a header or footer".into(),
        ));
    }
    let total = crate::color::page_ids(pdf.inner(), &inputs[0].limits, ctx)?.len() as u32;
    let rules = furniture_common::labels(pdf.inner(), ctx)?;
    let mut plan = FontPlan::new(params.font);
    let mut prepared = Vec::new();
    let slots = [
        (
            &params.header.left,
            Anchor::TopLeft,
            Marking::Header,
            TextAlign::Left,
        ),
        (
            &params.header.center,
            Anchor::TopCenter,
            Marking::Header,
            TextAlign::Center,
        ),
        (
            &params.header.right,
            Anchor::TopRight,
            Marking::Header,
            TextAlign::Right,
        ),
        (
            &params.footer.left,
            Anchor::BottomLeft,
            Marking::Footer,
            TextAlign::Left,
        ),
        (
            &params.footer.center,
            Anchor::BottomCenter,
            Marking::Footer,
            TextAlign::Center,
        ),
        (
            &params.footer.right,
            Anchor::BottomRight,
            Marking::Footer,
            TextAlign::Right,
        ),
    ];
    for (number, page) in pages {
        ctx.check()?;
        let geometry = PageGeometry::from_page(pdf.inner(), page)?;
        let width = (geometry.width_pt - 2.0 * params.margin_pt) / 3.0;
        if width <= 0.0 {
            return Err(PdfError::InvalidParams(
                "page is too narrow for header and footer margins".into(),
            ));
        }
        let label = furniture_common::label(&rules, number)?;
        let values = TokenContext {
            number: Some(number),
            total: Some(total),
            label: Some(&label),
            filename: Some(&filename),
            date: params.date.as_deref(),
            date_format: params.date_format,
        };
        for (template, anchor, marking, align) in &slots {
            let Some(template) = template.as_deref().filter(|text| !text.is_empty()) else {
                continue;
            };
            furniture_common::check_tokens(template, &["filename", "date", "n", "total", "label"])?;
            let text = tokens::expand(template, &values, ctx)?;
            if text.is_empty() {
                continue;
            }
            let layout =
                TextLayout::prepare(&mut plan, &text, params.font_size_pt, Some(width), 1.2)?;
            prepared.push((page, geometry, *anchor, *marking, *align, layout));
        }
    }
    if prepared.is_empty() {
        return Ok((vec![original], vec![OutputDoc::unnamed_pdf(pdf)]));
    }
    let font = plan.embed(pdf.inner_mut()?)?;
    let opacity: Opacity =
        serde_json::from_value(serde_json::json!(1.0)).map_err(|_| PdfError::Malformed)?;
    for (page, geometry, anchor, marking, align, layout) in prepared {
        ctx.check()?;
        let (x, y) = marks_common::position(
            &geometry,
            anchor,
            layout.width,
            layout.height,
            params.margin_pt,
            0.0,
            0.0,
            None,
            None,
            0.0,
        )?;
        let style = TextStyle {
            size_pt: params.font_size_pt,
            color: params.color.clone(),
            opacity,
            stroke: None,
            align,
            rotation_deg: 0.0,
        };
        text::draw(
            &mut pdf,
            page,
            &geometry,
            &font,
            &layout,
            &style,
            x,
            y,
            marking,
            LayerPosition::Over,
            ctx,
        )?;
    }
    Ok((vec![original], vec![OutputDoc::unnamed_pdf(pdf)]))
}
