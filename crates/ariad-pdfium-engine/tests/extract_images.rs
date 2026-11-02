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
fn extracts_type3_bitmap_font_without_refusal() {
    let lib_dir = require_pdfium!();
    let ws = TestWorkspace::from_fixture("fixtures/pdf/type3-bitmap-font.pdf");
    let mut sink = Vec::new();
    let mut ctx = ToolContext::new(
        "bitmap-font",
        "pdf.extract_images",
        ws.paths,
        Limits::local(),
        &mut sink,
    )
    .with_library_dir(lib_dir);
    extract_images::run(&mut ctx, ExtractImagesParams::default()).expect("bitmap font is accepted");
}

#[test]
fn inline_images_check_dimensions_before_decoding() {
    let lib_dir = require_pdfium!();
    let ws = TestWorkspace::from_fixture("fixtures/pdf/inline-image-bomb.pdf");
    let mut sink = Vec::new();
    let mut ctx = ToolContext::new(
        "inline-limit",
        "pdf.extract_images",
        ws.paths,
        Limits::local(),
        &mut sink,
    )
    .with_library_dir(lib_dir);
    let start = std::time::Instant::now();
    let err = extract_images::run(&mut ctx, ExtractImagesParams::default())
        .expect_err("oversized inline images must be rejected before decoding");
    assert!(start.elapsed() < std::time::Duration::from_secs(5));
    assert_eq!(err.code, ErrorCode::LimitExceeded);
}

#[test]
fn extracts_single_inline_image_pixels() {
    let lib_dir = require_pdfium!();
    let content = b"q 1 0 0 1 0 0 cm BI /W 1 /H 1 /BPC 8 /CS /RGB /F /AHx ID FF0000> EI Q";
    let pdf = assemble_pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R >>".to_vec(),
        stream_object("", content),
    ]);
    let ws = TestWorkspace::from_bytes("inline.pdf", &pdf);
    let mut sink = Vec::new();
    let mut ctx = ToolContext::new(
        "inline-pixel",
        "pdf.extract_images",
        ws.paths,
        Limits::local(),
        &mut sink,
    )
    .with_library_dir(lib_dir);
    let params = ExtractImagesParams {
        min_px: 1,
        ..ExtractImagesParams::default()
    };
    let outcome =
        extract_images::extract_document_images(&mut ctx, &params).expect("extract image");
    assert_eq!(outcome.images.len(), 1);
    let image = image::load_from_memory_with_format(&outcome.images[0].bytes, ImageFormat::Png)
        .expect("valid png")
        .to_rgb8();
    assert_eq!(image.as_raw(), &[255, 0, 0]);
}

#[test]
fn deduplicates_identical_inline_images_and_keeps_distinct_pixels() {
    let lib_dir = require_pdfium!();
    let content = b"q BI /W 1 /H 1 /BPC 8 /CS /RGB /F /AHx ID FF0000> EI Q q BI /W 1 /H 1 /BPC 8 /CS /RGB /F /AHx ID FF0000> EI Q q BI /W 1 /H 1 /BPC 8 /CS /RGB /F /AHx ID 00FF00> EI Q";
    let pdf = assemble_pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R >>".to_vec(),
        stream_object("", content),
    ]);
    let ws = TestWorkspace::from_bytes("inline.pdf", &pdf);
    let mut sink = Vec::new();
    let mut ctx = ToolContext::new(
        "inline-dedup",
        "pdf.extract_images",
        ws.paths,
        Limits::local(),
        &mut sink,
    )
    .with_library_dir(lib_dir);
    let params = ExtractImagesParams {
        min_px: 1,
        ..ExtractImagesParams::default()
    };
    let outcome =
        extract_images::extract_document_images(&mut ctx, &params).expect("extract images");
    assert_eq!(outcome.images.len(), 2);
    assert_eq!(outcome.images_deduplicated, 1);
    let pixels: Vec<_> = outcome
        .images
        .iter()
        .map(|img| {
            image::load_from_memory_with_format(&img.bytes, ImageFormat::Png)
                .unwrap()
                .to_rgb8()
                .into_raw()
        })
        .collect();
    assert!(pixels.contains(&vec![255, 0, 0]));
    assert!(pixels.contains(&vec![0, 255, 0]));
}

