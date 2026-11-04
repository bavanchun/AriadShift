//! Source glyph positions and operator indices for a flat page-content stream.

use ariad_core::{limits::Limits, reader::pdf::PdfTextDecoder};
use lopdf::{Dictionary, Document, Object};

use crate::{PdfError, ToolCtx, xobject::Matrix};

use super::{number, resolved};

const MAX_GLYPHS: usize = 1_000_000;
const MAX_TEXT_BYTES: usize = 16 * 1024 * 1024;

/// One source glyph and its position in the original decoded content stream.
#[derive(Clone, PartialEq)]
pub struct TextGlyphTrace {
    /// Zero-based content operation index.
    pub operation: usize,
    /// Zero-based encoded string operand within the operation.
    pub string: usize,
    /// Zero-based glyph within the encoded string.
    pub glyph: usize,
    /// Inclusive start byte in that encoded string.
    pub start: usize,
    /// Exclusive end byte in that encoded string.
    pub end: usize,
    /// Unicode mapped by the font. This is private job evidence.
    pub text: String,
    /// Glyph baseline origin in unrotated PDF page points.
    pub origin: [f64; 2],
    /// PDF text rendering mode.
    pub render_mode: u8,
}

impl std::fmt::Debug for TextGlyphTrace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextGlyphTrace")
            .field("operation", &self.operation)
            .field("string", &self.string)
            .field("glyph", &self.glyph)
            .field("start", &self.start)
            .field("end", &self.end)
            .field("text", &"[redacted]")
            .field("origin", &self.origin)
            .field("render_mode", &self.render_mode)
            .finish()
    }
}

/// Flat-stream trace. Form and image objects must be analyzed separately.
#[derive(Clone, PartialEq)]
pub struct TextStreamTrace {
    pub(super) content_sha256: [u8; 32],
    /// Text glyphs in content execution order.
    pub(super) glyphs: Vec<TextGlyphTrace>,
    /// XObjects, vector paints, clipping or marked content not traversed by this trace.
    pub(super) uninspected_objects: usize,
    pub(super) uninspected_carrier: Option<String>,
    pub(super) painted_paths: Vec<[f64; 4]>,
}

impl TextStreamTrace {
    /// Source glyphs in content execution order.
    #[must_use]
    pub fn glyphs(&self) -> &[TextGlyphTrace] {
        &self.glyphs
    }
    /// Number of page-content operations that still require separate inspection.
    #[must_use]
    pub const fn uninspected_objects(&self) -> usize {
        self.uninspected_objects
    }
    /// Name of the uninspected carrier that caused correlation failure, if any.
    #[must_use]
    pub fn uninspected_carrier(&self) -> Option<&str> {
        self.uninspected_carrier.as_deref()
    }
    /// Bounding boxes in page points of painted vector paths.
    #[must_use]
    pub fn painted_paths(&self) -> &[[f64; 4]] {
        &self.painted_paths
    }
}

impl std::fmt::Debug for TextStreamTrace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextStreamTrace")
            .field("glyphs", &self.glyphs.len())
            .field("uninspected_objects", &self.uninspected_objects)
            .finish()
    }
}

#[derive(Clone)]
struct Graphics {
    ctm: Matrix,
    font: Option<Object>,
    size: f64,
    character_spacing: f64,
    word_spacing: f64,
    horizontal_scale: f64,
    leading: f64,
    rise: f64,
    render_mode: u8,
}

impl Default for Graphics {
    fn default() -> Self {
        Self {
            ctm: Matrix::identity(),
            font: None,
            size: 0.0,
            character_spacing: 0.0,
            word_spacing: 0.0,
            horizontal_scale: 1.0,
            leading: 0.0,
            rise: 0.0,
            render_mode: 0,
        }
    }
}

