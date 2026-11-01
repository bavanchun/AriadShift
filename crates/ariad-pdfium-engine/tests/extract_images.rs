//! Integration tests for `pdf.extract_images`.

use std::{
    fs,
    io::Cursor,
    path::{Path, PathBuf},
};

use ariad_core::{
    limits::Limits,
    protocol::ErrorCode,
    tools::{
        PageRange,
        params::{ExtractFormat, ExtractImagesParams},
    },
};
use ariad_engine_io::RunToolPaths;
use ariad_pdfium_engine::{
    extract_images::{self, ImageEncoding},
    output::ToolContext,
};
use image::{ColorType, ImageEncoder, ImageFormat, RgbImage, codecs::jpeg::JpegEncoder};
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

/// Builds a valid PDF 1.4 byte buffer from numbered indirect object bodies (`1..=N`), with object 1
/// as the `/Catalog`.
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

fn make_test_jpeg(width: u32, height: u32) -> Vec<u8> {
    let mut img = RgbImage::new(width, height);
    for (x, y, pixel) in img.enumerate_pixels_mut() {
        *pixel = image::Rgb([
            u8::try_from((x * 17) % 256).unwrap_or(0),
            u8::try_from((y * 31) % 256).unwrap_or(0),
            128,
        ]);
    }
    let mut buf = Cursor::new(Vec::new());
    let encoder = JpegEncoder::new_with_quality(&mut buf, 90);
    encoder
        .write_image(img.as_raw(), width, height, ColorType::Rgb8.into())
        .expect("encode jpeg");
    buf.into_inner()
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

/// Builds a two-page PDF where:
/// - Object 5 is a `24x24` `DCTDecode` JPEG referenced on BOTH Page 1 (`/ImShared`) and Page 2 (`/ImShared`).
/// - Object 6 is a `6x6` uncompressed `DeviceRGB` decorative image on Page 1 (`/ImTiny`).
/// - Object 7 is a `20x16` uncompressed `DeviceRGB` image on Page 2 (`/ImSecond`).
fn build_shared_and_multi_image_pdf(jpeg_bytes: &[u8]) -> Vec<u8> {
    let tiny_rgb = vec![200u8; 6 * 6 * 3];
    let second_rgb = vec![90u8; 20 * 16 * 3];

    let p1_content = b"q 24 0 0 24 50 500 cm /ImShared Do Q\nq 6 0 0 6 200 500 cm /ImTiny Do Q";
    let p2_content = b"q 24 0 0 24 50 500 cm /ImShared Do Q\nq 20 0 0 16 200 500 cm /ImSecond Do Q";

    let objects = vec![
        // 1: Catalog
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        // 2: Pages
        b"<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>".to_vec(),
        // 3: Page 1
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /XObject << /ImShared 5 0 R /ImTiny 6 0 R >> >> /Contents 8 0 R >>".to_vec(),
        // 4: Page 2
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /XObject << /ImShared 5 0 R /ImSecond 7 0 R >> >> /Contents 9 0 R >>".to_vec(),
        // 5: Shared 24x24 JPEG XObject
        stream_object(
            "/Type /XObject /Subtype /Image /Width 24 /Height 24 /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode",
            jpeg_bytes,
        ),
        // 6: Tiny 6x6 RGB XObject
        stream_object(
            "/Type /XObject /Subtype /Image /Width 6 /Height 6 /ColorSpace /DeviceRGB /BitsPerComponent 8",
            &tiny_rgb,
        ),
        // 7: Second 20x16 RGB XObject
        stream_object(
            "/Type /XObject /Subtype /Image /Width 20 /Height 16 /ColorSpace /DeviceRGB /BitsPerComponent 8",
            &second_rgb,
        ),
        // 8: Page 1 content stream
        stream_object("", p1_content),
        // 9: Page 2 content stream
        stream_object("", p2_content),
    ];

    assemble_pdf(&objects)
}

#[test]
fn extracts_images_from_vi_garden_and_scan_and_digital_fixtures() {
    let lib_dir = require_pdfium!();

    for fixture in [
        "fixtures/pdf/vi-garden-image-only.pdf",
        "fixtures/scan/vi-garden-300dpi.pdf",
        "fixtures/scan/en-alice-p3.pdf",
        "fixtures/scan/en-harpers-weekly-p10.pdf",
    ] {
        let ws = TestWorkspace::from_fixture(fixture);
        let mut sink = Vec::new();
        let mut ctx = ToolContext::new(
            "job-scan-extract",
            "pdf.extract_images",
            ws.paths.clone(),
            Limits::local(),
            &mut sink,
        )
        .with_library_dir(&lib_dir);

        let metrics = extract_images::run(&mut ctx, ExtractImagesParams::default())
            .unwrap_or_else(|e| panic!("extract_images failed on {fixture}: {:?}", e))
            .expect("metrics present");
        let extracted = metrics
            .get("images_extracted")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        assert!(
            extracted >= 1,
            "expected at least 1 image extracted from {fixture}, got {extracted}"
        );
        assert_eq!(ctx.artifact_count(), u32::try_from(extracted).unwrap());
    }

    // Also verify a born-digital PDF with embedded images (arxiv-tableformer page 1-3).
    let digital_ws =
        TestWorkspace::from_fixture("fixtures/pdf/arxiv-tableformer-2203-01017-v1.pdf");
    let mut sink = Vec::new();
    let mut ctx = ToolContext::new(
        "job-digital-extract",
        "pdf.extract_images",
        digital_ws.paths.clone(),
        Limits::local(),
        &mut sink,
    )
    .with_library_dir(lib_dir);

    let params = ExtractImagesParams {
        pages: PageRange::parse("1-4").expect("valid range"),
        format: ExtractFormat::Original,
        min_px: 16,
    };
    let metrics = extract_images::run(&mut ctx, params)
        .expect("extract_images succeeds on digital PDF with embedded figures")
        .expect("metrics present");
    assert!(
        metrics
            .get("images_extracted")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            >= 1,
        "expected embedded figures in pages 1-4 of arxiv-tableformer"
    );
}

#[test]
fn deduplicates_shared_xobject_across_pages_and_respects_min_px() {
    let lib_dir = require_pdfium!();
    let jpeg_bytes = make_test_jpeg(24, 24);
    let pdf_bytes = build_shared_and_multi_image_pdf(&jpeg_bytes);
    let ws = TestWorkspace::from_bytes("shared.pdf", &pdf_bytes);

    let mut sink = Vec::new();
    let mut ctx = ToolContext::new(
        "job-dedup",
        "pdf.extract_images",
        ws.paths.clone(),
        Limits::local(),
        &mut sink,
    )
    .with_library_dir(lib_dir);

    // min_px = 16 skips the 6x6 tiny image on page 1; the 24x24 JPEG is extracted on page 1 and
    // deduplicated on page 2; the 20x16 RGB image on page 2 is extracted as PNG.
    let params = ExtractImagesParams {
        pages: PageRange::all(),
        format: ExtractFormat::Original,
        min_px: 16,
    };
    let metrics = extract_images::run(&mut ctx, params)
        .expect("extract_images succeeds")
        .expect("metrics present");

    assert_eq!(metrics.get("pages").and_then(Value::as_u64), Some(2));
    assert_eq!(
        metrics.get("images_extracted").and_then(Value::as_u64),
        Some(2)
    );
    assert_eq!(
        metrics.get("images_deduplicated").and_then(Value::as_u64),
        Some(1)
    );
    assert_eq!(
        metrics.get("images_skipped_min_px").and_then(Value::as_u64),
        Some(1)
    );

    let p1_jpg = ws.paths.output_dir.join("shared.page-001.image-001.jpg");
    let p2_png = ws.paths.output_dir.join("shared.page-002.image-001.png");
    assert!(p1_jpg.is_file(), "expected page 1 JPEG artifact");
    assert!(p2_png.is_file(), "expected page 2 PNG artifact");

    // Verify original JPEG stream bytes are preserved byte-for-byte when format = original.
    let saved_jpg = fs::read(&p1_jpg).expect("read p1 jpg");
    assert_eq!(saved_jpg, jpeg_bytes);

    // Verify the second image on page 2 is a valid 20x16 PNG.
    let saved_png = fs::read(&p2_png).expect("read p2 png");
    let decoded = image::load_from_memory_with_format(&saved_png, ImageFormat::Png)
        .expect("valid PNG output");
    assert_eq!(decoded.width(), 20);
    assert_eq!(decoded.height(), 16);
}

#[test]
fn format_png_reencodes_jpeg_stream_to_png() {
    let lib_dir = require_pdfium!();
    let jpeg_bytes = make_test_jpeg(24, 24);
    let pdf_bytes = build_shared_and_multi_image_pdf(&jpeg_bytes);
    let ws = TestWorkspace::from_bytes("reencode.pdf", &pdf_bytes);

    let mut sink = Vec::new();
    let mut ctx = ToolContext::new(
        "job-png-fmt",
        "pdf.extract_images",
        ws.paths.clone(),
        Limits::local(),
        &mut sink,
    )
    .with_library_dir(lib_dir);

    let params = ExtractImagesParams {
        pages: PageRange::parse("1").expect("page 1"),
        format: ExtractFormat::Png,
        min_px: 16,
    };
    let outcome = extract_images::extract_document_images(&mut ctx, &params)
        .expect("extract_document_images");
    assert_eq!(outcome.images.len(), 1);
    let img = &outcome.images[0];
    assert_eq!(img.encoding, ImageEncoding::Jpeg);
    assert_eq!(img.artifact_format, "png");
    assert_eq!(img.artifact_extension, "png");

    let decoded = image::load_from_memory_with_format(&img.bytes, ImageFormat::Png)
        .expect("re-encoded image must be valid PNG");
    assert_eq!(decoded.width(), 24);
    assert_eq!(decoded.height(), 24);
}

#[test]
fn refuses_image_exceeding_max_image_pixels_and_encrypted_pdfs() {
    let lib_dir = require_pdfium!();

    // 1. Image with 20_000 x 20_000 = 400_000_000 pixels (> MAX_IMAGE_PIXELS = 268_435_456).
    let oversized_objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /XObject << /ImHuge 4 0 R >> >> /Contents 5 0 R >>".to_vec(),
        stream_object(
            "/Type /XObject /Subtype /Image /Width 20000 /Height 20000 /ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /DCTDecode",
            b"\xFF\xD8\xFF\xD9",
        ),
        stream_object("", b"q 100 0 0 100 0 0 cm /ImHuge Do Q"),
    ];
    let oversized_pdf = assemble_pdf(&oversized_objects);
    let ws = TestWorkspace::from_bytes("oversized.pdf", &oversized_pdf);
    let mut sink = Vec::new();
    let mut ctx = ToolContext::new(
        "job-oversized",
        "pdf.extract_images",
        ws.paths,
        Limits::local(),
        &mut sink,
    )
    .with_library_dir(&lib_dir);

    let err = extract_images::run(&mut ctx, ExtractImagesParams::default())
        .expect_err("image exceeding MAX_IMAGE_PIXELS must be refused");
    assert_eq!(err.code, ErrorCode::LimitExceeded);

    // 2. Encrypted PDF is refused with message pointing at pdf.decrypt.
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
        "job-enc-img",
        "pdf.extract_images",
        enc_ws.paths,
        Limits::local(),
        &mut enc_sink,
    )
    .with_library_dir(lib_dir);

    let enc_err = extract_images::run(&mut enc_ctx, ExtractImagesParams::default())
        .expect_err("encrypted PDF must be rejected");
    assert_eq!(enc_err.code, ErrorCode::EngineFailure);
    assert!(enc_err.message.contains("pdf.decrypt"));
}

