use std::io::{self, Read};

use ariad_core::{
    ir::Document,
    json_depth::{self, JsonDepthError, JsonDepthScanner},
    limits::Limits,
};
use serde::Deserialize;
use thiserror::Error;

/// Errors that can occur when reading an IR document.
#[derive(Debug, Error)]
pub enum ReadError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("IR JSON size exceeds limit of {limit} bytes")]
    ByteLimitExceeded { limit: u64 },
    #[error("JSON nesting depth exceeds budget: {0}")]
    DepthExceeded(#[from] JsonDepthError),
    #[error("JSON deserialization error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Document block count {count} exceeds limit of {limit}")]
    BlockLimitExceeded { count: usize, limit: u32 },
    #[error("unsupported IR version")]
    UnsupportedVersion,
}

/// An adapter that streams from an underlying `Read`, enforcing a byte limit
/// and incrementally feeding a [`JsonDepthScanner`].
struct BoundedScannerReader<R> {
    inner: R,
    bytes_read: u64,
    max_bytes: u64,
    scanner: JsonDepthScanner,
    overflow_error: Option<ReadError>,
    inner_io_error: Option<io::Error>,
}

impl<R: Read> BoundedScannerReader<R> {
    fn new(inner: R, max_bytes: u64, max_depth: usize) -> Self {
        Self {
            inner,
            bytes_read: 0,
            max_bytes,
            scanner: JsonDepthScanner::new(max_depth),
            overflow_error: None,
            inner_io_error: None,
        }
    }
}

impl<R: Read> Read for BoundedScannerReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.overflow_error.is_some() {
            return Err(io::Error::other("limit exceeded"));
        }
        if self.inner_io_error.is_some() {
            return Err(io::Error::other("inner I/O error"));
        }

        let remaining = self
            .max_bytes
            .saturating_add(1)
            .saturating_sub(self.bytes_read);
        if remaining == 0 {
            let err = ReadError::ByteLimitExceeded {
                limit: self.max_bytes,
            };
            self.overflow_error = Some(err);
            return Err(io::Error::other("byte limit exceeded"));
        }

        let to_read = (buf.len() as u64).min(remaining) as usize;
        let n = match self.inner.read(&mut buf[..to_read]) {
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => return Err(e),
            Err(e) => {
                self.inner_io_error = Some(io::Error::new(e.kind(), e.to_string()));
                return Err(e);
            }
        };

        if n == 0 {
            return Ok(0);
        }

        self.bytes_read = self.bytes_read.saturating_add(n as u64);
        if self.bytes_read > self.max_bytes {
            let err = ReadError::ByteLimitExceeded {
                limit: self.max_bytes,
            };
            self.overflow_error = Some(err);
            return Err(io::Error::other("byte limit exceeded"));
        }

        if let Err(depth_err) = self.scanner.feed(&buf[..n]) {
            let err = ReadError::DepthExceeded(depth_err);
            self.overflow_error = Some(err);
            return Err(io::Error::other("depth exceeded"));
        }

        Ok(n)
    }
}

/// Reads any JSON structure with bounded byte size, incremental nesting depth scan,
/// and deep deserialization using `serde_stacker` and `disable_recursion_limit()`.
pub fn read_json<T: for<'de> Deserialize<'de>>(
    reader: impl Read,
    max_bytes: u64,
    depth_budget: usize,
) -> Result<T, ReadError> {
    let mut adapter = BoundedScannerReader::new(reader, max_bytes, depth_budget);
    let mut deserializer = serde_json::Deserializer::from_reader(&mut adapter);
    deserializer.disable_recursion_limit();

    let result = {
        let stacked = serde_stacker::Deserializer::new(&mut deserializer);
        T::deserialize(stacked)
    };

    let value = match result {
        Ok(val) => val,
        Err(err) => {
            if let Some(overflow) = adapter.overflow_error {
                return Err(overflow);
            }
            if let Some(io_err) = adapter.inner_io_error {
                return Err(ReadError::Io(io_err));
            }
            return Err(ReadError::Json(err));
        }
    };

    if let Err(err) = deserializer.end() {
        if let Some(overflow) = adapter.overflow_error {
            return Err(overflow);
        }
        if let Some(io_err) = adapter.inner_io_error {
            return Err(ReadError::Io(io_err));
        }
        return Err(ReadError::Json(err));
    }

    Ok(value)
}