#[test]
fn refuses_inline_image_data_over_asset_limit() {
    let lib_dir = require_pdfium!();
    let content =
        b"q BI /W 1 /H 1 /BPC 8 /CS /G /F /AHx ID 0000000000000000000000000000000000000000> EI Q";
    let pdf = assemble_pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R >>".to_vec(),
        stream_object("", content),
    ]);
    let ws = TestWorkspace::from_bytes("long-inline.pdf", &pdf);
    let mut limits = Limits::local();
    limits.max_asset_bytes = Some(32);
    let mut sink = Vec::new();
    let mut ctx = ToolContext::new(
        "inline-data",
        "pdf.extract_images",
        ws.paths,
        limits,
        &mut sink,
    )
    .with_library_dir(lib_dir);
    let err = extract_images::run(&mut ctx, ExtractImagesParams::default())
        .expect_err("inline data must be bounded");
    assert_eq!(err.code, ErrorCode::LimitExceeded);
}

#[test]
fn extracts_flate_inline_image() {
    use std::io::Write;
    let lib_dir = require_pdfium!();
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&[255, 0, 0]).expect("write pixels");
    let compressed = encoder.finish().expect("compress pixels");
    let mut content = b"q BI /W 1 /H 1 /BPC 8 /CS /RGB /F /Fl ID ".to_vec();
    content.extend_from_slice(&compressed);
    content.extend_from_slice(b" EI Q");
    let pdf = assemble_pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R >>".to_vec(),
        stream_object("", &content),
    ]);
    let ws = TestWorkspace::from_bytes("flate-inline.pdf", &pdf);
    let mut sink = Vec::new();
    let mut ctx = ToolContext::new(
        "inline-flate",
        "pdf.extract_images",
        ws.paths,
        Limits::local(),
        &mut sink,
    )
    .with_library_dir(lib_dir);
    let params = ExtractImagesParams {
        min_px: 1,
        ..ExtractImagesParams::default()
    };
    let outcome =
        extract_images::extract_document_images(&mut ctx, &params).expect("extract inline image");
    assert_eq!(outcome.images.len(), 1);
    let decoded = image::load_from_memory_with_format(&outcome.images[0].bytes, ImageFormat::Png)
        .expect("valid PNG")
        .to_rgb8();
    assert_eq!(decoded.as_raw(), &[255, 0, 0]);
}

#[test]
fn refuses_oversized_mask_before_opening_page() {
    let lib_dir = require_pdfium!();
    let pdf = assemble_pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] /Resources << /XObject << /Im 4 0 R /Mask 6 0 R >> >> /Contents 7 0 R >>".to_vec(),
        stream_object("/Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8", &[128]),
        stream_object("/Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8 /SMask 6 0 R", &[128]),
        stream_object("/Type /XObject /Subtype /Image /Width 20000 /Height 20000 /ColorSpace /DeviceGray /BitsPerComponent 8", &[0]),
        stream_object("", b"/Im Do /Mask Do"),
    ]);
    let ws = TestWorkspace::from_bytes("large-mask.pdf", &pdf);
    let mut sink = Vec::new();
    let mut ctx = ToolContext::new(
        "mask-limit",
        "pdf.extract_images",
        ws.paths,
        Limits::local(),
        &mut sink,
    )
    .with_library_dir(lib_dir);
    let start = std::time::Instant::now();
    let err = extract_images::run(&mut ctx, ExtractImagesParams::default())
        .expect_err("oversized mask must be refused");
    assert_eq!(err.code, ErrorCode::LimitExceeded);
    assert!(start.elapsed() < std::time::Duration::from_secs(5));
}