/// Traces text in a flat content stream using declared font metrics and PDF graphics state.
/// This evidence is not a verification result; the host must match it against PDFium and
/// separately inspect nested forms, vector content, images, masks and other graph carriers.
pub fn trace(
    doc: &Document,
    resources: &Dictionary,
    content: &[u8],
    input_len: usize,
    limits: &Limits,
    ctx: &ToolCtx,
) -> Result<TextStreamTrace, PdfError> {
    ctx.check()?;
    let parsed = crate::scan_text::decode_checked(content, ctx)?;
    let mut walker = Walker {
        doc,
        resources,
        decoder: PdfTextDecoder::new(doc, input_len, limits),
        ctx,
        graphics: Graphics::default(),
        saved: Vec::new(),
        text_matrix: Matrix::identity(),
        line_matrix: Matrix::identity(),
        in_text: false,
        total_text: 0,
        current_path: Vec::new(),
        trace: TextStreamTrace {
            content_sha256: crate::writer::sha256_chunks(&[content]),
            glyphs: Vec::new(),
            uninspected_objects: 0,
            uninspected_carrier: None,
            painted_paths: Vec::new(),
        },
    };
    for (index, operation) in parsed.operations.iter().enumerate() {
        ctx.check()?;
        let operands = &operation.operands;
        match operation.operator.as_str() {
            "q" if operands.is_empty() => {
                if walker.saved.len() >= 256 {
                    return Err(PdfError::Malformed);
                }
                walker.saved.push(walker.graphics.clone());
            }
            "Q" if operands.is_empty() => {
                walker.graphics = walker.saved.pop().ok_or(PdfError::Malformed)?
            }
            "cm" if operands.len() == 6 => {
                let matrix = matrix(operands)?;
                walker.graphics.ctm = matrix.then(walker.graphics.ctm);
                walker.validate_matrix(walker.graphics.ctm)?;
            }
            "BT" if !walker.in_text && operands.is_empty() => {
                walker.in_text = true;
                walker.text_matrix = Matrix::identity();
                walker.line_matrix = Matrix::identity();
            }
            "ET" if walker.in_text && operands.is_empty() => walker.in_text = false,
            "Tf" if walker.in_text && operands.len() == 2 => {
                let name = operands[0].as_name().map_err(|_| PdfError::Malformed)?;
                let fonts = resolved(
                    doc,
                    resources.get(b"Font").map_err(|_| PdfError::Malformed)?,
                )?
                .as_dict()
                .map_err(|_| PdfError::Malformed)?;
                walker.graphics.font =
                    Some(fonts.get(name).map_err(|_| PdfError::Malformed)?.clone());
                walker.graphics.size = number(&operands[1])?;
            }
            "Tc" if walker.in_text && operands.len() == 1 => {
                walker.graphics.character_spacing = number(&operands[0])?
            }
            "Tw" if walker.in_text && operands.len() == 1 => {
                walker.graphics.word_spacing = number(&operands[0])?
            }
            "Tz" if walker.in_text && operands.len() == 1 => {
                walker.graphics.horizontal_scale = number(&operands[0])? / 100.0
            }
            "TL" if walker.in_text && operands.len() == 1 => {
                walker.graphics.leading = number(&operands[0])?
            }
            "Ts" if walker.in_text && operands.len() == 1 => {
                walker.graphics.rise = number(&operands[0])?
            }
            "Tr" if walker.in_text && operands.len() == 1 => {
                let value = operands[0].as_i64().map_err(|_| PdfError::Malformed)?;
                if !(0..=3).contains(&value) {
                    return Err(PdfError::NotImplemented("text clipping redaction".into()));
                }
                walker.graphics.render_mode = value as u8;
            }
            "Tm" if walker.in_text && operands.len() == 6 => {
                walker.text_matrix = matrix(operands)?;
                walker.line_matrix = walker.text_matrix;
            }
            "Td" | "TD" if walker.in_text && operands.len() == 2 => {
                let x = number(&operands[0])?;
                let y = number(&operands[1])?;
                if operation.operator == "TD" {
                    walker.graphics.leading = -y;
                }
                walker.move_line(x, y)?;
            }
            "T*" if walker.in_text && operands.is_empty() => walker.next_line()?,
            "Tj" if walker.in_text && operands.len() == 1 => walker.show(
                index,
                0,
                operands[0].as_str().map_err(|_| PdfError::Malformed)?,
            )?,
            "'" if walker.in_text && operands.len() == 1 => {
                walker.next_line()?;
                walker.show(
                    index,
                    0,
                    operands[0].as_str().map_err(|_| PdfError::Malformed)?,
                )?;
            }
            "\"" if walker.in_text && operands.len() == 3 => {
                walker.graphics.word_spacing = number(&operands[0])?;
                walker.graphics.character_spacing = number(&operands[1])?;
                walker.next_line()?;
                walker.show(
                    index,
                    0,
                    operands[2].as_str().map_err(|_| PdfError::Malformed)?,
                )?;
            }
            "TJ" if walker.in_text && operands.len() == 1 => {
                let mut string = 0;
                for item in operands[0].as_array().map_err(|_| PdfError::Malformed)? {
                    match item {
                        Object::String(bytes, _) => {
                            walker.show(index, string, bytes)?;
                            string += 1;
                        }
                        Object::Integer(_) | Object::Real(_) => {
                            let adjustment = number(item)?;
                            walker.advance(
                                -adjustment / 1000.0
                                    * walker.graphics.size
                                    * walker.graphics.horizontal_scale,
                            )?;
                        }
                        _ => return Err(PdfError::Malformed),
                    }
                }
            }
            "Do" if operands.len() == 1 => {
                let name = operands[0].as_name().map_err(|_| PdfError::Malformed)?;
                let is_image = walker
                    .resources
                    .get(b"XObject")
                    .ok()
                    .and_then(|v| resolved(walker.doc, v).ok())
                    .and_then(|v| v.as_dict().ok())
                    .and_then(|d| d.get(name).ok())
                    .and_then(|v| resolved(walker.doc, v).ok())
                    .and_then(|v| v.as_stream().ok())
                    .is_some_and(|s| {
                        s.dict
                            .get(b"Subtype")
                            .is_ok_and(|sub| matches!(sub.as_name(), Ok(b"Image")))
                    });
                if !is_image {
                    walker.trace.uninspected_objects = walker
                        .trace
                        .uninspected_objects
                        .checked_add(1)
                        .ok_or(PdfError::Malformed)?;
                    walker
                        .trace
                        .uninspected_carrier
                        .get_or_insert_with(|| "form_xobject".into());
                }
            }
            "BI" => {
                walker.trace.uninspected_objects = walker
                    .trace
                    .uninspected_objects
                    .checked_add(1)
                    .ok_or(PdfError::Malformed)?;
                walker
                    .trace
                    .uninspected_carrier
                    .get_or_insert_with(|| "inline_image".into());
            }
            "sh" => {
                walker.trace.uninspected_objects = walker
                    .trace
                    .uninspected_objects
                    .checked_add(1)
                    .ok_or(PdfError::Malformed)?;
                walker
                    .trace
                    .uninspected_carrier
                    .get_or_insert_with(|| "shading".into());
            }
            "gs" if operands.len() == 1 => {
                if let Ok(name) = operands[0].as_name() {
                    let has_font = walker
                        .resources
                        .get(b"ExtGState")
                        .ok()
                        .and_then(|v| resolved(walker.doc, v).ok())
                        .and_then(|v| v.as_dict().ok())
                        .and_then(|d| d.get(name).ok())
                        .and_then(|v| resolved(walker.doc, v).ok())
                        .and_then(|v| v.as_dict().ok())
                        .is_some_and(|d| d.has(b"Font"));
                    if has_font {
                        walker.trace.uninspected_objects = walker
                            .trace
                            .uninspected_objects
                            .checked_add(1)
                            .ok_or(PdfError::Malformed)?;
                        walker
                            .trace
                            .uninspected_carrier
                            .get_or_insert_with(|| "ext_gstate_font".into());
                    }
                }
            }
            "re" if operands.len() == 4 => {
                let x = number(&operands[0])?;
                let y = number(&operands[1])?;
                let w = number(&operands[2])?;
                let h = number(&operands[3])?;
                walker.add_point(x, y);
                walker.add_point(x + w, y);
                walker.add_point(x + w, y + h);
                walker.add_point(x, y + h);
            }
            "m" if operands.len() == 2 => {
                let x = number(&operands[0])?;
                let y = number(&operands[1])?;
                walker.add_point(x, y);
            }
            "l" if operands.len() == 2 => {
                let x = number(&operands[0])?;
                let y = number(&operands[1])?;
                walker.add_point(x, y);
            }
            "c" if operands.len() == 6 => {
                let x1 = number(&operands[0])?;
                let y1 = number(&operands[1])?;
                let x2 = number(&operands[2])?;
                let y2 = number(&operands[3])?;
                let x3 = number(&operands[4])?;
                let y3 = number(&operands[5])?;
                walker.add_point(x1, y1);
                walker.add_point(x2, y2);
                walker.add_point(x3, y3);
            }
            "v" if operands.len() == 4 => {
                let x2 = number(&operands[0])?;
                let y2 = number(&operands[1])?;
                let x3 = number(&operands[2])?;
                let y3 = number(&operands[3])?;
                walker.add_point(x2, y2);
                walker.add_point(x3, y3);
            }
            "y" if operands.len() == 4 => {
                let x1 = number(&operands[0])?;
                let y1 = number(&operands[1])?;
                let x3 = number(&operands[2])?;
                let y3 = number(&operands[3])?;
                walker.add_point(x1, y1);
                walker.add_point(x3, y3);
            }
            "h" => {}
            "S" | "s" | "f" | "F" | "f*" | "B" | "B*" | "b" | "b*" => {
                walker.end_paint_path();
            }
            "n" => {
                walker.current_path.clear();
            }
            "q" | "Q" | "cm" | "BT" | "ET" | "Tf" | "Tc" | "Tw" | "Tz" | "TL" | "Ts" | "Tr"
            | "Tm" | "Td" | "TD" | "T*" | "Tj" | "'" | "\"" | "TJ" => {
                return Err(PdfError::Malformed);
            }
            // Clipping, path construction, marked content, and graphics state
            // operators do not emit text glyphs.
            "w" | "J" | "j" | "M" | "d" | "ri" | "i" | "G" | "g" | "RG" | "rg" | "K" | "k"
            | "CS" | "cs" | "SC" | "SCN" | "sc" | "scn" | "gs" | "W" | "W*" | "BMC" | "BDC"
            | "EMC" | "MP" | "DP" => {}
            // Unknown, malformed or extension operators cannot establish complete content
            // correlation.
            _ => {
                walker.trace.uninspected_objects = walker
                    .trace
                    .uninspected_objects
                    .checked_add(1)
                    .ok_or(PdfError::Malformed)?;
            }
        }
    }
    if walker.in_text || !walker.saved.is_empty() {
        return Err(PdfError::Malformed);
    }
    Ok(walker.trace)
}

