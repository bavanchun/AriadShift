#![no_main]

use ariad_core::{limits::Limits, reader::markdown};
use libfuzzer_sys::fuzz_target;

const MAX_INPUT_BYTES: usize = 256 * 1024;

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_INPUT_BYTES {
        return;
    }

    let text = String::from_utf8_lossy(data);
    let limits = Limits::local();

    if let Ok(output) = markdown::read(&text, &limits) {
        // Output IR must serialize to JSON
        let serialized =
            serde_json::to_string(&output.document).expect("document IR must serialize to JSON");

        // Document block count must satisfy Limits
        assert!(
            output.document.block_count() <= limits.max_blocks as usize,
            "block count {} exceeds max_blocks {}",
            output.document.block_count(),
            limits.max_blocks
        );

        // Document nesting depth must satisfy Limits
        ariad_fuzz::assert_blocks_depth(&output.document.body, limits.max_nesting_depth as usize);

        // Deserialized IR must round-trip with disable_recursion_limit
        use serde::Deserialize;
        let mut de = serde_json::Deserializer::from_str(&serialized);
        de.disable_recursion_limit();
        let deserialized = ariad_core::ir::Document::deserialize(&mut de)
            .expect("serialized document IR must deserialize from JSON");
        assert_eq!(output.document, deserialized);
    }
});
