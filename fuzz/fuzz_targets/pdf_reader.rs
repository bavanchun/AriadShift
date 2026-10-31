#![no_main]

use ariad_core::{
    ir::Document,
    limits::Limits,
    reader::pdf::{PdfStats, read},
};
use ariad_fuzz::assert_document_nfc;
use libfuzzer_sys::fuzz_target;
use serde::Deserialize;

const MAX_INPUT_BYTES: usize = 256 * 1024;

fn check(data: &[u8], limits: &Limits) {
    let Ok(output) = read(data, limits) else {
        // Every failure is a typed error; reaching here without a panic is the invariant.
        return;
    };
    let document = &output.document;
    assert!(
        document.block_count() <= limits.max_blocks as usize,
        "block count {} exceeds max_blocks {}",
        document.block_count(),
        limits.max_blocks
    );
    assert_document_nfc(document, limits.max_nesting_depth as usize);

    let stats = PdfStats::from_document(document).expect("PDF statistics in provenance");
    if let Some(max_pages) = limits.max_pages {
        assert!(stats.pages <= max_pages);
    }
    assert!(stats.text_pages + stats.image_only_pages <= stats.pages);
    let layout = document.layout.as_ref().expect("layout index");
    assert_eq!(layout.pages.len(), stats.pages as usize);
    assert!(
        layout
            .blocks
            .values()
            .all(|position| (1..=stats.pages).contains(&position.page_no))
    );
    assert_eq!(
        layout.blocks.len(),
        document.body.len() + document.furniture.len()
    );

    let serialized = serde_json::to_string(document).expect("PDF document IR must serialize");
    let mut de = serde_json::Deserializer::from_str(&serialized);
    de.disable_recursion_limit();
    let deserialized =
        Document::deserialize(&mut de).expect("serialized PDF document IR must deserialize");
    assert_eq!(document, &deserialized);
}

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_INPUT_BYTES {
        return;
    }
    check(data, &Limits::local());
    check(
        data,
        &Limits {
            max_pages: Some(4),
            max_blocks: 64,
            max_nesting_depth: 2,
            max_decompressed_bytes: 1 << 20,
            ..Limits::local()
        },
    );
});
