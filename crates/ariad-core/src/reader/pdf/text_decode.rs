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

/// One mapped glyph with its exact byte range in a PDF text-show string.
///
/// Text can contain sensitive input and is intentionally omitted from debug output.
#[derive(Clone, PartialEq, Eq)]
pub struct PdfDecodedGlyph {
    /// Start offset into the encoded string, inclusive.
    pub start: usize,
    /// End offset into the encoded string, exclusive.
    pub end: usize,
    /// Unicode mapping for this character code.
    pub text: String,
}

impl std::fmt::Debug for PdfDecodedGlyph {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PdfDecodedGlyph")
            .field("start", &self.start)
            .field("end", &self.end)
            .field("text", &"[redacted]")
            .finish()
    }
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
        let Some(font) = self.load_font(object)? else {
            return Ok(None);
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

    /// Decodes each glyph with a byte range, refusing truncated or unmapped codes.
    pub fn decode_with_offsets(
        &mut self,
        object: &Object,
        bytes: &[u8],
    ) -> Result<Option<Vec<PdfDecodedGlyph>>, ReadError> {
        let Some(font) = self.load_font(object)? else {
            return Ok(None);
        };
        let mut glyphs = Vec::new();
        let mut offset = 0;
        while offset < bytes.len() {
            let Some((code, count)) = font.next_code_strict(&bytes[offset..]) else {
                return Ok(None);
            };
            self.budget.charge_work(16)?;
            let mut text = String::new();
            if !font.decode_strict(code, count, &mut text) {
                return Ok(None);
            }
            self.budget
                .charge_work(std::mem::size_of::<PdfDecodedGlyph>().saturating_add(text.len()))?;
            glyphs.push(PdfDecodedGlyph {
                start: offset,
                end: offset + count,
                text,
            });
            offset += count;
        }
        Ok(Some(glyphs))
    }

    fn load_font(&mut self, object: &Object) -> Result<Option<Rc<Font>>, ReadError> {
        let Some(dict) = resolve(self.doc, object).and_then(|o| o.as_dict().ok()) else {
            return Ok(None);
        };
        let key = object.as_reference().ok();
        if let Some(font) = key.and_then(|key| self.fonts.get(&key)) {
            return Ok(Some(Rc::clone(font)));
        }
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
        Ok(Some(font))
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
        let glyphs = decoder.decode_with_offsets(&font, b"A").unwrap().unwrap();
        assert_eq!(
            (glyphs[0].start, glyphs[0].end, glyphs[0].text.as_str()),
            (0, 1, "ạ")
        );
        assert!(!format!("{:?}", glyphs[0]).contains('ạ'));
        assert_eq!(decoder.decode(&font, b"B").unwrap().as_deref(), Some("B"));
        assert!(decoder.decode_with_offsets(&font, b"B").unwrap().is_none());
    }

    #[test]
    fn strict_decoder_rejects_truncated_two_byte_codes() {
        let doc = Document::with_version("1.7");
        let font = Object::Dictionary(
            dictionary! { "Type" => "Font", "Subtype" => "Type0", "Encoding" => "Identity-H" },
        );
        let mut decoder = PdfTextDecoder::new(&doc, 1024, &Limits::local());
        assert!(decoder.decode_with_offsets(&font, &[0]).unwrap().is_none());
    }

    #[test]
    fn strict_decoder_retains_multibyte_and_multiscalar_ranges() {
        let mut doc = Document::with_version("1.7");
        let cmap = doc.add_object(Stream::new(dictionary! {}, b"1 begincodespacerange <0000> <FFFF> endcodespacerange 2 beginbfchar <0041> <00660069> <0042> <0042> endbfchar".to_vec()));
        let font = Object::Dictionary(
            dictionary! { "Type" => "Font", "Subtype" => "Type0", "Encoding" => "Identity-H", "ToUnicode" => cmap },
        );
        let mut decoder = PdfTextDecoder::new(&doc, 1024, &Limits::local());
        let glyphs = decoder
            .decode_with_offsets(&font, &[0, 0x41, 0, 0x42])
            .unwrap()
            .unwrap();
        assert_eq!(
            (glyphs[0].start, glyphs[0].end, glyphs[0].text.as_str()),
            (0, 2, "fi")
        );
        assert_eq!(
            (glyphs[1].start, glyphs[1].end, glyphs[1].text.as_str()),
            (2, 4, "B")
        );
        assert!(
            decoder
                .decode_with_offsets(&font, &[0, 0x41, 0])
                .unwrap()
                .is_none()
        );
    }
}
