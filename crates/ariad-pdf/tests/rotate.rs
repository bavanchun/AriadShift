//! Integration tests for `pdf.rotate`.

use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use ariad_core::{limits::Limits, reader::pdf as core_pdf};
use ariad_pdf::{
    InputDoc, PdfError, REPAIRED_XREF_WARNING, ToolCtx, info, run_tool,
    tools::rotate,
    writer::{self, SaveOptions},
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
        .join(format!("rotate__{name}.snap"))
}

fn assert_json_snapshot(name: &str, value: &Value) {
    let body = serde_json::to_string_pretty(value).expect("serialize snapshot json");
    let full = format!(
        "---\nsource: crates/ariad-pdf/tests/rotate.rs\nexpression: info_json\n---\n{body}\n"
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

fn run_rotate_with_epoch(stem: &str, bytes: Vec<u8>, params: Value) -> ariad_pdf::ToolOutput {
    let ctx = ToolCtx::default();
    let input = InputDoc::new(stem, bytes);
    let doc = input.open_for_mutation(&ctx).expect("open input");
    let typed: ariad_core::tools::params::rotate::RotateParams =
        serde_json::from_value(params).expect("parse RotateParams");
    let mut out_docs = rotate::run(std::slice::from_ref(&doc), &typed, &ctx).expect("run rotate");
    for od in &mut out_docs {
        od.save_options = SaveOptions {
            epoch_override: Some(1_700_000_000),
            ..SaveOptions::default()
        };
    }
    ariad_pdf::tools::finalize_tool_output(
        "pdf.rotate",
        std::slice::from_ref(&doc),
        &input.clean_stem(),
        out_docs,
        &ctx,
    )
    .expect("finalize rotate output")
}

#[test]
fn rotate_all_and_subset_pages_updates_rotation_mod_360_and_preserves_text() {
    let raw = read_fixture("rotated-pages.pdf");
    let out = run_rotate_with_epoch(
        "rotated-pages.pdf",
        raw.clone(),
        json!({"pages": "all", "angle": 90}),
    );
    assert_eq!(out.artifacts.len(), 1);
    assert_eq!(out.artifacts[0].name, "rotated-pages.rotated.pdf");
    assert!(out.warnings.is_empty());

    let report = info::run(&out.artifacts[0].bytes, &Limits::local()).expect("inspect output");
    assert_eq!(report.page_count, 4);
    let rotations: Vec<i32> = report.pages.iter().map(|p| p.rotation).collect();
    assert_eq!(rotations, vec![90, 180, 270, 0]);

    let info_json = serde_json::to_value(&report).expect("serialize PdfInfo");
    assert_json_snapshot("rotated_pages_all_90", &info_json);

    // Subset rotation with -90 (counter-clockwise 90 = +270 mod 360) on pages 1 and 3,
    // verifying unselected page 2 retains its inherited 90-degree rotation.
    let subset_out = run_tool(
        "pdf.rotate",
        &[InputDoc::new("rotated-pages.pdf", raw)],
        &json!({"pages": "1,3", "angle": -90}),
        &ToolCtx::default(),
    )
    .expect("rotate subset");
    let subset_info =
        info::run(&subset_out.artifacts[0].bytes, &Limits::local()).expect("inspect subset");
    let subset_rots: Vec<i32> = subset_info.pages.iter().map(|p| p.rotation).collect();
    assert_eq!(subset_rots, vec![270, 90, 90, 270]);

    // Text order and annotations survive on annotations.pdf.
    let annot_raw = read_fixture("annotations.pdf");
    let before_text = extract_plain_text(&annot_raw);
    let annot_out = run_tool(
        "pdf.rotate",
        &[InputDoc::new("annotations.pdf", annot_raw)],
        &json!({"angle": 180}),
        &ToolCtx::default(),
    )
    .expect("rotate annotations.pdf");
    let annot_info =
        info::run(&annot_out.artifacts[0].bytes, &Limits::local()).expect("inspect rotated annots");
    assert!(annot_info.has_annotations);
    assert_eq!(annot_info.pages[0].rotation, 180);
    let after_text = extract_plain_text(&annot_out.artifacts[0].bytes);
    assert_eq!(after_text, before_text);
}

#[test]
fn rotate_is_byte_deterministic_and_repairs_broken_xref() {
    let raw = read_fixture("multipage-20.pdf");
    let ctx = ToolCtx::default();
    let params = json!({"pages": "even", "angle": 270});

    let out1 = run_tool(
        "pdf.rotate",
        &[InputDoc::new("multipage-20.pdf", raw.clone())],
        &params,
        &ctx,
    )
    .expect("run 1");
    let out2 = run_tool(
        "pdf.rotate",
        &[InputDoc::new("multipage-20.pdf", raw)],
        &params,
        &ctx,
    )
    .expect("run 2");
    assert_eq!(out1.artifacts[0].bytes, out2.artifacts[0].bytes);

    // Broken xref input emits REPAIRED_XREF_WARNING and saves a valid rewritten PDF.
    let broken = read_fixture("broken-xref.pdf");
    let broken_out = run_tool(
        "pdf.rotate",
        &[InputDoc::new("broken-xref.pdf", broken)],
        &json!({"angle": 90}),
        &ctx,
    )
    .expect("rotate broken-xref.pdf");
    assert_eq!(broken_out.warnings, vec![REPAIRED_XREF_WARNING.to_owned()]);
    let repaired_info =
        info::run(&broken_out.artifacts[0].bytes, &Limits::local()).expect("inspect repaired");
    assert!(!repaired_info.repaired_xref);
    assert_eq!(repaired_info.pages[0].rotation, 90);
    assert!(core_pdf::read(&broken_out.artifacts[0].bytes, &Limits::local()).is_ok());
}

#[test]
fn rotate_rejects_invalid_params_duplicates_bad_ranges_and_wrong_arity() {
    let raw = read_fixture("rotated-pages.pdf");
    let input = InputDoc::new("rotated-pages.pdf", raw);
    let ctx = ToolCtx::default();

    // Non-quarter-turn angle
    let err = run_tool(
        "pdf.rotate",
        std::slice::from_ref(&input),
        &json!({"angle": 45}),
        &ctx,
    )
    .unwrap_err();
    assert!(matches!(err, PdfError::InvalidParams(_)));

    // Out-of-bounds page range
    let err = run_tool(
        "pdf.rotate",
        std::slice::from_ref(&input),
        &json!({"pages": "1-10", "angle": 90}),
        &ctx,
    )
    .unwrap_err();
    assert!(
        matches!(err, PdfError::InvalidParams(ref msg) if msg.contains("1-10")),
        "expected offending token in error, got {err:?}"
    );

    // Duplicate pages rejected under DuplicatePolicy::Reject
    let err = run_tool(
        "pdf.rotate",
        std::slice::from_ref(&input),
        &json!({"pages": "1-2,2-3", "angle": 90}),
        &ctx,
    )
    .unwrap_err();
    assert!(
        matches!(err, PdfError::InvalidParams(ref msg) if msg.contains("duplicate page 2")),
        "expected duplicate page error, got {err:?}"
    );

    // Wrong input arity (2 inputs)
    let doc = input.open_for_mutation(&ctx).unwrap();
    let typed = ariad_core::tools::params::rotate::RotateParams {
        pages: ariad_core::tools::page_range::PageRange::all(),
        angle: 90,
    };
    let err = rotate::run(&[doc.clone(), doc], &typed, &ctx).unwrap_err();
    assert!(matches!(err, PdfError::InvalidParams(_)));

    // Cooperative cancellation
    let cancel = Arc::new(AtomicBool::new(true));
    let cancelled_ctx = ToolCtx::with_cancel(Arc::clone(&cancel));
    let err = run_tool(
        "pdf.rotate",
        std::slice::from_ref(&input),
        &json!({"angle": 90}),
        &cancelled_ctx,
    )
    .unwrap_err();
    assert!(err.is_cancelled());
    cancel.store(false, Ordering::Relaxed);
    let _ = writer::save;
}
