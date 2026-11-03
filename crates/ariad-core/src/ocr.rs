//! Engine-neutral OCR results in upright image coordinates.

use std::io::{self, Write};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;

/// Maximum number of recognized words on a page.
pub const MAX_OCR_WORDS_PER_PAGE: usize = 50_000;
/// Maximum UTF-8 length of one word.
pub const MAX_OCR_WORD_BYTES: usize = 256;
/// Maximum size of a serialized OCR result.
pub const MAX_OCR_JSON_BYTES: usize = 16 * 1024 * 1024;

/// Pixel bounds with a top-left origin and exclusive right and bottom edges.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OcrBox {
    pub x0: u32,
    pub y0: u32,
    pub x1: u32,
    pub y1: u32,
}

/// Recognized word and its confidence on a scale of 0 to 100.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OcrWord {
    pub bbox: OcrBox,
    pub text: String,
    pub confidence: f32,
}

/// Baseline relative to the line's bounding box.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OcrBaseline {
    pub slope: f32,
    pub offset_px: f32,
}

/// A line of recognized words.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OcrLine {
    pub bbox: OcrBox,
    pub baseline: Option<OcrBaseline>,
    pub words: Vec<OcrWord>,
}

/// A paragraph of recognized lines.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OcrParagraph {
    pub bbox: OcrBox,
    pub lines: Vec<OcrLine>,
}

/// OCR result for an image after the indicated clockwise rotation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OcrPage {
    pub width_px: u32,
    pub height_px: u32,
    pub dpi: u32,
    pub rotation_cw: u16,
    pub languages: Vec<String>,
    pub paragraphs: Vec<OcrParagraph>,
}

/// Orientation and optional script detection result.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OcrOrientation {
    pub rotation_cw: u16,
    pub confidence: f32,
    pub script: Option<String>,
    pub script_confidence: Option<f32>,
}

/// An invalid or oversized OCR page.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum OcrValidationError {
    #[error("invalid image dimensions, resolution, or rotation")]
    Image,
    #[error("OCR box is empty or outside the image")]
    Box,
    #[error("invalid OCR text")]
    Text,
    #[error("invalid OCR numeric value")]
    Number,
    #[error("OCR result exceeds a size limit")]
    Limit,
}

impl OcrPage {
    /// Validate bounds, text and size before consuming a recognition result.
    pub fn validate(&self) -> Result<(), OcrValidationError> {
        if self.width_px == 0
            || self.height_px == 0
            || self.dpi == 0
            || !matches!(self.rotation_cw, 0 | 90 | 180 | 270)
        {
            return Err(OcrValidationError::Image);
        }
        let check_box = |b: &OcrBox| {
            if b.x0 >= b.x1 || b.y0 >= b.y1 || b.x1 > self.width_px || b.y1 > self.height_px {
                Err(OcrValidationError::Box)
            } else {
                Ok(())
            }
        };
        let mut count = 0usize;
        for paragraph in &self.paragraphs {
            check_box(&paragraph.bbox)?;
            for line in &paragraph.lines {
                check_box(&line.bbox)?;
                if line
                    .baseline
                    .as_ref()
                    .is_some_and(|b| !b.slope.is_finite() || !b.offset_px.is_finite())
                {
                    return Err(OcrValidationError::Number);
                }
                for word in &line.words {
                    count += 1;
                    if count > MAX_OCR_WORDS_PER_PAGE {
                        return Err(OcrValidationError::Limit);
                    }
                    check_box(&word.bbox)?;
                    if word.text.is_empty()
                        || word.text.len() > MAX_OCR_WORD_BYTES
                        || !word.text.nfc().eq(word.text.chars())
                    {
                        return Err(OcrValidationError::Text);
                    }
                    if !word.confidence.is_finite() || !(0.0..=100.0).contains(&word.confidence) {
                        return Err(OcrValidationError::Number);
                    }
                }
            }
        }
        let mut counter = LimitedCounter(0);
        serde_json::to_writer(&mut counter, self).map_err(|_| OcrValidationError::Limit)?;
        Ok(())
    }
}

// Count serialized bytes without allocating another copy of an untrusted result.
struct LimitedCounter(usize);

impl Write for LimitedCounter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0 = self
            .0
            .checked_add(bytes.len())
            .ok_or_else(|| io::Error::other("OCR result too large"))?;
        if self.0 > MAX_OCR_JSON_BYTES {
            return Err(io::Error::other("OCR result too large"));
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page() -> OcrPage {
        let bbox = OcrBox {
            x0: 0,
            y0: 0,
            x1: 10,
            y1: 10,
        };
        OcrPage {
            width_px: 10,
            height_px: 10,
            dpi: 300,
            rotation_cw: 0,
            languages: vec!["vie".into()],
            paragraphs: vec![OcrParagraph {
                bbox: bbox.clone(),
                lines: vec![OcrLine {
                    bbox: bbox.clone(),
                    baseline: None,
                    words: vec![OcrWord {
                        bbox,
                        text: "Việt".into(),
                        confidence: 99.0,
                    }],
                }],
            }],
        }
    }

    #[test]
    fn rejects_word_and_serialized_size_limits() {
        let mut value = page();
        let word = value.paragraphs[0].lines[0].words[0].clone();
        value.paragraphs[0].lines[0].words = vec![word; MAX_OCR_WORDS_PER_PAGE + 1];
        assert_eq!(value.validate(), Err(OcrValidationError::Limit));
        let mut value = page();
        value.languages = vec!["x".repeat(MAX_OCR_JSON_BYTES)];
        assert_eq!(value.validate(), Err(OcrValidationError::Limit));
        let mut value = page();
        value.paragraphs[0].lines[0].words[0].text = "x".repeat(MAX_OCR_WORD_BYTES + 1);
        assert_eq!(value.validate(), Err(OcrValidationError::Text));
    }

    #[test]
    fn validates_and_round_trips_results() {
        let value = page();
        assert_eq!(value.validate(), Ok(()));
        let json = serde_json::to_vec(&value).unwrap();
        assert_eq!(serde_json::from_slice::<OcrPage>(&json).unwrap(), value);
        let mut invalid = value.clone();
        invalid.paragraphs[0].lines[0].words[0].text = "e\u{301}".into();
        assert_eq!(invalid.validate(), Err(OcrValidationError::Text));
        invalid = value.clone();
        invalid.paragraphs[0].lines[0].words[0].bbox.x1 = 11;
        assert_eq!(invalid.validate(), Err(OcrValidationError::Box));
        invalid = value;
        invalid.paragraphs[0].lines[0].words[0].confidence = f32::NAN;
        assert_eq!(invalid.validate(), Err(OcrValidationError::Number));
    }
}
