//! Bounded, reusable decoding of PDF text-show strings.

use std::{collections::HashMap, rc::Rc};

use lopdf::{Document, Object, ObjectId};

use super::{
    CONTENT_BUDGET_BASE, CONTENT_BUDGET_PER_INPUT_BYTE, ReadError,
    encoding::Encodings,
    font::{CMapCache, Font},
    objects::{Budget, resolve},
};
use crate::limits::Limits;

/// Decodes text strings using the reader's bounded font and ToUnicode implementation.
pub struct PdfTextDecoder<'a> {
    doc: &'a Document,
    budget: Budget,
    fonts: HashMap<ObjectId, Rc<Font>>,
    encodings: Encodings,
    cmaps: CMapCache,
}

impl<'a> PdfTextDecoder<'a> {
    /// Creates a decoder for a document, under the reader's input amplification limits.
    #[must_use]
    pub fn new(doc: &'a Document, input_len: usize, limits: &Limits) -> Self {
        let work = CONTENT_BUDGET_BASE
            .saturating_add(CONTENT_BUDGET_PER_INPUT_BYTE.saturating_mul(input_len as u64))
            .min((usize::MAX / 4) as u64);
        Self {
            doc,
            budget: Budget::new(limits.max_decompressed_bytes, work),
            fonts: HashMap::new(),
            encodings: Encodings::default(),
            cmaps: CMapCache::new(),
        }
    }

    /// Returns decoded text, or `None` if any character lacks a Unicode mapping.
    pub fn decode(&mut self, object: &Object, bytes: &[u8]) -> Result<Option<String>, ReadError> {
        let Some(dict) = resolve(self.doc, object).and_then(|o| o.as_dict().ok()) else {
            return Ok(None);
        };
        let key = object.as_reference().ok();
        let font = if let Some(font) = key.and_then(|key| self.fonts.get(&key)) {
            Rc::clone(font)
        } else {
            let font = Font::load(
                self.doc,
                dict,
                &mut self.budget,
                &mut self.encodings,
                &mut self.cmaps,
            )?;
            self.budget.charge_work(font.work_bytes())?;
            let font = Rc::new(font);
            if let Some(key) = key {
                self.fonts.insert(key, Rc::clone(&font));
            }
            font
        };
        let mut remaining = bytes;
        let mut decoded = String::new();
        while !remaining.is_empty() {
            let (code, count) = font.next_code(remaining);
            self.budget.charge_work(16)?;
            let previous = decoded.len();
            if !font.decode(code, count, &mut decoded) {
                return Ok(None);
            }
            self.budget.charge_work(decoded.len() - previous)?;
            remaining = &remaining[count..];
        }
        Ok(Some(decoded))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::{Stream, dictionary};

    #[test]
    fn uses_to_unicode() {
        let mut doc = Document::with_version("1.7");
        let cmap = doc.add_object(Stream::new(dictionary! {}, b"1 begincodespacerange <00> <FF> endcodespacerange 1 beginbfchar <41> <1EA1> endbfchar".to_vec()));
        let font = Object::Dictionary(
            dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica", "ToUnicode" => cmap },
        );
        let mut decoder = PdfTextDecoder::new(&doc, 1024, &Limits::local());
        assert_eq!(decoder.decode(&font, b"A").unwrap().as_deref(), Some("ạ"));
    }
}