struct Walker<'a> {
    doc: &'a Document,
    resources: &'a Dictionary,
    decoder: PdfTextDecoder<'a>,
    ctx: &'a ToolCtx,
    graphics: Graphics,
    saved: Vec<Graphics>,
    text_matrix: Matrix,
    line_matrix: Matrix,
    in_text: bool,
    total_text: usize,
    current_path: Vec<[f64; 2]>,
    trace: TextStreamTrace,
}

impl Walker<'_> {
    fn add_point(&mut self, x: f64, y: f64) {
        let [a, b, c, d, e, f] = self.graphics.ctm.0;
        let px = a * x + c * y + e;
        let py = b * x + d * y + f;
        if px.is_finite() && py.is_finite() && px.abs() <= 1e9 && py.abs() <= 1e9 {
            self.current_path.push([px, py]);
        }
    }
    fn end_paint_path(&mut self) {
        if !self.current_path.is_empty() {
            let mut min_x = f64::INFINITY;
            let mut min_y = f64::INFINITY;
            let mut max_x = f64::NEG_INFINITY;
            let mut max_y = f64::NEG_INFINITY;
            for &[px, py] in &self.current_path {
                min_x = min_x.min(px);
                min_y = min_y.min(py);
                max_x = max_x.max(px);
                max_y = max_y.max(py);
            }
            if min_x.is_finite() && min_y.is_finite() && max_x.is_finite() && max_y.is_finite() {
                self.trace.painted_paths.push([min_x, min_y, max_x, max_y]);
            }
            self.current_path.clear();
        }
    }
    fn validate_matrix(&self, matrix: Matrix) -> Result<(), PdfError> {
        if matrix
            .0
            .iter()
            .all(|n| n.is_finite() && n.abs() <= 1_000_000_000.0)
        {
            Ok(())
        } else {
            Err(PdfError::Malformed)
        }
    }
    fn move_line(&mut self, x: f64, y: f64) -> Result<(), PdfError> {
        self.line_matrix = Matrix::translate(x, y).then(self.line_matrix);
        self.validate_matrix(self.line_matrix)?;
        self.text_matrix = self.line_matrix;
        Ok(())
    }
    fn next_line(&mut self) -> Result<(), PdfError> {
        self.move_line(0.0, -self.graphics.leading)
    }
    fn advance(&mut self, amount: f64) -> Result<(), PdfError> {
        if !amount.is_finite() {
            return Err(PdfError::Malformed);
        }
        self.text_matrix = Matrix::translate(amount, 0.0).then(self.text_matrix);
        self.validate_matrix(self.text_matrix)
    }
    fn show(&mut self, operation: usize, string: usize, bytes: &[u8]) -> Result<(), PdfError> {
        let font = self.graphics.font.as_ref().ok_or(PdfError::Malformed)?;
        if self.graphics.size == 0.0 || self.graphics.horizontal_scale == 0.0 {
            return Err(PdfError::NotImplemented("zero-scale text tracing".into()));
        }
        let glyphs = self
            .decoder
            .decode_with_offsets(font, bytes)
            .map_err(PdfError::LimitExceeded)?
            .ok_or_else(|| PdfError::NotImplemented("font glyph mapping".into()))?;
        let widths = super::super::metrics::widths(self.doc, font, bytes, &glyphs, self.ctx)?;
        for (index, (glyph, width)) in glyphs.into_iter().zip(widths).enumerate() {
            if index % 128 == 0 {
                self.ctx.check()?;
            }
            if self.trace.glyphs.len() >= MAX_GLYPHS {
                return Err(PdfError::Malformed);
            }
            self.total_text = self
                .total_text
                .checked_add(glyph.text.len())
                .ok_or(PdfError::Malformed)?;
            if self.total_text > MAX_TEXT_BYTES {
                return Err(PdfError::Malformed);
            }
            let point = Matrix::translate(0.0, self.graphics.rise)
                .then(self.text_matrix)
                .then(self.graphics.ctm)
                .transform_point(0.0, 0.0);
            if !point.0.is_finite() || !point.1.is_finite() {
                return Err(PdfError::Malformed);
            }
            let word = if &bytes[glyph.start..glyph.end] == b" " {
                self.graphics.word_spacing
            } else {
                0.0
            };
            let advance =
                (width / 1000.0 * self.graphics.size + self.graphics.character_spacing + word)
                    * self.graphics.horizontal_scale;
            self.trace.glyphs.push(TextGlyphTrace {
                operation,
                string,
                glyph: index,
                start: glyph.start,
                end: glyph.end,
                text: glyph.text,
                origin: [point.0, point.1],
                render_mode: self.graphics.render_mode,
            });
            self.advance(advance)?;
        }
        Ok(())
    }
}

