//! Form-widget appearance streams using the bundled Unicode font subsetter.

use ariad_core::tools::params::{FontFace, form_model::MAX_FILL_VALUE_BYTES};
use lopdf::{
    Document, Object, ObjectId, Stream, StringFormat,
    content::{Content, Operation},
    dictionary,
};
use unicode_normalization::UnicodeNormalization;
use unicode_segmentation::UnicodeSegmentation;

use crate::{
    PdfError, ToolCtx,
    draw::font::{FontPlan, MAX_TEXT_CHARS},
};

const MAX_APPEARANCE_BYTES: usize = 1024 * 1024;

fn real(n: f64) -> Result<Object, PdfError> {
    if !n.is_finite() || n.abs() > 1_000_000.0 {
        return Err(PdfError::InvalidParams(
            "invalid appearance geometry".into(),
        ));
    }
    Ok(Object::Real(n as f32))
}
fn stream(
    doc: &mut Document,
    width: f64,
    height: f64,
    resources: lopdf::Dictionary,
    operations: Vec<Operation>,
) -> Result<ObjectId, PdfError> {
    let content = Content { operations }
        .encode()
        .map_err(|_| PdfError::Malformed)?;
    if content.len() > MAX_APPEARANCE_BYTES {
        return Err(PdfError::InvalidParams("appearance is too large".into()));
    }
    let dict = dictionary! { "Type" => "XObject", "Subtype" => "Form", "BBox" => vec![0.into(), 0.into(), real(width)?, real(height)?], "Resources" => resources };
    Ok(doc.add_object(Stream::new(dict, content)))
}
/// Appearance colors and border obtained from a widget and its inherited default appearance.
#[derive(Clone, Debug)]
pub struct WidgetPaint {
    pub background: Option<[f64; 3]>,
    pub border: Option<[f64; 3]>,
    pub border_width: f64,
    pub border_style: u8,
    pub dash: Vec<f64>,
    pub text: [f64; 3],
    pub rotation: u16,
    pub standard_font: Option<Object>,
}
impl Default for WidgetPaint {
    fn default() -> Self {
        Self {
            background: Some([1.0, 1.0, 1.0]),
            border: Some([0.0, 0.0, 0.0]),
            border_width: 0.75,
            border_style: b'S',
            dash: vec![3.0],
            text: [0.0, 0.0, 0.0],
            rotation: 0,
            standard_font: None,
        }
    }
}
fn colour_components(values: &[f64]) -> Option<[f64; 3]> {
    if values
        .iter()
        .any(|n| !n.is_finite() || !(0.0..=1.0).contains(n))
    {
        return None;
    }
    match values {
        [g] => Some([*g, *g, *g]),
        [r, g, b] => Some([*r, *g, *b]),
        [c, m, y, k] => Some([
            (1.0 - c) * (1.0 - k),
            (1.0 - m) * (1.0 - k),
            (1.0 - y) * (1.0 - k),
        ]),
        _ => None,
    }
}
fn color_object(doc: &Document, value: &Object, depth: u16) -> Option<[f64; 3]> {
    let values = crate::forms::resolve(doc, value, depth)
        .ok()?
        .as_array()
        .ok()?;
    if values.len() > 4 {
        return None;
    }
    let values: Option<Vec<f64>> = values
        .iter()
        .map(|v| match crate::forms::resolve(doc, v, depth).ok()? {
            Object::Integer(n) => Some(*n as f64),
            Object::Real(n) => Some(f64::from(*n)),
            _ => None,
        })
        .collect();
    colour_components(&values?)
}
fn default_text_color(da: &str) -> Option<[f64; 3]> {
    let mut values = Vec::new();
    let mut color = None;
    for word in da.split_whitespace().take(256) {
        if let Ok(n) = word.parse::<f64>() {
            values.push(n);
            if values.len() > 4 {
                values.remove(0);
            }
            continue;
        }
        let count = match word {
            "g" => 1,
            "rg" => 3,
            "k" => 4,
            _ => 0,
        };
        if count > 0 && values.len() >= count {
            color = colour_components(&values[values.len() - count..]).or(color);
        }
        values.clear();
    }
    color
}
/// Return whether the default appearance names a resolvable font resource.
pub(crate) fn font_exists(
    doc: &Document,
    da: &str,
    resources: Option<&Object>,
    depth: u16,
) -> bool {
    let tokens: Vec<_> = da.split_whitespace().take(256).collect();
    let name = tokens.windows(3).rev().find_map(|parts| {
        (parts[2] == "Tf")
            .then(|| parts[0].strip_prefix('/'))
            .flatten()
    });
    name.zip(resources)
        .and_then(|(name, resources)| {
            crate::forms::resolve(doc, resources, depth)
                .ok()?
                .as_dict()
                .ok()?
                .get(b"Font")
                .ok()
                .and_then(|o| crate::forms::resolve(doc, o, depth).ok())?
                .as_dict()
                .ok()?
                .get(name.as_bytes())
                .ok()
                .and_then(|o| crate::forms::resolve(doc, o, depth).ok())?
                .as_dict()
                .ok()?
                .get(b"Subtype")
                .ok()
                .and_then(|o| crate::forms::resolve(doc, o, depth).ok())?
                .as_name()
                .ok()
        })
        .is_some()
}

