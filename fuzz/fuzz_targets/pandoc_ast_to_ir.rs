#![no_main]

use ariad_core::{
    json_depth::json_depth_budget_for_nesting,
    limits::Limits,
    pandoc::{
        ast::Pandoc,
        to_ir::{MapError, to_ir},
    },
};
use ariad_fuzz::assert_blocks_depth;
use ariad_host::ir_io::{ReadError, read_json};
use libfuzzer_sys::fuzz_target;
use serde::Deserialize;

const MAX_INPUT_BYTES: usize = 256 * 1024;

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_INPUT_BYTES {
        return;
    }

    let limits = Limits::local();
    let depth_budget = json_depth_budget_for_nesting(limits.max_nesting_depth);

    // Call engine's production read path: bounded read_json with depth budget
    match read_json::<Pandoc>(data, limits.max_ir_json_bytes, depth_budget) {
        Ok(pandoc) => {
            let res = to_ir(&pandoc, &limits);
            match res {
                Ok(output) => {
                    assert!(
                        output.document.block_count() <= limits.max_blocks as usize,
                        "block count {} exceeds max_blocks {}",
                        output.document.block_count(),
                        limits.max_blocks
                    );

                    assert_blocks_depth(&output.document.body, limits.max_nesting_depth as usize);

                    let serialized = serde_json::to_string(&output.document)
                        .expect("pandoc mapped IR must serialize to JSON");

                    let mut de = serde_json::Deserializer::from_str(&serialized);
                    de.disable_recursion_limit();
                    let deserialized = ariad_core::ir::Document::deserialize(&mut de)
                        .expect("serialized IR must deserialize");
                    assert_eq!(output.document, deserialized);
                }
                Err(MapError::UnsupportedApiVersion { .. })
                | Err(MapError::NestingTooDeep { .. })
                | Err(MapError::TooManyBlocks { .. })
                | Err(MapError::InvalidLimits(_)) => {
                    // Valid typed errors from to_ir validation
                }
            }
        }
        Err(ReadError::ByteLimitExceeded { .. })
        | Err(ReadError::DepthExceeded(_))
        | Err(ReadError::Json(_))
        | Err(ReadError::BlockLimitExceeded { .. })
        | Err(ReadError::Io(_))
        | Err(ReadError::UnsupportedVersion) => {
            // Valid typed errors from bounded read_json. UnsupportedVersion is
            // defined on ReadError for version-checked readers and cannot be produced
            // by read_json::<Pandoc>, but is handled here to satisfy exhaustive matching.
        }
    }
});
