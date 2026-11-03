//! Deterministic normal appearances for supported PDF annotations.

use ariad_core::tools::params::annotations::{AnnotationItem, AnnotationType, StampName};
use lopdf::{
    Document, Object, ObjectId, Stream, StringFormat,
    content::{Content, Operation},
    dictionary,
};

use crate::{
    PdfError, ToolCtx,
    forms::appearance::{font_plan, split_lines},
};

const MAX_APPEARANCE_BYTES: usize = 1024 * 1024;
const MAX_SEGMENTS: usize = 20_000;
fn num(n: f64) -> Result<Object, PdfError> {
    if !n.is_finite() || n.abs() > 1_000_000.0 {
        return Err(PdfError::InvalidParams(
            "annotation geometry is out of range".into(),
        ));
    }
    Ok(Object::Real(n as f32))
}
/// Parse an RGB colour in short or long hexadecimal form.
pub fn rgb(value: &str) -> Result<[f64; 3], PdfError> {
    let raw = value
        .strip_prefix('#')
        .ok_or_else(|| PdfError::InvalidParams("invalid colour".into()))?;
    let full = match raw.len() {
        3 => raw.chars().flat_map(|c| [c, c]).collect::<String>(),
        6 => raw.to_owned(),
        _ => return Err(PdfError::InvalidParams("invalid colour".into())),
    };
    if !full.is_ascii() {
        return Err(PdfError::InvalidParams("invalid colour".into()));
    }
    let mut result = [0.0; 3];
    for (i, slot) in result.iter_mut().enumerate() {
        *slot = f64::from(
            u8::from_str_radix(&full[i * 2..i * 2 + 2], 16)
                .map_err(|_| PdfError::InvalidParams("invalid colour".into()))?,
        ) / 255.0;
    }
    Ok(result)
}
fn components(rgb: [f64; 3]) -> Result<Vec<Object>, PdfError> {
    rgb.into_iter().map(num).collect()
}
fn point(x: f64, y: f64, rect: [f64; 4]) -> Result<Vec<Object>, PdfError> {
    Ok(vec![num(x - rect[0])?, num(y - rect[1])?])
}
fn line(
    ops: &mut Vec<Operation>,
    a: [f64; 2],
    b: [f64; 2],
    rect: [f64; 4],
) -> Result<(), PdfError> {
    ops.push(Operation::new("m", point(a[0], a[1], rect)?));
    ops.push(Operation::new("l", point(b[0], b[1], rect)?));
    Ok(())
}
fn path(
    ops: &mut Vec<Operation>,
    points: &[[f64; 2]],
    close: bool,
    rect: [f64; 4],
) -> Result<(), PdfError> {
    let Some(first) = points.first() else {
        return Err(PdfError::InvalidParams("missing annotation points".into()));
    };
    ops.push(Operation::new("m", point(first[0], first[1], rect)?));
    for p in &points[1..] {
        ops.push(Operation::new("l", point(p[0], p[1], rect)?));
    }
    if close {
        ops.push(Operation::new("h", vec![]));
    }
    Ok(())
}
fn label(stamp: StampName) -> &'static str {
    match stamp {
        StampName::Approved => "APPROVED",
        StampName::Experimental => "EXPERIMENTAL",
        StampName::NotApproved => "NOT APPROVED",
        StampName::AsIs => "AS IS",
        StampName::Expired => "EXPIRED",
        StampName::NotForPublicRelease => "NOT FOR PUBLIC RELEASE",
        StampName::Confidential => "CONFIDENTIAL",
        StampName::Final => "FINAL",
        StampName::Sold => "SOLD",
        StampName::Departmental => "DEPARTMENTAL",
        StampName::ForComment => "FOR COMMENT",
        StampName::TopSecret => "TOP SECRET",
        StampName::Draft => "DRAFT",
        StampName::ForPublicRelease => "FOR PUBLIC RELEASE",
    }
}
fn paint(item: &AnnotationItem) -> &'static str {
    if item.fill.is_some() { "B" } else { "S" }
}