/// Resolve a safe Standard-14 font from inherited form resources.
pub(crate) fn standard_font(
    doc: &Document,
    da: &str,
    resources: Option<&Object>,
    depth: u16,
) -> Option<Object> {
    let tokens: Vec<_> = da.split_whitespace().take(256).collect();
    let font_name = tokens.windows(3).rev().find_map(|parts| {
        (parts[2] == "Tf")
            .then(|| parts[0].strip_prefix('/'))
            .flatten()
    });
    if let (Some(name), Some(resources)) = (font_name, resources) {
        let font = crate::forms::resolve(doc, resources, depth)
            .ok()
            .and_then(|o| o.as_dict().ok())
            .and_then(|d| d.get(b"Font").ok())
            .and_then(|o| crate::forms::resolve(doc, o, depth).ok())
            .and_then(|o| o.as_dict().ok())
            .and_then(|d| d.get(name.as_bytes()).ok());
        let usable = font
            .and_then(|o| crate::forms::resolve(doc, o, depth).ok())
            .and_then(|o| o.as_dict().ok())
            .is_some_and(|font| {
                let subtype = font.get(b"Subtype").ok().and_then(|o| o.as_name().ok());
                let base = font.get(b"BaseFont").ok().and_then(|o| o.as_name().ok());
                let encoding_ok = match font.get(b"Encoding") {
                    Err(_) => true,
                    Ok(value) => value.as_name().ok() == Some(b"WinAnsiEncoding"),
                };
                subtype == Some(b"Type1")
                    && matches!(
                        base,
                        Some(
                            b"Helvetica"
                                | b"Helvetica-Bold"
                                | b"Helvetica-Oblique"
                                | b"Helvetica-BoldOblique"
                                | b"Times-Roman"
                                | b"Times-Bold"
                                | b"Times-Italic"
                                | b"Times-BoldItalic"
                                | b"Courier"
                                | b"Courier-Bold"
                                | b"Courier-Oblique"
                                | b"Courier-BoldOblique"
                        )
                    )
                    && encoding_ok
            });
        if usable {
            return font.cloned();
        }
    }
    None
}
/// Read supported static widget styling without executing actions.
pub fn widget_paint(
    doc: &Document,
    id: ObjectId,
    da: &str,
    font: Option<&Object>,
    depth: u16,
) -> WidgetPaint {
    let mut paint = WidgetPaint {
        background: None,
        border: None,
        ..WidgetPaint::default()
    };
    if let Some(color) = default_text_color(da) {
        paint.text = color;
    }
    paint.standard_font = font.cloned();
    let Ok(widget) = doc.get_dictionary(id) else {
        return paint;
    };
    if let Some(mk) = widget
        .get(b"MK")
        .ok()
        .and_then(|o| crate::forms::resolve(doc, o, depth).ok())
        .and_then(|o| o.as_dict().ok())
    {
        if let Ok(value) = mk.get(b"BG") {
            paint.background = color_object(doc, value, depth);
        }
        if let Ok(value) = mk.get(b"BC") {
            paint.border = color_object(doc, value, depth);
        }
        paint.rotation = mk
            .get(b"R")
            .ok()
            .and_then(|v| v.as_i64().ok())
            .and_then(|v| u16::try_from(v.rem_euclid(360)).ok())
            .filter(|v| matches!(v, 0 | 90 | 180 | 270))
            .unwrap_or(0);
    }
    if let Some(bs) = widget
        .get(b"BS")
        .ok()
        .and_then(|o| crate::forms::resolve(doc, o, depth).ok())
        .and_then(|o| o.as_dict().ok())
    {
        paint.border_style = bs
            .get(b"S")
            .ok()
            .and_then(|v| crate::forms::resolve(doc, v, depth).ok())
            .and_then(|v| v.as_name().ok())
            .filter(|s| s.len() == 1)
            .and_then(|s| s.first().copied())
            .filter(|s| matches!(s, b'S' | b'D' | b'B' | b'I' | b'U'))
            .unwrap_or(b'S');
        if let Some(array) = bs
            .get(b"D")
            .ok()
            .and_then(|v| crate::forms::resolve(doc, v, depth).ok())
            .and_then(|v| v.as_array().ok())
            .filter(|v| !v.is_empty() && v.len() <= 8)
        {
            let dash: Option<Vec<f64>> = array
                .iter()
                .map(|v| match crate::forms::resolve(doc, v, depth).ok()? {
                    Object::Integer(n) => Some(*n as f64),
                    Object::Real(n) => Some(f64::from(*n)),
                    _ => None,
                })
                .collect();
            if let Some(dash) = dash.filter(|d| {
                d.iter()
                    .all(|n| n.is_finite() && (0.0..=1000.0).contains(n))
                    && d.iter().any(|n| *n > 0.0)
            }) {
                paint.dash = dash;
            }
        }
        let width = bs.get(b"W").ok().and_then(|v| match v {
            Object::Integer(n) => Some(*n as f64),
            Object::Real(n) => Some(f64::from(*n)),
            _ => None,
        });
        if let Some(width) = width.filter(|n| n.is_finite() && (0.0..=1000.0).contains(n)) {
            paint.border_width = width;
        }
    }
    paint
}
fn background_with_paint(
    width: f64,
    height: f64,
    paint: &WidgetPaint,
) -> Result<Vec<Operation>, PdfError> {
    if width <= 0.0 || height <= 0.0 {
        return Err(PdfError::InvalidParams("empty widget rectangle".into()));
    }
    let mut ops = vec![Operation::new("q", vec![])];
    if let Some(color) = paint.background {
        ops.push(Operation::new(
            "rg",
            color.into_iter().map(real).collect::<Result<Vec<_>, _>>()?,
        ));
        ops.push(Operation::new(
            "re",
            vec![0.into(), 0.into(), real(width)?, real(height)?],
        ));
        ops.push(Operation::new("f", vec![]));
    }
    if let Some(color) = paint.border.filter(|_| paint.border_width > 0.0) {
        ops.push(Operation::new(
            "RG",
            color.into_iter().map(real).collect::<Result<Vec<_>, _>>()?,
        ));
        ops.push(Operation::new("w", vec![real(paint.border_width)?]));
        let inset = paint.border_width / 2.0;
        if paint.border_style == b'D' {
            ops.push(Operation::new(
                "d",
                vec![
                    Object::Array(
                        paint
                            .dash
                            .iter()
                            .copied()
                            .map(real)
                            .collect::<Result<Vec<_>, _>>()?,
                    ),
                    0.into(),
                ],
            ));
        }
        if paint.border_style == b'U' {
            ops.push(Operation::new("m", vec![real(inset)?, real(inset)?]));
            ops.push(Operation::new(
                "l",
                vec![real((width - inset).max(inset))?, real(inset)?],
            ));
        } else {
            ops.push(Operation::new(
                "re",
                vec![
                    real(inset)?,
                    real(inset)?,
                    real((width - paint.border_width).max(0.0))?,
                    real((height - paint.border_width).max(0.0))?,
                ],
            ));
        }
        ops.push(Operation::new("S", vec![]));
        if matches!(paint.border_style, b'B' | b'I') {
            let light = color.map(|c| (c + 1.0) / 2.0);
            let dark = color.map(|c| c / 2.0);
            let (top, bottom) = if paint.border_style == b'B' {
                (light, dark)
            } else {
                (dark, light)
            };
            let right = (width - inset).max(inset);
            let upper = (height - inset).max(inset);
            for (shade, points) in [
                (top, [(inset, inset), (inset, upper), (right, upper)]),
                (bottom, [(inset, inset), (right, inset), (right, upper)]),
            ] {
                ops.push(Operation::new(
                    "RG",
                    shade.into_iter().map(real).collect::<Result<Vec<_>, _>>()?,
                ));
                ops.push(Operation::new(
                    "m",
                    vec![real(points[0].0)?, real(points[0].1)?],
                ));
                for point in &points[1..] {
                    ops.push(Operation::new("l", vec![real(point.0)?, real(point.1)?]));
                }
                ops.push(Operation::new("S", vec![]));
            }
        }
    }
    Ok(ops)
}
fn background(width: f64, height: f64) -> Result<Vec<Operation>, PdfError> {
    background_with_paint(width, height, &WidgetPaint::default())
}

