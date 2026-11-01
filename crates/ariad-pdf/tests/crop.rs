//! Integration tests for `pdf.crop`.

use std::path::{Path, PathBuf};

use ariad_core::{limits::Limits, reader::pdf as core_pdf};
use ariad_pdf::{
    InputDoc, PdfError, REPAIRED_XREF_WARNING, ToolCtx, info, run_tool, tools::crop,
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
        .join(format!("crop__{name}.snap"))
}

fn assert_json_snapshot(name: &str, value: &Value) {
    let body = serde_json::to_string_pretty(value).expect("serialize snapshot json");
    let full = format!(
        "---\nsource: crates/ariad-pdf/tests/crop.rs\nexpression: info_json\n---\n{body}\n"
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

fn run_crop_with_epoch(stem: &str, bytes: Vec<u8>, params: Value) -> ariad_pdf::ToolOutput {
    let ctx = ToolCtx::default();
    let input = InputDoc::new(stem, bytes);
    let doc = input.open_for_mutation(&ctx).expect("open input");
    let typed: ariad_core::tools::params::crop::CropParams =
        serde_json::from_value(params).expect("parse CropParams");
    let mut out_docs = crop::run(std::slice::from_ref(&doc), &typed, &ctx).expect("run crop");
    for od in &mut out_docs {
        od.save_options = SaveOptions {
            epoch_override: Some(1_700_000_000),
            ..SaveOptions::default()
        };
    }
    ariad_pdf::tools::finalize_tool_output(
        "pdf.crop",
        std::slice::from_ref(&doc),
        &input.clean_stem(),
        out_docs,
        &ctx,
    )
    .expect("finalize crop output")
}

#[test]
fn crop_updates_cropbox_and_mediabox_on_selected_pages_and_preserves_text() {
    let raw = read_fixture("mixed-sizes.pdf");
    let out = run_crop_with_epoch(
        "mixed-sizes.pdf",
        raw.clone(),
        json!({
            "pages": "1,3",
            "box": { "left": 36.0, "bottom": 36.0, "right": 436.0, "top": 636.0 },
            "target": "crop"
        }),
    );
    assert_eq!(out.artifacts.len(), 1);
    assert_eq!(out.artifacts[0].name, "mixed-sizes.cropped.pdf");

    let report = info::run(&out.artifacts[0].bytes, &Limits::local()).expect("inspect cropped");
    assert_eq!(report.page_count, 3);
    assert_eq!(report.pages[0].width_pt, 400.0);
    assert_eq!(report.pages[0].height_pt, 600.0);
    // Page 2 was unselected (US Letter 612 x 792)
    assert_eq!(report.pages[1].width_pt, 612.0);
    assert_eq!(report.pages[1].height_pt, 792.0);
    // Page 3 was cropped to 400 x 600
    assert_eq!(report.pages[2].width_pt, 400.0);
    assert_eq!(report.pages[2].height_pt, 600.0);

    let info_json = serde_json::to_value(&report).expect("serialize PdfInfo");
    assert_json_snapshot("mixed_sizes_crop_pages_1_3", &info_json);

    // MediaBox target on annotations.pdf preserves text and annotations
    let annot_raw = read_fixture("annotations.pdf");
    let before_text = extract_plain_text(&annot_raw);
    let media_out = run_tool(
        "pdf.crop",
        &[InputDoc::new("annotations.pdf", annot_raw)],
        &json!({
            "box": { "left": 0.0, "bottom": 0.0, "right": 550.0, "top": 800.0 },
            "target": "media"
        }),
        &ToolCtx::default(),
    )
    .expect("crop media box");
    let media_info =
        info::run(&media_out.artifacts[0].bytes, &Limits::local()).expect("inspect media crop");
    assert_eq!(media_info.pages[0].width_pt, 550.0);
    assert_eq!(media_info.pages[0].height_pt, 800.0);
    assert!(media_info.has_annotations);
    assert_eq!(
        extract_plain_text(&media_out.artifacts[0].bytes),
        before_text
    );
}

#[test]
fn crop_is_byte_deterministic_and_repairs_broken_xref() {
    let raw = read_fixture("multipage-20.pdf");
    let ctx = ToolCtx::default();
    let params = json!({
        "pages": "1-5",
        "box": { "left": 20.0, "bottom": 20.0, "right": 520.0, "top": 720.0 }
    });

    let out1 = run_tool(
        "pdf.crop",
        &[InputDoc::new("multipage-20.pdf", raw.clone())],
        &params,
        &ctx,
    )
    .expect("run 1");
    let out2 = run_tool(
        "pdf.crop",
        &[InputDoc::new("multipage-20.pdf", raw)],
        &params,
        &ctx,
    )
    .expect("run 2");
    assert_eq!(out1.artifacts[0].bytes, out2.artifacts[0].bytes);

    let broken = read_fixture("broken-xref.pdf");
    let broken_out = run_tool(
        "pdf.crop",
        &[InputDoc::new("broken-xref.pdf", broken)],
        &json!({
            "box": { "left": 10.0, "bottom": 10.0, "right": 510.0, "top": 710.0 }
        }),
        &ctx,
    )
    .expect("crop broken-xref.pdf");
    assert_eq!(broken_out.warnings, vec![REPAIRED_XREF_WARNING.to_owned()]);
    let broken_info =
        info::run(&broken_out.artifacts[0].bytes, &Limits::local()).expect("inspect repaired");
    assert!(!broken_info.repaired_xref);
    assert_eq!(broken_info.pages[0].width_pt, 500.0);
    assert_eq!(broken_info.pages[0].height_pt, 700.0);
}

#[test]
fn crop_rejects_inverted_boxes_disjoint_cropbox_duplicates_and_wrong_arity() {
    let raw = read_fixture("mixed-sizes.pdf");
    let input = InputDoc::new("mixed-sizes.pdf", raw);
    let ctx = ToolCtx::default();

    // right <= left
    let err = run_tool(
        "pdf.crop",
        std::slice::from_ref(&input),
        &json!({
            "box": { "left": 300.0, "bottom": 0.0, "right": 100.0, "top": 400.0 }
        }),
        &ctx,
    )
    .unwrap_err();
    assert!(matches!(err, PdfError::InvalidParams(_)));

    // Disjoint CropBox outside page MediaBox
    let err = run_tool(
        "pdf.crop",
        std::slice::from_ref(&input),
        &json!({
            "box": { "left": 5000.0, "bottom": 5000.0, "right": 6000.0, "top": 6000.0 },
            "target": "crop"
        }),
        &ctx,
    )
    .unwrap_err();
    assert!(matches!(err, PdfError::InvalidParams(_)));

    // Deferred trim-whitespace-later mode rejected in structural tool
    let err = run_tool(
        "pdf.crop",
        std::slice::from_ref(&input),
        &json!({ "box": "trim-whitespace-later" }),
        &ctx,
    )
    .unwrap_err();
    assert!(matches!(err, PdfError::InvalidParams(_)));

    // Out-of-bounds page token
    let err = run_tool(
        "pdf.crop",
        std::slice::from_ref(&input),
        &json!({
            "pages": "99",
            "box": { "left": 0.0, "bottom": 0.0, "right": 200.0, "top": 200.0 }
        }),
        &ctx,
    )
    .unwrap_err();
    assert!(matches!(err, PdfError::InvalidParams(ref msg) if msg.contains("99")));

    // Duplicate page rejected
    let err = run_tool(
        "pdf.crop",
        std::slice::from_ref(&input),
        &json!({
            "pages": "1,1",
            "box": { "left": 0.0, "bottom": 0.0, "right": 200.0, "top": 200.0 }
        }),
        &ctx,
    )
    .unwrap_err();
    assert!(matches!(err, PdfError::InvalidParams(ref msg) if msg.contains("duplicate page 1")));

    // Wrong arity
    let doc = input.open_for_mutation(&ctx).unwrap();
    let typed: ariad_core::tools::params::crop::CropParams = serde_json::from_value(json!({
        "box": { "left": 0.0, "bottom": 0.0, "right": 200.0, "top": 200.0 }
    }))
    .unwrap();
    assert!(matches!(
        crop::run(&[doc.clone(), doc.clone()], &typed, &ctx),
        Err(PdfError::InvalidParams(_))
    ));

    // Cyclic indirect reference in `/MediaBox` does not overflow the stack
    let mut cyclic_doc = doc;
    let inner = cyclic_doc.inner_mut().unwrap();
    let cycle_id = inner.new_object_id();
    inner
        .objects
        .insert(cycle_id, lopdf::Object::Reference(cycle_id));
    let first_page_id = inner.page_iter().next().unwrap();
    inner
        .get_dictionary_mut(first_page_id)
        .unwrap()
        .set("MediaBox", lopdf::Object::Reference(cycle_id));
    let cropped = crop::run(&[cyclic_doc], &typed, &ctx)
        .expect("crop on page with cyclic MediaBox reference falls back to default MediaBox");
    assert_eq!(cropped.len(), 1);
}
