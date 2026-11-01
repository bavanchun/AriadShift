//! Integration tests for `pdf.portfolio`.

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

    fn should_update_snapshots() -> bool {
        std::env::var("UPDATE_SNAPSHOTS")
            .ok()
            .is_some_and(|v| !v.is_empty() && v != "0" && !v.eq_ignore_ascii_case("false"))
            || std::env::var("INSTA_UPDATE").as_deref() == Ok("always")
    }

    pub fn check_json_snapshot(module: &str, name: &str, value: &serde_json::Value) {
        let snap_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/snapshots")
            .join(format!("{module}__{name}.snap"));
        let pretty = serde_json::to_string_pretty(value).expect("serialize snapshot json");
        if should_update_snapshots() {
            if let Some(parent) = snap_path.parent() {
                std::fs::create_dir_all(parent).expect("create snapshots dir");
            }
            let content = format!(
                "---\nsource: crates/ariad-pdf/tests/{module}.rs\nexpression: value\n---\n{pretty}\n"
            );
            std::fs::write(&snap_path, content).expect("write snapshot file");
            return;
        }
        assert!(
            snap_path.exists(),
            "missing snapshot file {}; set UPDATE_SNAPSHOTS=1 to create it",
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
            $crate::insta::check_json_snapshot("portfolio", $name, &val);
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
fn portfolio_create_list_and_extract_round_trip() {
    let ctx = ToolCtx::default();
    let rot_bytes = fixture_bytes("rotated-pages.pdf");
    let ann_bytes = fixture_bytes("annotations.pdf");

    let rot_doc = ariad_pdf::document::open(&rot_bytes, &Limits::local()).expect("open rotated");
    let ann_doc =
        ariad_pdf::document::open(&ann_bytes, &Limits::local()).expect("open annotations");
    let in1 = InputDoc::from_doc("rotated-pages.pdf", rot_doc);
    let in2 = InputDoc::from_doc("annotations.pdf", ann_doc);

    let created = run_tool(
        "pdf.portfolio",
        &[in1, in2],
        &json!({
            "action": "create",
            "view": "details"
        }),
        &ctx,
    )
    .expect("create portfolio");

    assert_eq!(created.artifacts.len(), 1);
    assert_eq!(created.artifacts[0].name, "rotated-pages.portfolio.pdf");
    let portfolio_bytes = &created.artifacts[0].bytes;

    let p_info = info::run(portfolio_bytes, &Limits::local()).expect("portfolio info");
    assert_eq!(p_info.page_count, 1);
    assert!(p_info.has_attachments);

    // Check cover page text via ariad_core::reader::pdf::read
    let cover_texts = extract_plain_texts(portfolio_bytes);
    assert!(
        cover_texts.iter().any(|t| t.contains("PDF Portfolio")),
        "expected cover page text to mention PDF Portfolio, got {cover_texts:?}"
    );

    // Check /Collection dictionary on Catalog
    let doc = Document::load_mem(portfolio_bytes).expect("load portfolio");
    let catalog = doc.catalog().expect("catalog");
    let coll_obj = catalog.get(b"Collection").expect("Collection in catalog");
    let coll = match coll_obj {
        lopdf::Object::Reference(id) => doc.get_object(*id).unwrap().as_dict().unwrap(),
        lopdf::Object::Dictionary(d) => d,
        other => panic!("unexpected Collection: {other:?}"),
    };
    assert_eq!(coll.get(b"View").unwrap().as_name().unwrap(), b"D");

    // List portfolio entries
    let list_in = InputDoc::new("bundle.portfolio.pdf", portfolio_bytes.clone());
    let list_out = run_tool(
        "pdf.portfolio",
        &[list_in],
        &json!({ "action": "list" }),
        &ctx,
    )
    .expect("portfolio list");
    assert_eq!(list_out.artifacts.len(), 1);
    assert_eq!(
        list_out.artifacts[0].name,
        "bundle.portfolio.portfolio.json"
    );
    let list_val: Value =
        serde_json::from_slice(&list_out.artifacts[0].bytes).expect("parse portfolio list");
    insta::assert_json_snapshot!("portfolio_list_entries", list_val);

    // Extract portfolio entries
    let ext_in = InputDoc::new("bundle.portfolio.pdf", portfolio_bytes.clone());
    let ext_out = run_tool(
        "pdf.portfolio",
        &[ext_in],
        &json!({ "action": "extract" }),
        &ctx,
    )
    .expect("portfolio extract");
    assert_eq!(ext_out.artifacts.len(), 2);
    assert_eq!(
        ext_out.artifacts[0].name,
        "bundle.portfolio.attachment-001.annotations.pdf"
    );
    assert_eq!(
        info::run(&ext_out.artifacts[0].bytes, &Limits::local())
            .expect("extracted annotations pdf")
            .page_count,
        1
    );
    assert_eq!(
        ext_out.artifacts[1].name,
        "bundle.portfolio.attachment-002.rotated-pages.pdf"
    );
    assert_eq!(
        info::run(&ext_out.artifacts[1].bytes, &Limits::local())
            .expect("extracted rotated-pages pdf")
            .page_count,
        4
    );
}

#[test]
fn portfolio_is_deterministic_and_handles_tile_view_broken_xref_and_errors() {
    let ctx = ToolCtx::default();
    let b_rot = fixture_bytes("rotated-pages.pdf");
    let params = json!({
        "action": "create",
        "view": "tile"
    });
    let in1 = InputDoc::new("rotated-pages.pdf", b_rot.clone());
    let in2 = InputDoc::new("rotated-pages.pdf", b_rot);
    let out1 = run_tool("pdf.portfolio", &[in1], &params, &ctx).expect("out1");
    let out2 = run_tool("pdf.portfolio", &[in2], &params, &ctx).expect("out2");
    assert_eq!(out1.artifacts[0].bytes, out2.artifacts[0].bytes);

    // broken-xref recovery on list
    let broken = InputDoc::new("broken-xref.pdf", fixture_bytes("broken-xref.pdf"));
    let broken_out = run_tool(
        "pdf.portfolio",
        &[broken],
        &json!({ "action": "list" }),
        &ctx,
    )
    .expect("broken list");
    assert_eq!(broken_out.warnings, vec![REPAIRED_XREF_WARNING.to_owned()]);
    assert_eq!(broken_out.artifacts.len(), 1);

    // Wrong arity for list (2 inputs instead of 1)
    let d1 = InputDoc::new("a.pdf", fixture_bytes("rotated-pages.pdf"));
    let d2 = InputDoc::new("b.pdf", fixture_bytes("annotations.pdf"));
    let err = run_tool(
        "pdf.portfolio",
        &[d1, d2],
        &json!({ "action": "list" }),
        &ctx,
    )
    .expect_err("list takes 1 input");
    assert!(matches!(err, PdfError::InvalidParams(_)));

    // Encrypted fixture
    let enc_doc = InputDoc::new("encrypted.pdf", make_encrypted_bytes());
    let err = run_tool(
        "pdf.portfolio",
        &[enc_doc],
        &json!({ "action": "list" }),
        &ctx,
    )
    .expect_err("encrypted");
    assert!(matches!(err, PdfError::Encrypted { .. }));
}

#[test]
fn portfolio_create_accepts_non_pdf_payloads_and_preserves_exact_raw_bytes() {
    let ctx = ToolCtx::default();
    let pdf_bytes = fixture_bytes("annotations.pdf");
    let csv_bytes = b"item,count\nwidget,12\n".to_vec();
    let txt_bytes = b"Portfolio manifest note.\n".to_vec();

    let in_pdf = InputDoc::new("annotations.pdf", pdf_bytes.clone());
    let in_csv = InputDoc::new("inventory.csv", csv_bytes.clone());
    let in_txt = InputDoc::new("readme.txt", txt_bytes.clone());

    let created = run_tool(
        "pdf.portfolio",
        &[in_pdf, in_csv, in_txt],
        &json!({
            "action": "create",
            "view": "details"
        }),
        &ctx,
    )
    .expect("create portfolio with mixed payloads");
    let portfolio_bytes = &created.artifacts[0].bytes;

    let listed = run_tool(
        "pdf.portfolio",
        &[InputDoc::new(
            "mixed.portfolio.pdf",
            portfolio_bytes.clone(),
        )],
        &json!({ "action": "list" }),
        &ctx,
    )
    .expect("list mixed portfolio");
    let arr = listed
        .json
        .as_ref()
        .and_then(Value::as_array)
        .expect("inline json array");
    assert_eq!(arr.len(), 3);
    assert_eq!(arr[0]["name"], "annotations.pdf");
    assert_eq!(arr[0]["media_type"], "application/pdf");
    assert_eq!(arr[0]["size"], pdf_bytes.len());
    assert_eq!(
        arr[0]["sha256"],
        "1f6825bbefa620c27ab5a3852cd60f27d18dbfa0fc4bf0e214a1222230d356a9"
    );
    assert_eq!(arr[1]["name"], "inventory.csv");
    assert_eq!(arr[1]["media_type"], "text/csv");
    assert_eq!(arr[1]["size"], csv_bytes.len());
    assert_eq!(arr[2]["name"], "readme.txt");
    assert_eq!(arr[2]["media_type"], "text/plain");
    assert_eq!(arr[2]["size"], txt_bytes.len());

    let extracted = run_tool(
        "pdf.portfolio",
        &[InputDoc::new(
            "mixed.portfolio.pdf",
            portfolio_bytes.clone(),
        )],
        &json!({ "action": "extract" }),
        &ctx,
    )
    .expect("extract mixed portfolio");
    assert_eq!(extracted.artifacts.len(), 3);
    assert_eq!(
        extracted.artifacts[0].name,
        "mixed.portfolio.attachment-001.annotations.pdf"
    );
    assert_eq!(extracted.artifacts[0].format, "pdf");
    assert_eq!(extracted.artifacts[0].bytes, pdf_bytes);
    assert_eq!(
        extracted.artifacts[1].name,
        "mixed.portfolio.attachment-002.inventory.csv"
    );
    assert_eq!(extracted.artifacts[1].format, "csv");
    assert_eq!(extracted.artifacts[1].bytes, csv_bytes);
    assert_eq!(
        extracted.artifacts[2].name,
        "mixed.portfolio.attachment-003.readme.txt"
    );
    assert_eq!(extracted.artifacts[2].format, "txt");
    assert_eq!(extracted.artifacts[2].bytes, txt_bytes);
}

#[test]
fn portfolio_respects_caller_asset_and_decompressed_limits() {
    let ctx = ToolCtx::default();
    let strict_asset = Limits {
        max_asset_bytes: Some(16),
        ..Limits::local()
    };
    let in_big = InputDoc::with_limits(
        "notes.txt",
        b"0123456789abcdefghijklmnopqrstuvwxyz".to_vec(),
        strict_asset.clone(),
    );
    let err_create = run_tool(
        "pdf.portfolio",
        &[in_big],
        &json!({ "action": "create" }),
        &ctx,
    )
    .expect_err("portfolio create exceeding max_asset_bytes must fail");
    assert!(matches!(err_create, PdfError::LimitExceeded(_)));

    let strict_cover = Limits {
        max_input_bytes: Some(64),
        ..Limits::local()
    };
    let in_small = InputDoc::with_limits("a.txt", b"hi".to_vec(), strict_cover);
    let err_cover = run_tool(
        "pdf.portfolio",
        &[in_small],
        &json!({ "action": "create" }),
        &ctx,
    )
    .expect_err("portfolio cover load must respect caller limits");
    assert!(matches!(err_cover, PdfError::LimitExceeded(_)));
}

#[test]
fn snapshot_helper_fails_when_snap_file_is_missing() {
    let missing_name = "definitely_missing_snapshot_for_portfolio_test";
    let snap_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/snapshots")
        .join(format!("portfolio__{missing_name}.snap"));
    assert!(!snap_path.exists());
    let res = std::panic::catch_unwind(|| {
        insta::check_json_snapshot("portfolio", missing_name, &json!([]));
    });
    assert!(res.is_err(), "missing snapshot must panic");
    assert!(
        !snap_path.exists(),
        "missing snapshot must not be silently written"
    );
}