fn radio_background(width: f64, height: f64) -> Result<Vec<Operation>, PdfError> {
    if width <= 0.0 || height <= 0.0 {
        return Err(PdfError::InvalidParams("empty widget rectangle".into()));
    }
    let cx = width / 2.0;
    let cy = height / 2.0;
    let r = ((width.min(height) - 0.75) / 2.0).max(0.0);
    let k = r * 0.552_284_749_8;
    Ok(vec![
        Operation::new("q", vec![]),
        Operation::new("g", vec![1.into()]),
        Operation::new("G", vec![0.into()]),
        Operation::new("w", vec![Object::Real(0.75)]),
        Operation::new("m", vec![real(cx + r)?, real(cy)?]),
        Operation::new(
            "c",
            vec![
                real(cx + r)?,
                real(cy + k)?,
                real(cx + k)?,
                real(cy + r)?,
                real(cx)?,
                real(cy + r)?,
            ],
        ),
        Operation::new(
            "c",
            vec![
                real(cx - k)?,
                real(cy + r)?,
                real(cx - r)?,
                real(cy + k)?,
                real(cx - r)?,
                real(cy)?,
            ],
        ),
        Operation::new(
            "c",
            vec![
                real(cx - r)?,
                real(cy - k)?,
                real(cx - k)?,
                real(cy - r)?,
                real(cx)?,
                real(cy - r)?,
            ],
        ),
        Operation::new(
            "c",
            vec![
                real(cx + k)?,
                real(cy - r)?,
                real(cx + r)?,
                real(cy - k)?,
                real(cx + r)?,
                real(cy)?,
            ],
        ),
        Operation::new("B", vec![]),
    ])
}