fn matrix(operands: &[Object]) -> Result<Matrix, PdfError> {
    let mut values = [0.0; 6];
    for (slot, value) in values.iter_mut().zip(operands) {
        *slot = number(value)?;
    }
    Ok(Matrix(values))
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::dictionary;

    #[test]
    fn traces_glyph_origins_with_transforms_spacing_and_tj_adjustments() {
        let doc = Document::with_version("1.7");
        let font = dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica", "FirstChar" => 65, "Widths" => vec![500.into(), 600.into()] };
        let resources = dictionary! { "Font" => dictionary! { "F1" => font } };
        let content = b"q 2 0 0 2 10 20 cm BT /F1 10 Tf 1 Tc 3 4 Td [(A) 100 (B)] TJ ET Q";
        let result = trace(
            &doc,
            &resources,
            content,
            1024,
            &Limits::local(),
            &ToolCtx::new(),
        )
        .unwrap();
        assert_eq!(result.glyphs.len(), 2);
        assert_eq!(result.glyphs[0].origin, [16.0, 28.0]);
        assert_eq!(result.glyphs[1].origin, [26.0, 28.0]);
        assert_eq!((result.glyphs[0].string, result.glyphs[1].string), (0, 1));
        assert!(!format!("{:?}", result.glyphs[0]).contains("\"A\""));
    }

    #[test]
    fn unknown_operators_require_separate_inspection() {
        let doc = Document::with_version("1.7");
        let result = trace(
            &doc,
            &Dictionary::new(),
            b"q 1 2 unknownop Q",
            1024,
            &Limits::local(),
            &ToolCtx::new(),
        );
        assert!(result.is_err() || result.unwrap().uninspected_objects() > 0);
    }

    #[test]
    fn inline_image_and_shading_are_uninspected_carriers() {
        let doc = Document::with_version("1.7");
        let result = trace(
            &doc,
            &Dictionary::new(),
            b"BI /W 1 /H 1 /BPC 8 /CS /G ID X EI",
            1024,
            &Limits::local(),
            &ToolCtx::new(),
        )
        .unwrap();
        assert_eq!(result.uninspected_objects(), 1);
        assert_eq!(result.uninspected_carrier(), Some("inline_image"));

        let result_sh = trace(
            &doc,
            &Dictionary::new(),
            b"/Sh0 sh",
            1024,
            &Limits::local(),
            &ToolCtx::new(),
        )
        .unwrap();
        assert_eq!(result_sh.uninspected_objects(), 1);
        assert_eq!(result_sh.uninspected_carrier(), Some("shading"));
    }

    #[test]
    fn painted_paths_record_transformed_bounding_boxes() {
        let doc = Document::with_version("1.7");
        let content = b"q 2 0 0 2 10 20 cm 5 5 20 30 re f Q";
        let result = trace(
            &doc,
            &Dictionary::new(),
            content,
            1024,
            &Limits::local(),
            &ToolCtx::new(),
        )
        .unwrap();
        assert_eq!(result.painted_paths().len(), 1);
        assert_eq!(result.painted_paths()[0], [20.0, 30.0, 60.0, 90.0]);
    }
}
