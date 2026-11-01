//! Integration tests for `pdf.delete_pages`.

use std::{fs, path::PathBuf};

use ariad_core::{
    ir::{Block, Inline},
    limits::Limits,
    reader,
};
use ariad_pdf::{InputDoc, PdfError, PdfInfo, ToolCtx, ToolOutput, document::open, info, run_tool};
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
    let snap_path = snaps_dir.join(format!("delete_pages__{snap_name}.snap"));
    let body = serde_json::to_string_pretty(value).expect("serialize snapshot json");
    let full = format!(
        "---\nsource: crates/ariad-pdf/tests/delete_pages.rs\nexpression: report\n---\n{body}\n"
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

fn run_delete(inputs: &[InputDoc], pages_expr: &str) -> Result<ToolOutput, PdfError> {
    let params = json!({ "pages": pages_expr });
    run_tool("pdf.delete_pages", inputs, &params, &ToolCtx::default())
}

#[test]
fn delete_pages_removes_selected_ranges_and_preserves_remaining_structure() {
    let input = load_input("multipage-20.pdf");
    // Delete pages 2..=5 and 11..=20 -> surviving pages are 1, 6, 7, 8, 9, 10 (6 pages).
    let out =
        run_delete(std::slice::from_ref(&input), "2-5,11-20").expect("delete_pages should succeed");
    assert_eq!(out.artifacts.len(), 1);
    assert_eq!(out.artifacts[0].name, "multipage-20.trimmed.pdf");
    assert!(out.warnings.is_empty());

    let report = inspect_bytes(&out.artifacts[0].bytes);
    assert_eq!(report.page_count, 6);
    // Outlines on page 1 (Chapter 1), page 6 (Chapter 2), and page 8 (Section 2.1) survive (3 total).
    assert_eq!(report.outline_count, 3);

    let text = extract_text_from_bytes(&out.artifacts[0].bytes);
    assert!(text.contains("Page 1 of 20"));
    assert!(!text.contains("Page 2 of 20"));
    assert!(!text.contains("Page 5 of 20"));
    assert!(text.contains("Page 6 of 20"));
    assert!(text.contains("Page 10 of 20"));
    assert!(!text.contains("Page 11 of 20"));

    // Determinism check.
    let out2 = run_delete(&[input], "2-5,11-20").expect("second run should succeed");
    assert_eq!(out.artifacts[0].bytes, out2.artifacts[0].bytes);

    assert_json_snapshot("multipage_deleted_2_5_and_11_20", &report);
}

#[test]
fn delete_pages_preserves_inherited_rotation_when_deleting_siblings() {
    let input = load_input("rotated-pages.pdf");
    // Delete pages 1 and 3 -> surviving pages are 2 (inherited 90) and 4 (270).
    let out = run_delete(&[input], "1,3").expect("delete pages 1 and 3");
    let report = inspect_bytes(&out.artifacts[0].bytes);
    assert_eq!(report.page_count, 2);
    assert_eq!(report.pages[0].rotation, 90);
    assert_eq!(report.pages[0].inherited_rotation, Some(true));
    assert_eq!(report.pages[1].rotation, 270);
    assert_eq!(report.pages[1].inherited_rotation, None);
}

#[test]
fn delete_pages_rejects_deleting_all_pages_duplicates_and_oob() {
    let rotated = load_input("rotated-pages.pdf"); // 4 pages

    let err_all = run_delete(std::slice::from_ref(&rotated), "1-4").unwrap_err();
    assert!(matches!(err_all, PdfError::InvalidParams(_)));

    let err_dup = run_delete(std::slice::from_ref(&rotated), "2,2").unwrap_err();
    assert!(matches!(err_dup, PdfError::InvalidParams(_)));

    let err_oob = run_delete(std::slice::from_ref(&rotated), "5").unwrap_err();
    assert!(matches!(err_oob, PdfError::InvalidParams(_)));

    let single = load_input("annotations.pdf"); // 1 page
    let err_single = run_delete(&[single], "1").unwrap_err();
    assert!(matches!(err_single, PdfError::InvalidParams(_)));
}