/// Layout choices needed to regenerate a variable-text widget.
pub struct TextAppearance {
    pub width: f64,
    pub height: f64,
    pub font_size: f64,
    pub multiline: bool,
    pub wrap_lines: bool,
    pub alignment: u8,
    pub comb_cells: Option<usize>,
    pub selected_rows: Vec<usize>,
    pub paint: WidgetPaint,
}

pub(crate) fn font_plan(text: &str, ctx: &ToolCtx) -> Result<(FontPlan, String), PdfError> {
    if text.len() > MAX_FILL_VALUE_BYTES {
        return Err(PdfError::InvalidParams("form value is too large".into()));
    }
    let canonical_newlines = text
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\t', "    ");
    let text: String = canonical_newlines.nfc().collect();
    let mut plan = FontPlan::new(FontFace::Serif);
    let mut chunk = String::new();
    let mut count = 0usize;
    for (index, grapheme) in text.graphemes(true).enumerate() {
        if index % 256 == 0 {
            ctx.check()?;
        }
        let n = grapheme.chars().count();
        if n > MAX_TEXT_CHARS {
            return Err(PdfError::InvalidParams("text cluster is too large".into()));
        }
        if count + n > MAX_TEXT_CHARS {
            plan.add_text(&chunk)?;
            chunk.clear();
            count = 0;
        }
        chunk.push_str(grapheme);
        count += n;
    }
    if !chunk.is_empty() {
        plan.add_text(&chunk)?;
    }
    Ok((plan, text))
}
pub(crate) fn split_lines(
    plan: &FontPlan,
    text: &str,
    size: f64,
    available: f64,
    multiline: bool,
    ctx: &ToolCtx,
) -> Result<Vec<String>, PdfError> {
    let mut lines = Vec::new();
    let mut line = String::new();
    let mut width = 0.0;
    let mut checked = 0usize;
    for (paragraph_index, paragraph) in text.split('\n').enumerate() {
        if paragraph_index > 0 {
            if multiline {
                lines.push(std::mem::take(&mut line));
                width = 0.0;
            } else {
                line.push(' ');
                width += plan.measure_line(" ", size)?;
            }
        }
        for word in paragraph.split_word_bounds() {
            checked = checked.saturating_add(1);
            if checked % 256 == 0 {
                ctx.check()?;
            }
            let word_width = plan.measure_line(word, size)?;
            if multiline && !line.is_empty() && width + word_width > available {
                lines.push(line.trim_end().to_owned());
                line.clear();
                width = 0.0;
                if word.chars().all(char::is_whitespace) {
                    continue;
                }
            }
            if multiline && word_width > available {
                for grapheme in word.graphemes(true) {
                    checked = checked.saturating_add(1);
                    if checked % 256 == 0 {
                        ctx.check()?;
                    }
                    let advance = plan.measure_line(grapheme, size)?;
                    if !line.is_empty() && width + advance > available {
                        lines.push(std::mem::take(&mut line));
                        width = 0.0;
                    }
                    line.push_str(grapheme);
                    width += advance;
                }
            } else {
                line.push_str(word);
                width += word_width;
            }
        }
    }
    lines.push(line);
    Ok(lines)
}

