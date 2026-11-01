//! Integration tests for `pdf.decompress`.

use std::path::{Path, PathBuf};

use ariad_core::{limits::Limits, reader::pdf as core_pdf};
use ariad_pdf::{
    InputDoc, PdfError, REPAIRED_XREF_WARNING, ToolCtx,
    document::{open, open_with_ctx},
    info, run_tool,
    tools::decompress,
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
        .join(format!("decompress__{name}.snap"))
}

fn assert_json_snapshot(name: &str, value: &Value) {
    let body = serde_json::to_string_pretty(value).expect("serialize snapshot json");
    let full = format!(
        "---\nsource: crates/ariad-pdf/tests/decompress.rs\nexpression: info_json\n---\n{body}\n"
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

fn run_decompress_with_epoch(stem: &str, bytes: Vec<u8>, params: Value) -> ariad_pdf::ToolOutput {
    let ctx = ToolCtx::default();
    let input = InputDoc::new(stem, bytes);
    let doc = input.open_for_mutation(&ctx).expect("open input");
    let typed: ariad_core::tools::params::decompress::DecompressParams =
        serde_json::from_value(params).expect("parse DecompressParams");
    let mut out_docs =
        decompress::run(std::slice::from_ref(&doc), &typed, &ctx).expect("run decompress");
    for od in &mut out_docs {
        od.save_options = SaveOptions {
            epoch_override: Some(1_700_000_000),
            ..SaveOptions::default()
        };
    }
    ariad_pdf::tools::finalize_tool_output(
        "pdf.decompress",
        std::slice::from_ref(&doc),
        &input.clean_stem(),
        out_docs,
        &ctx,
    )
    .expect("finalize decompress output")
}

#[test]
fn decompress_strips_filters_and_preserves_text_and_structure() {
    let raw = read_fixture("object-streams.pdf");
    let before_text = extract_plain_text(&raw);

    let out = run_decompress_with_epoch("object-streams.pdf", raw, json!({}));
    assert_eq!(out.artifacts.len(), 1);
    assert_eq!(out.artifacts[0].name, "object-streams.decompressed.pdf");

    // Verify no stream in output has /Filter or /DecodeParms
    let parsed = open(&out.artifacts[0].bytes, &Limits::local()).expect("parse out");
    let mut stream_count = 0usize;
    for obj in parsed.inner().objects.values() {
        if let lopdf::Object::Stream(s) = obj {
            stream_count += 1;
            assert!(
                !s.dict.has(b"Filter"),
                "decompressed stream must not carry /Filter"
            );
            assert!(
                !s.dict.has(b"DecodeParms"),
                "decompressed stream must not carry /DecodeParms"
            );
        }
    }
    assert!(stream_count > 0);

    let report = info::run(&out.artifacts[0].bytes, &Limits::local()).expect("inspect out");
    assert_eq!(report.page_count, 2);
    assert_eq!(extract_plain_text(&out.artifacts[0].bytes), before_text);

    let info_json = serde_json::to_value(&report).expect("serialize PdfInfo");
    assert_json_snapshot("simple_text", &info_json);
}

#[test]
fn decompress_supports_page_scoped_decompression_is_deterministic_and_repairs_xref() {
    let raw = read_fixture("multipage-20.pdf");
    let before_text = extract_plain_text(&raw);
    let ctx = ToolCtx::default();

    let out1 = run_tool(
        "pdf.decompress",
        &[InputDoc::new("multipage-20.pdf", raw.clone())],
        &json!({ "pages": "1-2" }),
        &ctx,
    )
    .expect("run 1");
    let out2 = run_tool(
        "pdf.decompress",
        &[InputDoc::new("multipage-20.pdf", raw)],
        &json!({ "pages": "1-2" }),
        &ctx,
    )
    .expect("run 2");
    assert_eq!(out1.artifacts[0].bytes, out2.artifacts[0].bytes);
    assert_eq!(extract_plain_text(&out1.artifacts[0].bytes), before_text);

    let broken = read_fixture("broken-xref.pdf");
    let broken_out = run_tool(
        "pdf.decompress",
        &[InputDoc::new("broken-xref.pdf", broken)],
        &json!({}),
        &ctx,
    )
    .expect("decompress broken-xref.pdf");
    assert_eq!(broken_out.warnings, vec![REPAIRED_XREF_WARNING.to_owned()]);
    let broken_info =
        info::run(&broken_out.artifacts[0].bytes, &Limits::local()).expect("inspect repaired");
    assert!(!broken_info.repaired_xref);
}

#[test]
fn decompress_enforces_decode_budget_and_rejects_bad_params() {
    let raw = read_fixture("multipage-20.pdf");
    let input = InputDoc::new("multipage-20.pdf", raw.clone());
    let ctx = ToolCtx::default();

    // Tight decompressed byte budget triggers PdfError::LimitExceeded via DecodeBudget
    let mut tight_limits = Limits::local();
    tight_limits.max_decompressed_bytes = 64;
    let res = open_with_ctx(&raw, &tight_limits, &ctx).and_then(|tight_doc| {
        decompress::run(
            std::slice::from_ref(&tight_doc),
            &ariad_core::tools::params::decompress::DecompressParams::default(),
            &ctx,
        )
    });
    assert!(matches!(res, Err(PdfError::LimitExceeded(_))));
    // Out-of-bounds page range
    let err = run_tool(
        "pdf.decompress",
        std::slice::from_ref(&input),
        &json!({ "pages": "99" }),
        &ctx,
    )
    .unwrap_err();
    assert!(matches!(err, PdfError::InvalidParams(ref msg) if msg.contains("99")));

    // Duplicate pages rejected
    let err = run_tool(
        "pdf.decompress",
        std::slice::from_ref(&input),
        &json!({ "pages": "2,2" }),
        &ctx,
    )
    .unwrap_err();
    assert!(matches!(err, PdfError::InvalidParams(ref msg) if msg.contains("duplicate page 2")));

    // Wrong arity
    let doc = input.open_for_mutation(&ctx).unwrap();
    let typed = ariad_core::tools::params::decompress::DecompressParams::default();
    assert!(matches!(
        decompress::run(&[doc.clone(), doc], &typed, &ctx),
        Err(PdfError::InvalidParams(_))
    ));
}
