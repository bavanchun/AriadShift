#![no_main]

use ariad_core::limits::Limits;
use ariad_pdf::{ToolCtx, cbz::cbz_to_pdf};
use libfuzzer_sys::fuzz_target;

const MAX_INPUT_BYTES: usize = 1024 * 1024;

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_INPUT_BYTES {
        return;
    }

    let limits = Limits::local();
    let ctx = ToolCtx::default();

    if let Ok(pdf_bytes) = cbz_to_pdf(data, &limits, &ctx) {
        assert!(
            pdf_bytes.starts_with(b"%PDF-"),
            "successful CBZ conversion must produce valid PDF header"
        );
    }
});
