//! Correlates source glyphs with independent PDFium character evidence.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use ariad_core::tools::params::redact_internal::RedactGeometryPage;
use lopdf::Object;

use crate::{PdfError, ToolCtx};

use super::{TextEdit, TextStreamTrace};

const TOLERANCE: f64 = 0.05;
const MAX_CANDIDATES: usize = 64;
const MAX_GLYPHS: usize = 1_000_000;
const MAX_CORRELATION_WORK: usize = 256 * 1024 * 1024;

/// Maps selected PDFium character indices to source-stream edits.
///
/// This accepts only complete, unambiguous flat-stream evidence. Nested XObjects and glyphs
/// that map to multiple Unicode scalars must be handled by later processing rather than guessed.
pub fn select(
    trace: &TextStreamTrace,
    page: &RedactGeometryPage,
    selected_indices: &[usize],
    content: &[u8],
    ctx: &ToolCtx,
) -> Result<Vec<TextEdit>, PdfError> {
    ctx.check()?;
    if trace.content_sha256 != crate::writer::sha256_chunks(&[content]) {
        return Err(PdfError::InvalidParams(
            "text trace does not match the content stream".into(),
        ));
    }
    if trace.glyphs.len() > MAX_GLYPHS
        || page.chars.len() > MAX_GLYPHS
        || selected_indices.len() > MAX_GLYPHS
    {
        return Err(PdfError::LimitExceeded(
            ariad_core::reader::pdf::ReadError::TooManyObjects { limit: MAX_GLYPHS },
        ));
    }
    if trace.uninspected_objects != 0 {
        let carrier = trace
            .uninspected_carrier()
            .unwrap_or("page content correlation");
        return Err(PdfError::NotImplemented(format!(
            "uninspected carrier refused: {carrier}"
        )));
    }
    let selected: BTreeSet<_> = selected_indices.iter().copied().collect();
    if selected.len() != selected_indices.len() {
        return Err(PdfError::InvalidParams(
            "duplicate character selection".into(),
        ));
    }
    let mut bins: HashMap<(i64, i64, char), Vec<usize>> = HashMap::new();
    for (index, character) in page.chars.iter().enumerate() {
        if index % 128 == 0 {
            ctx.check()?;
        }
        if character.index != index || character.value == '\0' {
            return Err(PdfError::Malformed);
        }
        if character.generated {
            if selected.contains(&index) {
                return Err(PdfError::InvalidParams(
                    "generated characters cannot be selected".into(),
                ));
            }
            continue;
        }
        let origin = character
            .origin
            .ok_or_else(|| PdfError::NotImplemented("missing character origin".into()))?;
        let key = (bin(origin[0])?, bin(origin[1])?, character.value);
        let bucket = bins.entry(key).or_default();
        if bucket.len() >= MAX_CANDIDATES {
            return Err(PdfError::NotImplemented(
                "ambiguous character geometry".into(),
            ));
        }
        bucket.push(index);
    }
    if selected.iter().any(|&index| index >= page.chars.len()) {
        return Err(PdfError::InvalidParams(
            "character selection is outside the page".into(),
        ));
    }
    let mut used = vec![false; page.chars.len()];
    let mut masks: BTreeMap<(usize, usize), Vec<bool>> = BTreeMap::new();
    let mut selected_operations = BTreeSet::new();
    let mut work = 0usize;
    for (count, source) in trace.glyphs.iter().enumerate() {
        if count % 128 == 0 {
            ctx.check()?;
        }
        let mut scalars = source.text.chars();
        let scalar = scalars.next().ok_or(PdfError::Malformed)?;
        if scalars.next().is_some() {
            return Err(PdfError::NotImplemented(
                "multi-scalar glyph correlation".into(),
            ));
        }
        let (x, y) = (bin(source.origin[0])?, bin(source.origin[1])?);
        let mut candidate = None;
        for dx in -1..=1 {
            for dy in -1..=1 {
                if let Some(indices) = bins.get(&(x + dx, y + dy, scalar)) {
                    work = work.checked_add(indices.len()).ok_or(PdfError::Malformed)?;
                    if work > MAX_CORRELATION_WORK {
                        return Err(PdfError::LimitExceeded(
                            ariad_core::reader::pdf::ReadError::TooManyObjects {
                                limit: MAX_CORRELATION_WORK,
                            },
                        ));
                    }
                    for &index in indices {
                        let origin = page.chars[index].origin.ok_or(PdfError::Malformed)?;
                        if (origin[0] - source.origin[0]).abs() <= TOLERANCE
                            && (origin[1] - source.origin[1]).abs() <= TOLERANCE
                        {
                            if candidate.is_some() {
                                return Err(PdfError::NotImplemented(
                                    "ambiguous character geometry".into(),
                                ));
                            }
                            candidate = Some(index);
                        }
                    }
                }
            }
        }
        let index = candidate
            .ok_or_else(|| PdfError::NotImplemented("unmatched character geometry".into()))?;
        if std::mem::replace(&mut used[index], true) {
            return Err(PdfError::NotImplemented(
                "ambiguous character geometry".into(),
            ));
        }
        let values = masks.entry((source.operation, source.string)).or_default();
        if source.glyph != values.len() {
            return Err(PdfError::Malformed);
        }
        let remove = selected.contains(&index);
        values.push(remove);
        if remove {
            selected_operations.insert(source.operation);
        }
    }
    if page
        .chars
        .iter()
        .enumerate()
        .any(|(index, character)| !character.generated && !used[index])
    {
        return Err(PdfError::NotImplemented(
            "unmatched character geometry".into(),
        ));
    }
    let parsed = crate::scan_text::decode_checked(content, ctx)?;
    let mut edits = Vec::with_capacity(selected_operations.len());
    for operation in selected_operations {
        let op = parsed
            .operations
            .get(operation)
            .ok_or(PdfError::Malformed)?;
        let count = match op.operator.as_str() {
            "Tj" | "'"
                if op.operands.len() == 1 && matches!(&op.operands[0], Object::String(..)) =>
            {
                1
            }
            "\"" if op.operands.len() == 3 && matches!(&op.operands[2], Object::String(..)) => 1,
            "TJ" if op.operands.len() == 1 => op.operands[0]
                .as_array()
                .map_err(|_| PdfError::Malformed)?
                .iter()
                .filter(|operand| matches!(operand, Object::String(..)))
                .count(),
            _ => return Err(PdfError::Malformed),
        };
        let removals = (0..count)
            .map(|string| masks.remove(&(operation, string)).unwrap_or_default())
            .collect();
        edits.push(TextEdit {
            operation,
            removals,
        });
    }
    Ok(edits)
}

