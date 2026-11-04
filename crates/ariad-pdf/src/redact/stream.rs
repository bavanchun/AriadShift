//! Validated application of glyph removals to a content stream.

use std::collections::BTreeMap;

use ariad_core::{limits::Limits, reader::pdf::PdfTextDecoder};
use lopdf::{Dictionary, Document, Object, content::Content};

use crate::{PdfError, ToolCtx};

use super::text::{TextState, rewrite_show_with_font};

mod select;
mod trace;
pub use select::select;
pub use trace::{TextGlyphTrace, TextStreamTrace, trace};

/// One selection per string operand of the indicated text-show operation.
pub struct TextEdit {
    /// Zero-based index in the decoded content stream.
    pub operation: usize,
    /// Glyph selection for each encoded string operand.
    pub removals: Vec<Vec<bool>>,
}

#[derive(Clone, Default)]
struct State {
    font: Option<Object>,
    size: f64,
    char_spacing: f64,
    word_spacing: f64,
}

/// Rewrites already mapped text operations. This stage does not inspect nested forms, images,
/// vectors or the rest of the document; those require separate processing and verification.
pub fn apply(
    doc: &Document,
    resources: &Dictionary,
    content: &[u8],
    edits: &[TextEdit],
    input_len: usize,
    limits: &Limits,
    ctx: &ToolCtx,
) -> Result<Vec<u8>, PdfError> {
    ctx.check()?;
    let parsed = crate::scan_text::decode_checked(content, ctx)?;
    let mut edits_by_index = BTreeMap::new();
    for edit in edits {
        if edit.operation >= parsed.operations.len()
            || edits_by_index.insert(edit.operation, edit).is_some()
        {
            return Err(PdfError::InvalidParams(
                "invalid text operation selection".into(),
            ));
        }
    }
    let mut decoder = PdfTextDecoder::new(doc, input_len, limits);
    let mut state = State::default();
    let mut saved = Vec::new();
    let mut in_text = false;
    let mut output = Vec::new();
    for (index, operation) in parsed.operations.into_iter().enumerate() {
        ctx.check()?;
        match operation.operator.as_str() {
            "q" => {
                if saved.len() >= 256 {
                    return Err(PdfError::Malformed);
                }
                saved.push(state.clone());
            }
            "Q" => state = saved.pop().ok_or(PdfError::Malformed)?,
            "BT" if !in_text => in_text = true,
            "ET" if in_text => in_text = false,
            "BT" | "ET" => return Err(PdfError::Malformed),
            "Tf" if in_text && operation.operands.len() == 2 => {
                let name = operation.operands[0]
                    .as_name()
                    .map_err(|_| PdfError::Malformed)?;
                let fonts = resolved(
                    doc,
                    resources.get(b"Font").map_err(|_| PdfError::Malformed)?,
                )?
                .as_dict()
                .map_err(|_| PdfError::Malformed)?;
                state.font = Some(fonts.get(name).map_err(|_| PdfError::Malformed)?.clone());
                state.size = number(&operation.operands[1])?;
            }
            "Tc" if in_text && operation.operands.len() == 1 => {
                state.char_spacing = number(&operation.operands[0])?
            }
            "Tw" if in_text && operation.operands.len() == 1 => {
                state.word_spacing = number(&operation.operands[0])?
            }
            "\"" if in_text && operation.operands.len() == 3 => {
                state.word_spacing = number(&operation.operands[0])?;
                state.char_spacing = number(&operation.operands[1])?;
            }
            "Tf" | "Tc" | "Tw" | "\"" => return Err(PdfError::Malformed),
            _ => {}
        }
        if let Some(edit) = edits_by_index.remove(&index) {
            if !in_text {
                return Err(PdfError::Malformed);
            }
            let font = state.font.as_ref().ok_or(PdfError::Malformed)?;
            let text_state = TextState {
                font_size: state.size,
                character_spacing: state.char_spacing,
                word_spacing: state.word_spacing,
            };
            output.extend(rewrite_show_with_font(
                doc,
                font,
                &mut decoder,
                &operation,
                &edit.removals,
                &text_state,
                ctx,
            )?);
        } else {
            output.push(operation);
        }
    }
    if !edits_by_index.is_empty() || in_text || !saved.is_empty() {
        return Err(PdfError::Malformed);
    }
    Content { operations: output }
        .encode()
        .map_err(|_| PdfError::Malformed)
}

fn resolved<'a>(doc: &'a Document, value: &'a Object) -> Result<&'a Object, PdfError> {
    doc.dereference(value)
        .map(|(_, value)| value)
        .map_err(|_| PdfError::Malformed)
}

fn number(value: &Object) -> Result<f64, PdfError> {
    let result = match value {
        Object::Integer(n) => *n as f64,
        Object::Real(n) => f64::from(*n),
        _ => return Err(PdfError::Malformed),
    };
    if !result.is_finite() || result.abs() > 1_000_000.0 {
        return Err(PdfError::Malformed);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::dictionary;

    #[test]
    fn rewrites_a_selected_show_operator_without_touching_other_text() {
        let doc = Document::with_version("1.7");
        let font = dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica", "FirstChar" => 65, "Widths" => vec![500.into(), 600.into()] };
        let resources = dictionary! { "Font" => dictionary! { "F1" => font } };
        let content = b"BT /F1 10 Tf (AB) Tj (B) Tj ET";
        let edit = TextEdit {
            operation: 2,
            removals: vec![vec![true, false]],
        };
        let rewritten = apply(
            &doc,
            &resources,
            content,
            &[edit],
            1024,
            &Limits::local(),
            &ToolCtx::new(),
        )
        .unwrap();
        let result = Content::decode_strict(&rewritten).unwrap();
        assert_eq!(result.operations[2].operator, "TJ");
        let parts = result.operations[2].operands[0].as_array().unwrap();
        assert_eq!(parts[0].as_float().unwrap(), -500.0);
        assert_eq!(parts[1].as_str().unwrap(), b"B");
        assert_eq!(result.operations[3].operator, "Tj");
        assert_eq!(
            result.operations[3].operands,
            vec![Object::string_literal("B")]
        );
        assert!(!rewritten.windows(2).any(|bytes| bytes == b"AB"));
    }

    #[test]
    fn rejects_duplicate_or_non_text_selections() {
        let doc = Document::with_version("1.7");
        let resources = dictionary! {};
        let edit = || TextEdit {
            operation: 0,
            removals: vec![],
        };
        assert!(
            apply(
                &doc,
                &resources,
                b"q Q",
                &[edit(), edit()],
                1024,
                &Limits::local(),
                &ToolCtx::new()
            )
            .is_err()
        );
        assert!(
            apply(
                &doc,
                &resources,
                b"q Q",
                &[edit()],
                1024,
                &Limits::local(),
                &ToolCtx::new()
            )
            .is_err()
        );
    }
}
