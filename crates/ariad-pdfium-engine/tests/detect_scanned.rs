//! Integration tests for `pdf.detect_scanned`.

use std::{
    fs,
    path::{Path, PathBuf},
};

use ariad_core::{limits::Limits, protocol::ErrorCode, tools::params::DetectScannedParams};
use ariad_engine_io::RunToolPaths;
use ariad_pdfium_engine::{
    detect_scanned::{self, DetectScannedDocument, ScanVerdict, compute_image_coverage_ratio},
    output::ToolContext,
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

fn assemble_pdf(objects: &[Vec<u8>]) -> Vec<u8> {
    let mut out = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::with_capacity(objects.len());

    for (idx, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        let obj_num = idx + 1;
        out.extend_from_slice(format!("{obj_num} 0 obj\n").as_bytes());
        out.extend_from_slice(body);
        if !body.ends_with(b"\n") {
            out.push(b'\n');
        }
        out.extend_from_slice(b"endobj\n");
    }

    let xref_offset = out.len();
    let size = objects.len() + 1;
    out.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
    for off in offsets {
        out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n")
            .as_bytes(),
    );
    out
}

fn stream_object(dict_entries: &str, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(
        format!("<< {dict_entries} /Length {} >>\nstream\n", payload.len()).as_bytes(),
    );
    out.extend_from_slice(payload);
    out.extend_from_slice(b"\nendstream");
    out
}

#[test]
fn classifies_scanned_skewed_vi_as_scanned_and_multipage_20_as_digital() {
    let lib_dir = require_pdfium!();

    // 1. scanned-skewed-vi.pdf -> scanned
    let scanned_ws = TestWorkspace::from_fixture("fixtures/pdf-tools/scanned-skewed-vi.pdf");
    let mut scanned_sink = Vec::new();
    let mut scanned_ctx = ToolContext::new(
        "job-scanned",
        "pdf.detect_scanned",
        scanned_ws.paths.clone(),
        Limits::local(),
        &mut scanned_sink,
    )
    .with_library_dir(&lib_dir);

    let scanned_metrics = detect_scanned::run(&mut scanned_ctx, DetectScannedParams::default())
        .expect("detect_scanned succeeds on scanned-skewed-vi.pdf")
        .expect("metrics present");
    assert!(
        scanned_metrics
            .get("scanned_pages")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            >= 1
    );
    assert_eq!(
        scanned_metrics.get("digital_pages").and_then(Value::as_u64),
        Some(0)
    );

    let scanned_json_path = scanned_ws
        .paths
        .output_dir
        .join("scanned-skewed-vi.detect-scanned.json");
    let scanned_doc: DetectScannedDocument =
        serde_json::from_slice(&fs::read(&scanned_json_path).expect("read json"))
            .expect("parse DetectScannedDocument");

    assert_eq!(scanned_doc.schema, "ariad-pdf-detect-scanned/1");
    assert_eq!(scanned_doc.verdict, ScanVerdict::Scanned);
    assert!(!scanned_doc.thresholds.rule.is_empty());
    assert!(scanned_doc.thresholds.min_text_chars >= 1);
    for page in &scanned_doc.pages {
        assert!(!page.has_text_layer);
        assert_eq!(page.text_char_count, 0);
        assert!((0.0..=1.0).contains(&page.image_coverage_ratio));
        assert!(page.image_coverage_ratio > 0.5);
        assert_eq!(page.verdict, ScanVerdict::Scanned);
    }

    // 2. multipage-20.pdf -> digital
    let digital_ws = TestWorkspace::from_fixture("fixtures/pdf-tools/multipage-20.pdf");
    let mut digital_sink = Vec::new();
    let mut digital_ctx = ToolContext::new(
        "job-digital",
        "pdf.detect_scanned",
        digital_ws.paths.clone(),
        Limits::local(),
        &mut digital_sink,
    )
    .with_library_dir(lib_dir);

    let digital_metrics = detect_scanned::run(&mut digital_ctx, DetectScannedParams::default())
        .expect("detect_scanned succeeds on multipage-20.pdf")
        .expect("metrics present");
    assert_eq!(
        digital_metrics.get("pages").and_then(Value::as_u64),
        Some(20)
    );
    assert_eq!(
        digital_metrics.get("digital_pages").and_then(Value::as_u64),
        Some(20)
    );
    assert_eq!(
        digital_metrics.get("scanned_pages").and_then(Value::as_u64),
        Some(0)
    );
    assert_eq!(
        digital_metrics.get("mixed_pages").and_then(Value::as_u64),
        Some(0)
    );

    let digital_json_path = digital_ws
        .paths
        .output_dir
        .join("multipage-20.detect-scanned.json");
    let digital_doc: DetectScannedDocument =
        serde_json::from_slice(&fs::read(&digital_json_path).expect("read json"))
            .expect("parse DetectScannedDocument");

    assert_eq!(digital_doc.verdict, ScanVerdict::Digital);
    assert_eq!(digital_doc.pages.len(), 20);
    for page in &digital_doc.pages {
        assert!(page.has_text_layer);
        assert!(page.text_char_count > 0);
        assert_eq!(page.image_coverage_ratio, 0.0);
        assert_eq!(page.verdict, ScanVerdict::Digital);
    }
}

#[test]
fn classifies_mixed_page_and_clamps_overlapping_image_coverage_ratio_to_unit_interval() {
    let lib_dir = require_pdfium!();

    // Pure geometry check: overlapping and out-of-bounds images never exceed 1.0.
    let page_rect = [0.0, 0.0, 600.0, 800.0];
    let overlapping = [
        [-100.0, -100.0, 700.0, 900.0],
        [0.0, 0.0, 600.0, 800.0],
        [100.0, 100.0, 500.0, 700.0],
    ];
    let ratio = compute_image_coverage_ratio(page_rect, &overlapping);
    assert_eq!(ratio, 1.0);

    // Partial overlap check: two 300x800 halves covering the left and middle of a 600x800 page.
    let half_overlap = [[0.0, 0.0, 300.0, 800.0], [150.0, 0.0, 450.0, 800.0]];
    let partial_ratio = compute_image_coverage_ratio(page_rect, &half_overlap);
    assert!(
        (partial_ratio - 0.75).abs() < 1e-4,
        "expected 0.75 union coverage, got {partial_ratio}"
    );

    // Build a synthetic PDF with two overlapping full-page images AND a text layer on Page 1.
    let raw_rgb = vec![180u8; 8 * 8 * 3];
    let content = b"q 800 0 0 1000 -50 -50 cm /Im1 Do Q\n\
                    q 800 0 0 1000 0 0 cm /Im1 Do Q\n\
                    BT /F1 18 Tf 72 700 Td (Digital text over full page scan) Tj ET";
    let objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 4 0 R >> /XObject << /Im1 5 0 R >> >> /Contents 6 0 R >>".to_vec(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
        stream_object(
            "/Type /XObject /Subtype /Image /Width 8 /Height 8 /ColorSpace /DeviceRGB /BitsPerComponent 8",
            &raw_rgb,
        ),
        stream_object("", content),
    ];
    let pdf_bytes = assemble_pdf(&objects);
    let ws = TestWorkspace::from_bytes("mixed-overlap.pdf", &pdf_bytes);
    let mut sink = Vec::new();
    let mut ctx = ToolContext::new(
        "job-mixed",
        "pdf.detect_scanned",
        ws.paths.clone(),
        Limits::local(),
        &mut sink,
    )
    .with_library_dir(lib_dir);

    let metrics = detect_scanned::run(&mut ctx, DetectScannedParams::default())
        .expect("detect_scanned succeeds on mixed overlapping PDF")
        .expect("metrics present");
    assert_eq!(metrics.get("mixed_pages").and_then(Value::as_u64), Some(1));

    let out_path = ws
        .paths
        .output_dir
        .join("mixed-overlap.detect-scanned.json");
    let doc: DetectScannedDocument =
        serde_json::from_slice(&fs::read(&out_path).expect("read json")).expect("parse json");

    assert_eq!(doc.verdict, ScanVerdict::Mixed);
    assert_eq!(doc.pages.len(), 1);
    let p1 = &doc.pages[0];
    assert!(p1.has_text_layer);
    assert!(p1.text_char_count >= 20);
    assert_eq!(p1.image_coverage_ratio, 1.0);
    assert_eq!(p1.verdict, ScanVerdict::Mixed);
}