fn bin(value: f64) -> Result<i64, PdfError> {
    if !value.is_finite() || value.abs() > 1_000_000_000.0 {
        return Err(PdfError::Malformed);
    }
    Ok((value / TOLERANCE).floor() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::redact::stream::TextGlyphTrace;
    use ariad_core::tools::params::redact_internal::RedactGeometryChar;

    fn source(index: usize, x: f64, value: char) -> TextGlyphTrace {
        TextGlyphTrace {
            operation: 2,
            string: 0,
            glyph: index,
            start: index,
            end: index + 1,
            text: value.to_string(),
            origin: [x, 10.0],
            render_mode: 0,
        }
    }
    fn character(index: usize, x: f64, value: char) -> RedactGeometryChar {
        RedactGeometryChar {
            value,
            index,
            generated: false,
            origin: Some([x, 10.0]),
            bbox: Some([x, 10.0, x + 1.0, 11.0]),
        }
    }
    #[test]
    fn selects_only_uniquely_correlated_glyphs() {
        let trace = TextStreamTrace {
            content_sha256: crate::writer::sha256_chunks(&[b"BT /F1 10 Tf (AB) Tj ET"]),
            glyphs: vec![source(0, 1.0, 'A'), source(1, 2.0, 'B')],
            uninspected_objects: 0,
            uninspected_carrier: None,
            painted_paths: Vec::new(),
        };
        let page = RedactGeometryPage {
            page: 1,
            width: 100.0,
            height: 100.0,
            rotation: 0,
            chars: vec![character(0, 1.0, 'A'), character(1, 2.0, 'B')],
        };
        let edits = select(
            &trace,
            &page,
            &[0],
            b"BT /F1 10 Tf (AB) Tj ET",
            &ToolCtx::new(),
        )
        .unwrap();
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].operation, 2);
        assert_eq!(edits[0].removals, [vec![true, false]]);
        let mut ambiguous = page.clone();
        ambiguous.chars.push(character(2, 1.0, 'A'));
        assert!(
            select(
                &trace,
                &ambiguous,
                &[0],
                b"BT /F1 10 Tf (AB) Tj ET",
                &ToolCtx::new()
            )
            .is_err()
        );
        assert!(
            select(
                &trace,
                &page,
                &[0],
                b"BT /F1 10 Tf (BA) Tj ET",
                &ToolCtx::new()
            )
            .is_err()
        );
    }
}
