#![no_main]

use ariad_core::{
    json_depth::{json_depth_budget_for_nesting, prescan_json_depth},
    limits::Limits,
};
use ariad_host::ir_io::{ReadError, read};
use libfuzzer_sys::fuzz_target;

const MAX_INPUT_BYTES: usize = 256 * 1024;

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_INPUT_BYTES {
        return;
    }

    // Small test limits to thoroughly exercise boundary checks
    let small_limits = Limits {
        max_ir_json_bytes: 4096,
        max_nesting_depth: 8,
        max_blocks: 16,
        ..Limits::local()
    };
    let small_budget = json_depth_budget_for_nesting(small_limits.max_nesting_depth);

    let res = read(data, &small_limits);

    match res {
        Ok(document) => {
            // Over-cap bytes, over-budget depth, over-count blocks must never yield Ok
            assert!(
                data.len() as u64 <= small_limits.max_ir_json_bytes,
                "input size {} exceeded byte cap {}",
                data.len(),
                small_limits.max_ir_json_bytes
            );

            assert!(
                prescan_json_depth(data, small_budget).is_ok(),
                "successful read must pass prescan_json_depth with budget {small_budget}"
            );

            assert!(
                document.block_count() <= small_limits.max_blocks as usize,
                "document block count {} exceeded max_blocks {}",
                document.block_count(),
                small_limits.max_blocks
            );

            // Safe drop without stack overflow
            drop(document);
        }
        Err(ReadError::ByteLimitExceeded { limit }) => {
            assert_eq!(limit, small_limits.max_ir_json_bytes);
            assert!(
                data.len() as u64 > limit,
                "ByteLimitExceeded requires data length {} > limit {}",
                data.len(),
                limit
            );
        }
        Err(ReadError::DepthExceeded(_)) => {
            assert!(
                prescan_json_depth(data, small_budget).is_err(),
                "DepthExceeded requires prescan_json_depth to fail"
            );
        }
        Err(ReadError::BlockLimitExceeded { count, limit }) => {
            assert_eq!(limit, small_limits.max_blocks);
            assert!(count > limit as usize);
        }
        Err(ReadError::Json(_)) | Err(ReadError::Io(_)) => {
            // Standard syntax/IO errors on malformed inputs
        }
        Err(ReadError::UnsupportedVersion) => {
            // Benign typed rejection: an input whose IR version is not supported is a legitimate rejection, not a bug.
        }
    }

    // Second limits set with local depth (64) to exercise deep build and drop
    let local_limits = Limits::local();
    let local_budget = json_depth_budget_for_nesting(local_limits.max_nesting_depth);
    if let Ok(document) = read(data, &local_limits) {
        assert!(
            prescan_json_depth(data, local_budget).is_ok(),
            "successful read must pass prescan_json_depth with local budget {local_budget}"
        );
        assert!(data.len() as u64 <= local_limits.max_ir_json_bytes);
        assert!(document.block_count() <= local_limits.max_blocks as usize);
        drop(document);
    }
});
