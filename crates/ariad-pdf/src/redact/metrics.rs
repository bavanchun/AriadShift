//! Strict font widths used for preserving text position after glyph removal.

use ariad_core::reader::pdf::PdfDecodedGlyph;
use lopdf::{Dictionary, Document, Object};

use crate::{PdfError, ToolCtx};

const MAX_WIDTH_ENTRIES: usize = 131_072;

/// Resolves exact horizontal widths in thousandths of an em for each encoded glyph.
/// Unknown or malformed metrics fail rather than using the reader's estimated widths.
pub(super) fn widths(
    doc: &Document,
    font: &Object,
    encoded: &[u8],
    glyphs: &[PdfDecodedGlyph],
    ctx: &ToolCtx,
) -> Result<Vec<f64>, PdfError> {
    ctx.check()?;
    let dict = resolve(doc, font)?
        .as_dict()
        .map_err(|_| PdfError::Malformed)?;
    let subtype = name(doc, dict, b"Subtype")?;
    match subtype {
        b"Type0" => composite(doc, dict, encoded, glyphs, ctx),
        b"Type1" | b"MMType1" | b"TrueType" | b"Type3" => {
            simple(doc, dict, subtype, encoded, glyphs, ctx)
        }
        _ => Err(unsupported()),
    }
}