#[test]
fn refuses_jpx_header_larger_than_image_dictionary() {
    let lib_dir = require_pdfium!();
    let mut jpx = vec![0u8; 45];
    jpx[..6].copy_from_slice(&[0xff, 0x4f, 0xff, 0x51, 0, 41]);
    jpx[8..12].copy_from_slice(&20_000u32.to_be_bytes());
    jpx[12..16].copy_from_slice(&20_000u32.to_be_bytes());
    jpx[40..42].copy_from_slice(&1u16.to_be_bytes());
    jpx[42..45].copy_from_slice(&[7, 1, 1]);
    let pdf = assemble_pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] /Resources << /XObject << /Im 4 0 R >> >> /Contents 5 0 R >>".to_vec(),
        stream_object("/Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /JPXDecode", &jpx),
        stream_object("", b"/Im Do"),
    ]);
    let ws = TestWorkspace::from_bytes("jpx-size.pdf", &pdf);
    let mut sink = Vec::new();
    let mut ctx = ToolContext::new(
        "jpx-size",
        "pdf.extract_images",
        ws.paths,
        Limits::local(),
        &mut sink,
    )
    .with_library_dir(lib_dir);
    let err = extract_images::run(&mut ctx, ExtractImagesParams::default())
        .expect_err("oversized JPX header must be refused");
    assert_eq!(err.code, ErrorCode::LimitExceeded);
}

#[test]
fn refuses_inline_flate_expansion_over_asset_limit() {
    use std::io::Write;
    let lib_dir = require_pdfium!();
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&[0; 4096]).expect("write pixels");
    let compressed = encoder.finish().expect("compress pixels");
    let mut content = b"q BI /W 1 /H 1 /BPC 8 /CS /G /F /Fl ID ".to_vec();
    content.extend_from_slice(&compressed);
    content.extend_from_slice(b" EI Q");
    let pdf = assemble_pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R >>".to_vec(),
        stream_object("", &content),
    ]);
    let ws = TestWorkspace::from_bytes("flate-bomb.pdf", &pdf);
    let mut limits = Limits::local();
    limits.max_asset_bytes = Some(128);
    let mut sink = Vec::new();
    let mut ctx = ToolContext::new(
        "inline-flate-limit",
        "pdf.extract_images",
        ws.paths,
        limits,
        &mut sink,
    )
    .with_library_dir(lib_dir);
    let err = extract_images::run(&mut ctx, ExtractImagesParams::default())
        .expect_err("expanded inline data must be bounded");
    assert_eq!(err.code, ErrorCode::LimitExceeded);
}

#[test]
fn refuses_uninspectable_page_content_entry() {
    let lib_dir = require_pdfium!();
    let pdf = assemble_pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] /Contents [4 0 R 42] >>".to_vec(),
        stream_object("", b"q Q"),
    ]);
    let ws = TestWorkspace::from_bytes("bad-content.pdf", &pdf);
    let mut sink = Vec::new();
    let mut ctx = ToolContext::new(
        "bad-content",
        "pdf.extract_images",
        ws.paths,
        Limits::local(),
        &mut sink,
    )
    .with_library_dir(lib_dir);
    let err = extract_images::run(&mut ctx, ExtractImagesParams::default())
        .expect_err("uninspectable content must be refused");
    assert_eq!(err.code, ErrorCode::LimitExceeded);
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
fn nested_form_fanout_exceeds_expansion_budget_before_page_load() {
    let lib_dir = require_pdfium!();
    let mut objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] /Resources << /XObject << /F 5 0 R >> >> /Contents 4 0 R >>".to_vec(),
        stream_object("", b"/F Do"),
    ];
    for index in 0..20 {
        let child = index + 6;
        let resources = format!(
            "/Type /XObject /Subtype /Form /BBox [0 0 10 10] /Resources << /XObject << /F {child} 0 R >> >>"
        );
        objects.push(stream_object(&resources, b"/F Do /F Do"));
    }
    objects.push(stream_object(
        "/Type /XObject /Subtype /Form /BBox [0 0 10 10]",
        b"q Q",
    ));
    let ws = TestWorkspace::from_bytes("fanout.pdf", &assemble_pdf(&objects));
    let mut sink = Vec::new();
    let mut ctx = ToolContext::new(
        "fanout",
        "pdf.extract_images",
        ws.paths,
        Limits::local(),
        &mut sink,
    )
    .with_library_dir(lib_dir);
    let start = std::time::Instant::now();
    let err = extract_images::run(&mut ctx, ExtractImagesParams::default())
        .expect_err("nested expansion must be bounded");
    assert_eq!(err.code, ErrorCode::LimitExceeded);
    assert!(start.elapsed() < std::time::Duration::from_secs(5));
}

