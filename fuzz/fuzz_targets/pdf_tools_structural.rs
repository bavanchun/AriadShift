#![no_main]

use std::time::{Duration, Instant};

use arbitrary::Unstructured;
use ariad_core::limits::Limits;
use ariad_pdf::{
    ToolCtx, document,
    tools::{InputDoc, run_tool},
};
use libfuzzer_sys::fuzz_target;
use serde_json::{Map, Value, json};

const MAX_FUZZ_BYTES: usize = 64 * 1024;
const PER_INPUT_BUDGET: Duration = Duration::from_millis(500);

const STRUCTURAL_TOOLS: &[&str] = &[
    "pdf.merge",
    "pdf.split",
    "pdf.extract_pages",
    "pdf.delete_pages",
    "pdf.reorder",
    "pdf.alternate_mix",
    "pdf.insert_blank",
    "pdf.rotate",
    "pdf.crop",
    "pdf.scale",
    "pdf.single_page",
    "pdf.rotate_content",
    "pdf.decompress",
    "pdf.nup",
    "pdf.booklet",
    "pdf.overlay",
    "pdf.divide_pages",
    "pdf.posterize",
    "pdf.metadata",
    "pdf.outline",
    "pdf.attachments",
    "pdf.page_labels",
    "pdf.viewer_prefs",
    "pdf.portfolio",
    "pdf.layers",
];

fn tight_limits() -> Limits {
    Limits {
        max_input_bytes: Some(MAX_FUZZ_BYTES as u64),
        max_pages: Some(8),
        max_blocks: 128,
        max_nesting_depth: 16,
        max_decompressed_bytes: 512 * 1024,
        ..Limits::local()
    }
}

fn bounded_json_value(u: &mut Unstructured<'_>, depth: usize) -> Value {
    if depth == 0 || u.is_empty() {
        return match u.int_in_range(0u8..=4).unwrap_or(0) {
            0 => Value::Null,
            1 => Value::Bool(u.arbitrary().unwrap_or(false)),
            2 => json!(u.int_in_range(-360i32..=720).unwrap_or(0)),
            3 => {
                let len = u.int_in_range(0usize..=16).unwrap_or(0);
                let bytes = u.bytes(len).unwrap_or(&[]);
                Value::String(String::from_utf8_lossy(bytes).into_owned())
            }
            _ => json!("all"),
        };
    }

    match u.int_in_range(0u8..=5).unwrap_or(0) {
        0..=2 => bounded_json_value(u, 0),
        3 => {
            let count = u.int_in_range(0usize..=4).unwrap_or(0);
            let mut arr = Vec::with_capacity(count);
            for _ in 0..count {
                arr.push(bounded_json_value(u, depth - 1));
            }
            Value::Array(arr)
        }
        _ => {
            let count = u.int_in_range(0usize..=5).unwrap_or(0);
            let mut map = Map::with_capacity(count);
            for _ in 0..count {
                let k_len = u.int_in_range(1usize..=12).unwrap_or(1);
                let k_bytes = u.bytes(k_len).unwrap_or(b"k");
                let key = String::from_utf8_lossy(k_bytes).into_owned();
                let val = bounded_json_value(u, depth - 1);
                map.insert(key, val);
            }
            Value::Object(map)
        }
    }
}

fn pick<T: Copy>(u: &mut Unstructured<'_>, items: &[T]) -> T {
    let idx = u.int_in_range(0usize..=items.len() - 1).unwrap_or(0);
    items[idx]
}

