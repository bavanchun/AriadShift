//! Integration tests for `pdf.layers`.

use std::{collections::BTreeMap, path::Path, sync::Arc};

use ariad_core::{
    ir::{Block, Inline},
    limits::Limits,
    reader::pdf as core_pdf,
};
use ariad_pdf::{
    PdfError, ToolCtx, info,
    tools::{InputDoc, REPAIRED_XREF_WARNING, run_tool},
};
use lopdf::{
    Document, EncryptionState, EncryptionVersion, Permissions,
    encryption::crypt_filters::{Aes256CryptFilter, CryptFilter},
};
use serde_json::{Value, json};

mod insta {
    use std::path::Path;

    pub fn check_json_snapshot(module: &str, name: &str, value: &serde_json::Value) {
        let snap_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/snapshots")
            .join(format!("{module}__{name}.snap"));
        let pretty = serde_json::to_string_pretty(value).expect("serialize snapshot json");
        if std::env::var("INSTA_UPDATE").as_deref() == Ok("always") || !snap_path.exists() {
            if let Some(parent) = snap_path.parent() {
                std::fs::create_dir_all(parent).expect("create snapshots dir");
            }
            let content = format!(
                "---\nsource: crates/ariad-pdf/tests/{module}.rs\nexpression: value\n---\n{pretty}\n"
            );
            std::fs::write(&snap_path, content).expect("write snapshot file");
        }
        let raw = std::fs::read_to_string(&snap_path).expect("read snapshot file");
        let body = raw
            .strip_prefix("---\n")
            .and_then(|s| s.split_once("\n---\n").map(|(_, b)| b))
            .unwrap_or(&raw)
            .trim();
        let expected: serde_json::Value =
            serde_json::from_str(body).expect("parse snapshot json body");
        assert_eq!(
            value,
            &expected,
            "snapshot mismatch for {}",
            snap_path.display()
        );
    }

    macro_rules! assert_json_snapshot {
        ($name:expr, $value:expr) => {{
            let val = serde_json::to_value(&$value).expect("to_value");
            $crate::insta::check_json_snapshot("layers", $name, &val);
        }};
    }
    pub(crate) use assert_json_snapshot;
}

fn fixture_bytes(name: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/pdf-tools")
        .join(name);
    std::fs::read(path).expect("read fixture")
}

fn fixture_truth(name: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/pdf-tools")
        .join(name);
    let raw = std::fs::read_to_string(path).expect("read truth json");
    serde_json::from_str(&raw).expect("parse truth json")
}

fn extract_plain_texts(pdf_bytes: &[u8]) -> Vec<String> {
    fn inline_text(inlines: &[Inline], out: &mut String) {
        for inline in inlines {
            match inline {
                Inline::Text { text } | Inline::Code { text } | Inline::Raw { text, .. } => {
                    out.push_str(text)
                }
                Inline::Math { tex, .. } => out.push_str(tex),
                Inline::SoftBreak {} | Inline::LineBreak {} => out.push(' '),
                Inline::Emph { content }
                | Inline::Strong { content }
                | Inline::Strikeout { content }
                | Inline::Superscript { content }
                | Inline::Subscript { content }
                | Inline::Link { content, .. } => inline_text(content, out),
                Inline::Image { alt, .. } => out.push_str(alt),
                Inline::FootnoteRef { id } => out.push_str(id),
            }
        }
    }

    let read_out = core_pdf::read(pdf_bytes, &Limits::local()).expect("read pdf text");
    read_out
        .document
        .body
        .iter()
        .map(|block| {
            let mut s = String::new();
            if let Block::Heading { content, .. } | Block::Paragraph { content } = block {
                inline_text(content, &mut s);
            }
            s
        })
        .filter(|s| !s.is_empty())
        .collect()
}

