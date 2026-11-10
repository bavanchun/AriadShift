//! Checks that a candidate rewrite has one fresh cross-reference history.

use std::collections::BTreeSet;

use ariad_core::{
    limits::Limits,
    tools::params::{
        redact::{RedactParams, RedactionTarget},
        redact_internal::RedactionCheckStatus,
    },
};
use unicode_normalization::UnicodeNormalization;

use crate::{PdfError, ToolCtx};

const MAX_RAW_WORK: usize = 256 * 1024 * 1024;

pub(super) fn raw_literals(
    bytes: &[u8],
    params: &RedactParams,
    ctx: &ToolCtx,
) -> Result<RedactionCheckStatus, PdfError> {
    let mut texts = Vec::new();
    for target in &params.targets {
        if let RedactionTarget::Exact { text } = target {
            texts.push(text.clone());
        }
    }
    raw_literals_for_texts(bytes, params, &texts, ctx)
}

pub(super) fn raw_literals_for_texts(
    bytes: &[u8],
    params: &RedactParams,
    texts: &[String],
    ctx: &ToolCtx,
) -> Result<RedactionCheckStatus, PdfError> {
    use RedactionCheckStatus::{Fail, Pass, Unavailable};
    params
        .validate()
        .map_err(|e| PdfError::InvalidParams(e.to_string()))?;
    if !params.scope.is_complete() || texts.is_empty() {
        return Ok(Unavailable);
    }
    let mut needles = BTreeSet::new();
    for text in texts {
        if text.is_empty() {
            continue;
        }
        let normalized = text.nfc().collect::<String>();
        for value in [text.as_str(), normalized.as_str()] {
            let mut le = Vec::new();
            let mut be = Vec::new();
            for unit in value.encode_utf16() {
                le.extend_from_slice(&unit.to_le_bytes());
                be.extend_from_slice(&unit.to_be_bytes());
            }
            for raw in [value.as_bytes().to_vec(), le, be] {
                let mut lower = Vec::with_capacity(raw.len() * 2);
                for &byte in &raw {
                    lower.push(b"0123456789abcdef"[(byte >> 4) as usize]);
                    lower.push(b"0123456789abcdef"[(byte & 15) as usize]);
                }
                needles.insert((raw, false));
                needles.insert((lower, true));
            }
        }
    }
    let mut work = 0usize;
    for (needle, fold_ascii) in needles {
        ctx.check()?;
        work = work
            .checked_add(bytes.len().saturating_mul(2))
            .ok_or(PdfError::Malformed)?;
        if work > MAX_RAW_WORK {
            return Err(PdfError::InvalidParams(
                "raw verification work limit exceeded".into(),
            ));
        }
        if contains(bytes, &needle, fold_ascii, ctx)? {
            return Ok(Fail);
        }
    }
    Ok(Pass)
}

