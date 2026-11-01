//! Integration tests for `pdf.split`.

use std::{fs, path::PathBuf};

use ariad_core::{
    ir::{Block, Inline},
    limits::Limits,
    reader,
};
use ariad_pdf::{InputDoc, PdfError, PdfInfo, ToolCtx, ToolOutput, document::open, info, run_tool};
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
    let snap_path = snaps_dir.join(format!("split__{snap_name}.snap"));
    let body = serde_json::to_string_pretty(value).expect("serialize snapshot json");
    let full =
        format!("---\nsource: crates/ariad-pdf/tests/split.rs\nexpression: report\n---\n{body}\n");
    if std::env::var("INSTA_UPDATE").ok().as_deref() == Some("always") || !snap_path.exists() {
        fs::create_dir_all(&snaps_dir).expect("create snapshots dir");
        fs::write(&snap_path, &full).expect("write snapshot file");
    }
    let expected = fs::read_to_string(&snap_path)
        .unwrap_or_else(|e| panic!("failed to read snapshot {}: {e}", snap_path.display()));
    assert_eq!(expected, full, "snapshot mismatch for {snap_name}");
}

fn run_split(inputs: &[InputDoc], params: &Value) -> Result<ToolOutput, PdfError> {
    run_tool("pdf.split", inputs, params, &ToolCtx::default())
}

#[test]
fn split_by_ranges_and_every_mode() {
    let multi = load_input("multipage-20.pdf");

    // 1. Ranges mode
    let out_ranges = run_split(
        std::slice::from_ref(&multi),
        &json!({ "mode": "ranges", "ranges": ["1-3", "10-12", "20"] }),
    )
    .expect("split by ranges");
    assert_eq!(out_ranges.artifacts.len(), 3);
    assert_eq!(out_ranges.artifacts[0].name, "multipage-20.part-001.pdf");
    assert_eq!(out_ranges.artifacts[1].name, "multipage-20.part-002.pdf");
    assert_eq!(out_ranges.artifacts[2].name, "multipage-20.part-003.pdf");

    let r1 = inspect_bytes(&out_ranges.artifacts[0].bytes);
    let r2 = inspect_bytes(&out_ranges.artifacts[1].bytes);
    let r3 = inspect_bytes(&out_ranges.artifacts[2].bytes);
    assert_eq!(r1.page_count, 3);
    assert_eq!(r2.page_count, 3);
    assert_eq!(r3.page_count, 1);

    let t2 = extract_text_from_bytes(&out_ranges.artifacts[1].bytes);
    assert!(t2.contains("Page 10 of 20"));
    assert!(t2.contains("Page 12 of 20"));
    assert!(!t2.contains("Page 9 of 20"));

    // Determinism check.
    let out_ranges_2 = run_split(
        std::slice::from_ref(&multi),
        &json!({ "mode": "ranges", "ranges": ["1-3", "10-12", "20"] }),
    )
    .expect("second split by ranges");
    for (a, b) in out_ranges.artifacts.iter().zip(&out_ranges_2.artifacts) {
        assert_eq!(a.name, b.name);
        assert_eq!(a.bytes, b.bytes);
    }

    // 2. Every mode (`every = 6` -> parts of 6, 6, 6, 2 pages)
    let out_every =
        run_split(&[multi], &json!({ "mode": "every", "every": 6 })).expect("split every 6 pages");
    assert_eq!(out_every.artifacts.len(), 4);
    let counts: Vec<u32> = out_every
        .artifacts
        .iter()
        .map(|f| inspect_bytes(&f.bytes).page_count)
        .collect();
    assert_eq!(counts, vec![6, 6, 6, 2]);
}

