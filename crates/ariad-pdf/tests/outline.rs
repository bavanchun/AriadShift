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

#[test]
fn outline_export_bounds_cyclic_and_diamond_dag_destinations_and_import_caps_tree_depth() {
    use std::time::{Duration, Instant};

    use lopdf::{Object, dictionary};

    let mut doc = Document::load_mem(&fixture_bytes("rotated-pages.pdf")).expect("load base pdf");
    let page2_id = *doc.get_pages().get(&2).expect("page 2 id");
    let catalog_id = doc
        .trailer
        .get(b"Root")
        .and_then(Object::as_reference)
        .expect("catalog id");

    // 1. Build a 24-level diamond DAG in `/Names /Dests` where each node references the next level
    //    4 times (`4^24` exponential paths if unvisited-DAG traversal is used), ending in a leaf
    //    with `"RealTarget" -> [page2_id /Fit]`, plus a cycle back to the root.
    let leaf_id = doc.add_object(dictionary! {
        "Names" => vec![
            Object::string_literal("RealTarget"),
            Object::Array(vec![Object::Reference(page2_id), Object::Name(b"Fit".to_vec())]),
        ],
    });
    let mut current_id = leaf_id;
    for _ in 0..24 {
        current_id = doc.add_object(dictionary! {
            "Kids" => vec![
                Object::Reference(current_id),
                Object::Reference(current_id),
                Object::Reference(current_id),
                Object::Reference(current_id),
            ],
        });
    }
    let dag_root_id = current_id;
    let cyclic_kid_id = doc.add_object(dictionary! {});
    if let Ok(Object::Dictionary(cyclic_dict)) = doc.get_object_mut(cyclic_kid_id) {
        cyclic_dict.set("Kids", vec![Object::Reference(dag_root_id)]);
    }
    if let Ok(Object::Dictionary(root_dict)) = doc.get_object_mut(dag_root_id)
        && let Ok(Object::Array(kids)) = root_dict.get_mut(b"Kids")
    {
        kids.insert(0, Object::Reference(cyclic_kid_id));
    }

    // 2. Legacy `/Dests` dictionary with mutual name cycle `/LoopA -> (LoopB)` and `/LoopB -> (LoopA)`
    let legacy_dests_id = doc.add_object(dictionary! {
        "LoopA" => Object::string_literal("LoopB"),
        "LoopB" => Object::string_literal("LoopA"),
    });

    // 3. Self-referential `/GoTo` action dictionary `/A << /S /GoTo /D <self_ref> >>`
    let self_action_id = doc.add_object(dictionary! {
        "S" => "GoTo",
    });
    if let Ok(Object::Dictionary(act_dict)) = doc.get_object_mut(self_action_id) {
        act_dict.set("D", Object::Reference(self_action_id));
    }

    // 4. Outline items exercising self-action cycle, legacy named cycle, and diamond DAG lookup
    let outlines_root_id = doc.add_object(dictionary! {
        "Type" => "Outlines",
    });
    let item1_id = doc.add_object(dictionary! {
        "Title" => Object::string_literal("Self Action Cycle"),
        "Parent" => Object::Reference(outlines_root_id),
        "A" => Object::Reference(self_action_id),
    });
    let item2_id = doc.add_object(dictionary! {
        "Title" => Object::string_literal("Named Loop Cycle"),
        "Parent" => Object::Reference(outlines_root_id),
        "Prev" => Object::Reference(item1_id),
        "Dest" => Object::Name(b"LoopA".to_vec()),
    });
    let item3_id = doc.add_object(dictionary! {
        "Title" => Object::string_literal("Diamond Resolved Target"),
        "Parent" => Object::Reference(outlines_root_id),
        "Prev" => Object::Reference(item2_id),
        "Dest" => Object::string_literal("RealTarget"),
    });
    if let Ok(Object::Dictionary(d1)) = doc.get_object_mut(item1_id) {
        d1.set("Next", Object::Reference(item2_id));
    }
    if let Ok(Object::Dictionary(d2)) = doc.get_object_mut(item2_id) {
        d2.set("Next", Object::Reference(item3_id));
    }
    if let Ok(Object::Dictionary(outlines_dict)) = doc.get_object_mut(outlines_root_id) {
        outlines_dict.set("First", Object::Reference(item1_id));
        outlines_dict.set("Last", Object::Reference(item3_id));
        outlines_dict.set("Count", 3);
    }
    if let Ok(Object::Dictionary(cat)) = doc.get_object_mut(catalog_id) {
        cat.set("Outlines", Object::Reference(outlines_root_id));
        cat.set("Dests", Object::Reference(legacy_dests_id));
        cat.set(
            "Names",
            dictionary! {
                "Dests" => Object::Reference(dag_root_id),
            },
        );
    }

    let mut pdf_bytes = Vec::new();
    doc.save_to(&mut pdf_bytes).expect("save cyclic/dag pdf");

    let ctx = ToolCtx::with_deadline(Instant::now() + Duration::from_secs(2));
    let out = run_tool(
        "pdf.outline",
        &[InputDoc::new("cyclic-dag.pdf", pdf_bytes)],
        &json!({ "action": "export" }),
        &ctx,
    )
    .expect("export outlines on cyclic/dag pdf within bounded steps");

    let exported: Value =
        serde_json::from_slice(&out.artifacts[0].bytes).expect("parse exported outline json");
    assert_eq!(
        exported,
        json!([
            { "title": "Self Action Cycle", "page": 1, "children": [] },
            { "title": "Named Loop Cycle", "page": 1, "children": [] },
            { "title": "Diamond Resolved Target", "page": 2, "children": [] }
        ])
    );

    // 5. Importing an outline hierarchy deeper than MAX_TREE_DEPTH (64) must fail with InvalidParams
    let mut deep_node = json!({
        "title": "Leaf",
        "page": 1,
        "children": []
    });
    for level in 0..80 {
        deep_node = json!({
            "title": format!("Level {level}"),
            "page": 1,
            "children": [deep_node]
        });
    }
    let import_err = run_tool(
        "pdf.outline",
        &[InputDoc::new(
            "rotated-pages.pdf",
            fixture_bytes("rotated-pages.pdf"),
        )],
        &json!({
            "action": "import",
            "json": [deep_node]
        }),
        &ToolCtx::default(),
    )
    .expect_err("reject overly deep outline import");
    assert!(
        matches!(import_err, PdfError::InvalidParams(_)),
        "expected PdfError::InvalidParams on excessive outline depth, got {import_err:?}"
    );
}