fn structured_params_for_tool(tool_id: &str, u: &mut Unstructured<'_>) -> Value {
    let page_expr = pick(u, &["all", "1", "1-2", "odd", "even", "last"]);
    let paper = pick(u, &["A4", "Letter", "Legal", "A3"]);

    match tool_id {
        "pdf.merge" => json!({
            "outline": pick(u, &["per-file", "keep", "none"]),
            "page_labels": pick(u, &["renumber", "keep"]),
        }),
        "pdf.split" => match u.int_in_range(0u8..=4).unwrap_or(0) {
            0 => json!({ "mode": "every", "every": u.int_in_range(1u32..=4).unwrap_or(1) }),
            1 => json!({ "mode": "ranges", "ranges": ["1", "1-last"] }),
            2 => json!({ "mode": "bookmarks", "level": u.int_in_range(1u8..=3).unwrap_or(1) }),
            3 => json!({ "mode": "size", "max_bytes": u.int_in_range(512u64..=65536).unwrap_or(4096) }),
            _ => json!({ "mode": "blank-separator", "blank_threshold": 0.99 }),
        },
        "pdf.extract_pages" | "pdf.delete_pages" => json!({ "pages": page_expr }),
        "pdf.reorder" => json!({
            "order": pick(u, &["reverse", "duplex-interleave", "odd-then-even", "1"])
        }),
        "pdf.alternate_mix" => json!({
            "reverse_second": u.arbitrary::<bool>().unwrap_or(false)
        }),
        "pdf.insert_blank" => json!({
            "after": page_expr,
            "count": u.int_in_range(1u32..=2).unwrap_or(1),
            "size": "same"
        }),
        "pdf.rotate" => json!({
            "pages": page_expr,
            "angle": pick(u, &[90, 180, 270, -90])
        }),
        "pdf.crop" => json!({
            "pages": page_expr,
            "box": { "left": 10.0, "bottom": 10.0, "right": 200.0, "top": 200.0 },
            "target": pick(u, &["crop", "media"])
        }),
        "pdf.scale" => json!({
            "pages": page_expr,
            "to": paper,
            "fit": pick(u, &["contain", "cover", "stretch"])
        }),
        "pdf.single_page" => json!({}),
        "pdf.rotate_content" => json!({
            "pages": page_expr,
            "angle_deg": u.int_in_range(-180i16..=180).unwrap_or(45),
            "expand": u.arbitrary::<bool>().unwrap_or(true)
        }),
        "pdf.decompress" => json!({
            "pages": page_expr,
            "object_streams": pick(u, &["expand", "keep"])
        }),
        "pdf.nup" => json!({
            "per_sheet": pick(u, &[2, 4, 6, 8, 9, 16]),
            "sheet": paper,
            "order": pick(u, &["row", "column"]),
            "margins_pt": 4.0,
            "borders": u.arbitrary::<bool>().unwrap_or(false)
        }),
        "pdf.booklet" => json!({
            "sheet": paper,
            "binding": pick(u, &["left", "right"]),
            "creep_pt": 0.5
        }),
        "pdf.overlay" => json!({
            "overlay_pages": pick(u, &["first", "repeat", "match"]),
            "position": pick(u, &["over", "under"]),
            "pages": page_expr
        }),
        "pdf.divide_pages" => json!({
            "pages": page_expr,
            "cols": u.int_in_range(1u32..=3).unwrap_or(2),
            "rows": u.int_in_range(1u32..=3).unwrap_or(2),
            "order": pick(u, &["row", "column"])
        }),
        "pdf.posterize" => json!({
            "pages": page_expr,
            "sheet": paper,
            "tiles": { "cols": 2, "rows": 2 },
            "overlap_mm": 5.0,
            "crop_marks": u.arbitrary::<bool>().unwrap_or(false)
        }),
        "pdf.metadata" => json!({
            "set": { "title": "Fuzz Title", "author": "Fuzz Author" },
            "remove": ["subject"],
            "strip_all": u.arbitrary::<bool>().unwrap_or(false)
        }),
        "pdf.outline" => match u.int_in_range(0u8..=3).unwrap_or(0) {
            0 => json!({ "action": "export" }),
            1 => json!({ "action": "clear" }),
            2 => json!({ "action": "from_headings" }),
            _ => json!({
                "action": "import",
                "json": [{ "title": "Ch 1", "page": 1, "children": [] }]
            }),
        },
        "pdf.attachments" => match u.int_in_range(0u8..=3).unwrap_or(0) {
            0 => json!({ "action": "list" }),
            1 => json!({ "action": "add" }),
            2 => json!({ "action": "extract" }),
            _ => json!({ "action": "remove", "names": ["note.txt"] }),
        },
        "pdf.page_labels" => json!({
            "ranges": [{
                "from": 1,
                "style": pick(u, &["decimal", "roman-lower", "roman-upper", "alpha-lower", "alpha-upper", "none"]),
                "prefix": "p-",
                "start": 1
            }]
        }),
        "pdf.viewer_prefs" => json!({
            "page_layout": "single-page",
            "page_mode": "use-outlines",
            "hide_toolbar": true,
            "fit_window": true,
            "display_doc_title": true,
            "open_page": 1
        }),
        "pdf.portfolio" => match u.int_in_range(0u8..=2).unwrap_or(0) {
            0 => json!({ "action": "create", "view": pick(u, &["details", "tile"]) }),
            1 => json!({ "action": "list" }),
            _ => json!({ "action": "extract" }),
        },
        "pdf.layers" => match u.int_in_range(0u8..=2).unwrap_or(0) {
            0 => json!({ "action": "list" }),
            1 => json!({ "action": "set_default_visibility", "names": ["Layer1"], "visible": false }),
            _ => json!({ "action": "remove", "names": ["AriadShift Watermark"] }),
        },
        _ => json!({}),
    }
}

