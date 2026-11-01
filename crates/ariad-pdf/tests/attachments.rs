//! Integration tests for `pdf.attachments`.

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
            $crate::insta::check_json_snapshot("attachments", $name, &val);
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
fn attachments_list_matches_truth_fixture_and_snapshot() {
    let ctx = ToolCtx::default();
    let bytes = fixture_bytes("with-attachments.pdf");
    let truth = fixture_truth("with-attachments.truth.json");

    let input = InputDoc::new("with-attachments.pdf", bytes);
    let out = run_tool(
        "pdf.attachments",
        &[input],
        &json!({ "action": "list" }),
        &ctx,
    )
    .expect("run pdf.attachments list");

    assert_eq!(out.artifacts.len(), 1);
    assert_eq!(out.artifacts[0].name, "with-attachments.attachments.json");
    let val: Value = serde_json::from_slice(&out.artifacts[0].bytes).expect("parse json");
    assert_eq!(&val, &truth["attachments"]);

    insta::assert_json_snapshot!("attachments_list_summary", val);

    // Also test list on a fixture without attachments
    let empty_in = InputDoc::new("annotations.pdf", fixture_bytes("annotations.pdf"));
    let empty_out = run_tool(
        "pdf.attachments",
        &[empty_in],
        &json!({ "action": "list" }),
        &ctx,
    )
    .expect("list empty");
    let empty_val: Value =
        serde_json::from_slice(&empty_out.artifacts[0].bytes).expect("parse empty json");
    assert_eq!(empty_val, json!([]));
}

#[test]
fn attachments_extract_all_and_filtered() {
    let ctx = ToolCtx::default();
    let bytes = fixture_bytes("with-attachments.pdf");
    let truth = fixture_truth("with-attachments.truth.json");
    let expected = truth["attachments"].as_array().expect("attachments array");

    let input = InputDoc::new("with-attachments.pdf", bytes.clone());
    let all_out = run_tool(
        "pdf.attachments",
        &[input],
        &json!({ "action": "extract" }),
        &ctx,
    )
    .expect("extract all");

    assert_eq!(all_out.artifacts.len(), 2);
    assert_eq!(
        all_out.artifacts[0].name,
        "with-attachments.attachment-001.inventory.csv"
    );
    assert_eq!(all_out.artifacts[0].format, "csv");
    assert_eq!(
        all_out.artifacts[0].bytes.len() as u64,
        expected[0]["size"].as_u64().unwrap()
    );
    assert_eq!(
        all_out.artifacts[1].name,
        "with-attachments.attachment-002.notes.txt"
    );
    assert_eq!(all_out.artifacts[1].format, "txt");
    assert_eq!(
        all_out.artifacts[1].bytes.len() as u64,
        expected[1]["size"].as_u64().unwrap()
    );

    // Filtered extract
    let input2 = InputDoc::new("with-attachments.pdf", bytes);
    let filtered_out = run_tool(
        "pdf.attachments",
        &[input2],
        &json!({
            "action": "extract",
            "names": ["notes.txt"]
        }),
        &ctx,
    )
    .expect("extract filtered");
    assert_eq!(filtered_out.artifacts.len(), 1);
    assert_eq!(
        filtered_out.artifacts[0].name,
        "with-attachments.attachment-001.notes.txt"
    );
}

#[test]
fn attachments_add_and_remove_preserve_text_and_update_pdf_info() {
    let ctx = ToolCtx::default();
    let carrier_bytes = fixture_bytes("rotated-pages.pdf");
    let attachment_bytes = fixture_bytes("annotations.pdf");
    let orig_texts = extract_plain_texts(&carrier_bytes);

    let carrier = InputDoc::new("rotated-pages.pdf", carrier_bytes);
    let att = InputDoc::new("annotations.pdf", attachment_bytes.clone());

    let added_out = run_tool(
        "pdf.attachments",
        &[carrier, att],
        &json!({ "action": "add" }),
        &ctx,
    )
    .expect("add attachment");
    assert_eq!(added_out.artifacts.len(), 1);
    assert_eq!(added_out.artifacts[0].name, "rotated-pages.attachments.pdf");

    let added_bytes = &added_out.artifacts[0].bytes;
    let added_info = info::run(added_bytes, &Limits::local()).expect("info after add");
    assert!(added_info.has_attachments);
    assert_eq!(extract_plain_texts(added_bytes), orig_texts);

    // List on added PDF shows annotations.pdf
    let list_in = InputDoc::new("added.pdf", added_bytes.clone());
    let list_out = run_tool(
        "pdf.attachments",
        &[list_in],
        &json!({ "action": "list" }),
        &ctx,
    )
    .expect("list after add");
    let list_val: Value = serde_json::from_slice(&list_out.artifacts[0].bytes).expect("parse json");
    let arr = list_val.as_array().expect("array");
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["name"], "annotations.pdf");
    assert_eq!(arr[0]["media_type"], "application/pdf");
    assert!(arr[0]["size"].as_u64().unwrap() > 1000);

    // Remove all attachments
    let rem_in = InputDoc::new("added.pdf", added_bytes.clone());
    let rem_out = run_tool(
        "pdf.attachments",
        &[rem_in],
        &json!({ "action": "remove" }),
        &ctx,
    )
    .expect("remove all");
    let rem_bytes = &rem_out.artifacts[0].bytes;
    let rem_info = info::run(rem_bytes, &Limits::local()).expect("info after remove");
    assert!(!rem_info.has_attachments);
    assert_eq!(extract_plain_texts(rem_bytes), orig_texts);
}