#[test]
fn outline_from_headings_enforces_decode_budget_and_tool_ctx_deadline() {
    use std::time::{Duration, Instant};

    let bytes = fixture_bytes("multipage-20.pdf");

    // 1. Tight decompressed-bytes budget must be enforced by `from_headings`
    let mut tight_limits = Limits::local();
    tight_limits.max_decompressed_bytes = 64;
    let tight_input = InputDoc::with_limits("multipage-20.pdf", bytes.clone(), tight_limits);
    let err = run_tool(
        "pdf.outline",
        &[tight_input],
        &json!({ "action": "from_headings" }),
        &ToolCtx::default(),
    )
    .expect_err("from_headings must enforce DecodeBudget");
    assert!(
        matches!(err, PdfError::LimitExceeded(_)),
        "expected PdfError::LimitExceeded from from_headings under tight budget, got {err:?}"
    );

    // 2. Expired deadline on ToolCtx must abort `from_headings` with TIMED_OUT
    let expired_ctx = ToolCtx::with_deadline(Instant::now() - Duration::from_millis(1));
    let timeout_err = run_tool(
        "pdf.outline",
        &[InputDoc::new("multipage-20.pdf", bytes)],
        &json!({ "action": "from_headings" }),
        &expired_ctx,
    )
    .expect_err("from_headings must honour expired ToolCtx deadline");
    assert_eq!(timeout_err, PdfError::TIMED_OUT);
}

