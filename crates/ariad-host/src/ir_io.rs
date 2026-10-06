use std::io::Read;

use ariad_core::{
    ir::Document,
    json_depth::{self, JsonDepthError},
    limits::Limits,
};
use serde::Deserialize;
use thiserror::Error;

/// Errors that can occur when reading an IR document.
#[derive(Debug, Error)]
pub enum ReadError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("IR JSON size exceeds limit of {limit} bytes")]
    ByteLimitExceeded { limit: u64 },
    #[error("JSON nesting depth exceeds budget: {0}")]
    DepthExceeded(#[from] JsonDepthError),
    #[error("JSON deserialization error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Document block count {count} exceeds limit of {limit}")]
    BlockLimitExceeded { count: usize, limit: u32 },
}

/// Reads an IR document with bounded byte size, nesting depth pre-scan,
/// deep deserialization, and post-parse block count validation.
pub fn read(mut reader: impl Read, limits: &Limits) -> Result<Document, ReadError> {
    let byte_cap = limits.max_ir_json_bytes;
    let mut buffer = Vec::new();
    let read_count = reader
        .by_ref()
        .take(byte_cap.saturating_add(1))
        .read_to_end(&mut buffer)?;

    if read_count as u64 > byte_cap {
        return Err(ReadError::ByteLimitExceeded { limit: byte_cap });
    }

    let depth_budget = json_depth::json_depth_budget_for_nesting(limits.max_nesting_depth);
    json_depth::prescan_json_depth(&buffer, depth_budget)?;

    let mut deserializer = serde_json::Deserializer::from_slice(&buffer);
    deserializer.disable_recursion_limit();
    let document = {
        let stacked = serde_stacker::Deserializer::new(&mut deserializer);
        Document::deserialize(stacked)?
    };
    deserializer.end()?;

    let block_count = document.block_count();
    if block_count > limits.max_blocks as usize {
        return Err(ReadError::BlockLimitExceeded {
            count: block_count,
            limit: limits.max_blocks,
        });
    }

    Ok(document)
}
