//! Integration tests for `pdf.text_layout`.

use std::{
    fs,
    path::{Path, PathBuf},
};

use ariad_core::{
    limits::Limits,
    protocol::ErrorCode,
    tools::{PageRange, params::TextLayoutParams},
};
use ariad_engine_io::RunToolPaths;
use ariad_pdfium_engine::{
    output::{CancelFlag, ToolContext},
    text_layout::{self, TextLayoutDocument},
};
use serde_json::Value;

fn pdfium_dir() -> Option<PathBuf> {
    let raw = std::env::var_os("ARIAD_TEST_PDFIUM_DIR")?;
    if raw.is_empty() {
        return None;
    }
    Some(PathBuf::from(raw))
}

macro_rules! require_pdfium {
    () => {
        match pdfium_dir() {
            Some(dir) => dir,
            None => {
                eprintln!("skipping test: ARIAD_TEST_PDFIUM_DIR is not set");
                return;
            }
        }
    };
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_path_buf()
}

struct TestWorkspace {
    _dir: tempfile::TempDir,
    paths: RunToolPaths,
}

impl TestWorkspace {
    fn from_fixture(rel_path: &str) -> Self {
        let src = repo_root().join(rel_path);
        let bytes =
            fs::read(&src).unwrap_or_else(|e| panic!("read fixture {}: {e}", src.display()));
        let file_name = src
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("input.pdf");
        Self::from_bytes(file_name, &bytes)
    }

    fn from_bytes(file_name: &str, bytes: &[u8]) -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().canonicalize().expect("canonicalize tempdir");
        for sub in ["in", "out", "tmp"] {
            fs::create_dir(root.join(sub)).expect("create workspace subdir");
        }
        let input_path = root.join("in").join(file_name);
        fs::write(&input_path, bytes).expect("write input pdf");
        let paths = RunToolPaths {
            workspace: root.clone(),
            inputs: vec![input_path],
            output_dir: root.join("out"),
            work_dir: root.join("tmp"),
        };
        Self { _dir: dir, paths }
    }
}

