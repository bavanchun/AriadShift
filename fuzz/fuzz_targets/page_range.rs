#![no_main]

use ariad_core::tools::{DuplicatePolicy, PageRange};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(s) = std::str::from_utf8(data) else {
        return;
    };
    if let Ok(range) = PageRange::parse(s) {
        let _ = range.expand(0, DuplicatePolicy::Reject);
        for page_count in [1u32, 2, 10, 64] {
            if let Ok(pages) = range.expand(page_count, DuplicatePolicy::KeepInOrder) {
                for page in pages {
                    assert!((1..=page_count).contains(&page));
                }
            }
            if let Ok(pages) = range.expand(page_count, DuplicatePolicy::Reject) {
                for page in pages {
                    assert!((1..=page_count).contains(&page));
                }
            }
        }
    }
});
