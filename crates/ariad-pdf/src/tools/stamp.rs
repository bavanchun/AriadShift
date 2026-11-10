//! Add removable text stamps to PDF pages.

use crate::{
    PdfDoc, PdfError, ToolCtx,
    draw::{
        font::FontPlan,
        geometry::PageGeometry,
        layer::{LayerPosition, Marking},
        path::{self, Path, PathStyle},
        text::{self, TextAlign, TextLayout, TextStyle},
        tokens::{self, TokenContext},
    },
    tools::{OutputDoc, marks_common},
    xobject::Matrix,
};
use ariad_core::{
    limits::Limits,
    tools::params::{HexColor, StampParams, StampPreset},
};

fn has_date_token(template: &str) -> bool {
    let mut chars = template.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '{' && chars.peek() == Some(&'{') {
            chars.next();
            continue;
        }
        if ch == '{' {
            let mut token = String::new();
            for c in chars.by_ref() {
                if c == '}' {
                    break;
                }
                token.push(c);
            }
            if token == "date" {
                return true;
            }
        } else if ch == '}' && chars.peek() == Some(&'}') {
            chars.next();
        }
    }
    false
}

/// Place a preset or custom stamp on selected pages.
pub fn run(
    inputs: &[PdfDoc],
    params: &StampParams,
    limits: &Limits,
    ctx: &ToolCtx,
) -> Result<Vec<OutputDoc>, PdfError> {
    if inputs.len() != 1 {
        return Err(PdfError::InvalidParams(
            "pdf.stamp requires exactly one input".into(),
        ));
    }
    params
        .validate()
        .map_err(|e| PdfError::InvalidParams(e.into()))?;
    let mut pdf = inputs[0].clone();
    let pages = marks_common::numbered_pages(&pdf, &params.pages, limits, ctx)?;
    let total = crate::color::page_ids(pdf.inner(), limits, ctx)?.len() as u32;
    let template = match params.preset {
        StampPreset::Approved => "APPROVED",
        StampPreset::Draft => "DRAFT",
        StampPreset::Confidential => "CONFIDENTIAL",
        StampPreset::Custom => params
            .text
            .as_deref()
            .ok_or_else(|| PdfError::InvalidParams("custom stamp requires text".into()))?,
    };
    let template = if params.show_date && !has_date_token(template) {
        format!("{template} {{date}}")
    } else {
        template.to_owned()
    };
    let mut plan = FontPlan::new(params.font);
    let mut prepared = Vec::with_capacity(pages.len());
    for (number, page) in pages {
        ctx.check()?;
        let values = TokenContext {
            number: Some(number),
            total: Some(total),
            date: params.date.as_deref(),
            date_format: params.date_format,
            ..TokenContext::default()
        };
        let label = tokens::expand(&template, &values, ctx)?;
        let layout = TextLayout::prepare(&mut plan, &label, params.font_size_pt, None, 1.2)?;
        prepared.push((page, layout));
    }
    let font = plan.embed(pdf.inner_mut()?)?;
    let color: HexColor = if let Some(color) = &params.color {
        color.clone()
    } else {
        serde_json::from_value(serde_json::json!(match params.preset {
            StampPreset::Approved => "#1B7F3B",
            StampPreset::Draft => "#6B6B6B",
            StampPreset::Confidential => "#B00020",
            StampPreset::Custom => "#1B7F3B",
        }))
        .map_err(|_| PdfError::Malformed)?
    };
    let style = TextStyle {
        size_pt: params.font_size_pt,
        color: color.clone(),
        opacity: params.opacity,
        stroke: None,
        align: TextAlign::Center,
        rotation_deg: params.rotation_deg,
    };
    let border_style = PathStyle {
        fill: None,
        stroke: Some(color),
        stroke_width_pt: 1.5,
        opacity: params.opacity,
    };
    let padding = if params.border {
        params.font_size_pt * 0.25
    } else {
        0.0
    };
    for (page, layout) in prepared {
        ctx.check()?;
        let geometry = PageGeometry::from_page(pdf.inner(), page)?;
        let (bx, by) = marks_common::position(
            &geometry,
            params.anchor,
            layout.width + 2.0 * padding,
            layout.height + 2.0 * padding,
            params.margin_pt,
            params.offset_x_pt,
            params.offset_y_pt,
            params.x_pt,
            params.y_pt,
            params.rotation_deg,
        )?;
        let (sin, cos) = params.rotation_deg.to_radians().sin_cos();
        let (x, y) = (bx + padding * (cos - sin), by + padding * (sin + cos));
        if params.border {
            let mut border = Path::new();
            border.rect(
                -padding,
                -padding,
                layout.width + 2.0 * padding,
                layout.height + 2.0 * padding,
            )?;
            let transform = Matrix::rotate_deg(params.rotation_deg).then(Matrix::translate(x, y));
            path::draw_transformed(
                &mut pdf,
                page,
                &geometry,
                &border,
                &border_style,
                transform,
                Marking::Watermark,
                LayerPosition::Over,
                ctx,
            )?;
        }
        text::draw(
            &mut pdf,
            page,
            &geometry,
            &font,
            &layout,
            &style,
            x,
            y,
            Marking::Watermark,
            LayerPosition::Over,
            ctx,
        )?;
    }
    Ok(vec![OutputDoc::unnamed_pdf(pdf)])
}
