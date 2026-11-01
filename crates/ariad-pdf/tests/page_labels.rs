//! Integration tests for `pdf.page_labels`.

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
    Document, EncryptionState, EncryptionVersion, Object, Permissions,
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
        let update = std::env::var("UPDATE_SNAPSHOTS").as_deref() == Ok("1")
            || std::env::var("UPDATE_SNAPSHOTS").as_deref() == Ok("always")
            || std::env::var("INSTA_UPDATE").as_deref() == Ok("always");
        if update {
            if let Some(parent) = snap_path.parent() {
                std::fs::create_dir_all(parent).expect("create snapshots dir");
            }
            let content = format!(
                "---\nsource: crates/ariad-pdf/tests/{module}.rs\nexpression: value\n---\n{pretty}\n"
            );
            std::fs::write(&snap_path, content).expect("write snapshot file");
        }
        assert!(
            snap_path.exists(),
            "missing snapshot file {} (set UPDATE_SNAPSHOTS=1 to create)",
            snap_path.display()
        );
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
            $crate::insta::check_json_snapshot("page_labels", $name, &val);
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

fn read_page_labels_summary(pdf_bytes: &[u8]) -> Vec<Value> {
    let doc = Document::load_mem(pdf_bytes).expect("load lopdf");
    let catalog = doc.catalog().expect("catalog");
    let pl_obj = catalog.get(b"PageLabels").expect("PageLabels in catalog");
    let pl_dict = match pl_obj {
        Object::Reference(id) => doc.get_object(*id).unwrap().as_dict().unwrap(),
        Object::Dictionary(d) => d,
        other => panic!("unexpected PageLabels: {other:?}"),
    };
    let nums = pl_dict.get(b"Nums").unwrap().as_array().unwrap();
    let mut out = Vec::new();
    for pair in nums.chunks(2) {
        let page_idx = pair[0].as_i64().unwrap();
        let label_dict = match &pair[1] {
            Object::Reference(id) => doc.get_object(*id).unwrap().as_dict().unwrap(),
            Object::Dictionary(d) => d,
            other => panic!("unexpected label entry: {other:?}"),
        };
        let style = label_dict
            .get(b"S")
            .ok()
            .and_then(|o| o.as_name().ok())
            .map(|n| String::from_utf8_lossy(n).into_owned());
        let prefix = label_dict
            .get(b"P")
            .ok()
            .and_then(|o| o.as_str().ok())
            .map(|b| String::from_utf8_lossy(b).into_owned());
        let start = label_dict.get(b"St").ok().and_then(|o| o.as_i64().ok());
        out.push(json!({
            "zero_based_page": page_idx,
            "S": style,
            "P": prefix,
            "St": start,
        }));
    }
    out
}

#[test]
fn page_labels_writes_all_six_styles_and_preserves_text() {
    let ctx = ToolCtx::default();
    let bytes = fixture_bytes("multipage-20.pdf");
    let orig_texts = extract_plain_texts(&bytes);

    let input = InputDoc::new("multipage-20.pdf", bytes);
    let out = run_tool(
        "pdf.page_labels",
        &[input],
        &json!({
            "ranges": [
                { "from": 1, "style": "none", "prefix": "Cover" },
                { "from": 2, "style": "roman_lower", "start": 1 },
                { "from": 5, "style": "decimal", "start": 1 },
                { "from": 12, "style": "alpha_upper", "prefix": "App-", "start": 1 },
                { "from": 16, "style": "alpha_lower", "start": 3 },
                { "from": 19, "style": "roman_upper", "prefix": "Idx-", "start": 1 }
            ]
        }),
        &ctx,
    )
    .expect("run pdf.page_labels");

    assert_eq!(out.artifacts.len(), 1);
    assert_eq!(out.artifacts[0].name, "multipage-20.labeled.pdf");
    let out_bytes = &out.artifacts[0].bytes;

    let pdf_info = info::run(out_bytes, &Limits::local()).expect("info");
    assert_eq!(pdf_info.page_count, 20);
    assert_eq!(extract_plain_texts(out_bytes), orig_texts);

    let summary = read_page_labels_summary(out_bytes);
    insta::assert_json_snapshot!("page_labels_six_styles", summary);
}

#[test]
fn page_labels_works_on_rotated_pages_and_is_deterministic() {
    let ctx = ToolCtx::default();
    let bytes = fixture_bytes("rotated-pages.pdf");
    let params = json!({
        "ranges": [
            { "from": 1, "style": "roman_lower", "start": 1 },
            { "from": 3, "style": "decimal", "prefix": "P-", "start": 10 }
        ]
    });

    let in1 = InputDoc::new("rotated-pages.pdf", bytes.clone());
    let in2 = InputDoc::new("rotated-pages.pdf", bytes);
    let b1 = run_tool("pdf.page_labels", &[in1], &params, &ctx).expect("out1");
    let b2 = run_tool("pdf.page_labels", &[in2], &params, &ctx).expect("out2");
    assert_eq!(b1.artifacts[0].bytes, b2.artifacts[0].bytes);
}

#[test]
fn page_labels_handles_broken_xref_and_rejects_invalid_params() {
    let ctx = ToolCtx::default();
    let broken = InputDoc::new("broken-xref.pdf", fixture_bytes("broken-xref.pdf"));
    let broken_out = run_tool(
        "pdf.page_labels",
        &[broken],
        &json!({
            "ranges": [{ "from": 1, "style": "decimal", "start": 1 }]
        }),
        &ctx,
    )
    .expect("broken-xref recovery");
    assert_eq!(broken_out.warnings, vec![REPAIRED_XREF_WARNING.to_owned()]);
    assert_eq!(
        info::run(&broken_out.artifacts[0].bytes, &Limits::local())
            .expect("info")
            .page_count,
        1
    );

    // Out-of-bounds `from`
    let doc = InputDoc::new("rotated-pages.pdf", fixture_bytes("rotated-pages.pdf"));
    let err = run_tool(
        "pdf.page_labels",
        std::slice::from_ref(&doc),
        &json!({
            "ranges": [{ "from": 99, "style": "decimal" }]
        }),
        &ctx,
    )
    .expect_err("out of bounds");
    assert!(matches!(err, PdfError::InvalidParams(_)));

    // Duplicate `from`
    let err = run_tool(
        "pdf.page_labels",
        std::slice::from_ref(&doc),
        &json!({
            "ranges": [
                { "from": 1, "style": "decimal" },
                { "from": 1, "style": "roman_lower" }
            ]
        }),
        &ctx,
    )
    .expect_err("duplicate from");
    assert!(matches!(err, PdfError::InvalidParams(_)));

    // Empty ranges
    let err = run_tool("pdf.page_labels", &[doc], &json!({ "ranges": [] }), &ctx)
        .expect_err("empty ranges");
    assert!(matches!(err, PdfError::InvalidParams(_)));

    // Encrypted fixture
    let enc_doc = InputDoc::new("encrypted.pdf", make_encrypted_bytes());
    let err = run_tool(
        "pdf.page_labels",
        &[enc_doc],
        &json!({ "ranges": [{ "from": 1, "style": "decimal" }] }),
        &ctx,
    )
    .expect_err("encrypted");
    assert!(matches!(err, PdfError::Encrypted { .. }));
}
