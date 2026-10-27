//! Lenient accessors over the `lopdf` object model and the PDF work budgets.

use lopdf::{DecompressError, Dictionary, Document, Object, Stream};

use super::ReadError;

/// Follows references (bounded by `lopdf`) to the object they name.
pub(super) fn resolve<'a>(doc: &'a Document, object: &'a Object) -> Option<&'a Object> {
    doc.dereference(object).ok().map(|(_, object)| object)
}

/// A dictionary entry with references followed.
pub(super) fn get<'a>(doc: &'a Document, dict: &'a Dictionary, key: &[u8]) -> Option<&'a Object> {
    resolve(doc, dict.get(key).ok()?)
}

pub(super) fn get_dict<'a>(
    doc: &'a Document,
    dict: &'a Dictionary,
    key: &[u8],
) -> Option<&'a Dictionary> {
    match get(doc, dict, key)? {
        Object::Dictionary(dict) => Some(dict),
        Object::Stream(stream) => Some(&stream.dict),
        _ => None,
    }
}

pub(super) fn get_array<'a>(
    doc: &'a Document,
    dict: &'a Dictionary,
    key: &[u8],
) -> Option<&'a [Object]> {
    get(doc, dict, key)?.as_array().ok().map(Vec::as_slice)
}

pub(super) fn get_name<'a>(
    doc: &'a Document,
    dict: &'a Dictionary,
    key: &[u8],
) -> Option<&'a [u8]> {
    get(doc, dict, key)?.as_name().ok()
}

pub(super) fn get_number(doc: &Document, dict: &Dictionary, key: &[u8]) -> Option<f64> {
    number(get(doc, dict, key)?)
}

/// A finite numeric value.
pub(super) fn number(object: &Object) -> Option<f64> {
    let value = match object {
        Object::Integer(value) => *value as f64,
        Object::Real(value) => f64::from(*value),
        _ => return None,
    };
    value.is_finite().then_some(value)
}

/// A rectangle `[x0 y0 x1 y1]`, normalized so that `x0 <= x1` and `y0 <= y1`.
pub(super) fn rectangle(doc: &Document, object: &Object) -> Option<[f64; 4]> {
    let array = resolve(doc, object)?.as_array().ok()?;
    if array.len() != 4 {
        return None;
    }
    let mut values = [0.0; 4];
    for (slot, item) in values.iter_mut().zip(array) {
        *slot = number(resolve(doc, item)?)?;
    }
    Some([
        values[0].min(values[2]),
        values[1].min(values[3]),
        values[0].max(values[2]),
        values[1].max(values[3]),
    ])
}

/// Work charged for each glyph the interpreter records, in bytes, on top of its text.
pub(super) const GLYPH_WORK: u64 = 16;

/// Charges decoded stream bytes against the configured decompression limit, and decoded bytes,
/// glyph work, font array walks and retained font allocations against an amplification limit
/// that grows with the input size.
#[derive(Debug)]
pub(super) struct Budget {
    decoded: u64,
    decoded_limit: u64,
    work: u64,
    work_limit: u64,
}

impl Budget {
    pub(super) const fn new(decoded_limit: u64, work_limit: u64) -> Self {
        Self {
            decoded: 0,
            decoded_limit,
            work: 0,
            work_limit,
        }
    }

    fn remaining(&self) -> u64 {
        self.decoded_limit
            .saturating_sub(self.decoded)
            .min(self.work_limit.saturating_sub(self.work))
    }

    fn exceeded(&self) -> Result<(), ReadError> {
        if self.decoded > self.decoded_limit {
            return Err(ReadError::DecompressedTooLarge {
                limit: self.decoded_limit,
            });
        }
        if self.work > self.work_limit {
            return Err(ReadError::DecompressedTooLarge {
                limit: self.work_limit,
            });
        }
        Ok(())
    }

    /// Records `bytes` of decoded stream data, failing once a total passes its limit.
    pub(super) fn charge(&mut self, bytes: usize) -> Result<(), ReadError> {
        self.decoded = self.decoded.saturating_add(bytes as u64);
        self.work = self.work.saturating_add(bytes as u64);
        self.exceeded()
    }

    /// Charges retained parser allocations that do not come from decoded streams.
    pub(super) fn charge_work(&mut self, bytes: usize) -> Result<(), ReadError> {
        self.work = self.work.saturating_add(bytes as u64);
        self.exceeded()
    }

    /// Records one glyph with `text_bytes` of decoded text.
    pub(super) fn charge_glyph(&mut self, text_bytes: usize) -> Result<(), ReadError> {
        self.work = self.work.saturating_add(GLYPH_WORK + text_bytes as u64);
        self.exceeded()
    }

    /// Charges every walk over font array elements, including shared arrays.
    pub(super) fn charge_items(&mut self, items: usize) -> Result<(), ReadError> {
        self.work = self
            .work
            .saturating_add((items as u64).saturating_mul(GLYPH_WORK));
        self.exceeded()
    }

    /// Decodes a stream within the remaining budget.
    ///
    /// Returns `Ok(None)` for a stream that cannot be decoded (an unsupported or corrupt
    /// filter), which callers treat as empty.
    pub(super) fn decode(&mut self, stream: &Stream) -> Result<Option<Vec<u8>>, ReadError> {
        let remaining = self.remaining().min((usize::MAX / 4) as u64) as usize;
        match stream.decompressed_content_with_limit(remaining) {
            Ok(data) => {
                self.charge(data.len())?;
                Ok(Some(data))
            }
            Err(lopdf::Error::Decompress(DecompressError::MemoryLimitExceeded { .. })) => {
                let limit = if self.decoded_limit.saturating_sub(self.decoded)
                    <= self.work_limit.saturating_sub(self.work)
                {
                    self.decoded_limit
                } else {
                    self.work_limit
                };
                Err(ReadError::DecompressedTooLarge { limit })
            }
            Err(_) => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use lopdf::{Dictionary, Object, Stream};

    use super::Budget;
    use crate::reader::pdf::ReadError;

    #[test]
    fn budget_counts_every_decoded_byte() {
        let stream = Stream::new(Dictionary::new(), vec![b'x'; 10]);
        let mut budget = Budget::new(25, 1_000);
        assert_eq!(
            budget.decode(&stream).unwrap().map(|data| data.len()),
            Some(10)
        );
        assert!(budget.decode(&stream).unwrap().is_some());
        assert_eq!(
            budget.decode(&stream),
            Err(ReadError::DecompressedTooLarge { limit: 25 })
        );
        let mut dict = Dictionary::new();
        dict.set("Filter", Object::Name(b"Unknown".to_vec()));
        let mut budget = Budget::new(100, 100);
        assert_eq!(budget.decode(&Stream::new(dict, vec![1, 2, 3])), Ok(None));

        let mut budget = Budget::new(1_000, 40);
        assert!(budget.charge_glyph(4).is_ok());
        assert_eq!(
            budget.charge_glyph(5),
            Err(ReadError::DecompressedTooLarge { limit: 40 })
        );
    }
}
