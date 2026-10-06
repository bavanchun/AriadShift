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

/// Reads an IR document with bounded byte size, incremental nesting depth scan,
/// deep deserialization without whole-file buffering, and post-parse block count validation.
pub fn read(reader: impl Read, limits: &Limits) -> Result<Document, ReadError> {
    let byte_cap = limits.max_ir_json_bytes;
    let depth_budget = json_depth::json_depth_budget_for_nesting(limits.max_nesting_depth);

    let mut adapter = BoundedScannerReader::new(reader, byte_cap, depth_budget);
    let mut deserializer = serde_json::Deserializer::from_reader(&mut adapter);
    deserializer.disable_recursion_limit();

    let result = {
        let stacked = serde_stacker::Deserializer::new(&mut deserializer);
        Document::deserialize(stacked)
    };

    let document = match result {
        Ok(doc) => doc,
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

    let block_count = document.block_count();
    if block_count > limits.max_blocks as usize {
        return Err(ReadError::BlockLimitExceeded {
            count: block_count,
            limit: limits.max_blocks,
        });
    }

    Ok(document)
}