fn contains(
    haystack: &[u8],
    needle: &[u8],
    fold_ascii: bool,
    ctx: &ToolCtx,
) -> Result<bool, PdfError> {
    if needle.is_empty() {
        return Err(PdfError::Malformed);
    }
    let mut prefix = vec![0usize; needle.len()];
    let mut matched = 0usize;
    for index in 1..needle.len() {
        while matched > 0 && needle[matched] != needle[index] {
            matched = prefix[matched - 1];
        }
        if needle[matched] == needle[index] {
            matched += 1;
        }
        prefix[index] = matched;
    }
    matched = 0;
    for (index, &byte) in haystack.iter().enumerate() {
        let byte = if fold_ascii {
            byte.to_ascii_lowercase()
        } else {
            byte
        };
        if index.is_multiple_of(4096) {
            ctx.check()?;
        }
        while matched > 0 && needle[matched] != byte {
            matched = prefix[matched - 1];
        }
        if needle[matched] == byte {
            matched += 1;
        }
        if matched == needle.len() {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(super) fn verify(bytes: &[u8], limits: &Limits, ctx: &ToolCtx) -> Result<bool, PdfError> {
    ctx.check()?;
    let doc = crate::document::open_with_ctx(bytes, limits, ctx)?;
    if doc.repaired_xref()
        || doc.inner().trailer.has(b"Prev")
        || doc.inner().trailer.has(b"XRefStm")
        || !bytes.starts_with(b"%PDF-")
    {
        return Ok(false);
    }
    let Some(eof) = unique_marker(bytes, b"%%EOF", ctx)? else {
        return Ok(false);
    };
    if bytes[eof + 5..].iter().any(|b| !whitespace(*b)) {
        return Ok(false);
    }
    let Some(startxref) = unique_marker(bytes, b"startxref", ctx)? else {
        return Ok(false);
    };
    if startxref >= eof {
        return Ok(false);
    }
    let raw = &bytes[startxref + 9..eof];
    let raw = raw.trim_ascii();
    let Ok(raw) = std::str::from_utf8(raw) else {
        return Ok(false);
    };
    let Ok(offset) = raw.parse::<usize>() else {
        return Ok(false);
    };
    if offset >= startxref {
        return Ok(false);
    }
    let xref = &bytes[offset..startxref];
    // Traditional tables start with xref. Stream-based tables start with an
    // indirect object header, and were already parsed by the bounded reader.
    Ok(xref.starts_with(b"xref") || object_header(xref))
}

fn unique_marker(bytes: &[u8], marker: &[u8], ctx: &ToolCtx) -> Result<Option<usize>, PdfError> {
    let mut found = None;
    for (index, part) in bytes.windows(marker.len()).enumerate() {
        if index.is_multiple_of(65_536) {
            ctx.check()?;
        }
        if part == marker && found.replace(index).is_some() {
            return Ok(None);
        }
    }
    Ok(found)
}

fn whitespace(byte: u8) -> bool {
    matches!(byte, 0 | 9 | 10 | 12 | 13 | 32)
}

fn object_header(bytes: &[u8]) -> bool {
    let end = bytes
        .iter()
        .position(|byte| matches!(byte, b'\r' | b'\n'))
        .unwrap_or(bytes.len())
        .min(64);
    let Ok(header) = std::str::from_utf8(&bytes[..end]) else {
        return false;
    };
    let mut words = header.split_ascii_whitespace();
    words.next().is_some_and(|n| n.parse::<u32>().is_ok())
        && words.next().is_some_and(|n| n.parse::<u16>().is_ok())
        && words.next() == Some("obj")
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::{Document, Object, dictionary};

    fn pdf() -> Vec<u8> {
        let mut doc = Document::with_version("1.7");
        let pages = doc.new_object_id();
        let page = doc.add_object(dictionary! { "Type" => "Page", "Parent" => pages, "MediaBox" => vec![0.into(), 0.into(), 100.into(), 100.into()] });
        doc.objects.insert(pages, Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => vec![Object::Reference(page)], "Count" => 1 }));
        let root = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
        doc.trailer.set("Root", root);
        crate::writer::save_document_with_options(&mut doc, &ToolCtx::new(), &Default::default())
            .unwrap()
    }

    #[test]
    fn rejects_prior_eof_and_trailing_payload() {
        let bytes = pdf();
        assert!(verify(&bytes, &Limits::local(), &ToolCtx::new()).unwrap());
        let mut trailing = bytes.clone();
        trailing.extend_from_slice(b"private history");
        assert!(!verify(&trailing, &Limits::local(), &ToolCtx::new()).unwrap_or(false));
        let mut previous = bytes.clone();
        previous.extend_from_slice(&bytes);
        assert!(!verify(&previous, &Limits::local(), &ToolCtx::new()).unwrap_or(false));
    }

    #[test]
    fn checks_raw_and_hex_literal_encodings_without_reporting_target_values() {
        use ariad_core::tools::params::redact::RedactionScope;
        let mut request = RedactParams {
            targets: vec![RedactionTarget::Exact { text: "é".into() }],
            color: None,
            overlay_text: None,
            scope: RedactionScope::default(),
            invalidate_signatures: false,
        };
        for bytes in [b"prefix\xc3\xa9".as_slice(), b"prefix\x00\xe9", b"<C3a9>"] {
            assert_eq!(
                raw_literals(bytes, &request, &ToolCtx::new()).unwrap(),
                RedactionCheckStatus::Fail
            );
        }
        assert_eq!(
            raw_literals(b"unrelated", &request, &ToolCtx::new()).unwrap(),
            RedactionCheckStatus::Pass
        );
        request.targets = vec![RedactionTarget::Regex {
            pattern: "secret".into(),
        }];
        assert_eq!(
            raw_literals(b"unrelated", &request, &ToolCtx::new()).unwrap(),
            RedactionCheckStatus::Unavailable
        );
    }
}
