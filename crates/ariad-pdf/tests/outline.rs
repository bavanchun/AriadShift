//! Integration tests for `pdf.outline`.

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
            $crate::insta::check_json_snapshot("outline", $name, &val);
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
fn outline_export_clear_import_and_from_headings() {
    let ctx = ToolCtx::default();
    let mp_bytes = fixture_bytes("multipage-20.pdf");
    let mp_truth = fixture_truth("multipage-20.truth.json");
    let orig_texts = extract_plain_texts(&mp_bytes);
    let mp_input = InputDoc::new("multipage-20.pdf", mp_bytes);

    // 1. Export on multipage-20.pdf matches truth["outlines"]
    let export_out = run_tool(
        "pdf.outline",
        std::slice::from_ref(&mp_input),
        &json!({ "action": "export" }),
        &ctx,
    )
    .expect("export outlines");
    assert_eq!(export_out.artifacts[0].name, "multipage-20.outline.json");
    assert_eq!(export_out.artifacts[0].format, "json");
    let exported_json: Value =
        serde_json::from_slice(&export_out.artifacts[0].bytes).expect("parse exported outline");
    assert_eq!(exported_json, mp_truth["outlines"]);

    // 2. Clear outlines on multipage-20.pdf
    let clear_out = run_tool(
        "pdf.outline",
        std::slice::from_ref(&mp_input),
        &json!({ "action": "clear" }),
        &ctx,
    )
    .expect("clear outlines");
    let cleared_bytes = &clear_out.artifacts[0].bytes;
    let cleared_info = info::run(cleared_bytes, &Limits::local()).expect("info on cleared pdf");
    assert_eq!(cleared_info.outline_count, 0);
    assert_eq!(extract_plain_texts(cleared_bytes), orig_texts);

    // 3. Import custom nested outline tree onto rotated-pages.pdf (4 pages)
    let rot_bytes = fixture_bytes("rotated-pages.pdf");
    let rot_texts = extract_plain_texts(&rot_bytes);
    let rot_input = InputDoc::new("rotated-pages.pdf", rot_bytes);
    let custom_tree = json!([
        {
            "title": "Chapter 1 — Orientation",
            "page": 1,
            "children": [
                {
                    "title": "Section 1.1 — Landscape",
                    "page": 2,
                    "children": []
                }
            ]
        },
        {
            "title": "Chapter 2 — Inverted",
            "page": 3,
            "children": []
        }
    ]);
    let import_out = run_tool(
        "pdf.outline",
        std::slice::from_ref(&rot_input),
        &json!({
            "action": "import",
            "json": custom_tree
        }),
        &ctx,
    )
    .expect("import outlines");
    let imported_bytes = &import_out.artifacts[0].bytes;
    let imported_info = info::run(imported_bytes, &Limits::local()).expect("info on imported pdf");
    assert_eq!(imported_info.outline_count, 3);
    assert_eq!(extract_plain_texts(imported_bytes), rot_texts);

    insta::assert_json_snapshot!("outline_rotated_pages_imported_info", imported_info);

    let reexport_out = run_tool(
        "pdf.outline",
        &[InputDoc::new("rotated-pages.pdf", imported_bytes.clone())],
        &json!({ "action": "export" }),
        &ctx,
    )
    .expect("re-export outlines");
    let reexported_json: Value =
        serde_json::from_slice(&reexport_out.artifacts[0].bytes).expect("parse re-exported");
    assert_eq!(reexported_json, custom_tree);

    // 4. Generate outlines from headings (`from_headings`) on multipage-20.pdf (after clearing)
    let from_headings_out = run_tool(
        "pdf.outline",
        &[InputDoc::new("multipage-20.pdf", cleared_bytes.clone())],
        &json!({ "action": "from_headings" }),
        &ctx,
    )
    .expect("from_headings outlines");
    let fh_info =
        info::run(&from_headings_out.artifacts[0].bytes, &Limits::local()).expect("fh info");
    assert!(
        fh_info.outline_count > 0,
        "from_headings must populate outline bookmarks"
    );
}

#[test]
fn outline_is_byte_for_byte_deterministic() {
    let ctx = ToolCtx::default();
    let input = InputDoc::new("multipage-20.pdf", fixture_bytes("multipage-20.pdf"));
    let params = json!({
        "action": "import",
        "json": [
            { "title": "Intro", "page": 1, "children": [] },
            { "title": "Outro", "page": 20, "children": [] }
        ]
    });

    let out1 = run_tool("pdf.outline", std::slice::from_ref(&input), &params, &ctx).unwrap();
    let out2 = run_tool("pdf.outline", std::slice::from_ref(&input), &params, &ctx).unwrap();
    assert_eq!(out1.artifacts[0].bytes, out2.artifacts[0].bytes);
}

#[test]
fn outline_rejects_invalid_params_out_of_bounds_page_and_encrypted_inputs() {
    let ctx = ToolCtx::default();
    let input = InputDoc::new("rotated-pages.pdf", fixture_bytes("rotated-pages.pdf"));

    // Wrong arity
    let err = run_tool(
        "pdf.outline",
        &[input.clone(), input.clone()],
        &json!({ "action": "clear" }),
        &ctx,
    )
    .unwrap_err();
    assert!(matches!(err, PdfError::InvalidParams(_)));

    // Out-of-bounds page (page 99 on 4-page PDF)
    let err = run_tool(
        "pdf.outline",
        std::slice::from_ref(&input),
        &json!({
            "action": "import",
            "json": [{ "title": "Too far", "page": 99 }]
        }),
        &ctx,
    )
    .unwrap_err();
    assert!(matches!(err, PdfError::InvalidParams(_)));

    // Missing `json` on `import`
    let err = run_tool(
        "pdf.outline",
        std::slice::from_ref(&input),
        &json!({ "action": "import" }),
        &ctx,
    )
    .unwrap_err();
    assert!(matches!(err, PdfError::InvalidParams(_)));

    // Encrypted input
    let enc_input = InputDoc::new("encrypted.pdf", make_encrypted_bytes());
    let err = run_tool(
        "pdf.outline",
        &[enc_input],
        &json!({ "action": "clear" }),
        &ctx,
    )
    .unwrap_err();
    assert!(matches!(err, PdfError::Encrypted { .. }));
}

#[test]
fn outline_recovers_broken_xref_with_warning() {
    let ctx = ToolCtx::default();
    let input = InputDoc::new("broken-xref.pdf", fixture_bytes("broken-xref.pdf"));
    let out = run_tool(
        "pdf.outline",
        &[input],
        &json!({
            "action": "import",
            "json": [{ "title": "Recovered Page 1", "page": 1 }]
        }),
        &ctx,
    )
    .expect("outline import on broken-xref.pdf");

    assert_eq!(out.warnings, vec![REPAIRED_XREF_WARNING.to_owned()]);
    let report = info::run(&out.artifacts[0].bytes, &Limits::local()).expect("valid rewritten pdf");
    assert!(!report.repaired_xref);
    assert_eq!(report.outline_count, 1);
}