#[test]
fn rejects_encrypted_and_corrupt_pdfs() {
    let lib_dir = require_pdfium!();

    let enc_objects: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >>".to_vec(),
        b"<< /Filter /Standard /V 2 /R 3 /Length 128 /P -4 /O <000102030405060708090A0B0C0D0E0F101112131415161718191A1B1C1D1E1F> /U <000102030405060708090A0B0C0D0E0F101112131415161718191A1B1C1D1E1F> >>".to_vec(),
    ];
    let encrypted_bytes = String::from_utf8_lossy(&assemble_pdf(&enc_objects))
        .replace("/Root 1 0 R", "/Root 1 0 R /Encrypt 4 0 R")
        .into_bytes();
    let enc_ws = TestWorkspace::from_bytes("encrypted.pdf", &encrypted_bytes);
    let mut enc_sink = Vec::new();
    let mut enc_ctx = ToolContext::new(
        "job-enc-scan",
        "pdf.detect_scanned",
        enc_ws.paths,
        Limits::local(),
        &mut enc_sink,
    )
    .with_library_dir(&lib_dir);

    let err = detect_scanned::run(&mut enc_ctx, DetectScannedParams::default())
        .expect_err("encrypted owner-only PDF must be rejected");
    assert_eq!(err.code, ErrorCode::EngineFailure);
    assert!(err.message.contains("pdf.decrypt"));

    let bad_ws = TestWorkspace::from_bytes("bad.pdf", b"not a pdf at all");
    let mut bad_sink = Vec::new();
    let mut bad_ctx = ToolContext::new(
        "job-bad-scan",
        "pdf.detect_scanned",
        bad_ws.paths,
        Limits::local(),
        &mut bad_sink,
    )
    .with_library_dir(lib_dir);

    let bad_err = detect_scanned::run(&mut bad_ctx, DetectScannedParams::default())
        .expect_err("corrupt PDF must be rejected");
    assert_eq!(bad_err.code, ErrorCode::EngineFailure);
}
