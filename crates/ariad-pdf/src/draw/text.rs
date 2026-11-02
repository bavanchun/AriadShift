//! Text layout and content-stream generation for embedded font subsets.

use ariad_core::tools::params::{HexColor, Opacity};
use lopdf::{
    Object, StringFormat,
    content::{Content, Operation},
    dictionary,
};

use crate::{
    PdfDoc, PdfError, ToolCtx,
    draw::{
        font::{EmbeddedFont, FontPlan},
        geometry::PageGeometry,
        layer::{self, LayerPosition, Marking},
    },
    xobject::Matrix,
};

/// Horizontal alignment within the layout width.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
}

/// Prepared lines and measurements in physical points.
#[derive(Clone, Debug)]
pub struct TextLayout {
    lines: Vec<String>,
    widths: Vec<f64>,
    /// Width of the layout box.
    pub width: f64,
    /// Height of the layout box.
    pub height: f64,
    /// Ascender above the baseline in points.
    pub ascent: f64,
    line_step: f64,
    size_pt: f64,
}

impl TextLayout {
    /// Normalize text, add its glyphs to the font plan, and calculate line breaks.
    pub fn prepare(
        plan: &mut FontPlan,
        text: &str,
        size: f64,
        max_width: Option<f64>,
        line_height: f64,
    ) -> Result<Self, PdfError> {
        if !size.is_finite()
            || !(0.0..=10_000.0).contains(&size)
            || size == 0.0
            || !line_height.is_finite()
            || !(0.1..=10.0).contains(&line_height)
            || max_width.is_some_and(|w| !w.is_finite() || w <= 0.0 || w > 1_000_000.0)
        {
            return Err(PdfError::InvalidParams(
                "invalid text layout dimensions".into(),
            ));
        }
        let text = plan.add_text(text)?;
        let mut lines = Vec::new();
        for paragraph in text.split('\n') {
            if let Some(limit) = max_width {
                let mut line = String::new();
                for word in paragraph.split_inclusive(' ') {
                    let candidate = format!("{line}{word}");
                    if !line.is_empty() && plan.measure_line(&candidate, size)? > limit {
                        lines.push(std::mem::take(&mut line));
                    }
                    line.push_str(word);
                }
                lines.push(line);
            } else {
                lines.push(paragraph.to_owned());
            }
        }
        let widths: Vec<f64> = lines
            .iter()
            .map(|line| plan.measure_line(line, size))
            .collect::<Result<_, _>>()?;
        let max_line = widths.iter().copied().fold(0.0_f64, f64::max);
        let width = max_width.unwrap_or(max_line).max(max_line);
        let (ascent, descent) = plan.vertical_metrics(size)?;
        let line_step = size * line_height;
        let height = ascent - descent + (lines.len().saturating_sub(1) as f64) * line_step;
        Ok(Self {
            lines,
            widths,
            width,
            height,
            ascent,
            line_step,
            size_pt: size,
        })
    }
}

/// Visual text appearance and placement.
#[derive(Clone, Debug)]
pub struct TextStyle {
    /// Font size in physical points.
    pub size_pt: f64,
    /// RGB fill colour.
    pub color: HexColor,
    /// Fill opacity.
    pub opacity: Opacity,
    /// Optional stroke colour and width in points.
    pub stroke: Option<(HexColor, f64)>,
    /// Horizontal alignment.
    pub align: TextAlign,
    /// Counterclockwise angle in displayed coordinates.
    pub rotation_deg: f64,
}

fn real(value: f64) -> Result<Object, PdfError> {
    if !value.is_finite() || value.abs() > 1_000_000.0 {
        return Err(PdfError::InvalidParams(
            "drawing coordinate out of range".into(),
        ));
    }
    Ok(Object::Real(value as f32))
}

/// Add the prepared layout at visual-space `(x, y)`, the lower-left of its box.
#[allow(clippy::too_many_arguments)]
pub fn draw(
    pdf: &mut PdfDoc,
    page: lopdf::ObjectId,
    geometry: &PageGeometry,
    font: &EmbeddedFont,
    layout: &TextLayout,
    style: &TextStyle,
    x: f64,
    y: f64,
    marking: Marking,
    position: LayerPosition,
    ctx: &ToolCtx,
) -> Result<(), PdfError> {
    draw_many(
        pdf,
        page,
        geometry,
        font,
        layout,
        style,
        &[(x, y)],
        marking,
        position,
        ctx,
    )
}