/// Build a visible normal appearance using an embedded, subset Unicode font.
pub fn text(
    doc: &mut Document,
    value: &str,
    style: &TextAppearance,
    ctx: &ToolCtx,
) -> Result<ObjectId, PdfError> {
    ctx.check()?;
    let mut ops = background_with_paint(style.width, style.height, &style.paint)?;
    if !style.font_size.is_finite()
        || style.font_size < 0.0
        || style.alignment > 2
        || style.comb_cells == Some(0)
    {
        return Err(PdfError::InvalidParams(
            "invalid text appearance style".into(),
        ));
    }
    if value.is_empty() {
        ops.push(Operation::new("Q", vec![]));
        return stream(doc, style.width, style.height, dictionary! {}, ops);
    }
    ops.extend([
        Operation::new(
            "re",
            vec![
                1.into(),
                1.into(),
                real((style.width - 2.0).max(0.0))?,
                real((style.height - 2.0).max(0.0))?,
            ],
        ),
        Operation::new("W", vec![]),
        Operation::new("n", vec![]),
    ]);
    let (canvas_width, canvas_height) = match style.paint.rotation {
        90 => {
            ops.push(Operation::new(
                "cm",
                vec![
                    0.into(),
                    1.into(),
                    (-1).into(),
                    0.into(),
                    real(style.width)?,
                    0.into(),
                ],
            ));
            (style.height, style.width)
        }
        180 => {
            ops.push(Operation::new(
                "cm",
                vec![
                    (-1).into(),
                    0.into(),
                    0.into(),
                    (-1).into(),
                    real(style.width)?,
                    real(style.height)?,
                ],
            ));
            (style.width, style.height)
        }
        270 => {
            ops.push(Operation::new(
                "cm",
                vec![
                    0.into(),
                    (-1).into(),
                    1.into(),
                    0.into(),
                    0.into(),
                    real(style.height)?,
                ],
            ));
            (style.height, style.width)
        }
        _ => (style.width, style.height),
    };
    let (mut plan, mut value) = font_plan(value, ctx)?;
    // Newlines in a single-line field become spaces; include the replacement glyph.
    if !style.multiline && value.contains('\n') {
        plan.add_text(" ")?;
        value = value.replace('\n', " ");
    }
    let available = (canvas_width - 4.0).max(1.0);
    let mut size = if style.font_size == 0.0 {
        12.0_f64
    } else {
        style.font_size
    };
    if size > 1000.0 {
        return Err(PdfError::InvalidParams("font size is too large".into()));
    }
    let mut lines = if style.multiline && !style.wrap_lines {
        value.split('\n').map(str::to_owned).collect()
    } else {
        split_lines(&plan, &value, size, available, style.multiline, ctx)?
    };
    if style.font_size == 0.0 && style.wrap_lines {
        while size > 4.0 {
            let widest = lines
                .iter()
                .map(|line| plan.measure_line(line, size))
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .fold(0.0_f64, f64::max);
            if widest <= available
                && lines.len() as f64 * size * 1.2 <= (canvas_height - 4.0).max(1.0)
            {
                break;
            }
            size -= 0.5;
            lines = split_lines(&plan, &value, size, available, style.multiline, ctx)?;
        }
    }
    let standard = value
        .is_ascii()
        .then(|| style.paint.standard_font.clone())
        .flatten();
    let embedded = if standard.is_none() {
        Some(plan.embed(doc)?)
    } else {
        None
    };
    let font = match (&standard, &embedded) {
        (Some(font), _) => font.clone(),
        (None, Some(font)) => Object::Reference(font.id),
        (None, None) => return Err(PdfError::Malformed),
    };
    let baseline = if style.multiline {
        canvas_height - size - 2.0
    } else {
        (canvas_height - size) / 2.0
    };
    for row in &style.selected_rows {
        if *row >= lines.len() {
            continue;
        }
        let y = baseline - *row as f64 * size * 1.2;
        if y + size < 1.0 || y - size * 0.2 > canvas_height - 1.0 {
            continue;
        }
        ops.extend([
            Operation::new(
                "rg",
                vec![Object::Real(0.75), Object::Real(0.84), Object::Real(0.96)],
            ),
            Operation::new(
                "re",
                vec![
                    1.into(),
                    real((y - size * 0.2).max(1.0))?,
                    real((canvas_width - 2.0).max(0.0))?,
                    real(size * 1.2)?,
                ],
            ),
            Operation::new("f", vec![]),
        ]);
    }
    let resources = dictionary! { "Font" => dictionary! { "F0" => font } };
    ops.extend([
        Operation::new("BT", vec![]),
        Operation::new(
            "rg",
            style
                .paint
                .text
                .into_iter()
                .map(real)
                .collect::<Result<Vec<_>, _>>()?,
        ),
        Operation::new("Tf", vec![Object::Name(b"F0".to_vec()), real(size)?]),
    ]);
    if let Some(cells) = style.comb_cells {
        if cells > MAX_FILL_VALUE_BYTES {
            return Err(PdfError::InvalidParams("too many comb cells".into()));
        }
        let cell_width = canvas_width / cells as f64;
        for (index, cluster) in value.graphemes(true).take(cells).enumerate() {
            if index % 256 == 0 {
                ctx.check()?;
            }
            let width = plan.measure_line(cluster, size)?;
            let x = index as f64 * cell_width + (cell_width - width) / 2.0;
            let y = ((canvas_height - size) / 2.0).max(1.0);
            ops.push(Operation::new(
                "Tm",
                vec![1.into(), 0.into(), 0.into(), 1.into(), real(x)?, real(y)?],
            ));
            ops.push(Operation::new(
                "Tj",
                vec![Object::String(
                    if let Some(embedded) = &embedded {
                        embedded.encode(cluster)?
                    } else {
                        cluster.as_bytes().to_vec()
                    },
                    StringFormat::Hexadecimal,
                )],
            ));
        }
    } else {
        for (index, line) in lines.iter().enumerate() {
            if index % 256 == 0 {
                ctx.check()?;
            }
            let width = plan.measure_line(line, size)?;
            let x = match style.alignment {
                1 => (canvas_width - width) / 2.0,
                2 => canvas_width - width - 2.0,
                _ => 2.0,
            };
            let y = baseline - index as f64 * size * 1.2;
            ops.push(Operation::new(
                "Tm",
                vec![1.into(), 0.into(), 0.into(), 1.into(), real(x)?, real(y)?],
            ));
            ops.push(Operation::new(
                "Tj",
                vec![Object::String(
                    if let Some(embedded) = &embedded {
                        embedded.encode(line)?
                    } else {
                        line.as_bytes().to_vec()
                    },
                    StringFormat::Hexadecimal,
                )],
            ));
        }
    }
    ops.push(Operation::new("ET", vec![]));
    ops.push(Operation::new("Q", vec![]));
    stream(doc, style.width, style.height, resources, ops)
}