/// Links `items` (already inserted objects) into a flat sibling list under a new `/Outlines` root
/// and registers the root in the catalog.
fn attach_flat_outline(doc: &mut Document, items: &[lopdf::ObjectId]) {
    use lopdf::{Object, dictionary};

    let root_id = doc.add_object(dictionary! { "Type" => "Outlines" });
    for (idx, id) in items.iter().enumerate() {
        if let Ok(Object::Dictionary(d)) = doc.get_object_mut(*id) {
            d.set("Parent", Object::Reference(root_id));
            if idx > 0 {
                d.set("Prev", Object::Reference(items[idx - 1]));
            }
            if idx + 1 < items.len() {
                d.set("Next", Object::Reference(items[idx + 1]));
            }
        }
    }
    let catalog_id = doc
        .trailer
        .get(b"Root")
        .and_then(Object::as_reference)
        .expect("catalog id");
    if let Ok(Object::Dictionary(root)) = doc.get_object_mut(root_id) {
        root.set("First", Object::Reference(items[0]));
        root.set("Last", Object::Reference(items[items.len() - 1]));
        root.set("Count", i64::try_from(items.len()).unwrap_or(i64::MAX));
    }
    if let Ok(Object::Dictionary(cat)) = doc.get_object_mut(catalog_id) {
        cat.set("Outlines", Object::Reference(root_id));
    }
}

fn export_outline_value(bytes: Vec<u8>) -> (Value, Vec<String>) {
    let out = run_tool(
        "pdf.outline",
        &[InputDoc::new("outline-source.pdf", bytes)],
        &json!({ "action": "export" }),
        &ToolCtx::default(),
    )
    .expect("export outline");
    // `ToolOutput::json` is the already-structured value; a 64-level export is deeper than
    // `serde_json` accepts when parsing the serialized bytes back.
    (out.json.expect("outline export json"), out.warnings)
}

#[test]
fn outline_export_resolves_many_named_destinations_from_a_large_name_tree() {
    use lopdf::{Object, dictionary};

    const ITEMS: usize = 20_000;
    const LEAVES: usize = 40;
    const NAMES_PER_LEAF: usize = 1_000;
    const NAMES: usize = LEAVES * NAMES_PER_LEAF;

    let mut doc = Document::load_mem(&fixture_bytes("multipage-20.pdf")).expect("load base pdf");
    let page_ids: Vec<lopdf::ObjectId> = doc.get_pages().into_values().collect();
    assert_eq!(page_ids.len(), 20);

    let mut leaves = Vec::with_capacity(LEAVES);
    for leaf in 0..LEAVES {
        let mut pairs = Vec::with_capacity(NAMES_PER_LEAF * 2);
        for name in leaf * NAMES_PER_LEAF..(leaf + 1) * NAMES_PER_LEAF {
            pairs.push(Object::string_literal(format!("dest-{name:05}")));
            pairs.push(Object::Array(vec![
                Object::Reference(page_ids[name % page_ids.len()]),
                Object::Name(b"Fit".to_vec()),
            ]));
        }
        leaves.push(Object::Reference(
            doc.add_object(dictionary! { "Names" => pairs }),
        ));
    }
    let tree_root = doc.add_object(dictionary! { "Kids" => leaves });

    let target_of = |item: usize| (item * 7_919) % NAMES;
    let mut items = Vec::with_capacity(ITEMS);
    for item in 0..ITEMS {
        items.push(doc.add_object(dictionary! {
            "Title" => Object::string_literal(format!("Item {item}")),
            "Dest" => Object::string_literal(format!("dest-{:05}", target_of(item))),
        }));
    }
    attach_flat_outline(&mut doc, &items);
    let catalog_id = doc
        .trailer
        .get(b"Root")
        .and_then(Object::as_reference)
        .expect("catalog id");
    if let Ok(Object::Dictionary(cat)) = doc.get_object_mut(catalog_id) {
        cat.set(
            "Names",
            dictionary! { "Dests" => Object::Reference(tree_root) },
        );
    }
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).expect("save named destination pdf");

    let (exported, warnings) = export_outline_value(bytes);
    assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");
    let nodes = exported.as_array().expect("outline array");
    assert_eq!(nodes.len(), ITEMS);
    for (item, node) in nodes.iter().enumerate() {
        assert_eq!(node["title"], format!("Item {item}"));
        let expected_page = target_of(item) % 20 + 1;
        assert_eq!(
            node["page"], expected_page,
            "item {item} should resolve through the name tree"
        );
    }
}

