//! Integration tests for `pdf.merge`.

use std::{
    collections::BTreeSet,
    fs,
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
    time::Instant,
};

use ariad_core::{limits::Limits, reader};
use ariad_pdf::{
    InputDoc, PdfError, PdfInfo, REPAIRED_XREF_WARNING, ToolCtx, ToolOutput, document::open, info,
    run_tool,
};
use serde_json::{Value, json};

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/pdf-tools")
}

fn load_input(name: &str) -> InputDoc {
    let path = fixture_dir().join(name);
    let bytes =
        fs::read(&path).unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    let stem = name.strip_suffix(".pdf").unwrap_or(name).to_owned();
    InputDoc::new(stem, bytes)
}

fn inspect_bytes(bytes: &[u8]) -> PdfInfo {
    let doc = open(bytes, &Limits::local()).expect("open output pdf");
    info::from_doc(&doc).expect("inspect output pdf")
}

fn extract_text_from_bytes(bytes: &[u8]) -> String {
    let ir = reader::pdf::read(bytes, &Limits::local()).expect("read output pdf text");
    ariad_core::fidelity::observe(&ir.document)
        .expect("observe output PDF text through transparent inline styles")
        .text
}

fn assert_json_snapshot(snap_name: &str, value: &impl serde::Serialize) {
    let snaps_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots");
    let snap_path = snaps_dir.join(format!("merge__{snap_name}.snap"));
    let body = serde_json::to_string_pretty(value).expect("serialize snapshot json");
    let full =
        format!("---\nsource: crates/ariad-pdf/tests/merge.rs\nexpression: report\n---\n{body}\n");
    let update = std::env::var("UPDATE_SNAPSHOTS").ok().is_some_and(|v| {
        v == "1" || v.eq_ignore_ascii_case("true") || v.eq_ignore_ascii_case("always")
    }) || std::env::var("INSTA_UPDATE").ok().as_deref() == Some("always");
    if update {
        fs::create_dir_all(&snaps_dir).expect("create snapshots dir");
        fs::write(&snap_path, &full).expect("write snapshot file");
    }
    assert!(
        snap_path.exists(),
        "snapshot file {} is missing; set UPDATE_SNAPSHOTS=1 to update snapshots",
        snap_path.display()
    );
    let expected = fs::read_to_string(&snap_path)
        .unwrap_or_else(|e| panic!("failed to read snapshot {}: {e}", snap_path.display()));
    assert_eq!(expected, full, "snapshot mismatch for {snap_name}");
}

fn run_merge(inputs: &[InputDoc], params: &Value) -> Result<ToolOutput, PdfError> {
    run_tool("pdf.merge", inputs, params, &ToolCtx::default())
}

#[test]
fn merge_heterogeneous_pdfs_preserves_geometry_rotation_text_and_determinism() {
    let annot = load_input("annotations.pdf"); // 1 page
    let rotated = load_input("rotated-pages.pdf"); // 4 pages
    let mixed = load_input("mixed-sizes.pdf"); // 3 pages
    let inputs = vec![annot, rotated, mixed];

    let out = run_merge(&inputs, &json!({})).expect("merge should succeed");
    assert_eq!(out.artifacts.len(), 1);
    assert_eq!(out.artifacts[0].name, "annotations.merged.pdf");
    assert!(out.warnings.is_empty());

    let report = inspect_bytes(&out.artifacts[0].bytes);
    assert_eq!(report.page_count, 8);
    // Default outline mode is `per_file` -> 3 top-level outline items.
    assert_eq!(report.outline_count, 3);
    assert_eq!(
        report.metadata.title.as_deref(),
        Some("Annotated Archive Review Sheet")
    );

    // Rotated pages at indices 1..=4 (0-based).
    assert_eq!(report.pages[1].rotation, 0);
    assert_eq!(report.pages[2].rotation, 90);
    assert_eq!(report.pages[2].inherited_rotation, Some(true));
    assert_eq!(report.pages[3].rotation, 180);
    assert_eq!(report.pages[4].rotation, 270);

    // Mixed page sizes at indices 5..=7 (0-based).
    assert_eq!(report.pages[5].paper, "A4");
    assert_eq!(report.pages[6].paper, "Letter");
    assert_eq!(report.pages[7].paper, "A3");

    let text = extract_text_from_bytes(&out.artifacts[0].bytes);
    let pos_annot = text
        .find("Highlighted key term on this line.")
        .expect("annotations content present");
    let pos_rot = text
        .find("Page 1: Explicit /Rotate 0")
        .expect("rotated-pages content present");
    let pos_mixed = text
        .find("Page 1: ISO A4 Portrait")
        .expect("mixed-sizes content present");
    assert!(pos_annot < pos_rot);
    assert!(pos_rot < pos_mixed);

    // Determinism check.
    let out2 = run_merge(&inputs, &json!({})).expect("second merge should succeed");
    assert_eq!(out.artifacts[0].bytes, out2.artifacts[0].bytes);

    assert_json_snapshot("heterogeneous_three_inputs", &report);
}