#[test]
fn repeated_large_form_exceeds_page_work_budget_before_load() {
    let lib_dir = require_pdfium!();
    let objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] /Resources << /XObject << /X 5 0 R >> >> /Contents 4 0 R >>".to_vec(),
        stream_object("", &b"/X Do ".repeat(50)),
        stream_object("/Type /XObject /Subtype /Form /BBox [0 0 10 10]", &b"q ".repeat(1 << 16)),
    ];
    let ws = TestWorkspace::from_bytes("repeated.pdf", &assemble_pdf(&objects));
    let mut sink = Vec::new();
    let mut ctx = ToolContext::new(
        "repeated",
        "pdf.extract_images",
        ws.paths,
        Limits::local(),
        &mut sink,
    )
    .with_library_dir(lib_dir);
    let start = std::time::Instant::now();
    let err = extract_images::run(&mut ctx, ExtractImagesParams::default())
        .expect_err("page expansion must be bounded");
    assert_eq!(err.code, ErrorCode::LimitExceeded);
    assert!(start.elapsed() < std::time::Duration::from_secs(5));
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

#[test]
fn refuses_cumulative_inline_pixels_on_page() {
    let lib_dir = require_pdfium!();
    let content = b"q BI /Width 4 /Height 4 /BitsPerComponent 8 /ColorSpace /DeviceGray /Filter /ASCIIHexDecode ID 00> EI Q\n".repeat(2);
    let pdf = assemble_pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 20 20] /Contents 4 0 R >>".to_vec(),
        stream_object("", &content),
    ]);
    let ws = TestWorkspace::from_bytes("inline-total.pdf", &pdf);
    let mut limits = Limits::local();
    limits.max_asset_bytes = Some(100);
    let mut sink = Vec::new();
    let mut ctx = ToolContext::new(
        "inline-total",
        "pdf.extract_images",
        ws.paths,
        limits,
        &mut sink,
    )
    .with_library_dir(lib_dir);
    let err = extract_images::run(&mut ctx, ExtractImagesParams::default())
        .expect_err("page total must be bounded");
    assert_eq!(err.code, ErrorCode::LimitExceeded);
}

#[test]
fn refuses_cumulative_inline_pixels_in_forms() {
    let lib_dir = require_pdfium!();
    let form = b"BI /W 4 /H 4 /BPC 8 /CS /G /F /AHx ID 00> EI";
    let pdf = assemble_pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 20 20] /Resources << /XObject << /F1 5 0 R /F2 6 0 R >> >> /Contents 4 0 R >>".to_vec(),
        stream_object("", b"/F1 Do /F2 Do"),
        stream_object("/Type /XObject /Subtype /Form /BBox [0 0 10 10]", form),
        stream_object("/Type /XObject /Subtype /Form /BBox [0 0 10 10]", form),
    ]);
    let ws = TestWorkspace::from_bytes("inline-form-total.pdf", &pdf);
    let mut limits = Limits::local();
    limits.max_asset_bytes = Some(100);
    let mut sink = Vec::new();
    let mut ctx = ToolContext::new(
        "form-total",
        "pdf.extract_images",
        ws.paths,
        limits,
        &mut sink,
    )
    .with_library_dir(lib_dir);
    let err = extract_images::run(&mut ctx, ExtractImagesParams::default())
        .expect_err("form images must count toward page total");
    assert_eq!(err.code, ErrorCode::LimitExceeded);
}