fn make_encrypted_bytes() -> Vec<u8> {
    let plain = fixture_bytes("encrypted-owner-only.plaintext.pdf");
    let mut doc = Document::load_mem(&plain).expect("load plaintext");
    let filter: Arc<dyn CryptFilter> = Arc::new(Aes256CryptFilter);
    let crypt_filters = BTreeMap::from([(b"StdCF".to_vec(), filter)]);
    let key = [0x42_u8; 32];
    let version = EncryptionVersion::V5 {
        encrypt_metadata: true,
        crypt_filters,
        file_encryption_key: &key,
        stream_filter: b"StdCF".to_vec(),
        string_filter: b"StdCF".to_vec(),
        owner_password: "owner",
        user_password: "",
        permissions: Permissions::empty(),
    };
    let state = EncryptionState::try_from(version).expect("encryption state");
    doc.encrypt(&state).expect("encrypt doc");
    let mut out = Vec::new();
    doc.save_to(&mut out).expect("save encrypted");
    out
}

#[test]
fn layers_list_matches_watermark_layer_truth_and_snapshot() {
    let ctx = ToolCtx::default();
    let bytes = fixture_bytes("watermark-layer.pdf");
    let truth = fixture_truth("watermark-layer.truth.json");
    let expected_name = truth["watermark"]["layer_name"]
        .as_str()
        .expect("watermark.layer_name");

    let input = InputDoc::new("watermark-layer.pdf", bytes);
    let out = run_tool("pdf.layers", &[input], &json!({ "action": "list" }), &ctx)
        .expect("run pdf.layers list");

    assert_eq!(out.artifacts.len(), 1);
    assert_eq!(out.artifacts[0].name, "watermark-layer.layers.json");
    let val: Value = serde_json::from_slice(&out.artifacts[0].bytes).expect("parse layers json");
    let arr = val.as_array().expect("layers array");
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["name"], expected_name);
    assert_eq!(arr[0]["visible"], true);

    insta::assert_json_snapshot!("layers_list_watermark_layer", val);

    // Also test list on a PDF with no OCG layers
    let empty_in = InputDoc::new("rotated-pages.pdf", fixture_bytes("rotated-pages.pdf"));
    let empty_out = run_tool(
        "pdf.layers",
        &[empty_in],
        &json!({ "action": "list" }),
        &ctx,
    )
    .expect("list empty");
    let empty_val: Value =
        serde_json::from_slice(&empty_out.artifacts[0].bytes).expect("parse empty");
    assert_eq!(empty_val, json!([]));
}

#[test]
fn layers_set_default_visibility_toggles_on_off_arrays() {
    let ctx = ToolCtx::default();
    let bytes = fixture_bytes("watermark-layer.pdf");
    let input = InputDoc::new("watermark-layer.pdf", bytes);

    let hidden_out = run_tool(
        "pdf.layers",
        &[input],
        &json!({
            "action": "set_default_visibility",
            "names": ["AriadShift Watermark"],
            "visible": false
        }),
        &ctx,
    )
    .expect("hide layer");
    let hidden_bytes = &hidden_out.artifacts[0].bytes;

    // Verify list now reports visible: false
    let list_in = InputDoc::new("hidden.pdf", hidden_bytes.clone());
    let list_out = run_tool("pdf.layers", &[list_in], &json!({ "action": "list" }), &ctx)
        .expect("list hidden");
    let val: Value = serde_json::from_slice(&list_out.artifacts[0].bytes).expect("parse json");
    assert_eq!(val[0]["visible"], false);

    // Toggle back to visible: true
    let show_in = InputDoc::new("hidden.pdf", hidden_bytes.clone());
    let shown_out = run_tool(
        "pdf.layers",
        &[show_in],
        &json!({
            "action": "set_default_visibility",
            "names": ["AriadShift Watermark"],
            "visible": true
        }),
        &ctx,
    )
    .expect("show layer");
    let shown_bytes = &shown_out.artifacts[0].bytes;
    let list_in2 = InputDoc::new("shown.pdf", shown_bytes.clone());
    let list_out2 = run_tool(
        "pdf.layers",
        &[list_in2],
        &json!({ "action": "list" }),
        &ctx,
    )
    .expect("list shown");
    let val2: Value = serde_json::from_slice(&list_out2.artifacts[0].bytes).expect("parse json");
    assert_eq!(val2[0]["visible"], true);
}

