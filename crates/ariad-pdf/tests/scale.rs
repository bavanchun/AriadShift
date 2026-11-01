//! Integration tests for `pdf.scale`.

use std::path::{Path, PathBuf};

use ariad_core::{limits::Limits, reader::pdf as core_pdf};
use ariad_pdf::{
    InputDoc, PdfError, REPAIRED_XREF_WARNING, ToolCtx, info, run_tool, tools::scale,
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
        .join(format!("scale__{name}.snap"))
}

fn assert_json_snapshot(name: &str, value: &Value) {
    let body = serde_json::to_string_pretty(value).expect("serialize snapshot json");
    let full = format!(
        "---\nsource: crates/ariad-pdf/tests/scale.rs\nexpression: info_json\n---\n{body}\n"
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

fn run_scale_with_epoch(stem: &str, bytes: Vec<u8>, params: Value) -> ariad_pdf::ToolOutput {
    let ctx = ToolCtx::default();
    let input = InputDoc::new(stem, bytes);
    let doc = input.open_for_mutation(&ctx).expect("open input");
    let typed: ariad_core::tools::params::scale::ScaleParams =
        serde_json::from_value(params).expect("parse ScaleParams");
    let mut out_docs = scale::run(std::slice::from_ref(&doc), &typed, &ctx).expect("run scale");
    for od in &mut out_docs {
        od.save_options = SaveOptions {
            epoch_override: Some(1_700_000_000),
            ..SaveOptions::default()
        };
    }
    ariad_pdf::tools::finalize_tool_output(
        "pdf.scale",
        std::slice::from_ref(&doc),
        &input.clean_stem(),
        out_docs,
        &ctx,
    )
    .expect("finalize scale output")
}

#[test]
fn scale_mixed_sizes_to_letter_and_custom_dimensions_preserves_text_and_annotations() {
    let raw = read_fixture("mixed-sizes.pdf");
    let before_text = extract_plain_text(&raw);

    let out = run_scale_with_epoch(
        "mixed-sizes.pdf",
        raw.clone(),
        json!({
            "pages": "all",
            "to": "Letter",
            "fit": "contain"
        }),
    );
    assert_eq!(out.artifacts.len(), 1);
    assert_eq!(out.artifacts[0].name, "mixed-sizes.scaled.pdf");

    let report = info::run(&out.artifacts[0].bytes, &Limits::local()).expect("inspect scaled");
    assert_eq!(report.page_count, 3);
    for page in &report.pages {
        assert_eq!(page.width_pt, 612.0);
        assert_eq!(page.height_pt, 792.0);
        assert_eq!(page.paper, "Letter");
        assert_eq!(page.rotation, 0);
        assert!(!page.blank);
    }
    assert_preserves_text_in_order(&before_text, &extract_plain_text(&out.artifacts[0].bytes));

    let info_json = serde_json::to_value(&report).expect("serialize PdfInfo");
    assert_json_snapshot("mixed_sizes_to_letter_contain", &info_json);

    // Cover and stretch modes with custom paper size `{w, h}` on subset of pages
    for fit in ["cover", "stretch"] {
        let custom_out = run_tool(
            "pdf.scale",
            &[InputDoc::new("mixed-sizes.pdf", raw.clone())],
            &json!({
                "pages": "1,3",
                "to": { "w": 500.0, "h": 700.0 },
                "fit": fit
            }),
            &ToolCtx::default(),
        )
        .expect("scale custom");
        let custom_info =
            info::run(&custom_out.artifacts[0].bytes, &Limits::local()).expect("inspect custom");
        assert_eq!(custom_info.pages[0].width_pt, 500.0);
        assert_eq!(custom_info.pages[0].height_pt, 700.0);
        assert_eq!(custom_info.pages[1].width_pt, 612.0);
        assert_eq!(custom_info.pages[1].height_pt, 792.0);
        assert_eq!(custom_info.pages[2].width_pt, 500.0);
        assert_eq!(custom_info.pages[2].height_pt, 700.0);
    }

    // Annotations survive scaling
    let annot_raw = read_fixture("annotations.pdf");
    let annot_out = run_tool(
        "pdf.scale",
        &[InputDoc::new("annotations.pdf", annot_raw)],
        &json!({ "to": "A5", "fit": "contain" }),
        &ToolCtx::default(),
    )
    .expect("scale annotations.pdf");
    let annot_info =
        info::run(&annot_out.artifacts[0].bytes, &Limits::local()).expect("inspect scaled annots");
    assert!(annot_info.has_annotations);
    assert_eq!(annot_info.pages[0].paper, "A5");
}

#[test]
fn scale_is_byte_deterministic_and_repairs_broken_xref() {
    let raw = read_fixture("rotated-pages.pdf");
    let ctx = ToolCtx::default();
    let params = json!({"to": "A4", "fit": "contain"});

    let out1 = run_tool(
        "pdf.scale",
        &[InputDoc::new("rotated-pages.pdf", raw.clone())],
        &params,
        &ctx,
    )
    .expect("run 1");
    let out2 = run_tool(
        "pdf.scale",
        &[InputDoc::new("rotated-pages.pdf", raw)],
        &params,
        &ctx,
    )
    .expect("run 2");
    assert_eq!(out1.artifacts[0].bytes, out2.artifacts[0].bytes);

    let broken = read_fixture("broken-xref.pdf");
    let broken_out = run_tool(
        "pdf.scale",
        &[InputDoc::new("broken-xref.pdf", broken)],
        &json!({"to": "Letter"}),
        &ctx,
    )
    .expect("scale broken-xref.pdf");
    assert_eq!(broken_out.warnings, vec![REPAIRED_XREF_WARNING.to_owned()]);
    let broken_info =
        info::run(&broken_out.artifacts[0].bytes, &Limits::local()).expect("inspect repaired");
    assert!(!broken_info.repaired_xref);
    assert_eq!(broken_info.pages[0].paper, "Letter");
}

#[test]
fn scale_rejects_bad_paper_sizes_duplicate_pages_out_of_bounds_and_wrong_arity() {
    let raw = read_fixture("mixed-sizes.pdf");
    let input = InputDoc::new("mixed-sizes.pdf", raw);
    let ctx = ToolCtx::default();

    // Invalid custom dimensions
    let err = run_tool(
        "pdf.scale",
        std::slice::from_ref(&input),
        &json!({ "to": { "w": -100.0, "h": 500.0 } }),
        &ctx,
    )
    .unwrap_err();
    assert!(matches!(err, PdfError::InvalidParams(_)));

    // Out-of-bounds page range
    let err = run_tool(
        "pdf.scale",
        std::slice::from_ref(&input),
        &json!({ "pages": "5-6", "to": "A4" }),
        &ctx,
    )
    .unwrap_err();
    assert!(matches!(err, PdfError::InvalidParams(ref msg) if msg.contains("5-6")));

    // Duplicate pages rejected
    let err = run_tool(
        "pdf.scale",
        std::slice::from_ref(&input),
        &json!({ "pages": "2,2", "to": "A4" }),
        &ctx,
    )
    .unwrap_err();
    assert!(matches!(err, PdfError::InvalidParams(ref msg) if msg.contains("duplicate page 2")));

    // Wrong arity
    let doc = input.open_for_mutation(&ctx).unwrap();
    let typed = ariad_core::tools::params::scale::ScaleParams::default();
    assert!(matches!(
        scale::run(&[doc.clone(), doc], &typed, &ctx),
        Err(PdfError::InvalidParams(_))
    ));
}
