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
    let update = std::env::var("UPDATE_SNAPSHOTS").as_deref() == Ok("1")
        || std::env::var("UPDATE_SNAPSHOTS").as_deref() == Ok("always")
        || std::env::var("INSTA_UPDATE").as_deref() == Ok("always")
        || std::env::var("BLESS_SNAPSHOTS").as_deref() == Ok("1");
    if update {
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

/// Returns every object number defined directly (`N G obj` on its own line) in `raw`, in file
/// order, so tests can assert that no object is written twice.
fn direct_object_numbers(raw: &[u8]) -> Vec<u32> {
    raw.split(|&b| b == b'\n')
        .filter_map(|line| {
            let text = std::str::from_utf8(line).ok()?;
            let mut parts = text.trim_end().split(' ');
            let id: u32 = parts.next()?.parse().ok()?;
            let generation = parts.next()?;
            (generation == "0" && parts.next() == Some("obj") && parts.next().is_none())
                .then_some(id)
        })
        .collect()
}

#[test]
fn decompress_object_streams_keep_emits_referenced_object_streams_and_an_xref_stream() {
    let raw = read_fixture("object-streams.pdf");
    let before_text = extract_plain_text(&raw);
    let ctx = ToolCtx::default();

    let expanded = run_tool(
        "pdf.decompress",
        &[InputDoc::new("object-streams.pdf", raw.clone())],
        &json!({ "object_streams": "expand" }),
        &ctx,
    )
    .expect("decompress expand");
    let expanded_doc = open(&expanded.artifacts[0].bytes, &Limits::local()).expect("open expanded");
    let expanded_objstm_count = expanded_doc
        .inner()
        .objects
        .values()
        .filter(|obj| matches!(obj, lopdf::Object::Stream(s) if s.dict.has_type(b"ObjStm")))
        .count();
    assert_eq!(
        expanded_objstm_count, 0,
        "object_streams=expand must remove /Type /ObjStm streams"
    );

    let kept1 = run_tool(
        "pdf.decompress",
        &[InputDoc::new("object-streams.pdf", raw.clone())],
        &json!({ "object_streams": "keep" }),
        &ctx,
    )
    .expect("decompress keep 1");
    let kept2 = run_tool(
        "pdf.decompress",
        &[InputDoc::new("object-streams.pdf", raw)],
        &json!({ "object_streams": "keep" }),
        &ctx,
    )
    .expect("decompress keep 2");
    let kept_bytes = &kept1.artifacts[0].bytes;
    assert_eq!(
        kept_bytes, &kept2.artifacts[0].bytes,
        "object_streams=keep must be byte-for-byte deterministic"
    );

    // The output is parsed through its own cross-reference stream.
    let parsed = lopdf::Document::load_mem(kept_bytes).expect("parse kept output");
    assert!(
        matches!(
            parsed.reference_table.cross_reference_type,
            lopdf::xref::XrefType::CrossReferenceStream
        ),
        "object_streams=keep must write an xref stream"
    );

    // Every compressed entry names an uncompressed /ObjStm container that is itself a direct
    // object, and each container's /N equals the number of xref entries pointing at it.
    let mut members_per_container: std::collections::BTreeMap<u32, i64> =
        std::collections::BTreeMap::new();
    let mut compressed_ids = Vec::new();
    for (&id, entry) in &parsed.reference_table.entries {
        if let lopdf::xref::XrefEntry::Compressed { container, .. } = entry {
            *members_per_container.entry(*container).or_default() += 1;
            compressed_ids.push(id);
        }
    }
    assert!(
        !members_per_container.is_empty(),
        "xref stream must reference object streams"
    );
    for (&container, &members) in &members_per_container {
        assert!(
            matches!(
                parsed.reference_table.entries.get(&container),
                Some(lopdf::xref::XrefEntry::Normal { .. })
            ),
            "container {container} must be a direct object"
        );
        let Some(lopdf::Object::Stream(stream)) = parsed.objects.get(&(container, 0)) else {
            panic!("container {container} must be a stream object");
        };
        assert!(stream.dict.has_type(b"ObjStm"));
        assert!(
            !stream.dict.has(b"Filter"),
            "kept /ObjStm must be uncompressed"
        );
        assert_eq!(
            stream.dict.get(b"N").and_then(lopdf::Object::as_i64).ok(),
            Some(members),
            "container {container} /N must match its xref members"
        );
    }

    // Each object is defined exactly once: compressed objects have no direct copy, and no
    // direct object number repeats.
    let direct = direct_object_numbers(kept_bytes);
    let unique: std::collections::BTreeSet<u32> = direct.iter().copied().collect();
    assert_eq!(
        unique.len(),
        direct.len(),
        "an object is defined twice: {direct:?}"
    );
    for id in &compressed_ids {
        assert!(
            !unique.contains(id),
            "object {id} is both compressed and written directly"
        );
    }
    for &id in &unique {
        assert!(
            matches!(
                parsed.reference_table.entries.get(&id),
                Some(lopdf::xref::XrefEntry::Normal { .. })
            ),
            "directly written object {id} must have a normal xref entry"
        );
    }

    for obj in parsed.objects.values() {
        if let lopdf::Object::Stream(s) = obj {
            assert!(
                !s.dict.has(b"Filter"),
                "all streams in decompressed PDF (including kept /ObjStm) must be uncompressed"
            );
        }
    }
    assert_eq!(extract_plain_text(kept_bytes), before_text);
}

/// Builds a one-page document whose resource name `/ObjStrX` and content string `ObjStr`
/// resemble the former placeholder bytes, saved with object streams and compressed streams.
fn build_objstr_lookalike_pdf() -> Vec<u8> {
    use lopdf::{Document, Object, Stream, dictionary};

    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
    });
    let content = b"q /ObjStrX gs BT /F1 12 Tf 10 10 Td (ObjStr marker) Tj ET Q\n".to_vec();
    let content_id = doc.add_object(Object::Stream(Stream::new(dictionary! {}, content)));
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => Object::Reference(pages_id),
        "MediaBox" => vec![0.into(), 0.into(), 200.into(), 200.into()],
        "Resources" => dictionary! {
            "Font" => dictionary! { "F1" => Object::Reference(font_id) },
            "ExtGState" => dictionary! {
                "ObjStrX" => dictionary! { "CA" => Object::Real(1.0) },
            },
        },
        "Contents" => Object::Reference(content_id),
    });
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![Object::Reference(page_id)],
            "Count" => 1,
        }),
    );
    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => Object::Reference(pages_id),
    });
    doc.trailer.set("Root", Object::Reference(catalog_id));
    doc.compress();
    let mut out = Vec::new();
    doc.save_modern(&mut out).expect("save with object streams");
    out
}

