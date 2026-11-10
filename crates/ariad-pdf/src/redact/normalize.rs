//! Unicode normalization with conservative mapping back to original byte ranges.

use unicode_normalization::{
    UnicodeNormalization,
    char::{canonical_combining_class, decompose_canonical},
};

const MAX_NORMALIZED_BYTES: usize = 16 * 1024 * 1024;

fn map_char(c: char) -> char {
    match c {
        '\u{FF10}'..='\u{FF19}' => (b'0' + (c as u32 - 0xFF10) as u8) as char,
        '\u{00A0}' => ' ',
        '\u{2012}' | '\u{2013}' | '\u{2014}' | '\u{2212}' => '-',
        _ => c,
    }
}

fn needs_mapping(c: char) -> bool {
    matches!(
        c,
        '\u{FF10}'..='\u{FF19}' | '\u{00A0}' | '\u{2012}' | '\u{2013}' | '\u{2014}' | '\u{2212}'
    )
}

#[derive(Clone, Copy)]
struct Part {
    character: char,
    start: usize,
    end: usize,
}

/// Normalized text and the input range contributing to each normalized scalar.
pub(super) struct Normalized {
    pub text: String,
    spans: Vec<(usize, usize, usize)>,
    identity: bool,
}

impl Normalized {
    pub fn new(input: &str) -> Result<Self, &'static str> {
        if input.len() > MAX_NORMALIZED_BYTES {
            return Err("searchable text limit exceeded");
        }
        let has_custom_mappings = input.chars().any(needs_mapping);
        if !has_custom_mappings && unicode_normalization::is_nfc(input) {
            return Ok(Self {
                text: input.to_owned(),
                spans: Vec::new(),
                identity: true,
            });
        }
        let mut parts = Vec::new();
        for (start, character) in input.char_indices() {
            let end = start + character.len_utf8();
            let mapped = map_char(character);
            decompose_canonical(mapped, |c| {
                parts.push(Part {
                    character: c,
                    start,
                    end,
                })
            });
            if parts.len() > MAX_NORMALIZED_BYTES {
                return Err("normalized text limit exceeded");
            }
        }
        canonical_order(&mut parts);
        let mapped_input: String = input.chars().map(map_char).collect();
        let text: String = mapped_input.nfc().collect();
        if text.len() > MAX_NORMALIZED_BYTES {
            return Err("normalized text limit exceeded");
        }
        let mut spans = Vec::new();
        let mut output_parts = Vec::new();
        for (offset, character) in text.char_indices() {
            let index = spans.len();
            spans.push((offset, usize::MAX, 0));
            decompose_canonical(character, |decomposed| {
                output_parts.push(Part {
                    character: decomposed,
                    start: index,
                    end: 0,
                });
            });
        }
        canonical_order(&mut output_parts);
        if output_parts.len() != parts.len() {
            return Err("unable to map normalized text");
        }
        for (input, output) in parts.iter().zip(output_parts) {
            if input.character != output.character {
                return Err("unable to map normalized text");
            }
            let span = &mut spans[output.start];
            span.1 = span.1.min(input.start);
            span.2 = span.2.max(input.end);
        }
        Ok(Self {
            text,
            spans,
            identity: false,
        })
    }

    /// Includes every original scalar contributing to the normalized range.
    pub fn original_range(&self, start: usize, end: usize) -> Result<(usize, usize), &'static str> {
        if start >= end || !self.text.is_char_boundary(start) || !self.text.is_char_boundary(end) {
            return Err("invalid normalized match range");
        }
        if self.identity {
            return Ok((start, end));
        }
        let first = self.spans.partition_point(|span| span.0 < start);
        let last = self.spans.partition_point(|span| span.0 < end);
        let selected = &self.spans[first..last];
        let from = selected
            .iter()
            .map(|span| span.1)
            .min()
            .ok_or("invalid normalized match range")?;
        let to = selected
            .iter()
            .map(|span| span.2)
            .max()
            .ok_or("invalid normalized match range")?;
        Ok((from, to))
    }
}

// Canonical ordering is stable within each run of non-starters. Counting sort
// bounds work independently of adversarially long combining sequences.
fn canonical_order(parts: &mut [Part]) {
    let mut first = 0;
    while first < parts.len() {
        if canonical_combining_class(parts[first].character) == 0 {
            first += 1;
        }
        let mut end = first;
        while end < parts.len() && canonical_combining_class(parts[end].character) != 0 {
            end += 1;
        }
        if end - first > 1 {
            let mut counts = [0usize; 256];
            for part in &parts[first..end] {
                counts[usize::from(canonical_combining_class(part.character))] += 1;
            }
            let mut positions = [0usize; 256];
            for i in 1..256 {
                positions[i] = positions[i - 1] + counts[i - 1];
            }
            let mut ordered = vec![parts[first]; end - first];
            for part in &parts[first..end] {
                let class = usize::from(canonical_combining_class(part.character));
                ordered[positions[class]] = *part;
                positions[class] += 1;
            }
            parts[first..end].copy_from_slice(&ordered);
        }
        first = end;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_composed_vietnamese_and_hangul_to_original_bytes() {
        let original = "A a\u{306}\u{301} \u{1100}\u{1161} Z";
        let value = Normalized::new(original).unwrap();
        assert_eq!(value.text, "A ắ 가 Z");
        let start = value.text.find('ắ').unwrap();
        let (from, to) = value.original_range(start, start + 'ắ'.len_utf8()).unwrap();
        assert_eq!(&original[from..to], "a\u{306}\u{301}");
        let start = value.text.find('가').unwrap();
        let (from, to) = value
            .original_range(start, start + '가'.len_utf8())
            .unwrap();
        assert_eq!(&original[from..to], "\u{1100}\u{1161}");
    }

    #[test]
    fn already_normalized_text_keeps_direct_byte_offsets() {
        let value = Normalized::new("x ắ y").unwrap();
        assert!(value.identity);
        let start = value.text.find('ắ').unwrap();
        assert_eq!(
            value.original_range(start, start + 'ắ'.len_utf8()).unwrap(),
            (start, start + 'ắ'.len_utf8())
        );
    }

    #[test]
    fn maps_fullwidth_digits_nbsp_and_dashes() {
        let original = "\u{FF11}\u{FF12}\u{FF13}\u{00A0}\u{FF14}\u{FF15}\u{FF16}\u{2013}\u{FF17}\u{FF18}\u{FF19}";
        let value = Normalized::new(original).unwrap();
        assert_eq!(value.text, "123 456-789");
        assert!(!value.identity);
        let (from, to) = value.original_range(0, 3).unwrap();
        assert_eq!(&original[from..to], "\u{FF11}\u{FF12}\u{FF13}");
        let (from_dash, to_dash) = value.original_range(7, 8).unwrap();
        assert_eq!(&original[from_dash..to_dash], "\u{2013}");
    }

    #[test]
    fn maps_reordered_combining_marks_conservatively() {
        let original = "a\u{315}\u{300}";
        let value = Normalized::new(original).unwrap();
        assert_eq!(value.text, "à\u{315}");
        let (from, to) = value.original_range(0, 'à'.len_utf8()).unwrap();
        assert_eq!(&original[from..to], original);
        let original = "a\u{327}\u{300}";
        let value = Normalized::new(original).unwrap();
        assert_eq!(value.text, "à\u{327}");
        let (from, to) = value.original_range(0, 'à'.len_utf8()).unwrap();
        assert_eq!(&original[from..to], original);
    }
}
