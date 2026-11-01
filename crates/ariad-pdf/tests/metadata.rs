//! Integration tests for `pdf.metadata`.

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
            $crate::insta::check_json_snapshot("metadata", $name, &val);
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
fn metadata_set_remove_and_strip_all_across_corpus_fixtures() {
    let ctx = ToolCtx::default();

    // 1. Set standard + custom metadata on multipage-20.pdf
    let mp_bytes = fixture_bytes("multipage-20.pdf");
    let orig_texts = extract_plain_texts(&mp_bytes);
    let mp_input = InputDoc::new("multipage-20.pdf", mp_bytes);

    let set_out = run_tool(
        "pdf.metadata",
        std::slice::from_ref(&mp_input),
        &json!({
            "set": {
                "title": "AriadShift Architecture Handbook",
                "author": "Ariadnev Team",
                "subject": "Structural PDF Suite",
                "keywords": "pdf, metadata, xmp",
                "creator": "AriadShift Test Suite",
                "producer": "AriadShift Producer 1.0",
                "created": "D:20260115093000Z",
                "modified": "D:20260220144500Z",
                "custom": {
                    "Department": "Core Engineering"
                }
            }
        }),
        &ctx,
    )
    .expect("set metadata on multipage-20.pdf");

    assert_eq!(set_out.artifacts.len(), 1);
    assert_eq!(set_out.artifacts[0].name, "multipage-20.metadata.pdf");
    let updated_bytes = &set_out.artifacts[0].bytes;

    let report = info::run(updated_bytes, &Limits::local()).expect("info on updated pdf");
    assert_eq!(report.page_count, 20);
    assert_eq!(
        report.metadata.title.as_deref(),
        Some("AriadShift Architecture Handbook")
    );
    assert_eq!(report.metadata.author.as_deref(), Some("Ariadnev Team"));
    assert_eq!(
        report.metadata.creator.as_deref(),
        Some("AriadShift Test Suite")
    );
    assert_eq!(
        report.metadata.producer.as_deref(),
        Some("AriadShift Producer 1.0")
    );
    assert_eq!(
        report.metadata.creation_date.as_deref(),
        Some("D:20260115093000Z")
    );
    assert_eq!(
        report.metadata.mod_date.as_deref(),
        Some("D:20260220144500Z")
    );
    assert_eq!(extract_plain_texts(updated_bytes), orig_texts);

    insta::assert_json_snapshot!("metadata_multipage_20_info", report);

    // 2. Remove specific fields (`author`, `producer`, custom `Department`) on the updated PDF
    let updated_input = InputDoc::new("multipage-20.pdf", updated_bytes.clone());
    let removed_out = run_tool(
        "pdf.metadata",
        &[updated_input],
        &json!({
            "remove": ["author", "producer", "Department"]
        }),
        &ctx,
    )
    .expect("remove metadata fields");

    let removed_report =
        info::run(&removed_out.artifacts[0].bytes, &Limits::local()).expect("info after remove");
    assert_eq!(
        removed_report.metadata.title.as_deref(),
        Some("AriadShift Architecture Handbook")
    );
    assert_eq!(removed_report.metadata.author, None);
    assert_eq!(removed_report.metadata.producer, None);

    // 3. Strip all metadata on rotated-pages.pdf
    let rot_bytes = fixture_bytes("rotated-pages.pdf");
    let rot_texts = extract_plain_texts(&rot_bytes);
    let rot_input = InputDoc::new("rotated-pages.pdf", rot_bytes);
    let stripped_out = run_tool(
        "pdf.metadata",
        &[rot_input],
        &json!({ "strip_all": true }),
        &ctx,
    )
    .expect("strip_all metadata on rotated-pages.pdf");

    let stripped_bytes = &stripped_out.artifacts[0].bytes;
    let stripped_report =
        info::run(stripped_bytes, &Limits::local()).expect("info on stripped pdf");
    assert_eq!(stripped_report.metadata.title, None);
    assert_eq!(stripped_report.metadata.author, None);
    assert_eq!(stripped_report.metadata.creator, None);
    assert_eq!(stripped_report.metadata.producer, None);
    assert_eq!(stripped_report.metadata.creation_date, None);
    assert_eq!(stripped_report.metadata.mod_date, None);
    assert_eq!(extract_plain_texts(stripped_bytes), rot_texts);
}

#[test]
fn metadata_is_byte_for_byte_deterministic() {
    let ctx = ToolCtx::default();
    let input = InputDoc::new("annotations.pdf", fixture_bytes("annotations.pdf"));
    let params: Value = json!({
        "set": {
            "title": "Deterministic Title",
            "author": "Deterministic Author"
        }
    });

    let out1 = run_tool("pdf.metadata", std::slice::from_ref(&input), &params, &ctx).unwrap();
    let out2 = run_tool("pdf.metadata", std::slice::from_ref(&input), &params, &ctx).unwrap();
    assert_eq!(out1.artifacts[0].bytes, out2.artifacts[0].bytes);
}

#[test]
fn metadata_rejects_invalid_params_wrong_arity_and_encrypted_inputs() {
    let ctx = ToolCtx::default();
    let input = InputDoc::new("annotations.pdf", fixture_bytes("annotations.pdf"));

    // Wrong arity (2 inputs)
    let err = run_tool(
        "pdf.metadata",
        &[input.clone(), input.clone()],
        &json!({ "strip_all": true }),
        &ctx,
    )
    .unwrap_err();
    assert!(matches!(err, PdfError::InvalidParams(_)));

    // Empty operation (neither set, remove, nor strip_all)
    let err = run_tool(
        "pdf.metadata",
        std::slice::from_ref(&input),
        &json!({}),
        &ctx,
    )
    .unwrap_err();
    assert!(matches!(err, PdfError::InvalidParams(_)));

    // Empty remove array
    let err = run_tool(
        "pdf.metadata",
        std::slice::from_ref(&input),
        &json!({ "remove": [] }),
        &ctx,
    )
    .unwrap_err();
    assert!(matches!(err, PdfError::InvalidParams(_)));

    // Encrypted input
    let enc_input = InputDoc::new("encrypted.pdf", make_encrypted_bytes());
    let err = run_tool(
        "pdf.metadata",
        &[enc_input],
        &json!({ "strip_all": true }),
        &ctx,
    )
    .unwrap_err();
    assert!(matches!(err, PdfError::Encrypted { .. }));
}

#[test]
fn metadata_recovers_broken_xref_with_warning() {
    let ctx = ToolCtx::default();
    let input = InputDoc::new("broken-xref.pdf", fixture_bytes("broken-xref.pdf"));
    let out = run_tool(
        "pdf.metadata",
        &[input],
        &json!({ "set": { "title": "Repaired Title" } }),
        &ctx,
    )
    .expect("metadata on broken-xref.pdf");

    assert_eq!(out.warnings, vec![REPAIRED_XREF_WARNING.to_owned()]);
    let report = info::run(&out.artifacts[0].bytes, &Limits::local()).expect("valid rewritten pdf");
    assert!(!report.repaired_xref);
    assert_eq!(report.metadata.title.as_deref(), Some("Repaired Title"));
}
