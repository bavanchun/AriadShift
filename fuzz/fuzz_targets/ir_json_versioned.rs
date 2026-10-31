#![no_main]

//! The version-checked IR JSON reader (`ir_io::read_versioned`) that `convert` uses for IR
//! input: byte cap, depth prescan, version probe, bounded parse and block count.

use ariad_core::{
    ir::{Document, IR_VERSION, is_supported_version},
    json_depth::{json_depth_budget_for_nesting, prescan_json_depth},
    limits::Limits,
    pipeline::PipelineError,
};
use ariad_host::ir_io::{ReadError, read, read_versioned};
use libfuzzer_sys::fuzz_target;
use serde::Deserialize;

const MAX_INPUT_BYTES: usize = 256 * 1024;

/// The top-level `version` member exactly as the input declares it.
#[derive(Deserialize)]
struct DeclaredVersion {
    version: Option<String>,
}

/// `None` when the input is not an object whose `version` is a string, null or absent.
fn declared_version(data: &[u8]) -> Option<Option<String>> {
    serde_json::from_slice::<DeclaredVersion>(data)
        .ok()
        .map(|declared| declared.version)
}

fn check(data: &[u8], limits: &Limits) {
    match read_versioned(data, limits) {
        Ok(document) => {
            let declared =
                declared_version(data).expect("an accepted document declares its version");
            assert!(
                is_supported_version(declared.as_deref()),
                "a document declaring {declared:?} was accepted"
            );
            assert_eq!(
                document.version, IR_VERSION,
                "an accepted document is upgraded to the current IR version"
            );
            assert!(data.len() as u64 <= limits.max_ir_json_bytes);
            let budget = json_depth_budget_for_nesting(limits.max_nesting_depth);
            assert!(prescan_json_depth(data, budget).is_ok());
            assert!(document.block_count() <= limits.max_blocks as usize);
            // Apart from upgrading the version, the version check only narrows what the plain
            // bounded reader accepts.
            let plain = read(data, limits).expect("the plain reader accepts a versioned document");
            assert_eq!(declared.as_deref(), Some(plain.version.as_str()));
            assert_eq!(
                Document {
                    version: IR_VERSION.to_owned(),
                    ..plain
                },
                document
            );
        }
        Err(ReadError::UnsupportedVersion) => {
            if let Some(declared) = declared_version(data) {
                assert!(
                    !is_supported_version(declared.as_deref()),
                    "a document declaring the supported {declared:?} was refused"
                );
            }
        }
        Err(ReadError::ByteLimitExceeded { limit }) => {
            assert_eq!(limit, limits.max_ir_json_bytes);
            assert!(data.len() as u64 > limit);
        }
        Err(ReadError::BlockLimitExceeded { count, limit }) => {
            assert_eq!(limit, limits.max_blocks);
            assert!(count > limit as usize);
        }
        Err(ReadError::Validation(error)) => {
            assert!(
                matches!(
                    error,
                    PipelineError::LimitExceeded { .. } | PipelineError::InvalidIr { .. }
                ),
                "unexpected IR validation error: {error}"
            );
        }
        Err(ReadError::DepthExceeded(_) | ReadError::Json(_) | ReadError::Io(_)) => {}
    }
}

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_INPUT_BYTES {
        return;
    }
    check(data, &Limits::local());
    check(
        data,
        &Limits {
            max_ir_json_bytes: 4096,
            max_nesting_depth: 8,
            max_blocks: 16,
            ..Limits::local()
        },
    );
});
