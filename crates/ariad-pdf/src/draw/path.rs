//! Bounded vector primitives in displayed page coordinates.

use ariad_core::tools::params::{HexColor, Opacity};
use lopdf::{
    Object,
    content::{Content, Operation},
    dictionary,
};

use crate::{
    PdfDoc, PdfError, ToolCtx,
    draw::{
        geometry::PageGeometry,
        layer::{self, LayerPosition, Marking},
    },
};

/// Maximum number of primitive segments in a path.
pub const MAX_PATH_SEGMENTS: usize = 65_536;

fn num(value: f64) -> Result<Object, PdfError> {
    if !value.is_finite() || value.abs() > 1_000_000.0 {
        return Err(PdfError::InvalidParams(
            "drawing coordinate out of range".into(),
        ));
    }
    Ok(Object::Real(value as f32))
}

/// A sequence of path operations; painted when passed to [`draw`].
#[derive(Clone, Debug, Default)]
pub struct Path {
    operations: Vec<Operation>,
}

impl Path {
    /// Create an empty path.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
    fn add(&mut self, operator: &str, values: &[f64]) -> Result<(), PdfError> {
        if self.operations.len() >= MAX_PATH_SEGMENTS {
            return Err(PdfError::InvalidParams("too many path segments".into()));
        }
        self.operations.push(Operation::new(
            operator,
            values
                .iter()
                .copied()
                .map(num)
                .collect::<Result<Vec<_>, _>>()?,
        ));
        Ok(())
    }
    /// Add a rectangle.
    pub fn rect(&mut self, x: f64, y: f64, width: f64, height: f64) -> Result<(), PdfError> {
        if width <= 0.0 || height <= 0.0 {
            return Err(PdfError::InvalidParams(
                "rectangle dimensions must be positive".into(),
            ));
        }
        self.add("re", &[x, y, width, height])
    }
    /// Add several rectangles to one path, such as merged QR module runs.
    pub fn rects(&mut self, rects: &[(f64, f64, f64, f64)]) -> Result<(), PdfError> {
        if rects.len() > MAX_PATH_SEGMENTS.saturating_sub(self.operations.len()) {
            return Err(PdfError::InvalidParams("too many path segments".into()));
        }
        for &(x, y, w, h) in rects {
            self.rect(x, y, w, h)?;
        }
        Ok(())
    }
    /// Add a line segment.
    pub fn line(&mut self, x1: f64, y1: f64, x2: f64, y2: f64) -> Result<(), PdfError> {
        self.add("m", &[x1, y1])?;
        self.add("l", &[x2, y2])
    }
    /// Add a closed rounded rectangle using cubic Bézier curves.
    pub fn rounded_rect(
        &mut self,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        radius: f64,
    ) -> Result<(), PdfError> {
        if width <= 0.0
            || height <= 0.0
            || !radius.is_finite()
            || radius < 0.0
            || radius > width.min(height) / 2.0
        {
            return Err(PdfError::InvalidParams("invalid rounded rectangle".into()));
        }
        let k = radius * 0.552_284_749_830_793_6;
        let (r, t) = (x + width, y + height);
        self.add("m", &[x + radius, y])?;
        self.add("l", &[r - radius, y])?;
        self.add("c", &[r - radius + k, y, r, y + radius - k, r, y + radius])?;
        self.add("l", &[r, t - radius])?;
        self.add("c", &[r, t - radius + k, r - radius + k, t, r - radius, t])?;
        self.add("l", &[x + radius, t])?;
        self.add("c", &[x + radius - k, t, x, t - radius + k, x, t - radius])?;
        self.add("l", &[x, y + radius])?;
        self.add("c", &[x, y + radius - k, x + radius - k, y, x + radius, y])?;
        self.add("h", &[])
    }
}

/// Fill and stroke parameters for a path.
#[derive(Clone, Debug)]
pub struct PathStyle {
    /// Fill colour, if filled.
    pub fill: Option<HexColor>,
    /// Stroke colour, if stroked.
    pub stroke: Option<HexColor>,
    /// Width of the stroke in points.
    pub stroke_width_pt: f64,
    /// Shared fill and stroke opacity.
    pub opacity: Opacity,
}

pub(crate) fn components(color: &HexColor) -> Result<Vec<Object>, PdfError> {
    let value = color.as_str().as_bytes();
    [1, 3, 5]
        .into_iter()
        .map(|i| {
            let raw = std::str::from_utf8(&value[i..i + 2]).map_err(|_| PdfError::Malformed)?;
            let channel = u8::from_str_radix(raw, 16).map_err(|_| PdfError::Malformed)?;
            num(f64::from(channel) / 255.0)
        })
        .collect()
}

/// Paint a path on a page, with inherited resources retained.
#[allow(clippy::too_many_arguments)]
pub fn draw(
    pdf: &mut PdfDoc,
    page: lopdf::ObjectId,
    geometry: &PageGeometry,
    path: &Path,
    style: &PathStyle,
    marking: Marking,
    position: LayerPosition,
    ctx: &ToolCtx,
) -> Result<(), PdfError> {
    if path.operations.is_empty()
        || (style.fill.is_none() && style.stroke.is_none())
        || !style.stroke_width_pt.is_finite()
        || style.stroke_width_pt < 0.0
    {
        return Err(PdfError::InvalidParams("invalid path style".into()));
    }
    let mut resources = layer::page_resources(pdf.inner(), page);
    let mut ops = vec![Operation::new(
        "cm",
        geometry
            .visual_to_user
            .0
            .into_iter()
            .map(num)
            .collect::<Result<_, _>>()?,
    )];
    if style.opacity.value() < 1.0 {
        let opacity = num(style.opacity.value())?;
        let id = pdf.inner_mut()?.add_object(
            dictionary! { "Type" => "ExtGState", "ca" => opacity.clone(), "CA" => opacity },
        );
        layer::require_transparency(pdf.inner_mut()?);
        let name = layer::insert_resource(
            pdf.inner(),
            &mut resources,
            b"ExtGState",
            "gs",
            Object::Reference(id),
        )?;
        ops.push(Operation::new("gs", vec![Object::Name(name)]));
    }
    if let Some(fill) = &style.fill {
        ops.push(Operation::new("rg", components(fill)?));
    }
    if let Some(stroke) = &style.stroke {
        ops.push(Operation::new("RG", components(stroke)?));
        ops.push(Operation::new("w", vec![num(style.stroke_width_pt)?]));
    }
    ops.extend(path.operations.iter().cloned());
    ops.push(Operation::new(
        match (style.fill.is_some(), style.stroke.is_some()) {
            (true, true) => "B",
            (true, false) => "f",
            (false, true) => "S",
            _ => unreachable!(),
        },
        vec![],
    ));
    let body = Content { operations: ops }
        .encode()
        .map_err(|_| PdfError::Malformed)?;
    layer::append(pdf, page, resources, &body, marking, position, ctx)
}
