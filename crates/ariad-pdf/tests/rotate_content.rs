//! Integration tests for `pdf.rotate_content`.

use std::path::{Path, PathBuf};

use ariad_core::{limits::Limits, reader::pdf as core_pdf};
use ariad_pdf::{
    InputDoc, PdfError, REPAIRED_XREF_WARNING, ToolCtx, info, run_tool, tools::rotate_content,
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
        .join(format!("rotate_content__{name}.snap"))
}

fn assert_json_snapshot(name: &str, value: &Value) {
    let body = serde_json::to_string_pretty(value).expect("serialize snapshot json");
    let full = format!(
        "---\nsource: crates/ariad-pdf/tests/rotate_content.rs\nexpression: info_json\n---\n{body}\n"
    );
    let path = snapshot_path(name);
    if std::env::var_os("UPDATE_SNAPSHOTS").is_some()
        || std::env::var("INSTA_UPDATE").as_deref() == Ok("always")
        || std::env::var("BLESS_SNAPSHOTS").as_deref() == Ok("1")
    {
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

fn run_rotate_content_with_epoch(
    stem: &str,
    bytes: Vec<u8>,
    params: Value,
) -> ariad_pdf::ToolOutput {
    let ctx = ToolCtx::default();
    let input = InputDoc::new(stem, bytes);
    let doc = input.open_for_mutation(&ctx).expect("open input");
    let typed: ariad_core::tools::params::rotate_content::RotateContentParams =
        serde_json::from_value(params).expect("parse RotateContentParams");
    let mut out_docs =
        rotate_content::run(std::slice::from_ref(&doc), &typed, &ctx).expect("run rotate_content");
    for od in &mut out_docs {
        od.save_options = SaveOptions {
            epoch_override: Some(1_700_000_000),
            ..SaveOptions::default()
        };
    }
    ariad_pdf::tools::finalize_tool_output(
        "pdf.rotate_content",
        std::slice::from_ref(&doc),
        &input.clean_stem(),
        out_docs,
        &ctx,
    )
    .expect("finalize rotate_content output")
}

#[test]
fn rotate_content_expands_media_box_and_preserves_text_stream() {
    let raw = read_fixture("incremental-update.pdf");
    let before_info = info::run(&raw, &Limits::local()).expect("inspect before");
    let before_text = extract_plain_text(&raw);
    let w = before_info.pages[0].width_pt;
    let h = before_info.pages[0].height_pt;

    let out = run_rotate_content_with_epoch(
        "incremental-update.pdf",
        raw.clone(),
        json!({
            "pages": "all",
            "angle_deg": 15.0,
            "expand": true
        }),
    );
    assert_eq!(out.artifacts.len(), 1);
    assert_eq!(
        out.artifacts[0].name,
        "incremental-update.rotated-content.pdf"
    );

    let report = info::run(&out.artifacts[0].bytes, &Limits::local()).expect("inspect rotated");
    assert_eq!(report.page_count, 1);
    let theta = 15.0_f64.to_radians();
    let expected_w = w * theta.cos().abs() + h * theta.sin().abs();
    let expected_h = w * theta.sin().abs() + h * theta.cos().abs();
    assert!((report.pages[0].width_pt - expected_w).abs() < 0.05);
    assert!((report.pages[0].height_pt - expected_h).abs() < 0.05);
    assert_eq!(report.pages[0].rotation, 0);
    assert!(!report.pages[0].blank);
    assert_eq!(extract_plain_text(&out.artifacts[0].bytes), before_text);

    let info_json = serde_json::to_value(&report).expect("serialize PdfInfo");
    assert_json_snapshot("simple_text_15_deg_expand", &info_json);

    // Without expand (`expand: false`), dimensions stay unchanged
    let no_expand_out = run_tool(
        "pdf.rotate_content",
        &[InputDoc::new("incremental-update.pdf", raw)],
        &json!({
            "pages": "all",
            "angle_deg": -22.5,
            "expand": false
        }),
        &ToolCtx::default(),
    )
    .expect("rotate_content no expand");
    let no_expand_info =
        info::run(&no_expand_out.artifacts[0].bytes, &Limits::local()).expect("inspect no expand");
    assert!((no_expand_info.pages[0].width_pt - w).abs() < 0.05);
    assert!((no_expand_info.pages[0].height_pt - h).abs() < 0.05);
}

#[test]
fn rotate_content_handles_rotated_pages_is_deterministic_and_repairs_xref() {
    let raw = read_fixture("rotated-pages.pdf");
    let before_text = extract_plain_text(&raw);
    let ctx = ToolCtx::default();
    let params = json!({
        "pages": "2-3",
        "angle_deg": 30.0,
        "expand": true
    });

    let out1 = run_tool(
        "pdf.rotate_content",
        &[InputDoc::new("rotated-pages.pdf", raw.clone())],
        &params,
        &ctx,
    )
    .expect("run 1");
    let out2 = run_tool(
        "pdf.rotate_content",
        &[InputDoc::new("rotated-pages.pdf", raw)],
        &params,
        &ctx,
    )
    .expect("run 2");
    assert_eq!(out1.artifacts[0].bytes, out2.artifacts[0].bytes);
    assert_eq!(extract_plain_text(&out1.artifacts[0].bytes), before_text);

    let broken = read_fixture("broken-xref.pdf");
    let broken_out = run_tool(
        "pdf.rotate_content",
        &[InputDoc::new("broken-xref.pdf", broken)],
        &json!({ "angle_deg": 10.0, "expand": false }),
        &ctx,
    )
    .expect("rotate_content broken-xref.pdf");
    assert_eq!(broken_out.warnings, vec![REPAIRED_XREF_WARNING.to_owned()]);
    let broken_info =
        info::run(&broken_out.artifacts[0].bytes, &Limits::local()).expect("inspect repaired");
    assert!(!broken_info.repaired_xref);
}

#[test]
fn rotate_content_rejects_angle_out_of_range_duplicate_pages_and_wrong_arity() {
    let raw = read_fixture("rotated-pages.pdf");
    let input = InputDoc::new("rotated-pages.pdf", raw);
    let ctx = ToolCtx::default();

    for bad_angle in [-360.5, 360.5, 720.0] {
        let err = run_tool(
            "pdf.rotate_content",
            std::slice::from_ref(&input),
            &json!({ "angle_deg": bad_angle }),
            &ctx,
        )
        .unwrap_err();
        assert!(matches!(err, PdfError::InvalidParams(_)));
    }

    // Out-of-bounds page range
    let err = run_tool(
        "pdf.rotate_content",
        std::slice::from_ref(&input),
        &json!({ "pages": "6", "angle_deg": 15.0 }),
        &ctx,
    )
    .unwrap_err();
    assert!(matches!(err, PdfError::InvalidParams(ref msg) if msg.contains('6')));

    // Duplicate pages rejected
    let err = run_tool(
        "pdf.rotate_content",
        std::slice::from_ref(&input),
        &json!({ "pages": "1,1", "angle_deg": 15.0 }),
        &ctx,
    )
    .unwrap_err();
    assert!(matches!(err, PdfError::InvalidParams(ref msg) if msg.contains("duplicate page 1")));

    // Wrong arity
    let doc = input.open_for_mutation(&ctx).unwrap();
    let typed = ariad_core::tools::params::rotate_content::RotateContentParams {
        pages: "all".parse().unwrap(),
        angle_deg: 15.0,
        expand: false,
    };
    assert!(matches!(
        rotate_content::run(&[doc.clone(), doc], &typed, &ctx),
        Err(PdfError::InvalidParams(_))
    ));
}