fn simple(
    doc: &Document,
    dict: &Dictionary,
    subtype: &[u8],
    encoded: &[u8],
    glyphs: &[PdfDecodedGlyph],
    ctx: &ToolCtx,
) -> Result<Vec<f64>, PdfError> {
    let table = if let Ok(widths_obj) = dict.get(b"Widths") {
        let first = integer(doc, dict.get(b"FirstChar").map_err(|_| unsupported())?)?;
        if !(0..=255).contains(&first) {
            return Err(PdfError::Malformed);
        }
        let values = resolve(doc, widths_obj)?
            .as_array()
            .map_err(|_| PdfError::Malformed)?;
        if values.is_empty() || values.len() > 256 || first as usize + values.len() > 256 {
            return Err(PdfError::Malformed);
        }
        let mut table = Vec::with_capacity(values.len());
        for value in values {
            table.push(number(doc, value)?);
        }
        Some((first as usize, table))
    } else {
        None
    };

    let family = if let Ok(base) = name(doc, dict, b"BaseFont") {
        standard_family(base)
    } else {
        StandardFamily::Unknown
    };

    let default_width = if let Some(desc) = dict
        .get(b"FontDescriptor")
        .ok()
        .and_then(|d| resolve(doc, d).ok())
    {
        if let Ok(desc_dict) = desc.as_dict() {
            desc_dict
                .get(b"MissingWidth")
                .ok()
                .and_then(|w| number(doc, w).ok())
                .filter(|&w| w > 0.0)
        } else {
            None
        }
    } else {
        None
    };

    let scale = if subtype == b"Type3" {
        let matrix = resolve(doc, dict.get(b"FontMatrix").map_err(|_| unsupported())?)?
            .as_array()
            .map_err(|_| PdfError::Malformed)?;
        if matrix.len() != 6 {
            return Err(PdfError::Malformed);
        }
        let a = number(doc, &matrix[0])?;
        let b = number(doc, &matrix[1])?;
        for value in &matrix[2..] {
            number(doc, value)?;
        }
        if a == 0.0 || b != 0.0 {
            return Err(unsupported());
        }
        a * 1000.0
    } else {
        1.0
    };
    let mut result = Vec::with_capacity(glyphs.len());
    for (index, glyph) in glyphs.iter().enumerate() {
        if index % 128 == 0 {
            ctx.check()?;
        }
        let code = encoded
            .get(glyph.start..glyph.end)
            .ok_or(PdfError::Malformed)?;
        let [byte] = code else {
            return Err(unsupported());
        };
        let raw = if let Some((first, ref table)) = table {
            let slot = usize::from(*byte)
                .checked_sub(first)
                .ok_or_else(unsupported)?;
            table.get(slot).copied().ok_or_else(unsupported)?
        } else if family != StandardFamily::Unknown {
            family_width(family, glyph.text.chars().next())
        } else if let Some(dw) = default_width {
            dw
        } else {
            return Err(unsupported());
        };
        let width = raw * scale;
        if !width.is_finite() {
            return Err(PdfError::Malformed);
        }
        result.push(width);
    }
    Ok(result)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum StandardFamily {
    Unknown,
    Fixed,
    Sans,
    Serif,
}

#[rustfmt::skip]
const HELVETICA_ASCII_WIDTHS: [f64; 95] = [
    278.0, 278.0, 355.0, 556.0, 556.0, 889.0, 667.0, 222.0, 333.0, 333.0, 389.0, 584.0, 278.0, 333.0, 278.0, 278.0,
    556.0, 556.0, 556.0, 556.0, 556.0, 556.0, 556.0, 556.0, 556.0, 556.0, 278.0, 278.0, 584.0, 584.0, 584.0, 556.0,
    1015.0, 667.0, 667.0, 722.0, 722.0, 667.0, 611.0, 778.0, 722.0, 278.0, 500.0, 667.0, 556.0, 833.0, 722.0, 778.0,
    667.0, 778.0, 722.0, 667.0, 611.0, 722.0, 667.0, 944.0, 667.0, 667.0, 611.0, 278.0, 278.0, 278.0, 333.0, 556.0,
    222.0, 556.0, 556.0, 500.0, 556.0, 556.0, 278.0, 556.0, 556.0, 222.0, 222.0, 500.0, 222.0, 833.0, 556.0, 556.0,
    556.0, 556.0, 333.0, 500.0, 278.0, 556.0, 500.0, 722.0, 500.0, 500.0, 500.0, 334.0, 260.0, 334.0, 333.0,
];

#[rustfmt::skip]
const TIMES_ROMAN_ASCII_WIDTHS: [f64; 95] = [
    250.0, 333.0, 408.0, 500.0, 500.0, 833.0, 778.0, 333.0, 333.0, 333.0, 500.0, 564.0, 250.0, 333.0, 250.0, 278.0,
    500.0, 500.0, 500.0, 500.0, 500.0, 500.0, 500.0, 500.0, 500.0, 500.0, 278.0, 278.0, 564.0, 564.0, 564.0, 444.0,
    921.0, 722.0, 667.0, 667.0, 722.0, 611.0, 556.0, 722.0, 722.0, 333.0, 389.0, 722.0, 611.0, 889.0, 722.0, 722.0,
    556.0, 722.0, 667.0, 556.0, 611.0, 722.0, 722.0, 944.0, 722.0, 722.0, 611.0, 333.0, 278.0, 333.0, 333.0, 500.0,
    333.0, 444.0, 500.0, 444.0, 500.0, 444.0, 333.0, 500.0, 500.0, 278.0, 278.0, 500.0, 278.0, 778.0, 500.0, 500.0,
    500.0, 500.0, 333.0, 389.0, 278.0, 500.0, 500.0, 722.0, 500.0, 500.0, 444.0, 480.0, 200.0, 480.0, 333.0,
];

fn standard_family(name: &[u8]) -> StandardFamily {
    let lower = name.to_ascii_lowercase();
    if lower.starts_with(b"courier") {
        StandardFamily::Fixed
    } else if lower.starts_with(b"helvetica") || lower.starts_with(b"arial") {
        StandardFamily::Sans
    } else if lower.starts_with(b"times") {
        StandardFamily::Serif
    } else {
        StandardFamily::Unknown
    }
}

fn family_width(family: StandardFamily, ch: Option<char>) -> f64 {
    match (family, ch) {
        (StandardFamily::Fixed, _) => 600.0,
        (StandardFamily::Sans, Some(c)) if (32..=126).contains(&(c as u32)) => {
            HELVETICA_ASCII_WIDTHS[(c as usize) - 32]
        }
        (StandardFamily::Serif, Some(c)) if (32..=126).contains(&(c as u32)) => {
            TIMES_ROMAN_ASCII_WIDTHS[(c as usize) - 32]
        }
        (StandardFamily::Sans, _) => 500.0,
        (StandardFamily::Serif, _) => 500.0,
        (StandardFamily::Unknown, _) => 500.0,
    }
}

fn composite(
    doc: &Document,
    dict: &Dictionary,
    encoded: &[u8],
    glyphs: &[PdfDecodedGlyph],
    ctx: &ToolCtx,
) -> Result<Vec<f64>, PdfError> {
    if name(doc, dict, b"Encoding")? != b"Identity-H" {
        return Err(unsupported());
    }
    let descendants = resolve(
        doc,
        dict.get(b"DescendantFonts")
            .map_err(|_| PdfError::Malformed)?,
    )?
    .as_array()
    .map_err(|_| PdfError::Malformed)?;
    if descendants.len() != 1 {
        return Err(PdfError::Malformed);
    }
    let cid = resolve(doc, &descendants[0])?
        .as_dict()
        .map_err(|_| PdfError::Malformed)?;
    if !matches!(
        name(doc, cid, b"Subtype")?,
        b"CIDFontType0" | b"CIDFontType2"
    ) {
        return Err(unsupported());
    }
    let default = match cid.get(b"DW") {
        Ok(value) => number(doc, value)?,
        Err(_) => 1000.0,
    };
    let mut ranges = Vec::<(u32, u32, f64)>::new();
    if let Ok(w) = cid.get(b"W") {
        let entries = resolve(doc, w)?
            .as_array()
            .map_err(|_| PdfError::Malformed)?;
        let mut index = 0usize;
        while index < entries.len() {
            ctx.check()?;
            if ranges.len() >= MAX_WIDTH_ENTRIES || entries.len() > MAX_WIDTH_ENTRIES {
                return Err(PdfError::Malformed);
            }
            let start = cid_code(doc, &entries[index])?;
            index += 1;
            let next = resolve(doc, entries.get(index).ok_or(PdfError::Malformed)?)?;
            index += 1;
            if let Object::Array(values) = next {
                if values.is_empty()
                    || ranges.len().saturating_add(values.len()) > MAX_WIDTH_ENTRIES
                {
                    return Err(PdfError::Malformed);
                }
                for (offset, value) in values.iter().enumerate() {
                    let code = start
                        .checked_add(u32::try_from(offset).map_err(|_| PdfError::Malformed)?)
                        .filter(|code| *code <= 65535)
                        .ok_or(PdfError::Malformed)?;
                    ranges.push((code, code, number(doc, value)?));
                }
            } else {
                let end = cid_code(doc, next)?;
                if end < start {
                    return Err(PdfError::Malformed);
                }
                let width = number(doc, entries.get(index).ok_or(PdfError::Malformed)?)?;
                index += 1;
                ranges.push((start, end, width));
            }
        }
    }
    ranges.sort_by_key(|range| range.0);
    if ranges.windows(2).any(|pair| pair[0].1 >= pair[1].0) {
        return Err(PdfError::Malformed);
    }
    let mut result = Vec::with_capacity(glyphs.len());
    for (index, glyph) in glyphs.iter().enumerate() {
        if index % 128 == 0 {
            ctx.check()?;
        }
        let code = encoded
            .get(glyph.start..glyph.end)
            .ok_or(PdfError::Malformed)?;
        let [high, low] = code else {
            return Err(unsupported());
        };
        let code = u32::from(u16::from_be_bytes([*high, *low]));
        let idx = ranges.partition_point(|range| range.0 <= code);
        let width = idx
            .checked_sub(1)
            .and_then(|i| ranges.get(i))
            .filter(|range| code <= range.1)
            .map_or(default, |range| range.2);
        result.push(width);
    }
    Ok(result)
}

fn resolve<'a>(doc: &'a Document, value: &'a Object) -> Result<&'a Object, PdfError> {
    doc.dereference(value)
        .map(|(_, value)| value)
        .map_err(|_| PdfError::Malformed)
}
fn name<'a>(doc: &'a Document, dict: &'a Dictionary, key: &[u8]) -> Result<&'a [u8], PdfError> {
    resolve(doc, dict.get(key).map_err(|_| PdfError::Malformed)?)?
        .as_name()
        .map_err(|_| PdfError::Malformed)
}
fn integer(doc: &Document, value: &Object) -> Result<i64, PdfError> {
    resolve(doc, value)?
        .as_i64()
        .map_err(|_| PdfError::Malformed)
}
fn cid_code(doc: &Document, value: &Object) -> Result<u32, PdfError> {
    let value = integer(doc, value)?;
    if !(0..=65535).contains(&value) {
        return Err(PdfError::Malformed);
    }
    Ok(value as u32)
}
fn number(doc: &Document, value: &Object) -> Result<f64, PdfError> {
    let result = match resolve(doc, value)? {
        Object::Integer(value) => *value as f64,
        Object::Real(value) => f64::from(*value),
        _ => return Err(PdfError::Malformed),
    };
    if !result.is_finite() || result.abs() > 1_000_000_000.0 {
        return Err(PdfError::Malformed);
    }
    Ok(result)
}
fn unsupported() -> PdfError {
    PdfError::NotImplemented("exact font metrics".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::dictionary;

    fn glyph(start: usize, end: usize) -> PdfDecodedGlyph {
        PdfDecodedGlyph {
            start,
            end,
            text: "x".into(),
        }
    }

    #[test]
    fn uses_declared_simple_and_type3_widths() {
        let doc = Document::with_version("1.7");
        let glyphs = [glyph(0, 1), glyph(1, 2)];
        let font = Object::Dictionary(
            dictionary! { "Subtype" => "Type3", "FirstChar" => 65, "Widths" => vec![500.into(), 600.into()], "FontMatrix" => vec![Object::Real(0.001), 0.into(), 0.into(), Object::Real(0.001), 0.into(), 0.into()] },
        );
        let values = widths(&doc, &font, b"AB", &glyphs, &ToolCtx::new()).unwrap();
        assert!((values[0] - 500.0).abs() < 0.001);
        assert!((values[1] - 600.0).abs() < 0.001);
        let missing = Object::Dictionary(dictionary! { "Subtype" => "Type1" });
        assert!(widths(&doc, &missing, b"A", &glyphs[..1], &ToolCtx::new()).is_err());
    }

    #[test]
    fn uses_cid_defaults_and_rejects_overlapping_widths() {
        let doc = Document::with_version("1.7");
        let glyphs = [glyph(0, 2), glyph(2, 4), glyph(4, 6)];
        let cid = dictionary! { "Subtype" => "CIDFontType2", "DW" => 900, "W" => vec![1.into(), Object::Array(vec![500.into(), 600.into()]), 3.into(), 4.into(), 700.into()] };
        let font = Object::Dictionary(
            dictionary! { "Subtype" => "Type0", "Encoding" => "Identity-H", "DescendantFonts" => vec![Object::Dictionary(cid)] },
        );
        assert_eq!(
            widths(&doc, &font, &[0, 1, 0, 3, 0, 5], &glyphs, &ToolCtx::new()).unwrap(),
            [500.0, 700.0, 900.0]
        );
        let cid = dictionary! { "Subtype" => "CIDFontType2", "W" => vec![1.into(), 3.into(), 500.into(), 3.into(), 4.into(), 600.into()] };
        let font = Object::Dictionary(
            dictionary! { "Subtype" => "Type0", "Encoding" => "Identity-H", "DescendantFonts" => vec![Object::Dictionary(cid)] },
        );
        assert!(widths(&doc, &font, &[0, 1], &glyphs[..1], &ToolCtx::new()).is_err());
    }
}
