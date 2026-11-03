#![no_main]

use std::time::{Duration, Instant};

use ariad_core::limits::Limits;
use ariad_pdf::{InputDoc, ToolCtx, run_tool};
use libfuzzer_sys::fuzz_target;
use serde_json::{Value, json};

const TOOLS: &[&str] = &[
    "pdf.watermark",
    "pdf.stamp",
    "pdf.add_image",
    "pdf.add_text",
    "pdf.qr",
    "pdf.page_numbers",
    "pdf.header_footer",
    "pdf.bates",
    "pdf.toc_page",
    "pdf.background",
    "pdf.invert_colors",
    "pdf.replace_color",
    "pdf.adjust_colors",
    "pdf.watermark_detect",
    "pdf.watermark_remove",
];

fuzz_target!(|data: &[u8]| {
    if data.is_empty() || data.len() > 4096 {
        return;
    }
    let tool = TOOLS[usize::from(data[0]) % TOOLS.len()];
    let text = String::from_utf8_lossy(&data[1..]);
    let params: Value = serde_json::from_slice(&data[1..]).unwrap_or_else(|_| match tool {
        "pdf.watermark" | "pdf.add_text" | "pdf.stamp" => json!({"text": text, "font_size_pt": f64::from(data[0]), "rotation_deg": i16::from(data[0]) - 128}),
        "pdf.page_numbers" => json!({"format": text}),
        "pdf.header_footer" => json!({"header": {"left": text}}),
        "pdf.toc_page" => json!({"title": text}),
        "pdf.bates" => json!({"prefix": text}),
        "pdf.watermark_remove" => json!({"candidates": [text]}),
        "pdf.background" => json!({"opacity": f64::from(data[0]) / 100.0}),
        _ => json!({}),
    });
    let limits = Limits {
        max_pages: Some(8),
        max_blocks: 128,
        max_nesting_depth: 16,
        max_decompressed_bytes: 512 * 1024,
        ..Limits::local()
    };
    let ctx = ToolCtx::with_deadline(Instant::now() + Duration::from_millis(500));
    let input = InputDoc::with_limits(
        "fuzz.pdf",
        include_bytes!("../../fixtures/pdf-tools/rotated-pages.pdf").to_vec(),
        limits,
    );
    let _ = run_tool(tool, &[input], &params, &ctx);
});