#[test]
fn repeated_inline_form_charges_each_invocation() {
    let lib_dir = require_pdfium!();
    let pdf = assemble_pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 20 20] /Resources << /XObject << /F 5 0 R >> >> /Contents 4 0 R >>".to_vec(),
        stream_object("", &b"/F Do ".repeat(50)),
        stream_object("/Type /XObject /Subtype /Form /BBox [0 0 10 10]", b"BI /W 4 /H 4 /BPC 8 /CS /G /F /AHx ID 00000000000000000000000000000000> EI"),
    ]);
    let ws = TestWorkspace::from_bytes("repeated-inline.pdf", &pdf);
    let mut limits = Limits::local();
    limits.max_asset_bytes = Some(100);
    let mut sink = Vec::new();
    let mut ctx = ToolContext::new(
        "repeated-inline",
        "pdf.extract_images",
        ws.paths,
        limits,
        &mut sink,
    )
    .with_library_dir(lib_dir);
    let params = ExtractImagesParams {
        min_px: 1,
        ..ExtractImagesParams::default()
    };
    let error = extract_images::extract_document_images(&mut ctx, &params)
        .expect_err("repeated inline images exceed the page pixel budget");
    assert_eq!(error.code, ErrorCode::LimitExceeded);
}

#[test]
fn refuses_inline_images_with_missing_or_invalid_dictionary_values() {
    let lib_dir = require_pdfium!();
    for content in [
        b"BI /W 1 /H 1 /BPC 8 /F /AHx ID 00> EI".as_slice(),
        b"BI /W 1 /H 1 /BPC 3 /CS /G /F /AHx ID 00> EI".as_slice(),
        b"BI /W 1 /H 1 /BPC 8 /CS /G /IM /bad /F /AHx ID 00> EI".as_slice(),
    ] {
        let pdf = assemble_pdf(&[
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] /Contents 4 0 R >>".to_vec(),
            stream_object("", content),
        ]);
        let ws = TestWorkspace::from_bytes("invalid-inline.pdf", &pdf);
        let mut sink = Vec::new();
        let mut ctx = ToolContext::new(
            "invalid-inline",
            "pdf.extract_images",
            ws.paths,
            Limits::local(),
            &mut sink,
        )
        .with_library_dir(&lib_dir);
        let err = extract_images::run(&mut ctx, ExtractImagesParams::default())
            .expect_err("invalid inline image must be refused");
        assert_eq!(err.code, ErrorCode::LimitExceeded);
    }
}

#[test]
fn inherited_form_resources_are_checked_before_page_load() {
    let lib_dir = require_pdfium!();
    let pdf = assemble_pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 20 20] /Resources << /XObject << /A 5 0 R /B 6 0 R >> >> /Contents 4 0 R >>".to_vec(),
        stream_object("", b"/A Do"),
        stream_object("/Type /XObject /Subtype /Form /BBox [0 0 10 10]", b"/B Do"),
        stream_object("/Type /XObject /Subtype /Form /BBox [0 0 10 10]", b"BI /W 4 /H 4 /BPC 8 /CS /G /F /AHx ID 00> EI"),
    ]);
    let ws = TestWorkspace::from_bytes("inherited-inline.pdf", &pdf);
    let mut limits = Limits::local();
    limits.max_asset_bytes = Some(100);
    let mut sink = Vec::new();
    let mut ctx = ToolContext::new(
        "inherited-inline",
        "pdf.extract_images",
        ws.paths,
        limits,
        &mut sink,
    )
    .with_library_dir(lib_dir);
    extract_images::run(&mut ctx, ExtractImagesParams::default())
        .expect("a form reached by multiple paths is charged once");
}

#[test]
fn refuses_ambiguous_inline_keys_and_excess_flate_output() {
    let lib_dir = require_pdfium!();
    for fixture in [
        "fixtures/pdf/inline-conflicting-keys.pdf",
        "fixtures/pdf/inline-flate-oversized.pdf",
    ] {
        let ws = TestWorkspace::from_fixture(fixture);
        let mut sink = Vec::new();
        let mut ctx = ToolContext::new(
            "inline-preflight",
            "pdf.extract_images",
            ws.paths,
            Limits::local(),
            &mut sink,
        )
        .with_library_dir(&lib_dir);
        let err = extract_images::run(&mut ctx, ExtractImagesParams::default())
            .expect_err("unsafe inline image must be refused");
        assert_eq!(err.code, ErrorCode::LimitExceeded, "{fixture}");
    }
}