fn requires_two_inputs(tool_id: &str) -> bool {
    matches!(
        tool_id,
        "pdf.merge" | "pdf.alternate_mix" | "pdf.overlay" | "pdf.attachments" | "pdf.portfolio"
    )
}

fuzz_target!(|data: &[u8]| {
    if data.is_empty() || data.len() > MAX_FUZZ_BYTES {
        return;
    }

    let limits = tight_limits();
    let deadline = Instant::now() + PER_INPUT_BUDGET;
    let ctx = ToolCtx::with_deadline(deadline);

    // Derive primary/secondary PDF slices and an arbitrary parameter stream.
    // Raw PDF corpus seeds starting with `%PDF-` stay intact as `primary_pdf` so seed
    // documents exercise deep structural paths immediately.
    let (primary_pdf, secondary_pdf, param_seed) = if data.starts_with(b"%PDF-") {
        let tail_start = data.len().saturating_sub(64);
        (data, data, &data[tail_start..])
    } else {
        let split = (usize::from(data[0]) % data.len()).max(1);
        let (left, right) = data.split_at(split);
        let second = if right.is_empty() { left } else { right };
        (left, second, right)
    };

    let mut u = Unstructured::new(param_seed);
    let selected_idx = usize::from(param_seed.first().copied().unwrap_or(0)) % STRUCTURAL_TOOLS.len();
    let selected_tool = STRUCTURAL_TOOLS[selected_idx];

    // 1. Fuzz the selected tool with completely arbitrary JSON (either parsed from `param_seed`
    //    or synthesized recursively) and raw byte inputs.
    let raw_params: Value = serde_json::from_slice(param_seed)
        .unwrap_or_else(|_| bounded_json_value(&mut u, 2));
    let raw_inputs = if requires_two_inputs(selected_tool) {
        vec![
            InputDoc::with_limits("in1.pdf", primary_pdf.to_vec(), limits.clone()),
            InputDoc::with_limits("in2.pdf", secondary_pdf.to_vec(), limits.clone()),
        ]
    } else {
        vec![InputDoc::with_limits("in1.pdf", primary_pdf.to_vec(), limits.clone())]
    };
    let _ = run_tool(selected_tool, &raw_inputs, &raw_params, &ctx);

    if Instant::now() >= deadline {
        return;
    }

    // 2. If `primary_pdf` opens as a valid bounded PDF document, exercise every registered
    //    structural tool against the parsed `PdfDoc` with both structured and arbitrary JSON
    //    parameters under the shared deadline.
    if let Ok(doc) = document::open_with_ctx(primary_pdf, &limits, &ctx) {
        for &tool_id in STRUCTURAL_TOOLS {
            if Instant::now() >= deadline {
                break;
            }
            let inputs = if requires_two_inputs(tool_id) {
                vec![
                    InputDoc::from_doc("a.pdf", doc.clone()),
                    InputDoc::from_doc("b.pdf", doc.clone()),
                ]
            } else {
                vec![InputDoc::from_doc("a.pdf", doc.clone())]
            };

            let structured = structured_params_for_tool(tool_id, &mut u);
            let _ = run_tool(tool_id, &inputs, &structured, &ctx);

            if Instant::now() >= deadline {
                break;
            }
            let arbitrary_obj = bounded_json_value(&mut u, 2);
            let _ = run_tool(tool_id, &inputs, &arbitrary_obj, &ctx);
        }
    }
});