fn validate_block_count(document: &Document, limits: &Limits) -> Result<(), ReadError> {
    let block_count = document.block_count();
    if block_count > limits.max_blocks as usize {
        return Err(ReadError::BlockLimitExceeded {
            count: block_count,
            limit: limits.max_blocks,
        });
    }
    Ok(())
}

/// Reads an IR document with bounded byte size, incremental nesting depth scan,
/// deep deserialization without whole-file buffering, and post-parse block count validation.
pub fn read(reader: impl Read, limits: &Limits) -> Result<Document, ReadError> {
    let byte_cap = limits.max_ir_json_bytes;
    let depth_budget = json_depth::json_depth_budget_for_nesting(limits.max_nesting_depth);

    let document: Document = read_json(reader, byte_cap, depth_budget)?;
    validate_block_count(&document, limits)?;

    Ok(document)
}

#[derive(Deserialize)]
struct VersionProbe {
    version: Option<String>,
}

/// Reads an IR document from a byte slice with version verification,
/// bounded depth scan, and block count limit.
pub fn read_versioned(slice: &[u8], limits: &Limits) -> Result<Document, ReadError> {
    if (slice.len() as u64) > limits.max_ir_json_bytes {
        return Err(ReadError::ByteLimitExceeded {
            limit: limits.max_ir_json_bytes,
        });
    }

    let depth_budget = json_depth::json_depth_budget_for_nesting(limits.max_nesting_depth);
    let mut scanner = JsonDepthScanner::new(depth_budget);
    scanner.feed(slice)?;

    let probe: Result<VersionProbe, serde_json::Error> = serde_json::from_slice(slice);
    match probe {
        Ok(v) => {
            if v.version.as_deref() != Some(ariad_core::ir::IR_VERSION) {
                return Err(ReadError::UnsupportedVersion);
            }
        }
        Err(err) => {
            if err.is_data() {
                return Err(ReadError::UnsupportedVersion);
            }
            return Err(ReadError::Json(err));
        }
    }

    let document: Document = read_json(slice, limits.max_ir_json_bytes, depth_budget)?;
    validate_block_count(&document, limits)?;

    Ok(document)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ariad_core::ir::Block;

    #[test]
    fn read_versioned_accepts_valid_ir() {
        let doc = Document {
            version: ariad_core::ir::IR_VERSION.to_owned(),
            body: vec![Block::Paragraph { content: vec![] }],
            ..Default::default()
        };
        let bytes = serde_json::to_vec(&doc).unwrap();
        let read = read_versioned(&bytes, &Limits::local()).unwrap();
        assert_eq!(read, doc);
    }

    #[test]
    fn read_versioned_rejects_unsupported_version() {
        let json = br#"{"version":"ariad-ir/999","meta":{"authors":[],"date":null},"body":[]}"#;
        let err = read_versioned(json, &Limits::local()).unwrap_err();
        assert!(matches!(err, ReadError::UnsupportedVersion));
    }

    #[test]
    fn read_versioned_rejects_missing_version() {
        let json = br#"{"meta":{"authors":[],"date":null},"body":[]}"#;
        let err = read_versioned(json, &Limits::local()).unwrap_err();
        assert!(matches!(err, ReadError::UnsupportedVersion));
    }

    #[test]
    fn read_versioned_rejects_top_level_array() {
        let json = br#"[1, 2, 3]"#;
        let err = read_versioned(json, &Limits::local()).unwrap_err();
        assert!(matches!(err, ReadError::UnsupportedVersion));
    }

    #[test]
    fn read_versioned_rejects_malformed_json_as_json_error() {
        let json = br#"{"version": "ariad-ir/0", broken"#;
        let err = read_versioned(json, &Limits::local()).unwrap_err();
        assert!(matches!(err, ReadError::Json(_)));
    }

    #[test]
    fn read_versioned_rejects_exceeded_blocks() {
        let mut limits = Limits::local();
        limits.max_blocks = 1;
        let doc = Document {
            version: ariad_core::ir::IR_VERSION.to_owned(),
            body: vec![
                Block::Paragraph { content: vec![] },
                Block::Paragraph { content: vec![] },
            ],
            ..Default::default()
        };
        let bytes = serde_json::to_vec(&doc).unwrap();
        let err = read_versioned(&bytes, &limits).unwrap_err();
        assert!(matches!(
            err,
            ReadError::BlockLimitExceeded { count: 2, limit: 1 }
        ));
    }
}
