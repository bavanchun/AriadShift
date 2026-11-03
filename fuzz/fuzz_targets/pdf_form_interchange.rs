#![no_main]

use ariad_core::tools::params::forms::EmptyCell;
use ariad_pdf::{
    ToolCtx,
    interchange::{csv, fdf, json, xfdf, xfdf_annotations},
};
use libfuzzer_sys::fuzz_target;
use std::time::{Duration, Instant};

fuzz_target!(|data: &[u8]| {
    if data.is_empty() || data.len() > 64 * 1024 {
        return;
    }
    let ctx = ToolCtx::with_deadline(Instant::now() + Duration::from_millis(500));
    let input = &data[1..];
    match data[0] % 6 {
        0 => {
            let _ = fdf::read_forms(input, &ctx);
        }
        1 => {
            let _ = xfdf::read_forms(input, &ctx);
        }
        2 => {
            let _ = xfdf_annotations::read(input, &ctx);
        }
        3 => {
            let _ = json::read_forms(input, 100, &ctx);
        }
        4 => {
            let _ = csv::read_forms(input, None, EmptyCell::Skip, 100, &ctx);
        }
        _ => {
            let _ = json::read_annotations(input, &ctx);
        }
    }
});