#[test]
fn outline_from_headings_takes_reader_limits_from_the_input_limits() {
    use ariad_core::{reader::pdf::ReadError, tools::params::outline::OutlineParams};
    use ariad_pdf::tools::outline;

    let params: OutlineParams =
        serde_json::from_value(json!({ "action": "from_headings" })).expect("params");
    let bytes = fixture_bytes("multipage-20.pdf");

    // The default limits read the fixture fine, so the failure below comes from the small cap.
    let (_, outputs) = outline::run_inputs(
        &[InputDoc::new("multipage-20.pdf", bytes.clone())],
        &params,
        &ToolCtx::default(),
    )
    .expect("from_headings under default limits");
    assert_eq!(outputs.len(), 1);

    let mut small_blocks = Limits::local();
    small_blocks.max_blocks = 2;
    let err = outline::run_inputs(
        &[InputDoc::with_limits(
            "multipage-20.pdf",
            bytes.clone(),
            small_blocks,
        )],
        &params,
        &ToolCtx::default(),
    )
    .expect_err("max_blocks from the caller must reach the PDF reader");
    assert_eq!(
        err,
        PdfError::LimitExceeded(ReadError::TooManyBlocks { limit: 2 })
    );

    // A limit the reader validates must also come from the caller, not from `Limits::local()`.
    let mut zero_ir = Limits::local();
    zero_ir.max_ir_json_bytes = 0;
    assert!(
        outline::run_inputs(
            &[InputDoc::with_limits(
                "multipage-20.pdf",
                bytes.clone(),
                zero_ir
            )],
            &params,
            &ToolCtx::default(),
        )
        .is_err(),
        "an invalid caller max_ir_json_bytes must not be replaced by the local default"
    );

    let mut list_doc = Document::load_mem(&bytes).expect("load pdf for list item");
    let page1_id = *list_doc.get_pages().get(&1).expect("page 1");
    let list_stream = list_doc.add_object(lopdf::Stream::new(
        lopdf::dictionary! {},
        b"BT /F1 12 Tf 72 720 Td (1. Nested item) Tj ET\n".to_vec(),
    ));
    if let Ok(lopdf::Object::Dictionary(page1)) = list_doc.get_object_mut(page1_id) {
        page1.set("Contents", lopdf::Object::Reference(list_stream));
    }
    let mut list_bytes = Vec::new();
    list_doc.save_to(&mut list_bytes).expect("save list pdf");

    let mut zero_nesting = Limits::local();
    zero_nesting.max_nesting_depth = 0;
    let nesting_err = outline::run_inputs(
        &[InputDoc::with_limits(
            "multipage-20.pdf",
            list_bytes,
            zero_nesting,
        )],
        &params,
        &ToolCtx::default(),
    )
    .expect_err("max_nesting_depth from the caller must reach the PDF reader");
    assert_eq!(
        nesting_err,
        PdfError::LimitExceeded(ReadError::NestingTooDeep { limit: 0 })
    );

    // Pre-opened PdfDoc with a small caller `max_pages` reaches `detect_heading_outlines`
    // and is rejected by `reader::pdf::read`.
    let opened =
        ariad_pdf::document::open(&bytes, &Limits::local()).expect("open under default limits");
    let mut doc_input = InputDoc::from_doc("multipage-20.pdf", opened);
    doc_input.limits.max_pages = Some(2);
    let page_err = outline::run_inputs(&[doc_input], &params, &ToolCtx::default())
        .expect_err("max_pages from the caller must reach the PDF reader");
    assert_eq!(
        page_err,
        PdfError::LimitExceeded(ReadError::TooManyPages {
            actual: 20,
            limit: 2,
        })
    );
}