#[test]
fn preserves_vietnamese_diacritics_and_style_metadata() {
    let lib_dir = require_pdfium!();
    let ws = TestWorkspace::from_fixture("fixtures/pdf-tools/vi-diacritics-subset-font.pdf");
    let mut sink = Vec::new();
    let mut ctx = ToolContext::new(
        "job-vi-layout",
        "pdf.text_layout",
        ws.paths.clone(),
        Limits::local(),
        &mut sink,
    )
    .with_library_dir(lib_dir);

    let metrics = text_layout::run(&mut ctx, TextLayoutParams::default())
        .expect("text_layout succeeds on Vietnamese fixture")
        .expect("metrics present");
    assert_eq!(metrics.get("pages").and_then(Value::as_u64), Some(1));
    assert!(metrics.get("chars").and_then(Value::as_u64).unwrap_or(0) > 10);
    assert!(metrics.get("words").and_then(Value::as_u64).unwrap_or(0) > 2);
    assert!(metrics.get("lines").and_then(Value::as_u64).unwrap_or(0) >= 1);

    let out_path = ws
        .paths
        .output_dir
        .join("vi-diacritics-subset-font.text-layout.json");
    let json_bytes = fs::read(&out_path).expect("read text-layout.json");
    let doc: TextLayoutDocument =
        serde_json::from_slice(&json_bytes).expect("parse TextLayoutDocument");

    assert_eq!(doc.schema, "ariad-pdf-text-layout/1");
    assert_eq!(doc.coord_system, "pdf_points");
    assert_eq!(doc.origin, "bottom_left");
    assert_eq!(doc.bbox_order, ["left", "bottom", "right", "top"]);
    assert_eq!(doc.page_count, 1);
    assert_eq!(doc.pages.len(), 1);

    let page = &doc.pages[0];
    assert_eq!(page.page, 1);
    assert!(page.width_pt > 0.0 && page.height_pt > 0.0);
    assert!(!page.chars.is_empty());
    assert!(!page.words.is_empty());
    assert!(!page.lines.is_empty());

    let full_text: String = page
        .lines
        .iter()
        .map(|l| l.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");

    let has_non_ascii_vi = !full_text.is_ascii();
    assert!(
        has_non_ascii_vi,
        "expected non-ASCII Vietnamese diacritics in extracted text, got: {full_text}"
    );

    for ch in &page.chars {
        assert!(!ch.text.is_empty());
        assert!(ch.bbox[0] <= ch.bbox[2], "left <= right for {:?}", ch);
        assert!(ch.bbox[1] <= ch.bbox[3], "bottom <= top for {:?}", ch);
        assert!(!ch.font_name.is_empty(), "non-empty font_name for {:?}", ch);
        assert!(ch.font_size > 0.0, "positive font_size for {:?}", ch);
        assert!(
            ch.fill_color.starts_with('#')
                && (ch.fill_color.len() == 7 || ch.fill_color.len() == 9),
            "valid hex fill_color for {:?}",
            ch
        );
    }
}

#[test]
fn orders_lines_and_words_and_matches_page_one_snapshot_on_multipage_20() {
    let lib_dir = require_pdfium!();
    let ws = TestWorkspace::from_fixture("fixtures/pdf-tools/multipage-20.pdf");
    let mut sink = Vec::new();
    let mut ctx = ToolContext::new(
        "job-mp20-p1",
        "pdf.text_layout",
        ws.paths.clone(),
        Limits::local(),
        &mut sink,
    )
    .with_library_dir(lib_dir);

    let pages = PageRange::parse("1, 5, 20").expect("valid page range");
    let metrics = text_layout::run(&mut ctx, TextLayoutParams { pages })
        .expect("text_layout succeeds on selected pages of multipage-20.pdf")
        .expect("metrics present");
    assert_eq!(metrics.get("pages").and_then(Value::as_u64), Some(3));

    let out_path = ws.paths.output_dir.join("multipage-20.text-layout.json");
    let doc: TextLayoutDocument =
        serde_json::from_slice(&fs::read(&out_path).expect("read output"))
            .expect("parse output json");

    assert_eq!(doc.page_count, 20);
    let extracted_page_nums: Vec<u32> = doc.pages.iter().map(|p| p.page).collect();
    assert_eq!(extracted_page_nums, vec![1, 5, 20]);

    for page in &doc.pages {
        let mut expected_word_start = 0;
        for line in &page.lines {
            assert_eq!(line.word_start, expected_word_start);
            assert!(line.word_end > line.word_start);
            assert!(line.word_end <= page.words.len());
            expected_word_start = line.word_end;

            let line_words = &page.words[line.word_start..line.word_end];
            for pair in line_words.windows(2) {
                assert!(
                    pair[0].bbox[0] <= pair[1].bbox[0] + 0.5,
                    "words within line must be left-to-right: {:?} vs {:?}",
                    pair[0],
                    pair[1]
                );
            }
        }
        assert_eq!(expected_word_start, page.words.len());

        let mut expected_char_start = 0;
        for word in &page.words {
            assert_eq!(word.char_start, expected_char_start);
            assert!(word.char_end > word.char_start);
            assert!(word.char_end <= page.chars.len());
            expected_char_start = word.char_end;
        }
        assert_eq!(expected_char_start, page.chars.len());

        for pair in page.lines.windows(2) {
            let cy0 = (pair[0].bbox[1] + pair[0].bbox[3]) * 0.5;
            let cy1 = (pair[1].bbox[1] + pair[1].bbox[3]) * 0.5;
            assert!(
                cy0 >= cy1 - 1.0,
                "lines must be ordered top-to-bottom: {:?} vs {:?}",
                pair[0],
                pair[1]
            );
        }
    }

    let p1 = &doc.pages[0];
    assert_eq!(p1.page, 1);
    assert!(
        p1.lines[0].text.contains("Page 1") || p1.lines.iter().any(|l| l.text.contains('1')),
        "page 1 lines should mention page 1: {:?}",
        p1.lines
    );
    let p20 = &doc.pages[2];
    assert_eq!(p20.page, 20);
    assert!(
        p20.lines.iter().any(|l| l.text.contains("20")),
        "page 20 lines should mention 20: {:?}",
        p20.lines
    );
}

#[test]
fn refuses_when_character_or_byte_cap_is_exceeded() {
    let lib_dir = require_pdfium!();
    let ws = TestWorkspace::from_fixture("fixtures/pdf-tools/multipage-20.pdf");

    // 1. Refuse when character count exceeds cap.
    let mut sink = Vec::new();
    let mut ctx = ToolContext::new(
        "job-char-cap",
        "pdf.text_layout",
        ws.paths.clone(),
        Limits::local(),
        &mut sink,
    )
    .with_library_dir(&lib_dir);

    let err = text_layout::run_with_char_limit(&mut ctx, TextLayoutParams::default(), 3)
        .expect_err("must fail when character cap is exceeded");
    assert_eq!(err.code, ErrorCode::LimitExceeded);

    // 2. Refuse when serialized output bytes exceed effective limit.
    let mut tight_limits = Limits::local();
    tight_limits.max_decompressed_bytes = 512;
    let mut sink2 = Vec::new();
    let mut ctx2 = ToolContext::new(
        "job-byte-cap",
        "pdf.text_layout",
        ws.paths.clone(),
        tight_limits,
        &mut sink2,
    )
    .with_library_dir(lib_dir);

    let err2 = text_layout::run(&mut ctx2, TextLayoutParams::default())
        .expect_err("must fail when output JSON exceeds byte limit");
    assert_eq!(err2.code, ErrorCode::LimitExceeded);
}

#[test]
fn rejects_out_of_bounds_page_range_with_invalid_request() {
    let lib_dir = require_pdfium!();
    let ws = TestWorkspace::from_fixture("fixtures/pdf-tools/multipage-20.pdf");
    let mut sink = Vec::new();
    let mut ctx = ToolContext::new(
        "job-bad-range",
        "pdf.text_layout",
        ws.paths.clone(),
        Limits::local(),
        &mut sink,
    )
    .with_library_dir(lib_dir);

    let pages = PageRange::parse("1, 25").expect("syntactically valid page range");
    let err = text_layout::run(&mut ctx, TextLayoutParams { pages })
        .expect_err("page 25 on 20-page PDF must fail");
    assert_eq!(err.code, ErrorCode::InvalidRequest);
}

fn build_encrypted_pdf() -> Vec<u8> {
    let objects: &[&[u8]] = &[
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >>",
        b"<< /Filter /Standard /V 2 /R 3 /Length 128 /P -4 /O <000102030405060708090A0B0C0D0E0F101112131415161718191A1B1C1D1E1F> /U <000102030405060708090A0B0C0D0E0F101112131415161718191A1B1C1D1E1F> >>",
    ];
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (idx, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", idx + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref_offset = out.len();
    let size = objects.len() + 1;
    out.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
    for off in offsets {
        out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {size} /Root 1 0 R /Encrypt 4 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n"
        )
        .as_bytes(),
    );
    out
}

#[test]
fn rejects_encrypted_and_corrupt_pdfs_with_typed_errors() {
    let lib_dir = require_pdfium!();

    let encrypted_bytes = build_encrypted_pdf();
    let ws = TestWorkspace::from_bytes("encrypted.pdf", &encrypted_bytes);
    let mut sink = Vec::new();
    let mut ctx = ToolContext::new(
        "job-enc",
        "pdf.text_layout",
        ws.paths.clone(),
        Limits::local(),
        &mut sink,
    )
    .with_library_dir(&lib_dir);

    let err = text_layout::run(&mut ctx, TextLayoutParams::default())
        .expect_err("encrypted PDF must be rejected");
    assert_eq!(err.code, ErrorCode::EngineFailure);
    assert!(
        err.message.contains("pdf.decrypt"),
        "error message must point at pdf.decrypt, got: {}",
        err.message
    );

    let corrupt_ws = TestWorkspace::from_bytes("corrupt.pdf", b"%PDF-1.7\n1 0 obj\n<< /Broken");
    let mut sink = Vec::new();
    let mut ctx = ToolContext::new(
        "job-corrupt",
        "pdf.text_layout",
        corrupt_ws.paths,
        Limits::local(),
        &mut sink,
    )
    .with_library_dir(lib_dir);

    let err = text_layout::run(&mut ctx, TextLayoutParams::default())
        .expect_err("corrupt PDF must be rejected");
    assert_eq!(err.code, ErrorCode::EngineFailure);
}

#[test]
fn honors_cancellation_and_deadline_and_ignores_javascript_actions() {
    let lib_dir = require_pdfium!();

    // 1. JavaScript action PDF opens cleanly without executing scripts.
    let js_ws = TestWorkspace::from_fixture("fixtures/pdf-tools/javascript-action.pdf");
    let mut js_sink = Vec::new();
    let mut js_ctx = ToolContext::new(
        "job-js",
        "pdf.text_layout",
        js_ws.paths,
        Limits::local(),
        &mut js_sink,
    )
    .with_library_dir(&lib_dir);
    let js_res = text_layout::run(&mut js_ctx, TextLayoutParams::default())
        .expect("javascript-action.pdf succeeds safely");
    assert!(js_res.is_some());

    // 2. Pre-cancelled token aborts immediately.
    let ws = TestWorkspace::from_fixture("fixtures/pdf-tools/multipage-20.pdf");
    let cancel = CancelFlag::new();
    cancel.cancel();
    let mut cancel_sink = Vec::new();
    let mut cancel_ctx = ToolContext::new(
        "job-cancel",
        "pdf.text_layout",
        ws.paths.clone(),
        Limits::local(),
        &mut cancel_sink,
    )
    .with_library_dir(&lib_dir)
    .with_cancel(cancel);

    let cancel_err = text_layout::run(&mut cancel_ctx, TextLayoutParams::default())
        .expect_err("cancelled job must fail");
    assert_eq!(cancel_err.code, ErrorCode::EngineFailure);

    // 3. Zero-second timeout aborts with E_LIMIT_EXCEEDED.
    let mut timeout_limits = Limits::local();
    timeout_limits.timeout_s = Some(0);
    let mut deadline_sink = Vec::new();
    let mut deadline_ctx = ToolContext::new(
        "job-deadline",
        "pdf.text_layout",
        ws.paths,
        timeout_limits,
        &mut deadline_sink,
    )
    .with_library_dir(lib_dir);

    let deadline_err = text_layout::run(&mut deadline_ctx, TextLayoutParams::default())
        .expect_err("expired deadline must fail");
    assert_eq!(deadline_err.code, ErrorCode::LimitExceeded);
}