#[test]
fn repeated_nested_forms_are_expanded_once() {
    let lib_dir = require_pdfium!();
    let depth = 12usize;
    let mut objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Resources << /XObject << /F 5 0 R >> >> /Contents 4 0 R >>".to_vec(),
        stream_object("", b"/F Do /F Do"),
    ];
    for index in 0..depth {
        let child = index + 6;
        let resources = format!(
            "/Type /XObject /Subtype /Form /BBox [0 0 100 100] /Resources << /XObject << /F {child} 0 R >> >>"
        );
        objects.push(stream_object(&resources, b"/F Do /F Do"));
    }
    objects.push(stream_object(
        "/Type /XObject /Subtype /Image /Width 16 /Height 16 /ColorSpace /DeviceRGB /BitsPerComponent 8",
        &vec![128; 16 * 16 * 3],
    ));
    let ws = TestWorkspace::from_bytes("nested.pdf", &assemble_pdf(&objects));
    let mut sink = Vec::new();
    let mut limits = Limits::local();
    limits.max_blocks = 200;
    let mut ctx = ToolContext::new("nested", "pdf.extract_images", ws.paths, limits, &mut sink)
        .with_library_dir(lib_dir);
    let outcome =
        extract_images::extract_document_images(&mut ctx, &ExtractImagesParams::default())
            .expect("nested forms stay within the traversal budget");
    assert_eq!(outcome.images.len(), 1);
    assert_eq!(outcome.images_deduplicated, 0);
    assert_eq!(
        (outcome.images[0].width, outcome.images[0].height),
        (16, 16)
    );
}

#[test]
fn repeated_operators_obey_the_traversal_budget() {
    let lib_dir = require_pdfium!();
    let content = b"/Im Do ".repeat(100);
    let objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Resources << /XObject << /Im 5 0 R >> >> /Contents 4 0 R >>".to_vec(),
        stream_object("", &content),
        stream_object("/Type /XObject /Subtype /Image /Width 16 /Height 16 /ColorSpace /DeviceRGB /BitsPerComponent 8", &vec![128; 16 * 16 * 3]),
    ];
    let ws = TestWorkspace::from_bytes("operators.pdf", &assemble_pdf(&objects));
    let mut sink = Vec::new();
    let mut limits = Limits::local();
    limits.max_blocks = 20;
    let mut ctx = ToolContext::new(
        "operators",
        "pdf.extract_images",
        ws.paths,
        limits,
        &mut sink,
    )
    .with_library_dir(lib_dir);
    let err = extract_images::extract_document_images(&mut ctx, &ExtractImagesParams::default())
        .expect_err("operator budget must be enforced");
    assert_eq!(err.code, ErrorCode::LimitExceeded);
}