#[test]
fn outline_export_warns_when_nesting_deeper_than_64_levels_is_dropped() {
    use lopdf::{Object, dictionary};

    let mut doc = Document::load_mem(&fixture_bytes("rotated-pages.pdf")).expect("load base pdf");
    let page1 = *doc.get_pages().get(&1).expect("page 1");
    let levels = 70usize;
    let ids: Vec<lopdf::ObjectId> = (0..levels)
        .map(|level| {
            doc.add_object(dictionary! {
                "Title" => Object::string_literal(format!("Level {level}")),
                "Dest" => vec![Object::Reference(page1), Object::Name(b"Fit".to_vec())],
            })
        })
        .collect();
    for level in 0..levels {
        let parent = if level == 0 {
            None
        } else {
            Some(ids[level - 1])
        };
        if let Ok(Object::Dictionary(d)) = doc.get_object_mut(ids[level]) {
            if let Some(parent) = parent {
                d.set("Parent", Object::Reference(parent));
            }
            if level + 1 < levels {
                d.set("First", Object::Reference(ids[level + 1]));
                d.set("Last", Object::Reference(ids[level + 1]));
            }
        }
    }
    attach_flat_outline(&mut doc, &ids[..1]);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).expect("save deep outline pdf");

    let (exported, warnings) = export_outline_value(bytes);

    let mut depth = 0usize;
    let mut cursor = &exported;
    while let Some(first) = cursor.as_array().and_then(|a| a.first()) {
        depth += 1;
        cursor = &first["children"];
    }
    assert_eq!(depth, 64, "export keeps exactly 64 nesting levels");

    let truncation: Vec<&String> = warnings
        .iter()
        .filter(|w| w.contains("truncated"))
        .collect();
    assert_eq!(truncation.len(), 1, "warnings: {warnings:?}");
    assert!(truncation[0].contains("64 levels"), "{}", truncation[0]);
    assert!(truncation[0].contains("Level 63"), "{}", truncation[0]);
    assert!(truncation[0].contains("page 1"), "{}", truncation[0]);
}

#[test]
fn outline_export_warns_when_the_bookmark_count_cap_stops_the_walk() {
    use lopdf::{Object, dictionary};

    const CAP: usize = 100_000;

    let mut doc = Document::load_mem(&fixture_bytes("rotated-pages.pdf")).expect("load base pdf");
    let page1 = *doc.get_pages().get(&1).expect("page 1");
    let items: Vec<lopdf::ObjectId> = (0..=CAP)
        .map(|item| {
            doc.add_object(dictionary! {
                "Title" => Object::string_literal(format!("Item {item}")),
                "Dest" => vec![Object::Reference(page1), Object::Name(b"Fit".to_vec())],
            })
        })
        .collect();
    attach_flat_outline(&mut doc, &items);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).expect("save wide outline pdf");

    let (exported, warnings) = export_outline_value(bytes);
    assert_eq!(exported.as_array().expect("array").len(), CAP);

    let truncation: Vec<&String> = warnings
        .iter()
        .filter(|w| w.contains("truncated"))
        .collect();
    assert_eq!(truncation.len(), 1, "warnings: {warnings:?}");
    assert!(truncation[0].contains("100000"), "{}", truncation[0]);
    assert!(truncation[0].contains("Item 99999"), "{}", truncation[0]);
    assert!(!truncation[0].contains("Item 100000"), "{}", truncation[0]);
}

#[test]
fn outline_export_of_a_complete_outline_has_no_truncation_warning() {
    let imported = run_tool(
        "pdf.outline",
        &[InputDoc::new(
            "multipage-20.pdf",
            fixture_bytes("multipage-20.pdf"),
        )],
        &json!({
            "action": "import",
            "json": [{ "title": "Part", "page": 2, "children": [
                { "title": "Chapter", "page": 3, "children": [] }
            ] }]
        }),
        &ToolCtx::default(),
    )
    .expect("import small outline");

    let (exported, warnings) = export_outline_value(imported.artifacts[0].bytes.clone());
    assert_eq!(
        exported,
        json!([{ "title": "Part", "page": 2, "children": [
            { "title": "Chapter", "page": 3, "children": [] }
        ] }])
    );
    assert!(warnings.is_empty(), "warnings: {warnings:?}");
}

