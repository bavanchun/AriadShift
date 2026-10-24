#![no_main]

use ariad_core::{limits::Limits, reader::markdown, writer::typst::TypstOptions};
use ariad_host::{ir_io, typst_render::render};
use libfuzzer_sys::fuzz_target;

const MAX_INPUT_BYTES: usize = 16 * 1024;

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_INPUT_BYTES {
        return;
    }
    let limits = Limits {
        max_input_bytes: Some(MAX_INPUT_BYTES as u64),
        max_ir_json_bytes: MAX_INPUT_BYTES as u64,
        max_nesting_depth: 12,
        max_blocks: 64,
        ..Limits::local()
    };
    // JSON seeds cover all IR variants, including invalid table spans and footnote cycles.
    // Other bytes exercise rich inline and block structures through the Markdown reader.
    let document = match ir_io::read(data, &limits) {
        Ok(document) => document,
        Err(_) => match markdown::read(&String::from_utf8_lossy(data), &limits) {
            Ok(output) => output.document,
            Err(_) => return,
        },
    };
    let options = TypstOptions {
        date: Some((2023, 11, 14)),
        ..TypstOptions::default()
    };
    let pdf = render(&document, &options, 1_700_000_000)
        .unwrap_or_else(|error| panic!("bounded IR must compile to PDF: {error:?}"));
    assert!(pdf.bytes.starts_with(b"%PDF-"));
    // Each production engine handles one job. Do not retain prior fuzz jobs in Typst's cache.
    typst::comemo::evict(0);
});