/// Build a checkbox or radio on/off appearance.
pub fn button(
    doc: &mut Document,
    width: f64,
    height: f64,
    checked: bool,
    radio: bool,
    ctx: &ToolCtx,
) -> Result<ObjectId, PdfError> {
    ctx.check()?;
    let mut resources = dictionary! {};
    let mut ops = if radio {
        radio_background(width, height)?
    } else {
        background(width, height)?
    };
    if checked && radio {
        let cx = width / 2.0;
        let cy = height / 2.0;
        let r = width.min(height) * 0.23;
        let k = r * 0.552_284_749_8;
        ops.extend([
            Operation::new("g", vec![0.into()]),
            Operation::new("m", vec![real(cx + r)?, real(cy)?]),
            Operation::new(
                "c",
                vec![
                    real(cx + r)?,
                    real(cy + k)?,
                    real(cx + k)?,
                    real(cy + r)?,
                    real(cx)?,
                    real(cy + r)?,
                ],
            ),
            Operation::new(
                "c",
                vec![
                    real(cx - k)?,
                    real(cy + r)?,
                    real(cx - r)?,
                    real(cy + k)?,
                    real(cx - r)?,
                    real(cy)?,
                ],
            ),
            Operation::new(
                "c",
                vec![
                    real(cx - r)?,
                    real(cy - k)?,
                    real(cx - k)?,
                    real(cy - r)?,
                    real(cx)?,
                    real(cy - r)?,
                ],
            ),
            Operation::new(
                "c",
                vec![
                    real(cx + k)?,
                    real(cy - r)?,
                    real(cx + r)?,
                    real(cy - k)?,
                    real(cx + r)?,
                    real(cy)?,
                ],
            ),
            Operation::new("f", vec![]),
        ]);
    } else if checked {
        let font = doc.add_object(
            dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "ZapfDingbats" },
        );
        resources.set("Font", dictionary! { "ZaDb" => Object::Reference(font) });
        // Font-specific code 51 is the a19 check mark in ZapfDingbats.
        let size = width.min(height) * 0.8;
        ops.extend([
            Operation::new("g", vec![0.into()]),
            Operation::new("BT", vec![]),
            Operation::new("Tf", vec![Object::Name(b"ZaDb".to_vec()), real(size)?]),
            Operation::new(
                "Tm",
                vec![
                    1.into(),
                    0.into(),
                    0.into(),
                    1.into(),
                    real(width * 0.2)?,
                    real(height * 0.2)?,
                ],
            ),
            Operation::new("Tj", vec![Object::String(vec![51], StringFormat::Literal)]),
            Operation::new("ET", vec![]),
        ]);
    }
    ops.push(Operation::new("Q", vec![]));
    stream(doc, width, height, resources, ops)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checkbox_appearance_uses_the_dingbats_check_glyph() {
        let mut doc = Document::with_version("1.7");
        let id = button(&mut doc, 20.0, 20.0, true, false, &ToolCtx::new()).unwrap();
        let stream = doc.get_object(id).unwrap().as_stream().unwrap();
        let resources = stream.dict.get(b"Resources").unwrap().as_dict().unwrap();
        let font_id = resources
            .get(b"Font")
            .unwrap()
            .as_dict()
            .unwrap()
            .get(b"ZaDb")
            .unwrap()
            .as_reference()
            .unwrap();
        assert_eq!(
            doc.get_dictionary(font_id)
                .unwrap()
                .get(b"BaseFont")
                .unwrap()
                .as_name()
                .unwrap(),
            b"ZapfDingbats"
        );
        let content = Content::decode(&stream.content).unwrap();
        assert!(content.operations.iter().any(|op| op.operator == "Tj"
            && op.operands.first().and_then(|o| o.as_str().ok()) == Some(&[51][..])));
    }

    #[test]
    fn widget_border_styles_are_preserved_in_appearance() {
        let mut doc = Document::with_version("1.7");
        let id = doc.add_object(dictionary! {
            "MK" => dictionary! { "BC" => vec![0.into(), 0.into(), 0.into()] },
            "BS" => dictionary! { "S" => "D", "W" => 2, "D" => vec![4.into(), 2.into()] },
        });
        let mut paint = widget_paint(&doc, id, "", None, 64);
        assert_eq!(paint.dash, vec![4.0, 2.0]);
        let ops = background_with_paint(100.0, 20.0, &paint).unwrap();
        assert!(ops.iter().any(|op| op.operator == "d"));
        paint.border_style = b'U';
        let ops = background_with_paint(100.0, 20.0, &paint).unwrap();
        assert!(ops.iter().any(|op| op.operator == "l"));
        assert!(!ops.iter().any(|op| op.operator == "re"));
        paint.border_style = b'B';
        let ops = background_with_paint(100.0, 20.0, &paint).unwrap();
        assert_eq!(ops.iter().filter(|op| op.operator == "S").count(), 3);
    }
}