#[test]
fn merge_outline_and_page_label_modes() {
    let multi = load_input("multipage-20.pdf"); // 20 pages, 6 outlines, 3 page label ranges
    let single = load_input("annotations.pdf"); // 1 page, 0 outlines
    let inputs = vec![multi, single];

    // 1. outline = keep, page_labels = keep
    let out_keep = run_merge(
        &inputs,
        &json!({ "outline": "keep", "page_labels": "keep" }),
    )
    .expect("merge keep/keep");
    let rep_keep = inspect_bytes(&out_keep.artifacts[0].bytes);
    assert_eq!(rep_keep.page_count, 21);
    assert_eq!(rep_keep.outline_count, 6);

    let doc_keep = open(&out_keep.artifacts[0].bytes, &Limits::local()).expect("open keep pdf");
    let cat_keep = doc_keep.inner().catalog().expect("catalog");
    let labels_keep = cat_keep
        .get(b"PageLabels")
        .and_then(|o| o.as_dict())
        .expect("PageLabels dict");
    let nums_keep = labels_keep
        .get(b"Nums")
        .and_then(|o| o.as_array())
        .expect("Nums array");
    // 4 label ranges (0, 2, 18 from multipage-20 + 20 for annotations) => 8 array elements.
    assert_eq!(nums_keep.len(), 8);

    // 2. outline = per_file, page_labels = renumber
    let out_per_file = run_merge(
        &inputs,
        &json!({ "outline": "per_file", "page_labels": "renumber" }),
    )
    .expect("merge per_file/renumber");
    let rep_per_file = inspect_bytes(&out_per_file.artifacts[0].bytes);
    // 2 top-level per-file items + 6 nested items under multipage-20 = 8 total.
    assert_eq!(rep_per_file.outline_count, 8);

    // 3. outline = none
    let out_none = run_merge(&inputs, &json!({ "outline": "none" })).expect("merge outline none");
    let rep_none = inspect_bytes(&out_none.artifacts[0].bytes);
    assert_eq!(rep_none.outline_count, 0);
}

