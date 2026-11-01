//! Integration tests for `pdf.merge`.

use std::{fs, path::PathBuf, time::Instant};

use ariad_core::{
    ir::{Block, Inline},
    limits::Limits,
    reader,
};
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
    let mut out = String::new();
    for block in &ir.document.body {
        if let Block::Paragraph { content } | Block::Heading { content, .. } = block {
            for inline in content {
                if let Inline::Text { text } = inline {
                    out.push_str(text);
                }
            }
            out.push('\n');
        }
    }
    out
}

fn assert_json_snapshot(snap_name: &str, value: &impl serde::Serialize) {
    let snaps_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots");
    let snap_path = snaps_dir.join(format!("merge__{snap_name}.snap"));
    let body = serde_json::to_string_pretty(value).expect("serialize snapshot json");
    let full =
        format!("---\nsource: crates/ariad-pdf/tests/merge.rs\nexpression: report\n---\n{body}\n");
    if std::env::var("INSTA_UPDATE").ok().as_deref() == Some("always") || !snap_path.exists() {
        fs::create_dir_all(&snaps_dir).expect("create snapshots dir");
        fs::write(&snap_path, &full).expect("write snapshot file");
    }
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
