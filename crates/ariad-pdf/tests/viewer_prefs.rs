//! Integration tests for `pdf.viewer_prefs`.

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
            $crate::insta::check_json_snapshot("viewer_prefs", $name, &val);
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

fn read_viewer_prefs_summary(pdf_bytes: &[u8]) -> Value {
    let doc = Document::load_mem(pdf_bytes).expect("load lopdf");
    let catalog = doc.catalog().expect("catalog");
    let page_layout = catalog
        .get(b"PageLayout")
        .ok()
        .and_then(|o| o.as_name().ok())
        .map(|n| String::from_utf8_lossy(n).into_owned());
    let page_mode = catalog
        .get(b"PageMode")
        .ok()
        .and_then(|o| o.as_name().ok())
        .map(|n| String::from_utf8_lossy(n).into_owned());
    let vp_dict = catalog
        .get(b"ViewerPreferences")
        .ok()
        .and_then(|o| match o {
            Object::Reference(id) => doc.get_object(*id).ok().and_then(|r| r.as_dict().ok()),
            Object::Dictionary(d) => Some(d),
            _ => None,
        });
    let hide_toolbar = vp_dict
        .and_then(|d| d.get(b"HideToolbar").ok())
        .and_then(|o| o.as_bool().ok());
    let fit_window = vp_dict
        .and_then(|d| d.get(b"FitWindow").ok())
        .and_then(|o| o.as_bool().ok());
    let display_doc_title = vp_dict
        .and_then(|d| d.get(b"DisplayDocTitle").ok())
        .and_then(|o| o.as_bool().ok());

    let pages = doc.get_pages();
    let open_page = catalog
        .get(b"OpenAction")
        .ok()
        .and_then(|o| o.as_array().ok())
        .and_then(|arr| arr.first())
        .and_then(|first| first.as_reference().ok())
        .and_then(|page_id| {
            pages
                .iter()
                .find_map(|(num, id)| (*id == page_id).then_some(*num))
        });

    json!({
        "page_layout": page_layout,
        "page_mode": page_mode,
        "hide_toolbar": hide_toolbar,
        "fit_window": fit_window,
        "display_doc_title": display_doc_title,
        "open_page": open_page,
    })
}

#[test]
fn viewer_prefs_sets_catalog_and_open_action_and_preserves_text() {
    let ctx = ToolCtx::default();
    let bytes = fixture_bytes("multipage-20.pdf");
    let orig_texts = extract_plain_texts(&bytes);

    let input = InputDoc::new("multipage-20.pdf", bytes);
    let out = run_tool(
        "pdf.viewer_prefs",
        &[input],
        &json!({
            "page_layout": "two_column_left",
            "page_mode": "use_outlines",
            "hide_toolbar": true,
            "fit_window": true,
            "display_doc_title": true,
            "open_page": 7
        }),
        &ctx,
    )
    .expect("run pdf.viewer_prefs");

    assert_eq!(out.artifacts.len(), 1);
    assert_eq!(out.artifacts[0].name, "multipage-20.prefs.pdf");
    let out_bytes = &out.artifacts[0].bytes;

    let pdf_info = info::run(out_bytes, &Limits::local()).expect("info");
    assert_eq!(pdf_info.page_count, 20);
    assert_eq!(extract_plain_texts(out_bytes), orig_texts);

    let summary = read_viewer_prefs_summary(out_bytes);
    insta::assert_json_snapshot!("viewer_prefs_catalog_summary", summary);
}

#[test]
fn viewer_prefs_is_deterministic_and_handles_broken_xref_and_errors() {
    let ctx = ToolCtx::default();
    let bytes = fixture_bytes("rotated-pages.pdf");
    let params = json!({
        "page_layout": "single_page",
        "page_mode": "use_thumbs",
        "hide_toolbar": false,
        "open_page": 2
    });
    let in1 = InputDoc::new("rotated-pages.pdf", bytes.clone());
    let in2 = InputDoc::new("rotated-pages.pdf", bytes);
    let b1 = run_tool("pdf.viewer_prefs", &[in1], &params, &ctx).expect("out1");
    let b2 = run_tool("pdf.viewer_prefs", &[in2], &params, &ctx).expect("out2");
    assert_eq!(b1.artifacts[0].bytes, b2.artifacts[0].bytes);

    // broken-xref recovery
    let broken = InputDoc::new("broken-xref.pdf", fixture_bytes("broken-xref.pdf"));
    let broken_out = run_tool(
        "pdf.viewer_prefs",
        &[broken],
        &json!({ "fit_window": true }),
        &ctx,
    )
    .expect("broken-xref recovery");
    assert_eq!(broken_out.warnings, vec![REPAIRED_XREF_WARNING.to_owned()]);
    assert_eq!(broken_out.artifacts.len(), 1);

    // Empty params rejected
    let doc = InputDoc::new("rotated-pages.pdf", fixture_bytes("rotated-pages.pdf"));
    let err = run_tool(
        "pdf.viewer_prefs",
        std::slice::from_ref(&doc),
        &json!({}),
        &ctx,
    )
    .expect_err("all fields None");
    assert!(matches!(err, PdfError::InvalidParams(_)));

    // Out-of-bounds open_page rejected
    let err = run_tool(
        "pdf.viewer_prefs",
        &[doc],
        &json!({ "open_page": 99 }),
        &ctx,
    )
    .expect_err("open_page out of bounds");
    assert!(matches!(err, PdfError::InvalidParams(_)));

    // Encrypted fixture
    let enc_doc = InputDoc::new("encrypted.pdf", make_encrypted_bytes());
    let err = run_tool(
        "pdf.viewer_prefs",
        &[enc_doc],
        &json!({ "fit_window": true }),
        &ctx,
    )
    .expect_err("encrypted");
    assert!(matches!(err, PdfError::Encrypted { .. }));
}