#[test]
fn merge_preserves_and_disambiguates_forms_annotations_and_attachments() {
    let form1 = load_input("form-acroform.pdf");
    let form2 = load_input("form-with-values.pdf");
    let annot = load_input("annotations.pdf");
    let attach1 = load_input("with-attachments.pdf");
    let attach2 = load_input("with-attachments.pdf");

    let out = run_merge(&[form1, form2, annot, attach1, attach2], &json!({}))
        .expect("merge forms/annots/attachments");
    let report = inspect_bytes(&out.artifacts[0].bytes);
    assert_eq!(report.page_count, 5);
    assert!(report.has_forms);
    assert!(report.has_annotations);
    assert!(report.has_attachments);

    // Verify colliding attachment names were disambiguated (`note.txt`, `note_2.txt`, `sample.csv`, `sample_2.csv`).
    let doc = open(&out.artifacts[0].bytes, &Limits::local()).expect("open merged pdf");
    let cat = doc.inner().catalog().expect("catalog");
    let names = cat.get(b"Names").and_then(|o| o.as_dict()).expect("Names");
    let ef_id = names
        .get(b"EmbeddedFiles")
        .and_then(|o| o.as_reference())
        .expect("EmbeddedFiles ref");
    let ef_dict = doc.inner().get_dictionary(ef_id).expect("ef dict");
    let ef_arr = ef_dict
        .get(b"Names")
        .and_then(|o| o.as_array())
        .expect("ef Names array");
    // 4 unique attachments => 8 array elements.
    assert_eq!(ef_arr.len(), 8);
}

#[test]
fn merge_propagates_repaired_xref_warning_and_rewrites_clean_xref() {
    let broken = load_input("broken-xref.pdf");
    let single = load_input("annotations.pdf");
    let out = run_merge(&[broken, single], &json!({})).expect("merge with broken-xref");
    assert_eq!(out.warnings, vec![REPAIRED_XREF_WARNING.to_owned()]);

    let reopened = open(&out.artifacts[0].bytes, &Limits::local()).expect("reopen merged pdf");
    assert!(!reopened.repaired_xref());
    assert_eq!(reopened.inner().get_pages().len(), 2);
}

#[test]
fn merge_rejects_single_input_and_enforces_page_limit() {
    let single = load_input("annotations.pdf");
    let err_one = run_merge(std::slice::from_ref(&single), &json!({})).unwrap_err();
    assert!(matches!(err_one, PdfError::InvalidParams(_)));

    let mut small_limits = Limits::local();
    small_limits.max_pages = Some(5);
    let multi_raw = fs::read(fixture_dir().join("multipage-20.pdf")).expect("read multipage-20");
    let limited_multi = InputDoc::with_limits("multipage-20", multi_raw, small_limits);
    let err_limit = run_merge(&[limited_multi, single], &json!({})).unwrap_err();
    assert!(matches!(err_limit, PdfError::LimitExceeded(_)));

    // Total output pages across inputs exceeding MAX_OUTPUT_PAGES (10,000) must fail with TooManyPages.
    let large = load_input("large-1000-pages.pdf");
    let eleven_thousand: Vec<InputDoc> = (0..11).map(|_| large.clone()).collect();
    let err_total = run_merge(&eleven_thousand, &json!({})).unwrap_err();
    assert!(
        matches!(
            err_total,
            PdfError::LimitExceeded(ariad_core::reader::pdf::ReadError::TooManyPages {
                actual: 11_000,
                limit: 10_000,
            })
        ),
        "expected TooManyPages(11000, 10000), got {err_total:?}"
    );
}

