//! Document-bound, query-recoverable identifiers for watermark candidates.

use ariad_core::tools::{page_range::PageRange, params::watermark_detect::WatermarkDetectParams};

use crate::PdfError;

const MAX_SELECTOR_BYTES: usize = 65_536;
const MAX_ID_BYTES: usize = 2 * MAX_SELECTOR_BYTES + 80;

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from(DIGITS[usize::from(byte >> 4)]));
        out.push(char::from(DIGITS[usize::from(byte & 15)]));
    }
    out
}

fn unhex(value: &str) -> Option<Vec<u8>> {
    if !value.len().is_multiple_of(2) {
        return None;
    }
    let digit = |c| match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        _ => None,
    };
    value
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| Some(digit(pair[0])? * 16 + digit(pair[1])?))
        .collect()
}

/// Binds the candidate identity to the exact input and detector parameters. The encoded query
/// enables removal to reproduce detections made with non-default selections and thresholds.
pub(super) fn candidate_id(
    input_hash: &[u8; 32],
    query: &WatermarkDetectParams,
    identity: &[u8],
) -> Result<String, PdfError> {
    query
        .validate()
        .map_err(|message| PdfError::InvalidParams(message.into()))?;
    let pages = query.pages.as_str().as_bytes();
    if pages.len() > MAX_SELECTOR_BYTES {
        return Err(PdfError::InvalidParams("page selector is too long".into()));
    }
    let bits = query.min_page_ratio.to_bits().to_be_bytes();
    let digest = crate::writer::sha256_chunks(&[
        b"AriadShift watermark candidate v1\0",
        input_hash,
        &bits,
        &(pages.len() as u64).to_be_bytes(),
        pages,
        &(identity.len() as u64).to_be_bytes(),
        identity,
    ]);
    Ok(format!(
        "wm1.{}.{}.{}",
        hex(&bits),
        hex(pages),
        hex(&digest)
    ))
}

/// Recover the query embedded in an identifier; its digest is verified against a fresh scan.
pub(super) fn query_from_id(id: &str) -> Option<WatermarkDetectParams> {
    if id.len() > MAX_ID_BYTES {
        return None;
    }
    let mut parts = id.split('.');
    if parts.next()? != "wm1" {
        return None;
    }
    let bits: [u8; 4] = unhex(parts.next()?)?.try_into().ok()?;
    let pages = String::from_utf8(unhex(parts.next()?)?).ok()?;
    let digest = unhex(parts.next()?)?;
    if digest.len() != 32 || parts.next().is_some() {
        return None;
    }
    let query = WatermarkDetectParams {
        min_page_ratio: f32::from_bits(u32::from_be_bytes(bits)),
        pages: PageRange::parse(&pages).ok()?,
    };
    query.validate().ok()?;
    Some(query)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identity_is_bound_to_input_and_reproduces_nondefault_query() {
        let query = WatermarkDetectParams {
            min_page_ratio: 0.8,
            pages: PageRange::parse("1-3,5").unwrap(),
        };
        let id = candidate_id(&[1; 32], &query, b"ocg identity").unwrap();
        assert_eq!(query_from_id(&id), Some(query.clone()));
        assert_ne!(id, candidate_id(&[2; 32], &query, b"ocg identity").unwrap());
        assert_ne!(
            id,
            candidate_id(&[1; 32], &query, b"other identity").unwrap()
        );
        assert!(query_from_id("all").is_none());
    }
}
