//! Shared helpers for document readers.

use unicode_normalization::UnicodeNormalization;

use crate::{ir::Document, warning::Warning};

/// Result of reading a document into the IR.
#[derive(Clone, Debug, PartialEq)]
pub struct ReadOutput {
    pub document: Document,
    pub warnings: Vec<Warning>,
}

/// Normalizes a string slice into Unicode Normalization Form C (NFC).
#[must_use]
pub fn nfc(value: &str) -> String {
    value.nfc().collect()
}

/// Returns `Some(value.to_owned())` if non-empty, otherwise `None`.
#[must_use]
pub fn nonempty(value: &str) -> Option<String> {
    (!value.is_empty()).then(|| value.to_owned())
}

/// Sniffs image media type from magic bytes (PNG, JPEG, GIF, WEBP).
#[must_use]
pub fn sniff_image_type(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        Some("image/jpeg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nfc_normalizes_decomposed_unicode() {
        let decomposed = "e\u{0301}";
        assert_eq!(nfc(decomposed), "é");
    }

    #[test]
    fn nonempty_filters_empty_strings() {
        assert_eq!(nonempty(""), None);
        assert_eq!(nonempty("hello"), Some("hello".to_owned()));
    }

    #[test]
    fn sniff_image_type_identifies_formats() {
        assert_eq!(
            sniff_image_type(b"\x89PNG\r\n\x1a\n\x00\x00"),
            Some("image/png")
        );
        assert_eq!(
            sniff_image_type(b"\xff\xd8\xff\xe0\x00\x10JFIF"),
            Some("image/jpeg")
        );
        assert_eq!(sniff_image_type(b"GIF87a\x01\x00"), Some("image/gif"));
        assert_eq!(sniff_image_type(b"GIF89a\x01\x00"), Some("image/gif"));
        assert_eq!(
            sniff_image_type(b"RIFF\x00\x00\x00\x00WEBPVP8 "),
            Some("image/webp")
        );
        assert_eq!(sniff_image_type(b"<html>hello</html>"), None);
        assert_eq!(sniff_image_type(b""), None);
    }
}