#[test]
fn refuses_oversized_inline_image_in_indirect_subtype_form() {
    let lib_dir = require_pdfium!();
    let ws = TestWorkspace::from_fixture("fixtures/pdf/indirect-image-preflight.pdf");
    let mut sink = Vec::new();
    let mut ctx = ToolContext::new(
        "indirect-form",
        "pdf.extract_images",
        ws.paths,
        Limits::local(),
        &mut sink,
    )
    .with_library_dir(lib_dir);
    let err = extract_images::run(&mut ctx, ExtractImagesParams::default())
        .expect_err("indirect form subtype must be preflighted");
    assert_eq!(err.code, ErrorCode::LimitExceeded);
}

#[test]
fn refuses_inconsistent_page_tree_before_page_load() {
    let lib_dir = require_pdfium!();
    let ws = TestWorkspace::from_fixture("fixtures/pdf/ambiguous-page-tree.pdf");
    let mut sink = Vec::new();
    let mut ctx = ToolContext::new(
        "page-tree",
        "pdf.extract_images",
        ws.paths,
        Limits::local(),
        &mut sink,
    )
    .with_library_dir(lib_dir);
    let err = extract_images::run(&mut ctx, ExtractImagesParams::default())
        .expect_err("inconsistent page tree must be rejected");
    assert_eq!(err.code, ErrorCode::LimitExceeded);
}

#[test]
fn extraction_streams_and_rejects_cumulative_output_without_artifacts() {
    struct Observe {
        dir: PathBuf,
        counts: Vec<usize>,
        events: Vec<u8>,
    }
    impl std::io::Write for Observe {
        fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
            self.events.extend_from_slice(data);
            Ok(data.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            if self
                .events
                .split(|&b| b == b'\n')
                .rev()
                .nth(1)
                .is_some_and(|event| {
                    event
                        .windows(b"\"stage\":\"extract_images\"".len())
                        .any(|w| w == b"\"stage\":\"extract_images\"")
                })
            {
                self.counts.push(fs::read_dir(&self.dir)?.count());
            }
            Ok(())
        }
    }
    let lib_dir = require_pdfium!();
    let jpeg = make_test_jpeg(24, 24);
    let mut objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        format!(
            "<< /Type /Pages /Kids [{}] /Count 8 >>",
            (0..8)
                .map(|i| format!("{} 0 R", 3 + i * 3))
                .collect::<Vec<_>>()
                .join(" ")
        )
        .into_bytes(),
    ];
    for i in 0..8 {
        let page = 3 + i * 3;
        objects.push(format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Resources << /XObject << /Im {} 0 R >> >> /Contents {} 0 R >>", page + 1, page + 2).into_bytes());
        objects.push(stream_object("/Type /XObject /Subtype /Image /Width 24 /Height 24 /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode", &jpeg));
        objects.push(stream_object("", b"q 24 0 0 24 0 0 cm /Im Do Q"));
    }
    let ws = TestWorkspace::from_bytes("stream.pdf", &assemble_pdf(&objects));
    let mut sink = Observe {
        dir: ws.paths.work_dir.clone(),
        counts: Vec::new(),
        events: Vec::new(),
    };
    let limit = (jpeg.len().max(24 * 24 * 3) * 2) as u64;
    let limits = Limits {
        max_decompressed_bytes: limit,
        ..Limits::local()
    };
    let mut ctx = ToolContext::new(
        "stream",
        "pdf.extract_images",
        ws.paths.clone(),
        limits,
        &mut sink,
    )
    .with_library_dir(lib_dir);
    let err = extract_images::run(&mut ctx, ExtractImagesParams::default())
        .expect_err("cumulative output must be bounded");
    assert_eq!(err.code, ErrorCode::LimitExceeded);
    assert!(
        !sink.counts.is_empty() && sink.counts[0] == 1,
        "the first page should be written before the next page"
    );
    assert!(
        !sink
            .events
            .windows(b"\"type\":\"artifact\"".len())
            .any(|w| w == b"\"type\":\"artifact\"")
    );
    assert_eq!(fs::read_dir(ws.paths.output_dir).unwrap().count(), 0);
}
