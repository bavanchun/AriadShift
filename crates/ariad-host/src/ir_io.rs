use std::io::{self, Read};

use ariad_core::{
    ir::Document,
    json_depth::{self, JsonDepthError, JsonDepthScanner},
    limits::Limits,
    pipeline::PipelineError,
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
    #[error("IR validation error: {0}")]
    Validation(#[from] PipelineError),
}

impl ReadError {
    /// Whether reading failed because a configured resource limit was exceeded.
    #[must_use]
    pub fn is_limit_exceeded(&self) -> bool {
        matches!(
            self,
            Self::ByteLimitExceeded { .. }
                | Self::DepthExceeded(_)
                | Self::BlockLimitExceeded { .. }
                | Self::Validation(PipelineError::LimitExceeded { .. })
        )
    }
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

/// Reads bounded IR, rejecting absent wire versions before applying defaults.
pub fn read(reader: impl Read, limits: &Limits) -> Result<Document, ReadError> {
    limits.validate().map_err(PipelineError::from)?;
    let mut reader = BoundedScannerReader::new(
        reader,
        limits.max_ir_json_bytes,
        json_depth::json_depth_budget_for_nesting(limits.max_nesting_depth),
    );
    let mut bytes = Vec::new();
    if let Err(error) = reader.read_to_end(&mut bytes) {
        return Err(reader
            .overflow_error
            .unwrap_or_else(|| ReadError::Io(reader.inner_io_error.unwrap_or(error))));
    }
    read_versioned(&bytes, limits)
}

/// Uses the common native/WASM IR version boundary after preserving host error categories.
pub fn read_versioned(slice: &[u8], limits: &Limits) -> Result<Document, ReadError> {
    limits.validate().map_err(PipelineError::from)?;
    if slice.len() as u64 > limits.max_ir_json_bytes {
        return Err(ReadError::ByteLimitExceeded {
            limit: limits.max_ir_json_bytes,
        });
    }
    let mut scanner = JsonDepthScanner::new(json_depth::json_depth_budget_for_nesting(
        limits.max_nesting_depth,
    ));
    scanner.feed(slice)?;
    // Engine-produced IR has its own byte cap; the source-input cap is enforced upstream.
    let mut ir_limits = limits.clone();
    ir_limits.max_input_bytes = None;
    ariad_core::pipeline::read_ir_json(slice, &ir_limits).map_err(|error| match error {
        PipelineError::UnsupportedIrVersion { .. } => ReadError::UnsupportedVersion,
        PipelineError::LimitExceeded { ref limit } if limit == "max_blocks" => {
            // Preserve the useful count without accepting the document.
            let count = read_json::<serde_json::Value>(
                slice,
                limits.max_ir_json_bytes,
                json_depth::json_depth_budget_for_nesting(limits.max_nesting_depth),
            )
            .ok()
            .map(|value| count_document_blocks(&value))
            .unwrap_or(limits.max_blocks as usize + 1);
            ReadError::BlockLimitExceeded {
                count,
                limit: limits.max_blocks,
            }
        }
        PipelineError::InvalidIr { .. } => match serde_json::from_slice::<serde_json::Value>(slice)
        {
            Err(json_error) if json_error.is_data() => ReadError::UnsupportedVersion,
            Err(json_error) if json_error.is_syntax() || json_error.is_eof() => {
                ReadError::Json(json_error)
            }
            Err(_) => ReadError::Validation(error),
            Ok(value) if value.get("version").and_then(|v| v.as_str()).is_none() => {
                ReadError::UnsupportedVersion
            }
            Ok(_) => ReadError::Validation(error),
        },
        _ => ReadError::Validation(error),
    })
}

fn count_document_blocks(document: &serde_json::Value) -> usize {
    let mut stack = Vec::new();
    for field in ["body", "furniture"] {
        push_blocks(&mut stack, document.get(field));
    }

    let mut count = 0;
    while let Some(block) = stack.pop() {
        count += 1;
        match block.get("type").and_then(serde_json::Value::as_str) {
            Some("quote" | "footnote") => push_blocks(&mut stack, block.get("blocks")),
            Some("list") => {
                if let Some(items) = block.get("items").and_then(serde_json::Value::as_array) {
                    for item in items {
                        push_blocks(&mut stack, item.get("blocks"));
                    }
                }
            }
            Some("table") => {
                for field in ["head", "body"] {
                    if let Some(rows) = block.get(field).and_then(serde_json::Value::as_array) {
                        for row in rows {
                            if let Some(cells) = row.as_array() {
                                for cell in cells {
                                    push_blocks(&mut stack, cell.get("blocks"));
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
    count
}

fn push_blocks<'a>(stack: &mut Vec<&'a serde_json::Value>, blocks: Option<&'a serde_json::Value>) {
    if let Some(blocks) = blocks.and_then(serde_json::Value::as_array) {
        stack.extend(blocks);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ariad_core::ir::Block;

    #[test]
    fn legacy_ir_is_accepted_without_losing_content() {
        let bytes = br#"{"version":"ariad-ir/0","body":[{"type":"paragraph","content":[{"type":"text","text":"Legacy content"}]}]}"#;
        let document = read_versioned(bytes, &Limits::local()).unwrap();
        assert_eq!(document.version, ariad_core::ir::IR_VERSION);
        assert_eq!(document.block_count(), 1);
        assert!(
            serde_json::to_string(&document)
                .unwrap()
                .contains("Legacy content")
        );
    }

    #[test]
    fn read_versioned_accepts_valid_ir() {
        let doc = Document {
            version: ariad_core::ir::IR_VERSION.to_owned(),
            body: vec![Block::Paragraph {
                id: Default::default(),
                style: None,
                direct: Default::default(),
                content: vec![],
            }],
            ..Default::default()
        }
        .finalize_import();
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
                Block::Paragraph {
                    id: Default::default(),
                    style: None,
                    direct: Default::default(),
                    content: vec![],
                },
                Block::Paragraph {
                    id: Default::default(),
                    style: None,
                    direct: Default::default(),
                    content: vec![],
                },
            ],
            ..Default::default()
        }
        .finalize_import();
        let bytes = serde_json::to_vec(&doc).unwrap();
        let err = read_versioned(&bytes, &limits).unwrap_err();
        assert!(matches!(
            err,
            ReadError::BlockLimitExceeded { count: 2, limit: 1 }
        ));
    }

    #[test]
    fn block_limit_count_ignores_inline_types_that_share_block_names() {
        let mut limits = Limits::local();
        limits.max_blocks = 1;
        let json = br#"{
            "version":"ariad-ir/2",
            "body":[
                {"type":"paragraph","id":"first","content":[
                    {"type":"code","text":"code"},
                    {"type":"math","tex":"x","display":false},
                    {"type":"raw","format":"html","text":"<b>raw</b>"}
                ]},
                {"type":"paragraph","id":"second","content":[]}
            ]
        }"#;

        let err = read_versioned(json, &limits).unwrap_err();
        assert!(matches!(
            err,
            ReadError::BlockLimitExceeded { count: 2, limit: 1 }
        ));
    }
}