/// Draw one layout at several positions in a single isolated content stream.
#[allow(clippy::too_many_arguments)]
pub fn draw_many(
    pdf: &mut PdfDoc,
    page: lopdf::ObjectId,
    geometry: &PageGeometry,
    font: &EmbeddedFont,
    layout: &TextLayout,
    style: &TextStyle,
    placements: &[(f64, f64)],
    marking: Marking,
    position: LayerPosition,
    ctx: &ToolCtx,
) -> Result<(), PdfError> {
    const MAX_OPERATIONS: usize = 262_144;
    const MAX_TEXT_BYTES: usize = 64 * 1024 * 1024;
    if !style.rotation_deg.is_finite()
        || style.rotation_deg.abs() > 360_000.0
        || style.size_pt != layout.size_pt
        || placements.is_empty()
        || placements.len() > super::geometry::MAX_PLACEMENTS_PER_PAGE
    {
        return Err(PdfError::InvalidParams(
            "invalid text style or placements".into(),
        ));
    }
    let mut resources = layer::page_resources(pdf.inner(), page);
    let font_name = layer::insert_resource(
        pdf.inner(),
        &mut resources,
        b"Font",
        "f",
        Object::Reference(font.id),
    )?;
    let alpha = style.opacity.value();
    let gs_name = if alpha < 1.0 {
        let gs = pdf
            .inner_mut()?
            .add_object(dictionary! {"Type"=>"ExtGState","ca"=>real(alpha)?,"CA"=>real(alpha)?});
        layer::require_transparency(pdf.inner_mut()?);
        Some(layer::insert_resource(
            pdf.inner(),
            &mut resources,
            b"ExtGState",
            "gs",
            Object::Reference(gs),
        )?)
    } else {
        None
    };
    let mut template = Vec::new();
    if let Some(gs) = gs_name {
        template.push(Operation::new("gs", vec![Object::Name(gs)]));
    }
    template.push(Operation::new("rg", super::path::components(&style.color)?));
    if let Some((color, width)) = &style.stroke {
        if !width.is_finite() || *width < 0.0 {
            return Err(PdfError::InvalidParams("invalid text stroke width".into()));
        }
        template.push(Operation::new("RG", super::path::components(color)?));
        template.push(Operation::new("w", vec![real(*width)?]));
    }
    template.push(Operation::new("BT", vec![]));
    if style.stroke.is_some() {
        template.push(Operation::new("Tr", vec![2.into()]));
    }
    template.push(Operation::new(
        "Tf",
        vec![Object::Name(font_name), real(style.size_pt)?],
    ));
    let mut encoded_bytes = 0usize;
    for (index, (line, width)) in layout.lines.iter().zip(&layout.widths).enumerate() {
        let dx = match style.align {
            TextAlign::Left => 0.0,
            TextAlign::Center => (layout.width - width) / 2.0,
            TextAlign::Right => layout.width - width,
        };
        let baseline = layout.height - layout.ascent - (index as f64 * layout.line_step);
        template.push(Operation::new(
            "Tm",
            vec![
                1.into(),
                0.into(),
                0.into(),
                1.into(),
                real(dx)?,
                real(baseline)?,
            ],
        ));
        let encoded = font.encode(line)?;
        encoded_bytes = encoded_bytes.saturating_add(encoded.len());
        template.push(Operation::new(
            "Tj",
            vec![Object::String(encoded, StringFormat::Hexadecimal)],
        ));
    }
    template.push(Operation::new("ET", vec![]));
    let operation_count = placements
        .len()
        .saturating_mul(template.len().saturating_add(3));
    if operation_count > MAX_OPERATIONS
        || placements.len().saturating_mul(encoded_bytes) > MAX_TEXT_BYTES
    {
        return Err(PdfError::InvalidParams(
            "text placements exceed drawing limits".into(),
        ));
    }
    let mut operations = Vec::with_capacity(operation_count);
    for &(x, y) in placements {
        ctx.check()?;
        let transform = Matrix::rotate_deg(style.rotation_deg)
            .then(Matrix::translate(x, y))
            .then(geometry.visual_to_user);
        operations.push(Operation::new("q", vec![]));
        operations.push(Operation::new(
            "cm",
            transform
                .0
                .into_iter()
                .map(real)
                .collect::<Result<Vec<_>, _>>()?,
        ));
        operations.extend(template.iter().cloned());
        operations.push(Operation::new("Q", vec![]));
    }
    let body = Content { operations }
        .encode()
        .map_err(|_| PdfError::Malformed)?;
    layer::append(pdf, page, resources, &body, marking, position, ctx)
}
