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
}
