//! Rewriting text-show strings without retaining removed encoded glyphs.

use ariad_core::reader::pdf::{PdfDecodedGlyph, PdfTextDecoder};
use lopdf::{Document, Object, StringFormat, content::Operation};

use crate::{PdfError, ToolCtx};

/// Text-state values needed to preserve displacement for removed characters.
pub(super) struct TextState {
    /// Font size in text space, before horizontal scaling.
    pub font_size: f64,
    /// Character spacing in text space.
    pub character_spacing: f64,
    /// Word spacing in text space.
    pub word_spacing: f64,
}

/// Exact mapping and removal decisions for one encoded string operand.
pub(super) struct StringSelection<'a> {
    pub glyphs: &'a [PdfDecodedGlyph],
    pub widths: &'a [f64],
    pub remove: &'a [bool],
}

/// Resolves each operand's glyph mapping and exact widths before rewriting it.
pub(super) fn rewrite_show_with_font(
    doc: &Document,
    font: &Object,
    decoder: &mut PdfTextDecoder<'_>,
    operation: &Operation,
    removals: &[Vec<bool>],
    state: &TextState,
    ctx: &ToolCtx,
) -> Result<Vec<Operation>, PdfError> {
    let invalid = || PdfError::InvalidParams("invalid text-show operation".into());
    let strings: Vec<&[u8]> = match operation.operator.as_str() {
        "Tj" | "'" if operation.operands.len() == 1 => {
            vec![operation.operands[0].as_str().map_err(|_| invalid())?]
        }
        "\"" if operation.operands.len() == 3 => {
            vec![operation.operands[2].as_str().map_err(|_| invalid())?]
        }
        "TJ" if operation.operands.len() == 1 => operation.operands[0]
            .as_array()
            .map_err(|_| invalid())?
            .iter()
            .filter_map(|item| item.as_str().ok())
            .collect(),
        _ => return Err(invalid()),
    };
    if strings.len() != removals.len() {
        return Err(invalid());
    }
    let mut decoded = Vec::with_capacity(strings.len());
    let mut widths = Vec::with_capacity(strings.len());
    for string in strings {
        ctx.check()?;
        let glyphs = decoder
            .decode_with_offsets(font, string)
            .map_err(PdfError::LimitExceeded)?
            .ok_or_else(|| PdfError::NotImplemented("font glyph mapping".into()))?;
        widths.push(super::metrics::widths(doc, font, string, &glyphs, ctx)?);
        decoded.push(glyphs);
    }
    let selections: Vec<_> = decoded
        .iter()
        .zip(&widths)
        .zip(removals)
        .map(|((glyphs, widths), remove)| StringSelection {
            glyphs,
            widths,
            remove,
        })
        .collect();
    rewrite_show(operation, &selections, state)
}

