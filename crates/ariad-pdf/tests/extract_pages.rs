//! Integration tests for `pdf.extract_pages`.

use std::{fs, path::PathBuf};

use ariad_core::{
    ir::{Block, Inline},
    limits::Limits,
    reader,
};
use ariad_pdf::{
    InputDoc, PdfError, PdfInfo, REPAIRED_XREF_WARNING, ToolCtx, ToolOutput, document::open, info,
    run_tool,
};
use serde_json::json;

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
    let snap_path = snaps_dir.join(format!("extract_pages__{snap_name}.snap"));
    let body = serde_json::to_string_pretty(value).expect("serialize snapshot json");
    let full = format!(
        "---\nsource: crates/ariad-pdf/tests/extract_pages.rs\nexpression: report\n---\n{body}\n"
    );
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

fn run_extract(inputs: &[InputDoc], pages_expr: &str) -> Result<ToolOutput, PdfError> {
    let params = json!({ "pages": pages_expr });
    run_tool("pdf.extract_pages", inputs, &params, &ToolCtx::default())
}

#[test]
fn extract_pages_subset_and_reordered_duplicates_from_multipage() {
    let input = load_input("multipage-20.pdf");
    let out = run_extract(std::slice::from_ref(&input), "1,6-7,20,1")
        .expect("extract_pages should succeed");
    assert_eq!(out.artifacts.len(), 1);
    assert_eq!(out.artifacts[0].name, "multipage-20.extracted.pdf");
    assert!(out.warnings.is_empty());

    let report = inspect_bytes(&out.artifacts[0].bytes);
    assert_eq!(report.page_count, 5);
    // Outlines on page 1 (Chapter 1) and page 6 (Chapter 2) survive; child outlines on pages 2 & 8 are dropped.
    assert_eq!(report.outline_count, 2);

    let text = extract_text_from_bytes(&out.artifacts[0].bytes);
    let pos_p1_first = text.find("Page 1 of 20").expect("page 1 text present");
    let pos_p6 = text.find("Page 6 of 20").expect("page 6 text present");
    let pos_p7 = text.find("Page 7 of 20").expect("page 7 text present");
    let pos_p20 = text.find("Page 20 of 20").expect("page 20 text present");
    let pos_p1_second = text
        .rfind("Page 1 of 20")
        .expect("duplicate page 1 text present");
    assert!(pos_p1_first < pos_p6);
    assert!(pos_p6 < pos_p7);
    assert!(pos_p7 < pos_p20);
    assert!(pos_p20 < pos_p1_second);
    assert!(!text.contains("Page 2 of 20"));

    // Determinism check: running a second time yields byte-identical PDF output.
    let out2 = run_extract(&[input], "1,6-7,20,1").expect("second run should succeed");
    assert_eq!(out.artifacts[0].bytes, out2.artifacts[0].bytes);

    assert_json_snapshot("multipage_subset_with_duplicates", &report);
}

#[test]
fn extract_pages_preserves_inherited_rotation_and_geometry() {
    let input = load_input("rotated-pages.pdf");
    // Extract page 2 (which inherits 90 deg rotation from /Pages) and page 4 (270 deg).
    let out = run_extract(&[input], "2,4").expect("extract rotated pages");
    let report = inspect_bytes(&out.artifacts[0].bytes);
    assert_eq!(report.page_count, 2);
    assert_eq!(report.pages[0].rotation, 90);
    assert_eq!(report.pages[0].inherited_rotation, Some(true));
    assert_eq!(report.pages[1].rotation, 270);
    assert_eq!(report.pages[1].inherited_rotation, None);

    let mixed = load_input("mixed-sizes.pdf");
    let mixed_out = run_extract(&[mixed], "2,3").expect("extract mixed page sizes");
    let mixed_report = inspect_bytes(&mixed_out.artifacts[0].bytes);
    assert_eq!(mixed_report.page_count, 2);
    assert_eq!(mixed_report.pages[0].paper, "Letter");
    assert_eq!(mixed_report.pages[1].paper, "A3");
}

#[test]
fn extract_pages_preserves_acroform_annotations_and_attachments() {
    let form_in = load_input("form-acroform.pdf");
    let form_out = run_extract(&[form_in], "1").expect("extract form page");
    let form_report = inspect_bytes(&form_out.artifacts[0].bytes);
    assert_eq!(form_report.page_count, 1);
    assert!(form_report.has_forms);
    assert!(form_report.has_annotations);

    let annot_in = load_input("annotations.pdf");
    let annot_out = run_extract(&[annot_in], "1").expect("extract annotations page");
    let annot_report = inspect_bytes(&annot_out.artifacts[0].bytes);
    assert!(annot_report.has_annotations);

    let attach_in = load_input("with-attachments.pdf");
    let attach_out = run_extract(&[attach_in], "1").expect("extract attachment page");
    let attach_report = inspect_bytes(&attach_out.artifacts[0].bytes);
    assert!(attach_report.has_attachments);
}

