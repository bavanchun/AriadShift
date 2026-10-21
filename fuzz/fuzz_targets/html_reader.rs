#![no_main]

use ariad_core::{
    ir::Document,
    limits::Limits,
    reader::html::{MAX_DOM_DEPTH, ReadError, read},
};
use ariad_fuzz::assert_document_nfc;
use libfuzzer_sys::fuzz_target;
use serde::Deserialize;

const MAX_INPUT_BYTES: usize = 128 * 1024;

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_INPUT_BYTES {
        return;
    }

    let limits = Limits::local();

    match read(data, &limits) {
        Ok(output) => {
            // Invariant: DOM depth never exceeds MAX_DOM_DEPTH, observable through typed error.
            // When Ok, nesting must satisfy Limits::local() max_nesting_depth
            assert!(
                output.document.block_count() <= limits.max_blocks as usize,
                "block count {} exceeds max_blocks {}",
                output.document.block_count(),
                limits.max_blocks
            );

            // Invariant: all text is NFC normalized
            assert_document_nfc(&output.document, limits.max_nesting_depth as usize);

            // Output IR serializes to JSON
            let serialized = serde_json::to_string(&output.document)
                .expect("HTML document IR must serialize to JSON");

            let mut de = serde_json::Deserializer::from_str(&serialized);
            de.disable_recursion_limit();
            let deserialized = Document::deserialize(&mut de)
                .expect("serialized HTML document IR must deserialize from JSON");
            assert_eq!(output.document, deserialized);
        }
        Err(ReadError::NestingTooDeep { limit }) => {
            // Observable typed error: depth exceeded either DOM limit or IR limit
            assert!(
                limit == MAX_DOM_DEPTH as u16 || limit == limits.max_nesting_depth,
                "unexpected nesting depth limit in error: {}",
                limit
            );
        }
        Err(ReadError::TooManyNodes { .. })
        | Err(ReadError::TooManyBlocks { .. })
        | Err(ReadError::InputTooLarge { .. })
        | Err(ReadError::InvalidLimits(_)) => {
            // Valid typed errors when bounds are reached
        }
    }
});
