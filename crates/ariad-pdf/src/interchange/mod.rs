//! Bounded form and annotation interchange codecs.

pub mod csv;
pub mod fdf;
pub mod json;
#[cfg(feature = "codecs")]
mod limits;
pub mod names;
#[cfg(feature = "codecs")]
pub mod run;
pub mod sniff;
pub mod xfdf;
pub mod xfdf_annotations;

use ariad_core::tools::params::form_model::FormValue;
use std::collections::BTreeMap;

/// One record of qualified field names and values.
pub type FormRecord = BTreeMap<String, FormValue>;
/// Maximum size of an interchange input.
pub const MAX_DATA_BYTES: usize = 16 * 1024 * 1024;
/// Maximum bytes in a decoded interchange string.
pub const MAX_DATA_STRING_BYTES: usize = 1024 * 1024;
/// Maximum nesting depth accepted by interchange parsers.
pub const MAX_DATA_DEPTH: usize = 64;

fn check_data_size(bytes: &[u8]) -> Result<(), crate::PdfError> {
    if bytes.len() > MAX_DATA_BYTES {
        return Err(crate::PdfError::LimitExceeded(
            ariad_core::reader::pdf::ReadError::InputTooLarge {
                actual: bytes.len() as u64,
                limit: MAX_DATA_BYTES as u64,
            },
        ));
    }
    Ok(())
}

/// Output buffer that rejects writes before exceeding the interchange byte ceiling.
#[derive(Default)]
pub(crate) struct BoundedWriter {
    bytes: Vec<u8>,
}
impl std::io::Write for BoundedWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.bytes.len().saturating_add(bytes.len()) > MAX_DATA_BYTES {
            return Err(std::io::Error::other(
                "interchange output exceeds size limit",
            ));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
impl BoundedWriter {
    pub(crate) fn finish(self) -> Vec<u8> {
        self.bytes
    }
}