#[test]
fn split_by_bookmarks_level_1_and_level_2() {
    let multi = load_input("multipage-20.pdf");

    // Level 1 (default): top-level chapters at pages 1, 6, 11, 16 -> 4 parts of 5 pages each.
    let out_l1 = run_split(
        std::slice::from_ref(&multi),
        &json!({ "mode": "bookmarks" }),
    )
    .expect("split bookmarks level 1");
    assert_eq!(out_l1.artifacts.len(), 4);
    let l1_reports: Vec<PdfInfo> = out_l1
        .artifacts
        .iter()
        .map(|f| inspect_bytes(&f.bytes))
        .collect();
    let l1_counts: Vec<u32> = l1_reports.iter().map(|r| r.page_count).collect();
    assert_eq!(l1_counts, vec![5, 5, 5, 5]);
    // Part 1 has Chapter 1 + Section 1.1 (2 outlines); Part 2 has Chapter 2 + Section 2.1 (2 outlines).
    assert_eq!(l1_reports[0].outline_count, 2);
    assert_eq!(l1_reports[1].outline_count, 2);

    assert_json_snapshot("bookmarks_level1_part1", &l1_reports[0]);

    // Level 2: bookmark starts at pages 1, 2, 6, 8, 11, 16 -> 6 parts of [1, 4, 2, 3, 5, 5] pages.
    let out_l2 = run_split(&[multi], &json!({ "mode": "bookmarks", "level": 2 }))
        .expect("split bookmarks level 2");
    assert_eq!(out_l2.artifacts.len(), 6);
    let l2_counts: Vec<u32> = out_l2
        .artifacts
        .iter()
        .map(|f| inspect_bytes(&f.bytes).page_count)
        .collect();
    assert_eq!(l2_counts, vec![1, 4, 2, 3, 5, 5]);
}

#[test]
fn split_by_size_and_blank_separator() {
    // 1. BlankSeparator mode on `blank-separators.pdf` (6 pages: 1,2 non-blank; 3 blank; 4,5 non-blank; 6 blank)
    let blanks = load_input("blank-separators.pdf");
    let out_blank = run_split(&[blanks], &json!({ "mode": "blank_separator" }))
        .expect("split by blank separator");
    assert_eq!(out_blank.artifacts.len(), 2);
    assert_eq!(out_blank.artifacts[0].name, "blank-separators.part-001.pdf");
    assert_eq!(out_blank.artifacts[1].name, "blank-separators.part-002.pdf");

    let b1 = inspect_bytes(&out_blank.artifacts[0].bytes);
    let b2 = inspect_bytes(&out_blank.artifacts[1].bytes);
    assert_eq!(b1.page_count, 2);
    assert_eq!(b2.page_count, 2);
    assert!(extract_text_from_bytes(&out_blank.artifacts[0].bytes).contains("Part 1, Page 1"));
    assert!(extract_text_from_bytes(&out_blank.artifacts[1].bytes).contains("Part 2, Page 1"));

    // 2. Size mode on `multipage-20.pdf`
    let multi = load_input("multipage-20.pdf");
    let max_bytes = 3_500_u64;
    let out_size = run_split(&[multi], &json!({ "mode": "size", "max_bytes": max_bytes }))
        .expect("split by size");
    assert!(out_size.artifacts.len() >= 2);
    let mut total_pages = 0_u32;
    for file in &out_size.artifacts {
        let rep = inspect_bytes(&file.bytes);
        if rep.page_count > 1 {
            assert!(
                (file.bytes.len() as u64) <= max_bytes,
                "multi-page part {} was {} bytes (> {max_bytes})",
                file.name,
                file.bytes.len()
            );
        }
        total_pages += rep.page_count;
    }
    assert_eq!(total_pages, 20);
}

#[test]
fn split_rejects_invalid_params_and_missing_bookmarks() {
    let single = load_input("annotations.pdf"); // 1 page, 0 bookmarks

    let err_no_bookmarks = run_split(
        std::slice::from_ref(&single),
        &json!({ "mode": "bookmarks" }),
    )
    .unwrap_err();
    assert!(matches!(err_no_bookmarks, PdfError::InvalidParams(_)));

    let err_no_ranges =
        run_split(std::slice::from_ref(&single), &json!({ "mode": "ranges" })).unwrap_err();
    assert!(matches!(err_no_ranges, PdfError::InvalidParams(_)));

    let err_zero_every = run_split(
        std::slice::from_ref(&single),
        &json!({ "mode": "every", "every": 0 }),
    )
    .unwrap_err();
    assert!(matches!(err_zero_every, PdfError::InvalidParams(_)));

    let err_zero_size = run_split(
        std::slice::from_ref(&single),
        &json!({ "mode": "size", "max_bytes": 0 }),
    )
    .unwrap_err();
    assert!(matches!(err_zero_size, PdfError::InvalidParams(_)));

    let err_bad_threshold = run_split(
        &[single],
        &json!({ "mode": "blank_separator", "blank_threshold": 1.5 }),
    )
    .unwrap_err();
    assert!(matches!(err_bad_threshold, PdfError::InvalidParams(_)));
}