#[test]
fn extract_pages_repairs_broken_xref_and_emits_clean_pdf() {
    let broken = load_input("broken-xref.pdf");
    let out = run_extract(&[broken], "1").expect("extract from broken-xref");
    assert_eq!(out.warnings, vec![REPAIRED_XREF_WARNING.to_owned()]);

    let reopened = open(&out.artifacts[0].bytes, &Limits::local()).expect("reopen rewritten pdf");
    assert!(
        !reopened.repaired_xref(),
        "rewritten PDF must have valid xref"
    );
    assert_eq!(reopened.inner().get_pages().len(), 1);
    let text = extract_text_from_bytes(&out.artifacts[0].bytes);
    assert!(text.contains("Repairable Cross-Reference Document"));
}

#[test]
fn extract_pages_rejects_invalid_ranges_and_input_counts() {
    let input = load_input("rotated-pages.pdf"); // 4 pages

    // Descending range `4-2` is valid in PageRange and extracts pages 4, 3, 2 in descending order.
    let rev_out = run_extract(std::slice::from_ref(&input), "4-2").expect("descending range valid");
    let rev_report = inspect_bytes(&rev_out.artifacts[0].bytes);
    assert_eq!(rev_report.page_count, 3);
    assert_eq!(rev_report.pages[0].rotation, 270);
    assert_eq!(rev_report.pages[1].rotation, 180);
    assert_eq!(rev_report.pages[2].rotation, 90);

    let err_oob = run_extract(std::slice::from_ref(&input), "1-5").unwrap_err();
    assert!(matches!(err_oob, PdfError::InvalidParams(_)));

    let err_zero = run_extract(std::slice::from_ref(&input), "0").unwrap_err();
    assert!(matches!(err_zero, PdfError::InvalidParams(_)));

    let err_arity = run_extract(&[], "1").unwrap_err();
    assert!(matches!(err_arity, PdfError::InvalidParams(_)));
}

#[test]
fn extract_pages_enforces_max_output_pages_and_caps_range_expansion() {
    let input = load_input("multipage-20.pdf"); // 20 pages
    // 501 repetitions of "all" on a 20-page PDF = 10,020 pages (> 10,000 limit).
    let repeated_all = vec!["all"; 501].join(",");
    let err = run_extract(&[input], &repeated_all).unwrap_err();
    assert!(
        matches!(
            err,
            PdfError::LimitExceeded(ariad_core::reader::pdf::ReadError::TooManyPages {
                actual: 10_020,
                limit: 10_000,
            })
        ),
        "expected TooManyPages limit error, got {err:?}"
    );
}

#[test]
fn extract_pages_handles_deep_indirect_chains_and_diamond_page_labels_dag() {
    use lopdf::{Document, Object, Stream, dictionary};

    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let content_id = doc.add_object(Stream::new(dictionary! {}, b"BT (Hello) Tj ET".to_vec()));

    // Build a deep chain of 4,000 indirect dictionaries attached to the page dictionary.
    let mut chain_id = doc.add_object(dictionary! { "Leaf" => 1 });
    for i in 0..4_000 {
        chain_id = doc.add_object(dictionary! {
            "Step" => i,
            "Next" => Object::Reference(chain_id),
        });
    }

    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        "Contents" => content_id,
        "PieceInfo" => Object::Reference(chain_id),
    });
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![Object::Reference(page_id)],
            "Count" => 1,
        }),
    );

    // Build a 10-level diamond DAG in `/PageLabels /Kids` where each node repeats the next 4 times.
    let mut label_node_id = doc.add_object(dictionary! {
        "Nums" => vec![
            0.into(),
            Object::Dictionary(dictionary! { "S" => "r", "St" => 1 }),
        ],
    });
    for _ in 0..10 {
        label_node_id = doc.add_object(dictionary! {
            "Kids" => vec![
                Object::Reference(label_node_id),
                Object::Reference(label_node_id),
                Object::Reference(label_node_id),
                Object::Reference(label_node_id),
            ],
        });
    }

    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
        "PageLabels" => Object::Reference(label_node_id),
    });
    doc.trailer.set("Root", catalog_id);

    let mut raw = Vec::new();
    doc.save_to(&mut raw).expect("save synthetic pdf");

    // Run on a 512 KiB stack thread and assert it completes within 200ms without stack overflow.
    let handle = std::thread::Builder::new()
        .stack_size(512 * 1024)
        .spawn(move || {
            let start = std::time::Instant::now();
            let input = InputDoc::new("synthetic", raw);
            let out = run_extract(&[input], "1").expect("extract deep chain + diamond labels");
            assert_eq!(out.artifacts.len(), 1);
            assert!(
                start.elapsed() < std::time::Duration::from_millis(200),
                "diamond DAG traversal took too long: {:?}",
                start.elapsed()
            );
        })
        .expect("spawn thread");
    handle.join().expect("thread must not stack-overflow");
}
