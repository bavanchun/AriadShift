//! Integration tests for `pdf.single_page`.

use std::path::{Path, PathBuf};

use ariad_core::{limits::Limits, reader::pdf as core_pdf};
use ariad_pdf::{
    InputDoc, PdfError, REPAIRED_XREF_WARNING, ToolCtx, info, run_tool, tools::single_page,
    writer::SaveOptions,
};
use serde_json::{Value, json};

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/pdf-tools")
}

fn read_fixture(name: &str) -> Vec<u8> {
    std::fs::read(fixtures_dir().join(name)).expect("read fixture")
}

fn snapshot_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/snapshots")
        .join(format!("single_page__{name}.snap"))
}

fn assert_json_snapshot(name: &str, value: &Value) {
    let body = serde_json::to_string_pretty(value).expect("serialize snapshot json");
    let full = format!(
        "---\nsource: crates/ariad-pdf/tests/single_page.rs\nexpression: info_json\n---\n{body}\n"
    );
    let path = snapshot_path(name);
    if std::env::var("BLESS_SNAPSHOTS").as_deref() == Ok("1") {
        std::fs::create_dir_all(path.parent().unwrap()).expect("create snapshots dir");
        std::fs::write(&path, &full).expect("write snapshot");
    }
    let committed = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("missing snapshot {}: {e}", path.display()))
        .replace("\r\n", "\n");
    assert_eq!(committed, full, "snapshot mismatch for {name}");
}

fn collect_json_texts(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::Object(map) => {
            if map.get("type").and_then(Value::as_str) == Some("text")
                && let Some(t) = map.get("text").and_then(Value::as_str)
            {
                out.push(t.to_owned());
            }
            for child in map.values() {
                collect_json_texts(child, out);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_json_texts(item, out);
            }
        }
        _ => {}
    }
}

fn extract_plain_text(pdf_bytes: &[u8]) -> String {
    let out = core_pdf::read(pdf_bytes, &Limits::local()).expect("core pdf read");
    let body_json = serde_json::to_value(&out.document.body).expect("serialize body");
    let mut parts = Vec::new();
    collect_json_texts(&body_json, &mut parts);
    parts.join("\n")
}

fn assert_preserves_text_in_order(before: &str, after: &str) {
    let mut cursor = 0usize;
    for line in before.lines().filter(|l| !l.trim().is_empty()) {
        let rel = after[cursor..].find(line).unwrap_or_else(|| {
            panic!("expected line {line:?} in order within extracted text:\n{after}")
        });
        cursor += rel + line.len();
    }
}

fn run_single_page_with_epoch(stem: &str, bytes: Vec<u8>) -> ariad_pdf::ToolOutput {
    let ctx = ToolCtx::default();
    let input = InputDoc::new(stem, bytes);
    let doc = input.open_for_mutation(&ctx).expect("open input");
    let typed = ariad_core::tools::params::single_page::SinglePageParams::default();
    let mut out_docs =
        single_page::run(std::slice::from_ref(&doc), &typed, &ctx).expect("run single_page");
    for od in &mut out_docs {
        od.save_options = SaveOptions {
            epoch_override: Some(1_700_000_000),
            ..SaveOptions::default()
        };
    }
    ariad_pdf::tools::finalize_tool_output(
        "pdf.single_page",
        std::slice::from_ref(&doc),
        &input.clean_stem(),
        out_docs,
        &ctx,
    )
    .expect("finalize single_page output")
}

#[test]
fn single_page_concatenates_mixed_sizes_vertically_and_preserves_text() {
    let raw = read_fixture("mixed-sizes.pdf");
    let before_info = info::run(&raw, &Limits::local()).expect("inspect before");
    let before_text = extract_plain_text(&raw);

    let expected_w = before_info
        .pages
        .iter()
        .map(|p| p.width_pt)
        .fold(0.0_f64, f64::max);
    let expected_h: f64 = before_info.pages.iter().map(|p| p.height_pt).sum();

    let out = run_single_page_with_epoch("mixed-sizes.pdf", raw);
    assert_eq!(out.artifacts.len(), 1);
    assert_eq!(out.artifacts[0].name, "mixed-sizes.single-page.pdf");

    let report = info::run(&out.artifacts[0].bytes, &Limits::local()).expect("inspect single");
    assert_eq!(report.page_count, 1);
    assert!((report.pages[0].width_pt - expected_w).abs() < 0.05);
    assert!((report.pages[0].height_pt - expected_h).abs() < 0.05);
    assert_eq!(report.pages[0].rotation, 0);
    assert!(!report.pages[0].blank);

    let after_text = extract_plain_text(&out.artifacts[0].bytes);
    assert_preserves_text_in_order(&before_text, &after_text);

    let info_json = serde_json::to_value(&report).expect("serialize PdfInfo");
    assert_json_snapshot("mixed_sizes", &info_json);
}

#[test]
fn single_page_preserves_bookmarks_and_annotations_and_is_deterministic() {
    let raw = read_fixture("multipage-20.pdf");
    let before_text = extract_plain_text(&raw);
    let ctx = ToolCtx::default();

    let out1 = run_tool(
        "pdf.single_page",
        &[InputDoc::new("multipage-20.pdf", raw.clone())],
        &json!({}),
        &ctx,
    )
    .expect("run 1");
    let out2 = run_tool(
        "pdf.single_page",
        &[InputDoc::new("multipage-20.pdf", raw)],
        &json!({}),
        &ctx,
    )
    .expect("run 2");
    assert_eq!(out1.artifacts[0].bytes, out2.artifacts[0].bytes);

    let report =
        info::run(&out1.artifacts[0].bytes, &Limits::local()).expect("inspect bookmarks single");
    assert_eq!(report.page_count, 1);
    assert!(report.outline_count > 0);
    assert_preserves_text_in_order(&before_text, &extract_plain_text(&out1.artifacts[0].bytes));

    // Annotations preserved on single page
    let annot_raw = read_fixture("annotations.pdf");
    let annot_out = run_tool(
        "pdf.single_page",
        &[InputDoc::new("annotations.pdf", annot_raw)],
        &json!({}),
        &ctx,
    )
    .expect("single_page annotations");
    let annot_info =
        info::run(&annot_out.artifacts[0].bytes, &Limits::local()).expect("inspect annots");
    assert_eq!(annot_info.page_count, 1);
    assert!(annot_info.has_annotations);

    // Broken xref repair warning
    let broken = read_fixture("broken-xref.pdf");
    let broken_out = run_tool(
        "pdf.single_page",
        &[InputDoc::new("broken-xref.pdf", broken)],
        &json!({}),
        &ctx,
    )
    .expect("single_page broken-xref.pdf");
    assert_eq!(broken_out.warnings, vec![REPAIRED_XREF_WARNING.to_owned()]);
    let broken_info =
        info::run(&broken_out.artifacts[0].bytes, &Limits::local()).expect("inspect repaired");
    assert!(!broken_info.repaired_xref);
    assert_eq!(broken_info.page_count, 1);
}

#[test]
fn single_page_rejects_wrong_arity() {
    let raw = read_fixture("incremental-update.pdf");
    let ctx = ToolCtx::default();
    let input = InputDoc::new("incremental-update.pdf", raw);
    let doc = input.open_for_mutation(&ctx).unwrap();
    let typed = ariad_core::tools::params::single_page::SinglePageParams::default();
    assert!(matches!(
        single_page::run(&[doc.clone(), doc], &typed, &ctx),
        Err(PdfError::InvalidParams(_))
    ));
}