#[test]
fn attachments_remove_specific_retains_remaining() {
    let ctx = ToolCtx::default();
    let bytes = fixture_bytes("with-attachments.pdf");
    let input = InputDoc::new("with-attachments.pdf", bytes);
    let out = run_tool(
        "pdf.attachments",
        &[input],
        &json!({
            "action": "remove",
            "names": ["inventory.csv"]
        }),
        &ctx,
    )
    .expect("remove inventory.csv");
    let out_bytes = &out.artifacts[0].bytes;

    let info_after = info::run(out_bytes, &Limits::local()).expect("info");
    assert!(info_after.has_attachments);

    let list_in = InputDoc::new("partial.pdf", out_bytes.clone());
    let list_out = run_tool(
        "pdf.attachments",
        &[list_in],
        &json!({ "action": "list" }),
        &ctx,
    )
    .expect("list remaining");
    let val: Value = serde_json::from_slice(&list_out.artifacts[0].bytes).expect("parse json");
    let arr = val.as_array().expect("array");
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["name"], "notes.txt");
}

#[test]
fn attachments_enforces_decompression_budget_limit() {
    let ctx = ToolCtx::default();
    let bytes = fixture_bytes("with-attachments.pdf");
    let tight_limits = Limits {
        max_decompressed_bytes: 50,
        ..Limits::local()
    };
    let input = InputDoc::with_limits("with-attachments.pdf", bytes, tight_limits);
    let err = run_tool(
        "pdf.attachments",
        &[input],
        &json!({ "action": "extract" }),
        &ctx,
    )
    .expect_err("should exceed tight decompression budget");
    assert!(matches!(err, PdfError::LimitExceeded(_)));
}

#[test]
fn attachments_is_deterministic_and_handles_broken_xref_and_errors() {
    let ctx = ToolCtx::default();
    let bytes = fixture_bytes("with-attachments.pdf");
    let params = json!({
        "action": "remove",
        "names": ["inventory.csv"]
    });
    let in1 = InputDoc::new("with-attachments.pdf", bytes.clone());
    let in2 = InputDoc::new("with-attachments.pdf", bytes);
    let out1 = run_tool("pdf.attachments", &[in1], &params, &ctx).expect("out1");
    let out2 = run_tool("pdf.attachments", &[in2], &params, &ctx).expect("out2");
    assert_eq!(out1.artifacts[0].bytes, out2.artifacts[0].bytes);

    // broken-xref recovery
    let broken_in = InputDoc::new("broken-xref.pdf", fixture_bytes("broken-xref.pdf"));
    let broken_out = run_tool(
        "pdf.attachments",
        &[broken_in],
        &json!({ "action": "list" }),
        &ctx,
    )
    .expect("broken list");
    assert_eq!(broken_out.warnings, vec![REPAIRED_XREF_WARNING.to_owned()]);
    assert_eq!(broken_out.artifacts.len(), 1);

    // Error cases: wrong arity for add (only 1 input)
    let single = InputDoc::new("rotated-pages.pdf", fixture_bytes("rotated-pages.pdf"));
    let err = run_tool(
        "pdf.attachments",
        &[single],
        &json!({ "action": "add" }),
        &ctx,
    )
    .expect_err("add needs >= 2 inputs");
    assert!(matches!(err, PdfError::InvalidParams(_)));

    // Unknown attachment name on extract
    let att_in = InputDoc::new(
        "with-attachments.pdf",
        fixture_bytes("with-attachments.pdf"),
    );
    let err = run_tool(
        "pdf.attachments",
        &[att_in],
        &json!({
            "action": "extract",
            "names": ["missing.bin"]
        }),
        &ctx,
    )
    .expect_err("missing attachment");
    assert!(matches!(err, PdfError::InvalidParams(_)));

    // Encrypted input
    let enc_input = InputDoc::new("encrypted.pdf", make_encrypted_bytes());
    let err = run_tool(
        "pdf.attachments",
        &[enc_input],
        &json!({ "action": "list" }),
        &ctx,
    )
    .expect_err("encrypted");
    assert!(matches!(err, PdfError::Encrypted { .. }));
}