#[test]
fn merge_handles_diamond_name_tree_dag_without_duplication_or_hang() {
    use lopdf::{Document, Object, Stream, StringFormat, dictionary};

    let make_doc_with_diamond_ef = || {
        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let content_id = doc.add_object(Stream::new(dictionary! {}, b"BT (Doc) Tj ET".to_vec()));
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Contents" => content_id,
        });
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![Object::Reference(page_id)],
                "Count" => 1,
            }),
        );

        let ef_stream_id = doc.add_object(Stream::new(dictionary! {}, b"payload".to_vec()));
        let filespec_id = doc.add_object(dictionary! {
            "Type" => "Filespec",
            "F" => Object::String(b"memo.txt".to_vec(), StringFormat::Literal),
            "EF" => dictionary! { "F" => Object::Reference(ef_stream_id) },
        });

        let mut tree_node_id = doc.add_object(dictionary! {
            "Names" => vec![
                Object::String(b"memo.txt".to_vec(), StringFormat::Literal),
                Object::Reference(filespec_id),
            ],
        });
        // 6-level diamond DAG where each `/Kids` repeats `tree_node_id` 4 times (4^6 = 4096 paths).
        for _ in 0..6 {
            tree_node_id = doc.add_object(dictionary! {
                "Kids" => vec![
                    Object::Reference(tree_node_id),
                    Object::Reference(tree_node_id),
                    Object::Reference(tree_node_id),
                    Object::Reference(tree_node_id),
                ],
            });
        }

        let catalog_id = doc.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
            "Names" => dictionary! {
                "EmbeddedFiles" => Object::Reference(tree_node_id),
            },
        });
        doc.trailer.set("Root", catalog_id);
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).expect("save diamond ef pdf");
        bytes
    };

    let d1 = InputDoc::new("d1", make_doc_with_diamond_ef());
    let d2 = load_input("annotations.pdf");
    let out = run_merge(&[d1, d2], &json!({})).expect("merge diamond name tree");
    let merged = open(&out.artifacts[0].bytes, &Limits::local()).expect("open merged");
    let cat = merged.inner().catalog().expect("catalog");
    let names = cat.get(b"Names").and_then(|o| o.as_dict()).expect("Names");
    let ef_id = names
        .get(b"EmbeddedFiles")
        .and_then(|o| o.as_reference())
        .expect("EmbeddedFiles");
    let ef_dict = merged.inner().get_dictionary(ef_id).expect("ef dict");
    let ef_arr = ef_dict
        .get(b"Names")
        .and_then(|o| o.as_array())
        .expect("Names array");
    // Only 1 attachment ("memo.txt") => 2 elements in Names array, not 4096 * 2.
    assert_eq!(ef_arr.len(), 2);
}

#[test]
fn merge_and_split_1000_page_fixture_within_10_seconds() {
    let large = load_input("large-1000-pages.pdf");
    let single = load_input("annotations.pdf");

    let start = Instant::now();
    let merged = run_merge(&[large.clone(), single], &json!({ "outline": "per_file" }))
        .expect("1001-page merge should succeed");
    assert_eq!(merged.artifacts.len(), 1);
    let merged_doc =
        open(&merged.artifacts[0].bytes, &Limits::local()).expect("open 1001-page pdf");
    assert_eq!(merged_doc.inner().get_pages().len(), 1001);

    let split_out = run_tool(
        "pdf.split",
        &[large],
        &json!({ "mode": "every", "every": 250 }),
        &ToolCtx::default(),
    )
    .expect("1000-page split should succeed");
    assert_eq!(split_out.artifacts.len(), 4);
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_secs_f64() < 10.0,
        "1000-page merge + split took {:.2}s (target < 10.0s)",
        elapsed.as_secs_f64()
    );
}

