#![no_main]

use arbitrary::Arbitrary;
use ariad_core::limits::Limits;
use libfuzzer_sys::fuzz_target;

#[derive(Arbitrary, Debug)]
struct ArbitraryLimits {
    max_input_bytes: Option<u64>,
    max_pages: Option<u32>,
    timeout_s: Option<u64>,
    max_memory_mb: Option<u32>,
    max_asset_bytes: Option<u64>,
    max_nesting_depth: u16,
    max_blocks: u32,
    max_front_matter_bytes: u32,
    max_archive_entries: u32,
    max_decompressed_bytes: u64,
    max_ir_json_bytes: u64,
}

fuzz_target!(|input: ArbitraryLimits| {
    // Build Limits field by field from arbitrary integers and options
    let limits = Limits {
        max_input_bytes: input.max_input_bytes,
        max_pages: input.max_pages,
        timeout_s: input.timeout_s,
        max_memory_mb: input.max_memory_mb,
        max_asset_bytes: input.max_asset_bytes,
        max_nesting_depth: input.max_nesting_depth,
        max_blocks: input.max_blocks,
        max_front_matter_bytes: input.max_front_matter_bytes,
        max_archive_entries: input.max_archive_entries,
        max_decompressed_bytes: input.max_decompressed_bytes,
        max_ir_json_bytes: input.max_ir_json_bytes,
    };

    let result = limits.validate();

    // Invariant: Never panics; accepted values satisfy the documented ranges
    if result.is_ok() {
        assert_ne!(limits.max_input_bytes, Some(0));
        assert_ne!(limits.max_pages, Some(0));
        assert_ne!(limits.timeout_s, Some(0));
        assert_ne!(limits.max_memory_mb, Some(0));
        assert_ne!(limits.max_asset_bytes, Some(0));
        assert_ne!(limits.max_archive_entries, 0);
        assert_ne!(limits.max_decompressed_bytes, 0);
        assert_ne!(limits.max_ir_json_bytes, 0);
        assert!(limits.max_nesting_depth < 100);
    } else {
        // If it errored, at least one of the documented constraints was violated
        let violation = limits.max_input_bytes == Some(0)
            || limits.max_pages == Some(0)
            || limits.timeout_s == Some(0)
            || limits.max_memory_mb == Some(0)
            || limits.max_asset_bytes == Some(0)
            || limits.max_archive_entries == 0
            || limits.max_decompressed_bytes == 0
            || limits.max_ir_json_bytes == 0
            || limits.max_nesting_depth >= 100;
        assert!(
            violation,
            "Limits::validate() returned error but no constraint was violated: {:?}",
            limits
        );
    }
});
