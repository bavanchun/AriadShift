//! PDF text-string encoding shared by form and interchange writers.

use std::{collections::HashMap, sync::OnceLock};

use lopdf::{Object, StringFormat};

fn pdf_doc_encoding() -> &'static HashMap<char, u8> {
    static REVERSE: OnceLock<HashMap<char, u8>> = OnceLock::new();
    REVERSE.get_or_init(|| {
        let mut reverse = HashMap::new();
        for byte in 0u8..=u8::MAX {
            let object = Object::String(vec![byte], StringFormat::Literal);
            if let Ok(decoded) = lopdf::decode_text_string(&object) {
                let mut chars = decoded.chars();
                if let (Some(ch), None) = (chars.next(), chars.next()) {
                    if ch != char::REPLACEMENT_CHARACTER {
                        reverse.entry(ch).or_insert(byte);
                    }
                }
            }
        }
        reverse
    })
}

/// Encode using PDFDocEncoding when possible, otherwise UTF-16BE with its BOM.
pub(crate) fn encode_bytes(text: &str) -> Vec<u8> {
    let map = pdf_doc_encoding();
    let bytes = text
        .chars()
        .map(|c| map.get(&c).copied())
        .collect::<Option<Vec<_>>>();
    if let Some(bytes) = bytes {
        if !bytes.starts_with(&[0xfe, 0xff])
            && !bytes.starts_with(&[0xff, 0xfe])
            && !bytes.starts_with(&[0xef, 0xbb, 0xbf])
        {
            return bytes;
        }
    }
    let mut bytes = Vec::with_capacity(text.len().saturating_mul(2).saturating_add(2));
    bytes.extend_from_slice(&[0xfe, 0xff]);
    for unit in text.encode_utf16() {
        bytes.extend_from_slice(&unit.to_be_bytes());
    }
    bytes
}

/// Construct a PDF text string without relying on a viewer's default encoding.
#[cfg(any(feature = "codecs", test))]
pub(crate) fn encode(text: &str) -> Object {
    Object::String(encode_bytes(text), StringFormat::Hexadecimal)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_pdf_doc_encoding_and_unicode() {
        let simple = encode("café");
        assert!(!simple.as_str().unwrap().starts_with(&[0xfe, 0xff]));
        assert_eq!(lopdf::decode_text_string(&simple).unwrap(), "café");
        let vietnamese = encode("Tiếng Việt");
        assert!(vietnamese.as_str().unwrap().starts_with(&[0xfe, 0xff]));
        assert_eq!(
            lopdf::decode_text_string(&vietnamese).unwrap(),
            "Tiếng Việt"
        );
        let ambiguous = encode("þÿ");
        assert_eq!(lopdf::decode_text_string(&ambiguous).unwrap(), "þÿ");
    }
}