fn build_pdf_with_fields_and_attachments(
    field_names: &[&str],
    attachment_names: &[&str],
    duplicate_first_af: bool,
) -> Vec<u8> {
    use lopdf::{Document, Object, Stream, StringFormat, dictionary};

    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let page_id = doc.new_object_id();
    let content_id = doc.add_object(Stream::new(dictionary! {}, b"BT (Page) Tj ET".to_vec()));

    let mut page_annots = Vec::with_capacity(field_names.len());
    let mut acro_fields = Vec::with_capacity(field_names.len());
    for (idx, &name) in field_names.iter().enumerate() {
        let y = 700 - i64::try_from(idx).unwrap_or(0) * 30;
        let widget_id = doc.add_object(dictionary! {
            "Type" => "Annot",
            "Subtype" => "Widget",
            "FT" => "Tx",
            "T" => Object::String(name.as_bytes().to_vec(), StringFormat::Literal),
            "V" => Object::String(b"val".to_vec(), StringFormat::Literal),
            "Rect" => vec![72.into(), y.into(), 220.into(), (y + 20).into()],
            "P" => Object::Reference(page_id),
        });
        page_annots.push(Object::Reference(widget_id));
        acro_fields.push(Object::Reference(widget_id));
    }

    doc.objects.insert(
        page_id,
        Object::Dictionary(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Contents" => content_id,
            "Annots" => page_annots,
        }),
    );
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![Object::Reference(page_id)],
            "Count" => 1,
        }),
    );

    let mut ef_pairs = Vec::with_capacity(attachment_names.len() * 2);
    let mut af_refs = Vec::with_capacity(attachment_names.len() + usize::from(duplicate_first_af));
    for &att_name in attachment_names {
        let stream_id = doc.add_object(Stream::new(dictionary! {}, att_name.as_bytes().to_vec()));
        let filespec_id = doc.add_object(dictionary! {
            "Type" => "Filespec",
            "F" => Object::String(att_name.as_bytes().to_vec(), StringFormat::Literal),
            "UF" => Object::String(att_name.as_bytes().to_vec(), StringFormat::Literal),
            "EF" => dictionary! { "F" => Object::Reference(stream_id) },
        });
        ef_pairs.push(Object::String(
            att_name.as_bytes().to_vec(),
            StringFormat::Literal,
        ));
        ef_pairs.push(Object::Reference(filespec_id));
        af_refs.push(Object::Reference(filespec_id));
    }
    if duplicate_first_af && let Some(first) = af_refs.first().cloned() {
        af_refs.push(first);
    }

    let ef_tree_id = doc.add_object(dictionary! {
        "Names" => ef_pairs,
    });
    let acro_id = doc.add_object(dictionary! {
        "Fields" => acro_fields,
    });
    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
        "AcroForm" => Object::Reference(acro_id),
        "Names" => dictionary! {
            "EmbeddedFiles" => Object::Reference(ef_tree_id),
        },
        "AF" => af_refs,
    });
    doc.trailer.set("Root", catalog_id);

    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).expect("save synthetic pdf");
    bytes
}