fn count_occurrences(haystack: &[u8], needle: &[u8]) -> usize {
    haystack
        .windows(needle.len())
        .filter(|w| *w == needle)
        .count()
}

#[test]
fn decompress_keep_and_expand_leave_objstr_lookalike_bytes_untouched() {
    let input = build_objstr_lookalike_pdf();
    assert!(
        document_has_objstm(&input),
        "fixture must contain a real object stream so that keep takes the object-stream path"
    );
    let ctx = ToolCtx::default();

    for mode in ["keep", "expand"] {
        let out = run_tool(
            "pdf.decompress",
            &[InputDoc::new("lookalike.pdf", input.clone())],
            &json!({ "object_streams": mode }),
            &ctx,
        )
        .unwrap_or_else(|e| panic!("decompress {mode}: {e:?}"));
        let bytes = &out.artifacts[0].bytes;

        assert_eq!(
            count_occurrences(bytes, b"/ObjStrX"),
            2,
            "{mode}: /ObjStrX must survive in the content stream and the resource dictionary"
        );
        assert_eq!(
            count_occurrences(bytes, b"/ObjStmX"),
            0,
            "{mode}: /ObjStrX must never be rewritten to /ObjStmX"
        );
        assert_eq!(
            count_occurrences(bytes, b"(ObjStr marker) Tj"),
            1,
            "{mode}: the content string must survive byte for byte"
        );
        assert_eq!(
            count_occurrences(bytes, b"/ObjStr "),
            0,
            "{mode}: no /ObjStr placeholder may appear in the output"
        );

        let parsed = lopdf::Document::load_mem(bytes).expect("parse output");
        let page_id = *parsed.get_pages().values().next().expect("one page");
        let resources = parsed
            .get_page_resources(page_id)
            .expect("page resources")
            .0
            .expect("resources dictionary");
        let ext_g_state = resources
            .get(b"ExtGState")
            .and_then(|o| parsed.dereference(o))
            .and_then(|(_, o)| o.as_dict())
            .expect("ExtGState dictionary");
        assert!(
            ext_g_state.has(b"ObjStrX"),
            "{mode}: resource name ObjStrX must remain"
        );
        assert!(!ext_g_state.has(b"ObjStmX"));
    }
}

fn document_has_objstm(bytes: &[u8]) -> bool {
    lopdf::Document::load_mem(bytes).is_ok_and(|doc| {
        doc.objects
            .values()
            .any(|obj| matches!(obj, lopdf::Object::Stream(s) if s.dict.has_type(b"ObjStm")))
    })
}
