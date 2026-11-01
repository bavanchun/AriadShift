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

#[test]
fn split_strips_catalog_embedded_files_and_af_from_parts() {
    let attach = load_input("with-attachments.pdf");
    let rotated = load_input("rotated-pages.pdf");
    let merged = run_tool(
        "pdf.merge",
        &[attach, rotated],
        &json!({}),
        &ToolCtx::default(),
    )
    .expect("merge attachment pdf with rotated pages");
    let merged_input = InputDoc::new("merged-with-attach", merged.artifacts[0].bytes.clone());
    assert!(inspect_bytes(&merged.artifacts[0].bytes).has_attachments);

    let split_out = run_split(&[merged_input], &json!({ "mode": "every", "every": 1 }))
        .expect("split every 1 page");
    assert_eq!(split_out.artifacts.len(), 5);
    for part in &split_out.artifacts {
        let rep = inspect_bytes(&part.bytes);
        assert!(
            !rep.has_attachments,
            "split part {} must not duplicate catalog-level EmbeddedFiles/AF",
            part.name
        );
    }
}

mod output_caps {
    use ariad_core::reader::pdf::ReadError;
    use lopdf::{Document, Object, Stream, dictionary};

    use super::*;

    /// Builds `page_count` pages that all reference one shared, incompressible stream of
    /// `shared_bytes` bytes through their `/Resources`.
    fn doc_with_shared_resource(page_count: usize, shared_bytes: usize) -> Vec<u8> {
        let mut doc = Document::with_version("1.7");
        let pages_id = doc.new_object_id();
        let mut state = 0x2545_f491_u32;
        let payload: Vec<u8> = (0..shared_bytes)
            .map(|_| {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (state >> 24) as u8
            })
            .collect();
        let shared = doc.add_object(Stream::new(dictionary! { "Subtype" => "Image" }, payload));
        let mut kids = Vec::new();
        for n in 1..=page_count {
            let content = doc.add_object(Stream::new(
                dictionary! {},
                format!("BT (Page {n}) Tj ET").into_bytes(),
            ));
            kids.push(Object::Reference(doc.add_object(dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
                "Contents" => content,
                "Resources" => dictionary! {
                    "XObject" => dictionary! { "Shared" => shared },
                },
            })));
        }
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => kids,
                "Count" => i64::try_from(page_count).unwrap(),
            }),
        );
        let cat_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        doc.trailer.set("Root", cat_id);
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).expect("save synthetic pdf");
        bytes
    }

    #[test]
    fn ranges_over_the_cumulative_page_cap_fail_before_building_parts() {
        let input = InputDoc::new("big", doc_with_shared_resource(1_000, 16));
        // 11 ranges of 1,000 pages each: every range is below the per-range cap, the total is not.
        let ranges: Vec<&str> = vec!["1-1000"; 11];
        let Err(err) = run_split(&[input], &json!({ "mode": "ranges", "ranges": ranges })) else {
            panic!("11,000 cumulative output pages must be refused");
        };
        match err {
            PdfError::LimitExceeded(ReadError::TooManyPages { actual, limit }) => {
                assert_eq!(limit, 10_000);
                assert_eq!(actual, 11_000);
            }
            other => panic!("expected TooManyPages, got {other:?}"),
        }
    }

    #[test]
    fn every_one_with_large_shared_resource_exhausts_the_decode_budget() {
        let bytes = doc_with_shared_resource(24, 256 * 1024);
        let mut limits = Limits::local();
        limits.max_decompressed_bytes = 1024 * 1024;
        let constrained = InputDoc::with_limits("shared", bytes.clone(), limits);
        let Err(err) = run_split(&[constrained], &json!({ "mode": "every", "every": 1 })) else {
            panic!("24 parts of ~256 KiB must exceed a 1 MiB budget");
        };
        assert!(
            matches!(
                err,
                PdfError::LimitExceeded(ReadError::DecompressedTooLarge { .. })
            ),
            "expected the decode budget error, got {err:?}"
        );

        // The same document splits normally under the default limits.
        let ok = run_split(
            &[InputDoc::new("shared", bytes)],
            &json!({ "mode": "every", "every": 1 }),
        )
        .expect("split under default limits");
        assert_eq!(ok.artifacts.len(), 24);
    }

    #[test]
    fn size_mode_charges_serialized_parts_against_the_decode_budget() {
        let bytes = doc_with_shared_resource(24, 256 * 1024);
        let mut limits = Limits::local();
        limits.max_decompressed_bytes = 1024 * 1024;
        let constrained = InputDoc::with_limits("shared", bytes, limits);
        let Err(err) = run_split(
            &[constrained],
            &json!({ "mode": "size", "max_bytes": 100_000 }),
        ) else {
            panic!("one part per page exceeds a 1 MiB budget");
        };
        assert!(
            matches!(
                err,
                PdfError::LimitExceeded(ReadError::DecompressedTooLarge { .. })
            ),
            "expected the decode budget error, got {err:?}"
        );
    }

    #[test]
    fn caller_max_pages_limit_and_tagged_structure_warning_are_enforced() {
        let bytes = doc_with_shared_resource(4, 16);
        let mut limits = Limits::local();
        limits.max_pages = Some(6);
        let constrained = InputDoc::with_limits("small", bytes, limits);

        // 2 repetitions of 4 pages = 8 output pages > limits.max_pages (6).
        for (tool_id, params) in [
            ("pdf.extract_pages", json!({ "pages": "1-last,1-last" })),
            ("pdf.reorder", json!({ "order": "1-last,1-last" })),
            (
                "pdf.split",
                json!({ "mode": "ranges", "ranges": ["1-last", "1-last"] }),
            ),
        ] {
            let err = run_tool(
                tool_id,
                std::slice::from_ref(&constrained),
                &params,
                &ToolCtx::default(),
            )
            .expect_err("output pages exceeding limits.max_pages must fail");
            assert_eq!(
                err,
                PdfError::LimitExceeded(ReadError::TooManyPages {
                    actual: 8,
                    limit: 6,
                }),
                "unexpected error for {tool_id}"
            );
        }

        // Splitting a tagged PDF emits the tagged-structure warning once on the ToolOutput.
        let mut tagged_doc = Document::load_mem(&doc_with_shared_resource(2, 16)).unwrap();
        let struct_root = tagged_doc.add_object(dictionary! { "Type" => "StructTreeRoot" });
        let cat_id = tagged_doc
            .trailer
            .get(b"Root")
            .unwrap()
            .as_reference()
            .unwrap();
        if let Ok(cat) = tagged_doc.get_dictionary_mut(cat_id) {
            cat.set("StructTreeRoot", Object::Reference(struct_root));
            cat.set("MarkInfo", dictionary! { "Marked" => true });
        }
        let mut tagged_bytes = Vec::new();
        tagged_doc.save_to(&mut tagged_bytes).unwrap();
        let out = run_split(
            &[InputDoc::new("tagged", tagged_bytes)],
            &json!({ "mode": "every", "every": 1 }),
        )
        .expect("split tagged pdf");
        assert_eq!(out.artifacts.len(), 2);
        assert_eq!(
            out.warnings,
            vec!["tagged structure (StructTreeRoot) is not kept for a page subset; the output is not a tagged PDF".to_owned()]
        );
    }
}