#[test]
fn merge_many_colliding_fields_and_attachments_disambiguates_deterministically() {
    let seed_bytes = build_pdf_with_fields_and_attachments(
        &["shared_2", "shared_3", "other"],
        &["report_2.txt", "report_3.txt", "readme_2", "readme"],
        false,
    );
    let colliding_dup_af_bytes = build_pdf_with_fields_and_attachments(
        &["shared", "other"],
        &["report.txt", "readme"],
        true,
    );
    let colliding_bytes = build_pdf_with_fields_and_attachments(
        &["shared", "other"],
        &["report.txt", "readme"],
        false,
    );

    let mut inputs = Vec::with_capacity(250);
    inputs.push(InputDoc::new("doc-000", seed_bytes));
    inputs.push(InputDoc::new("doc-001", colliding_dup_af_bytes));
    for idx in 2..250 {
        inputs.push(InputDoc::new(
            format!("doc-{idx:03}"),
            colliding_bytes.clone(),
        ));
    }

    let out = run_merge(&inputs, &json!({})).expect("250-doc colliding merge should succeed");
    assert_eq!(out.artifacts.len(), 1);
    let report = inspect_bytes(&out.artifacts[0].bytes);
    assert_eq!(report.page_count, 250);
    assert!(report.has_forms);
    assert!(report.has_attachments);

    let merged = open(&out.artifacts[0].bytes, &Limits::local()).expect("open merged pdf");
    let cat = merged.inner().catalog().expect("catalog");

    // Verify all 501 AcroForm field `/T` names in exact order and uniqueness.
    let acro_id = cat
        .get(b"AcroForm")
        .and_then(|o| o.as_reference())
        .expect("AcroForm ref");
    let acro_dict = merged
        .inner()
        .get_dictionary(acro_id)
        .expect("AcroForm dict");
    let fields_arr = acro_dict
        .get(b"Fields")
        .and_then(|o| o.as_array())
        .expect("Fields array");
    let actual_fields: Vec<String> = fields_arr
        .iter()
        .map(|entry| {
            let fid = entry.as_reference().expect("field ref");
            let fdict = merged.inner().get_dictionary(fid).expect("field dict");
            let raw = fdict
                .get(b"T")
                .and_then(|o| o.as_str())
                .expect("field /T string");
            String::from_utf8(raw.to_vec()).expect("utf-8 field name")
        })
        .collect();

    let mut expected_fields = vec![
        "shared_2".to_owned(),
        "shared_3".to_owned(),
        "other".to_owned(),
        "shared".to_owned(),
        "other_2".to_owned(),
    ];
    for idx in 2..250 {
        expected_fields.push(format!("shared_{}", idx + 2));
        expected_fields.push(format!("other_{}", idx + 1));
    }
    assert_eq!(actual_fields, expected_fields);
    assert_eq!(
        actual_fields.iter().collect::<BTreeSet<_>>().len(),
        501,
        "all merged field names must be unique"
    );

    // Verify all 502 EmbeddedFiles `/Names` entries, uniqueness, and sorted order.
    let names_dict = cat
        .get(b"Names")
        .and_then(|o| o.as_dict())
        .expect("Names dict");
    let ef_id = names_dict
        .get(b"EmbeddedFiles")
        .and_then(|o| o.as_reference())
        .expect("EmbeddedFiles ref");
    let ef_dict = merged.inner().get_dictionary(ef_id).expect("ef dict");
    let ef_arr = ef_dict
        .get(b"Names")
        .and_then(|o| o.as_array())
        .expect("EmbeddedFiles Names array");
    assert_eq!(ef_arr.len(), 502 * 2);

    let actual_attachments: Vec<String> = ef_arr
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            let raw = pair[0].as_str().expect("attachment name string");
            String::from_utf8(raw.to_vec()).expect("utf-8 attachment name")
        })
        .collect();
    assert!(
        actual_attachments.windows(2).all(|w| w[0] < w[1]),
        "EmbeddedFiles /Names entries must be strictly sorted and unique"
    );

    let mut expected_attachments = BTreeSet::new();
    expected_attachments.insert("report.txt".to_owned());
    expected_attachments.insert("report_2.txt".to_owned());
    expected_attachments.insert("report_3.txt".to_owned());
    for suffix in 4..=251 {
        expected_attachments.insert(format!("report_{suffix}.txt"));
    }
    expected_attachments.insert("readme".to_owned());
    for suffix in 2..=251 {
        expected_attachments.insert(format!("readme_{suffix}"));
    }
    let actual_attachment_set: BTreeSet<String> = actual_attachments.into_iter().collect();
    assert_eq!(actual_attachment_set, expected_attachments);

    // Verify `/AF` array deduplicated the repeated ref from input 1 without losing entries.
    let af_arr = cat.get(b"AF").and_then(|o| o.as_array()).expect("AF array");
    assert_eq!(af_arr.len(), 502);
    let af_refs: BTreeSet<(u32, u16)> = af_arr
        .iter()
        .map(|o| o.as_reference().expect("AF entry ref"))
        .collect();
    assert_eq!(af_refs.len(), 502);

    // Verify byte-for-byte determinism.
    let out2 = run_merge(&inputs, &json!({})).expect("second 250-doc merge should succeed");
    assert_eq!(out.artifacts[0].bytes, out2.artifacts[0].bytes);
}

#[test]
fn merge_returns_cancelled_when_tool_ctx_is_cancelled() {
    let form1 = load_input("form-acroform.pdf");
    let form2 = load_input("form-with-values.pdf");
    let cancelled_ctx = ToolCtx::with_cancel(Arc::new(AtomicBool::new(true)));

    let err = run_tool("pdf.merge", &[form1, form2], &json!({}), &cancelled_ctx)
        .expect_err("cancelled ToolCtx must abort pdf.merge");
    assert_eq!(err, PdfError::CANCELLED);
}