/// Build a Form XObject for one annotation in local rectangle coordinates.
pub fn make(
    doc: &mut Document,
    item: &AnnotationItem,
    rect: [f64; 4],
    ctx: &ToolCtx,
) -> Result<Option<ObjectId>, PdfError> {
    ctx.check()?;
    item.validate().map_err(PdfError::InvalidParams)?;
    if item.annotation_type == AnnotationType::Link {
        return Ok(None);
    }
    let width = rect[2] - rect[0];
    let height = rect[3] - rect[1];
    if width <= 0.0 || height <= 0.0 {
        return Err(PdfError::InvalidParams("empty annotation rectangle".into()));
    }
    let default = if item.annotation_type == AnnotationType::Highlight {
        "#FFFF00"
    } else {
        "#000000"
    };
    let stroke = rgb(item.color.as_deref().unwrap_or(default))?;
    let fill = item.fill.as_deref().map(rgb).transpose()?;
    let blend = if item.annotation_type == AnnotationType::Highlight {
        "Multiply"
    } else {
        "Normal"
    };
    let mut resources = dictionary! { "ExtGState" => dictionary! { "GS" => dictionary! { "Type"=>"ExtGState", "CA"=>num(item.opacity)?, "ca"=>num(item.opacity)?, "BM"=>Object::Name(blend.as_bytes().to_vec()) } } };
    let mut ops = vec![
        Operation::new("q", vec![]),
        Operation::new("gs", vec![Object::Name(b"GS".to_vec())]),
        Operation::new("RG", components(stroke)?),
        Operation::new("rg", components(fill.unwrap_or(stroke))?),
        Operation::new("w", vec![num(item.line_width)?]),
    ];
    match item.annotation_type {
        AnnotationType::Highlight
        | AnnotationType::Underline
        | AnnotationType::Strikeout
        | AnnotationType::Squiggly => {
            for quad in item.quads.as_deref().unwrap_or_default() {
                ctx.check()?;
                let p = [
                    [quad[0], quad[1]],
                    [quad[2], quad[3]],
                    [quad[4], quad[5]],
                    [quad[6], quad[7]],
                ];
                match item.annotation_type {
                    AnnotationType::Highlight => {
                        path(&mut ops, &[p[0], p[1], p[3], p[2]], true, rect)?;
                        ops.push(Operation::new("f", vec![]));
                    }
                    AnnotationType::Underline => {
                        line(&mut ops, p[2], p[3], rect)?;
                        ops.push(Operation::new("S", vec![]));
                    }
                    AnnotationType::Strikeout => {
                        let a = [(p[0][0] + p[2][0]) / 2.0, (p[0][1] + p[2][1]) / 2.0];
                        let b = [(p[1][0] + p[3][0]) / 2.0, (p[1][1] + p[3][1]) / 2.0];
                        line(&mut ops, a, b, rect)?;
                        ops.push(Operation::new("S", vec![]));
                    }
                    AnnotationType::Squiggly => {
                        let dx = p[3][0] - p[2][0];
                        let dy = p[3][1] - p[2][1];
                        let len = dx.hypot(dy);
                        let steps = ((len / 2.0).ceil() as usize).max(1);
                        if ops.len().saturating_add(steps) > MAX_SEGMENTS {
                            return Err(PdfError::InvalidParams(
                                "annotation path is too complex".into(),
                            ));
                        }
                        ops.push(Operation::new("m", point(p[2][0], p[2][1], rect)?));
                        for i in 1..=steps {
                            let t = i as f64 / steps as f64;
                            let displacement = if i % 2 == 0 { 0.0 } else { 1.5 };
                            ops.push(Operation::new(
                                "l",
                                point(p[2][0] + dx * t, p[2][1] + dy * t + displacement, rect)?,
                            ));
                        }
                        ops.push(Operation::new("S", vec![]));
                    }
                    _ => return Err(PdfError::Malformed),
                }
            }
        }
        AnnotationType::Line | AnnotationType::Arrow => {
            let points = item.points.as_deref().unwrap_or_default();
            let (Some(&start), Some(&end)) = (points.first(), points.get(1)) else {
                return Err(PdfError::InvalidParams("line requires two points".into()));
            };
            line(&mut ops, start, end, rect)?;
            if item.annotation_type == AnnotationType::Arrow {
                let dx = end[0] - start[0];
                let dy = end[1] - start[1];
                let len = dx.hypot(dy);
                if len > 0.0 {
                    let (ux, uy) = (dx / len, dy / len);
                    let size = (item.line_width * 5.0).max(6.0);
                    let left = [
                        end[0] - size * ux + size * 0.4 * uy,
                        end[1] - size * uy - size * 0.4 * ux,
                    ];
                    let right = [
                        end[0] - size * ux - size * 0.4 * uy,
                        end[1] - size * uy + size * 0.4 * ux,
                    ];
                    line(&mut ops, left, end, rect)?;
                    line(&mut ops, right, end, rect)?;
                }
            }
            ops.push(Operation::new("S", vec![]));
        }
        AnnotationType::Ink => {
            ops.push(Operation::new("J", vec![1.into()]));
            ops.push(Operation::new("j", vec![1.into()]));
            for stroke in item.strokes.as_deref().unwrap_or_default() {
                ctx.check()?;
                path(&mut ops, stroke, false, rect)?;
            }
            ops.push(Operation::new("S", vec![]));
        }
        AnnotationType::Polygon | AnnotationType::Polyline => {
            path(
                &mut ops,
                item.points.as_deref().unwrap_or_default(),
                item.annotation_type == AnnotationType::Polygon,
                rect,
            )?;
            ops.push(Operation::new(paint(item), vec![]));
        }
        AnnotationType::Rectangle
        | AnnotationType::Stamp
        | AnnotationType::FreeText
        | AnnotationType::Note => {
            ops.push(Operation::new(
                "re",
                vec![
                    num(0.5)?,
                    num(0.5)?,
                    num((width - 1.0).max(0.0))?,
                    num((height - 1.0).max(0.0))?,
                ],
            ));
            ops.push(Operation::new(paint(item), vec![]));
            if item.annotation_type == AnnotationType::Note {
                for fraction in [0.35, 0.5, 0.65] {
                    ops.push(Operation::new(
                        "m",
                        vec![num(width * 0.2)?, num(height * fraction)?],
                    ));
                    ops.push(Operation::new(
                        "l",
                        vec![num(width * 0.8)?, num(height * fraction)?],
                    ));
                }
                ops.push(Operation::new("S", vec![]));
            }
        }
        AnnotationType::Ellipse => {
            let (cx, cy, rx, ry) = (width / 2.0, height / 2.0, width / 2.0, height / 2.0);
            let k = 0.552_284_749_830_793_6;
            ops.push(Operation::new("m", vec![num(cx + rx)?, num(cy)?]));
            for values in [
                [cx + rx, cy + k * ry, cx + k * rx, cy + ry, cx, cy + ry],
                [cx - k * rx, cy + ry, cx - rx, cy + k * ry, cx - rx, cy],
                [cx - rx, cy - k * ry, cx - k * rx, cy - ry, cx, cy - ry],
                [cx + k * rx, cy - ry, cx + rx, cy - k * ry, cx + rx, cy],
            ] {
                ops.push(Operation::new(
                    "c",
                    values.into_iter().map(num).collect::<Result<Vec<_>, _>>()?,
                ));
            }
            ops.push(Operation::new("h", vec![]));
            ops.push(Operation::new(paint(item), vec![]));
        }
        AnnotationType::Link => return Ok(None),
    }
    let text = match item.annotation_type {
        AnnotationType::FreeText => item.text.as_deref().or(item.contents.as_deref()),
        AnnotationType::Stamp => item.text.as_deref().or_else(|| item.stamp.map(label)),
        _ => None,
    };
    if let Some(text) = text.filter(|s| !s.is_empty()) {
        let (mut plan, normalized) = font_plan(text, ctx)?;
        if normalized.contains('\n') {
            plan.add_text(" ")?;
        }
        let mut size = item.font_size.min(height.max(4.0)).max(4.0);
        if item.annotation_type == AnnotationType::Stamp {
            while size > 4.0
                && plan.measure_line(&normalized.replace('\n', " "), size)? > (width - 4.0).max(1.0)
            {
                size -= 0.5;
            }
        }
        let lines = split_lines(
            &plan,
            &normalized,
            size,
            (width - 4.0).max(1.0),
            item.annotation_type == AnnotationType::FreeText,
            ctx,
        )?;
        let font = plan.embed(doc)?;
        resources.set("Font", dictionary! { "F0"=>Object::Reference(font.id) });
        ops.push(Operation::new(
            "re",
            vec![
                num(1.0)?,
                num(1.0)?,
                num((width - 2.0).max(0.0))?,
                num((height - 2.0).max(0.0))?,
            ],
        ));
        ops.push(Operation::new("W", vec![]));
        ops.push(Operation::new("n", vec![]));
        ops.push(Operation::new("BT", vec![]));
        ops.push(Operation::new(
            "Tf",
            vec![Object::Name(b"F0".to_vec()), num(size)?],
        ));
        for (index, line) in lines.iter().enumerate() {
            ctx.check()?;
            let baseline = height - size - 2.0 - index as f64 * size * 1.2;
            if baseline < 0.0 {
                break;
            }
            ops.push(Operation::new(
                "Tm",
                vec![
                    1.into(),
                    0.into(),
                    0.into(),
                    1.into(),
                    num(2.0)?,
                    num(baseline)?,
                ],
            ));
            ops.push(Operation::new(
                "Tj",
                vec![Object::String(
                    font.encode(line)?,
                    StringFormat::Hexadecimal,
                )],
            ));
        }
        ops.push(Operation::new("ET", vec![]));
    }
    ops.push(Operation::new("Q", vec![]));
    let data = Content { operations: ops }
        .encode()
        .map_err(|_| PdfError::Malformed)?;
    if data.len() > MAX_APPEARANCE_BYTES {
        return Err(PdfError::InvalidParams(
            "annotation appearance too large".into(),
        ));
    }
    let dict = dictionary! { "Type"=>"XObject", "Subtype"=>"Form", "BBox"=>vec![0.into(),0.into(),num(width)?,num(height)?], "Resources"=>resources };
    Ok(Some(doc.add_object(Stream::new(dict, data))))
}