#[test]
fn layers_remove_strips_ocg_content_stream_and_preserves_canonical_body() {
    let ctx = ToolCtx::default();
    let bytes = fixture_bytes("watermark-layer.pdf");
    let truth = fixture_truth("watermark-layer.truth.json");
    let canonical_text = truth["canonical"]["text"].as_str().expect("canonical.text");

    let input = InputDoc::new("watermark-layer.pdf", bytes);
    let removed_out = run_tool(
        "pdf.layers",
        &[input],
        &json!({
            "action": "remove",
            "names": ["AriadShift Watermark"]
        }),
        &ctx,
    )
    .expect("remove layer");
    let removed_bytes = &removed_out.artifacts[0].bytes;

    let pdf_info = info::run(removed_bytes, &Limits::local()).expect("info");
    assert_eq!(pdf_info.page_count, 3);

    // Verify list now returns empty array
    let list_in = InputDoc::new("removed.pdf", removed_bytes.clone());
    let list_out = run_tool("pdf.layers", &[list_in], &json!({ "action": "list" }), &ctx)
        .expect("list after remove");
    let val: Value = serde_json::from_slice(&list_out.artifacts[0].bytes).expect("parse json");
    assert_eq!(val, json!([]));

    // Verify watermark text "CONFIDENTIAL DRAFT" is gone and primary body text remains intact
    let texts = extract_plain_texts(removed_bytes);
    let joined = texts.join("\n");
    assert!(
        !joined.contains("CONFIDENTIAL DRAFT"),
        "expected watermark text to be stripped, got: {joined}"
    );
    let expected_body = canonical_text
        .strip_prefix("Engineering Specification with OCG Watermark\n")
        .unwrap_or(canonical_text);
    assert_eq!(joined, expected_body);
}

#[test]
fn layers_is_deterministic_and_handles_broken_xref_and_errors() {
    let ctx = ToolCtx::default();
    let bytes = fixture_bytes("watermark-layer.pdf");
    let params = json!({
        "action": "remove",
        "names": ["AriadShift Watermark"]
    });
    let in1 = InputDoc::new("watermark-layer.pdf", bytes.clone());
    let in2 = InputDoc::new("watermark-layer.pdf", bytes);
    let b1 = run_tool("pdf.layers", &[in1], &params, &ctx).expect("out1");
    let b2 = run_tool("pdf.layers", &[in2], &params, &ctx).expect("out2");
    assert_eq!(b1.artifacts[0].bytes, b2.artifacts[0].bytes);

    // broken-xref recovery
    let broken = InputDoc::new("broken-xref.pdf", fixture_bytes("broken-xref.pdf"));
    let broken_out =
        run_tool("pdf.layers", &[broken], &json!({ "action": "list" }), &ctx).expect("broken list");
    assert_eq!(broken_out.warnings, vec![REPAIRED_XREF_WARNING.to_owned()]);
    assert_eq!(broken_out.artifacts.len(), 1);

    // Unknown layer name rejected
    let doc = InputDoc::new("watermark-layer.pdf", fixture_bytes("watermark-layer.pdf"));
    let err = run_tool(
        "pdf.layers",
        std::slice::from_ref(&doc),
        &json!({
            "action": "remove",
            "names": ["Nonexistent Layer"]
        }),
        &ctx,
    )
    .expect_err("unknown layer");
    assert!(matches!(err, PdfError::InvalidParams(_)));

    // Missing `visible` on set_default_visibility
    let err = run_tool(
        "pdf.layers",
        &[doc],
        &json!({
            "action": "set_default_visibility",
            "names": ["AriadShift Watermark"]
        }),
        &ctx,
    )
    .expect_err("missing visible");
    assert!(matches!(err, PdfError::InvalidParams(_)));

    // Encrypted fixture
    let enc_doc = InputDoc::new("encrypted.pdf", make_encrypted_bytes());
    let err = run_tool("pdf.layers", &[enc_doc], &json!({ "action": "list" }), &ctx)
        .expect_err("encrypted");
    assert!(matches!(err, PdfError::Encrypted { .. }));
}
