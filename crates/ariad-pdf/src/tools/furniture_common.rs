//! Shared page-label formatting for page furniture and contents pages.

use crate::{
    PdfError, ToolCtx,
    tools::{
        outline::decode_pdf_object_string,
        page_set::{self, PageLabelRule},
    },
};
use lopdf::Document;
use std::collections::BTreeMap;
use unicode_normalization::UnicodeNormalization;

pub(super) fn labels(
    doc: &Document,
    ctx: &ToolCtx,
) -> Result<BTreeMap<u32, PageLabelRule>, PdfError> {
    page_set::parse_page_labels(doc, ctx)
}
pub(super) fn label(rules: &BTreeMap<u32, PageLabelRule>, page: u32) -> Result<String, PdfError> {
    let index = page.checked_sub(1).ok_or(PdfError::Malformed)?;
    let Some((&first, rule)) = rules.range(..=index).next_back() else {
        return Ok(page.to_string());
    };
    let prefix = rule
        .prefix
        .as_ref()
        .and_then(decode_pdf_object_string)
        .unwrap_or_default();
    let number = rule
        .start
        .checked_add(i64::from(index - first))
        .filter(|v| *v > 0)
        .ok_or_else(|| PdfError::InvalidParams("invalid page-label sequence".into()))?;
    let number = u64::try_from(number).map_err(|_| PdfError::Malformed)?;
    let suffix = match rule.style.as_deref() {
        None => String::new(),
        Some(b"D") => number.to_string(),
        Some(b"R") => roman(number)?,
        Some(b"r") => roman(number)?.to_ascii_lowercase(),
        Some(b"A") => alpha(number, false)?,
        Some(b"a") => alpha(number, true)?,
        Some(_) => {
            return Err(PdfError::InvalidParams(
                "unsupported page-label style".into(),
            ));
        }
    };
    let result: String = format!("{prefix}{suffix}").nfc().collect();
    if result.chars().count() > 4096 {
        return Err(PdfError::InvalidParams("page label is too long".into()));
    }
    Ok(result)
}
fn roman(mut value: u64) -> Result<String, PdfError> {
    if value > 3_999_999 {
        return Err(PdfError::InvalidParams(
            "roman page label is too long".into(),
        ));
    }
    let mut output = String::new();
    for (number, digits) in [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ] {
        while value >= number {
            output.push_str(digits);
            value -= number;
        }
    }
    if output.len() > 4096 {
        return Err(PdfError::InvalidParams(
            "roman page label is too long".into(),
        ));
    }
    Ok(output)
}
fn alpha(value: u64, lowercase: bool) -> Result<String, PdfError> {
    let copies = (value - 1) / 26 + 1;
    if copies > 4096 {
        return Err(PdfError::InvalidParams(
            "alphabetic page label is too long".into(),
        ));
    }
    let letter = (b'A' + ((value - 1) % 26) as u8) as char;
    Ok(std::iter::repeat_n(
        if lowercase {
            letter.to_ascii_lowercase()
        } else {
            letter
        },
        copies as usize,
    )
    .collect())
}

pub(super) fn pagination(anchor: ariad_core::tools::params::Anchor) -> crate::draw::layer::Marking {
    use ariad_core::tools::params::Anchor;
    match anchor {
        Anchor::TopLeft | Anchor::TopCenter | Anchor::TopRight => {
            crate::draw::layer::Marking::Header
        }
        _ => crate::draw::layer::Marking::Footer,
    }
}

/// Check the token set without interpreting escaped double braces.
pub(super) fn check_tokens(template: &str, allowed: &[&str]) -> Result<(), PdfError> {
    let mut chars = template.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
            }
            '}' if chars.peek() == Some(&'}') => {
                chars.next();
            }
            '{' => {
                let mut token = String::new();
                loop {
                    match chars.next() {
                        Some('}') => break,
                        Some('{') | None => {
                            return Err(PdfError::InvalidParams("malformed template token".into()));
                        }
                        Some(c) => token.push(c),
                    }
                }
                if !allowed.contains(&token.as_str()) {
                    return Err(PdfError::InvalidParams("unsupported template token".into()));
                }
            }
            '}' => return Err(PdfError::InvalidParams("malformed template token".into())),
            _ => {}
        }
    }
    Ok(())
}