#[test]
fn outline_export_bounds_shared_names_dag_and_enforces_named_destination_cap() {
    use ariad_core::reader::pdf::ReadError;
    use lopdf::{Object, dictionary};

    // 1. 600 distinct kid dictionaries all sharing one 200-pair `/Names` array (so an unvisited
    //    `/Names` walk would expand 120,000 entries and trip the 100,000 cap, whereas tracking
    //    visited `/Names` by ObjectId visits the 200 pairs exactly once), plus a 1,000-element
    //    destination array.
    let mut doc = Document::load_mem(&fixture_bytes("rotated-pages.pdf")).expect("load base pdf");
    let page3_id = *doc.get_pages().get(&3).expect("page 3 id");
    let mut oversized_dest = vec![
        Object::Reference(page3_id),
        Object::Name(b"FitR".to_vec()),
        Object::Integer(0),
        Object::Integer(0),
        Object::Integer(200),
        Object::Integer(300),
    ];
    oversized_dest.extend((0..994).map(Object::Integer));
    let dest_arr_id = doc.add_object(Object::Array(oversized_dest));

    let mut shared_pairs = Vec::with_capacity(400);
    for i in 0..200 {
        shared_pairs.push(Object::string_literal(format!("shared-dest-{i:03}")));
        shared_pairs.push(Object::Reference(dest_arr_id));
    }
    let shared_names_id = doc.add_object(Object::Array(shared_pairs));

    let mut kids = Vec::with_capacity(600);
    for _ in 0..600 {
        kids.push(Object::Reference(doc.add_object(dictionary! {
            "Names" => Object::Reference(shared_names_id),
        })));
    }
    let tree_root = doc.add_object(dictionary! { "Kids" => kids });

    let bm_item = doc.add_object(dictionary! {
        "Title" => Object::string_literal("Shared Target"),
        "Dest" => Object::string_literal("shared-dest-199"),
    });
    attach_flat_outline(&mut doc, &[bm_item]);
    let catalog_id = doc
        .trailer
        .get(b"Root")
        .and_then(Object::as_reference)
        .expect("catalog id");
    if let Ok(Object::Dictionary(cat)) = doc.get_object_mut(catalog_id) {
        cat.set(
            "Names",
            dictionary! { "Dests" => Object::Reference(tree_root) },
        );
    }
    let mut shared_bytes = Vec::new();
    doc.save_to(&mut shared_bytes)
        .expect("save shared names pdf");

    let (exported, warnings) = export_outline_value(shared_bytes);
    assert!(warnings.is_empty());
    assert_eq!(
        exported,
        json!([{ "title": "Shared Target", "page": 3, "children": [] }])
    );

    // 2. Exceeding 100,000 distinct named destinations fails with `TooManyObjects { limit: 100_000 }`.
    let mut cap_doc =
        Document::load_mem(&fixture_bytes("rotated-pages.pdf")).expect("load base pdf");
    let cap_page1 = *cap_doc.get_pages().get(&1).expect("page 1 id");
    let cap_dest_id = cap_doc.add_object(Object::Array(vec![
        Object::Reference(cap_page1),
        Object::Name(b"Fit".to_vec()),
    ]));
    let mut cap_kids = Vec::with_capacity(101);
    for leaf in 0..100 {
        let mut pairs = Vec::with_capacity(2_000);
        for idx in 0..1_000 {
            pairs.push(Object::string_literal(format!("d-{leaf:03}-{idx:03}")));
            pairs.push(Object::Reference(cap_dest_id));
        }
        cap_kids.push(Object::Reference(
            cap_doc.add_object(dictionary! { "Names" => pairs }),
        ));
    }
    cap_kids.push(Object::Reference(cap_doc.add_object(dictionary! {
        "Names" => vec![
            Object::string_literal("d-overflow"),
            Object::Reference(cap_dest_id),
        ],
    })));
    let cap_tree_root = cap_doc.add_object(dictionary! { "Kids" => cap_kids });
    let cap_bm = cap_doc.add_object(dictionary! {
        "Title" => Object::string_literal("Overflow Target"),
        "Dest" => Object::string_literal("d-000-000"),
    });
    attach_flat_outline(&mut cap_doc, &[cap_bm]);
    let cap_catalog_id = cap_doc
        .trailer
        .get(b"Root")
        .and_then(Object::as_reference)
        .expect("catalog id");
    if let Ok(Object::Dictionary(cat)) = cap_doc.get_object_mut(cap_catalog_id) {
        cat.set(
            "Names",
            dictionary! { "Dests" => Object::Reference(cap_tree_root) },
        );
    }
    let mut cap_bytes = Vec::new();
    cap_doc.save_to(&mut cap_bytes).expect("save over-cap pdf");

    let err = run_tool(
        "pdf.outline",
        &[InputDoc::new("over-cap.pdf", cap_bytes)],
        &json!({ "action": "export" }),
        &ToolCtx::default(),
    )
    .expect_err("outline export must reject named destinations past 100,000 cap");
    assert_eq!(
        err,
        PdfError::LimitExceeded(ReadError::TooManyObjects { limit: 100_000 })
    );
}