/// Rewrites a text-show operator into `TJ`, preserving pre-existing adjustments and the
/// line/spacing effects of the shorthand quote operators.
pub(super) fn rewrite_show(
    operation: &Operation,
    selections: &[StringSelection<'_>],
    state: &TextState,
) -> Result<Vec<Operation>, PdfError> {
    let invalid = || PdfError::InvalidParams("invalid text-show operation".into());
    let mut prefix = Vec::new();
    let mut effective = TextState {
        font_size: state.font_size,
        character_spacing: state.character_spacing,
        word_spacing: state.word_spacing,
    };
    let mut operands: Vec<&Object> = Vec::new();
    match operation.operator.as_str() {
        "Tj" if operation.operands.len() == 1 => operands.push(&operation.operands[0]),
        "TJ" if operation.operands.len() == 1 => {
            operands.extend(
                operation.operands[0]
                    .as_array()
                    .map_err(|_| invalid())?
                    .iter(),
            );
        }
        "'" if operation.operands.len() == 1 => {
            prefix.push(Operation::new("T*", vec![]));
            operands.push(&operation.operands[0]);
        }
        "\"" if operation.operands.len() == 3 => {
            let number = |value: &Object| -> Result<f64, PdfError> {
                match value {
                    Object::Integer(n) => Ok(*n as f64),
                    Object::Real(n) if n.is_finite() => Ok(f64::from(*n)),
                    _ => Err(invalid()),
                }
            };
            effective.word_spacing = number(&operation.operands[0])?;
            effective.character_spacing = number(&operation.operands[1])?;
            prefix.push(Operation::new("Tw", vec![operation.operands[0].clone()]));
            prefix.push(Operation::new("Tc", vec![operation.operands[1].clone()]));
            prefix.push(Operation::new("T*", vec![]));
            operands.push(&operation.operands[2]);
        }
        _ => return Err(invalid()),
    }
    let mut rewritten = Vec::new();
    let mut index = 0usize;
    for operand in operands {
        match operand {
            Object::String(bytes, format) => {
                let selection = selections.get(index).ok_or_else(invalid)?;
                rewritten.extend(rewrite_string(
                    bytes,
                    *format,
                    selection.glyphs,
                    selection.widths,
                    selection.remove,
                    &effective,
                )?);
                index += 1;
            }
            Object::Integer(_) if operation.operator == "TJ" => rewritten.push(operand.clone()),
            Object::Real(value) if operation.operator == "TJ" && value.is_finite() => {
                rewritten.push(operand.clone())
            }
            _ => return Err(invalid()),
        }
    }
    if index != selections.len() {
        return Err(invalid());
    }
    prefix.push(Operation::new("TJ", vec![Object::Array(rewritten)]));
    Ok(prefix)
}

/// Converts one encoded string into a `TJ` array, using exact glyph widths in thousandths of an
/// em. Callers must establish font mappings, width provenance and glyph selection first.
pub(super) fn rewrite_string(
    encoded: &[u8],
    format: StringFormat,
    glyphs: &[PdfDecodedGlyph],
    widths: &[f64],
    remove: &[bool],
    state: &TextState,
) -> Result<Vec<Object>, PdfError> {
    if glyphs.len() != widths.len()
        || glyphs.len() != remove.len()
        || !state.font_size.is_finite()
        || state.font_size == 0.0
        || !state.character_spacing.is_finite()
        || !state.word_spacing.is_finite()
    {
        return Err(PdfError::InvalidParams("invalid text rewrite state".into()));
    }
    let mut result = Vec::new();
    let mut retained = Vec::new();
    let mut position = 0usize;
    for ((glyph, width), remove) in glyphs.iter().zip(widths).zip(remove) {
        if glyph.start != position
            || glyph.end <= glyph.start
            || glyph.end > encoded.len()
            || !width.is_finite()
        {
            return Err(PdfError::InvalidParams("invalid text glyph mapping".into()));
        }
        let code = &encoded[glyph.start..glyph.end];
        position = glyph.end;
        if !remove {
            retained.extend_from_slice(code);
            continue;
        }
        if !retained.is_empty() {
            result.push(Object::String(std::mem::take(&mut retained), format));
        }
        let word = if code == b" " {
            state.word_spacing
        } else {
            0.0
        };
        let displacement = -*width - (state.character_spacing + word) * 1000.0 / state.font_size;
        let encoded_displacement = displacement as f32;
        if !displacement.is_finite()
            || !encoded_displacement.is_finite()
            || ((f64::from(encoded_displacement) - displacement) * state.font_size / 1000.0).abs()
                > 0.00001
        {
            return Err(PdfError::InvalidParams("invalid text displacement".into()));
        }
        result.push(Object::Real(encoded_displacement));
    }
    if position != encoded.len() {
        return Err(PdfError::InvalidParams(
            "incomplete text glyph mapping".into(),
        ));
    }
    if !retained.is_empty() {
        result.push(Object::String(retained, format));
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn glyph(start: usize, end: usize, text: &str) -> PdfDecodedGlyph {
        PdfDecodedGlyph {
            start,
            end,
            text: text.into(),
        }
    }

    #[test]
    fn removes_encoded_glyphs_and_preserves_text_advance() {
        let glyphs = [
            glyph(0, 1, "a"),
            glyph(1, 2, " "),
            glyph(2, 4, "fi"),
            glyph(4, 5, "z"),
        ];
        let state = TextState {
            font_size: 10.0,
            character_spacing: 2.0,
            word_spacing: 3.0,
        };
        let result = rewrite_string(
            b"a \x00\x01z",
            StringFormat::Hexadecimal,
            &glyphs,
            &[500.0, 250.0, 600.0, 400.0],
            &[false, true, true, false],
            &state,
        )
        .unwrap();
        assert_eq!(
            result,
            vec![
                Object::String(b"a".to_vec(), StringFormat::Hexadecimal),
                Object::Real(-750.0),
                Object::Real(-800.0),
                Object::String(b"z".to_vec(), StringFormat::Hexadecimal)
            ]
        );
    }

    #[test]
    fn refuses_incomplete_or_invalid_mapping() {
        let state = TextState {
            font_size: 12.0,
            character_spacing: 0.0,
            word_spacing: 0.0,
        };
        assert!(
            rewrite_string(
                b"ab",
                StringFormat::Literal,
                &[glyph(0, 1, "a")],
                &[500.0],
                &[true],
                &state
            )
            .is_err()
        );
        assert!(
            rewrite_show(&Operation::new("Tj", vec![Object::Integer(3)]), &[], &state).is_err()
        );
        assert!(
            rewrite_string(
                b"a",
                StringFormat::Literal,
                &[glyph(0, 1, "a")],
                &[f64::NAN],
                &[true],
                &state
            )
            .is_err()
        );
    }

    #[test]
    fn preserves_array_adjustments_and_quote_operator_state() {
        let first = [glyph(0, 1, "a")];
        let second = [glyph(0, 1, "b")];
        let first_width = [500.0];
        let second_width = [600.0];
        let remove_first = [true];
        let retain_second = [false];
        let selections = [
            StringSelection {
                glyphs: &first,
                widths: &first_width,
                remove: &remove_first,
            },
            StringSelection {
                glyphs: &second,
                widths: &second_width,
                remove: &retain_second,
            },
        ];
        let state = TextState {
            font_size: 10.0,
            character_spacing: 0.0,
            word_spacing: 0.0,
        };
        let operation = Operation::new(
            "TJ",
            vec![Object::Array(vec![
                Object::string_literal("a"),
                Object::Integer(25),
                Object::string_literal("b"),
            ])],
        );
        let result = rewrite_show(&operation, &selections, &state).unwrap();
        assert_eq!(
            result[0].operands,
            vec![Object::Array(vec![
                Object::Real(-500.0),
                Object::Integer(25),
                Object::string_literal("b")
            ])]
        );
        let quote = Operation::new(
            "\"",
            vec![
                Object::Integer(3),
                Object::Integer(2),
                Object::string_literal("a"),
            ],
        );
        let result = rewrite_show(&quote, &selections[..1], &state).unwrap();
        assert_eq!(
            result
                .iter()
                .map(|op| op.operator.as_str())
                .collect::<Vec<_>>(),
            ["Tw", "Tc", "T*", "TJ"]
        );
        assert_eq!(
            result[3].operands,
            vec![Object::Array(vec![Object::Real(-700.0)])]
        );
    }

    #[test]
    fn resolves_real_font_mapping_before_rewriting() {
        use ariad_core::limits::Limits;
        use lopdf::{Document, dictionary};
        let doc = Document::with_version("1.7");
        let font = Object::Dictionary(
            dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica", "FirstChar" => 65, "Widths" => vec![500.into(), 600.into()] },
        );
        let mut decoder = PdfTextDecoder::new(&doc, 1024, &Limits::local());
        let operation = Operation::new("Tj", vec![Object::string_literal("AB")]);
        let state = TextState {
            font_size: 10.0,
            character_spacing: 0.0,
            word_spacing: 0.0,
        };
        let result = rewrite_show_with_font(
            &doc,
            &font,
            &mut decoder,
            &operation,
            &[vec![true, false]],
            &state,
            &ToolCtx::new(),
        )
        .unwrap();
        assert_eq!(
            result[0].operands,
            vec![Object::Array(vec![
                Object::Real(-500.0),
                Object::string_literal("B")
            ])]
        );
    }
}
