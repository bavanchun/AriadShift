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
use pdfium_render::prelude::*;
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
    assert_eq!(err.message, "Image dimensions exceed the pixel limit.");

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
    assert_eq!(err.message, "Image dimensions exceed the pixel limit.");
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
    assert_eq!(err.message, "Encoded inline image data exceeds the limit.");
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
    assert_eq!(err.message, "Image dimensions exceed the pixel limit.");
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
    assert_eq!(err.message, "JPX dimensions exceed the pixel limit.");
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
    assert_eq!(err.message, "Decoded inline image data exceeds the limit.");
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
    assert_eq!(err.message, "Page content entry is not a stream or array.");
}

#[test]
fn repeated_nested_forms_are_expanded_once() {
    let lib_dir = require_pdfium!();
    let depth = 8usize;
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
    assert_eq!(err.message, "Form expansion work exceeds the limit.");
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
    assert_eq!(err.message, "Page form execution work exceeds the limit.");
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
    assert_eq!(
        err.message,
        "Page or document traversal work exceeds the limit."
    );
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
    assert_eq!(err.message, "Page inline image pixels exceed the limit.");
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
    assert_eq!(err.message, "Page inline image pixels exceed the limit.");
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
    assert_eq!(error.message, "Page inline image totals exceed the limit.");
}

#[test]
fn refuses_inline_images_with_missing_or_invalid_dictionary_values() {
    let lib_dir = require_pdfium!();
    for (content, reason) in [
        (
            b"BI /W 1 /H 1 /BPC 8 /F /AHx ID 00> EI".as_slice(),
            "Inline image is missing its color space.",
        ),
        (
            b"BI /W 1 /H 1 /BPC 3 /CS /G /F /AHx ID 00> EI".as_slice(),
            "Inline image has an invalid bits per component value.",
        ),
        (
            b"BI /W 1 /H 1 /BPC 8 /CS /G /IM /bad /F /AHx ID 00> EI".as_slice(),
            "Inline image mask flag is not boolean.",
        ),
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
        assert_eq!(err.message, reason);
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
fn shared_form_resources_are_checked_at_each_depth() {
    let lib_dir = require_pdfium!();
    let pdf = assemble_pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 20 20] /Resources << /XObject << /A 6 0 R /B 5 0 R >> >> /Contents 4 0 R >>".to_vec(),
        stream_object("", b"/B Do"),
        stream_object("/Type /XObject /Subtype /Form /BBox [0 0 10 10]", b"/A Do"),
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
    for (fixture, reason) in [
        (
            "fixtures/pdf/inline-hex-key-render.pdf",
            "Inline image contains an unsafe name token.",
        ),
        (
            "fixtures/pdf/inline-hex-key-bomb.pdf",
            "Inline image contains an unsafe name token.",
        ),
        (
            "fixtures/pdf/inline-conflicting-keys.pdf",
            "Inline image dictionary has conflicting keys.",
        ),
        (
            "fixtures/pdf/inline-flate-oversized.pdf",
            "Decoded inline image data exceeds the limit.",
        ),
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
        assert_eq!(err.message, reason, "{fixture}");
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
    assert_eq!(err.message, "Image dimensions exceed the pixel limit.");
}

#[test]
fn nested_form_uses_page_resources_when_category_is_missing() {
    fn image_data(iter: PdfPageObjectsIterator<'_>, out: &mut Vec<Vec<u8>>) {
        for object in iter {
            match object {
                PdfPageObject::Image(image) => out.push(image.get_raw_image_data().unwrap()),
                PdfPageObject::XObjectForm(form) => image_data(form.iter(), out),
                _ => {}
            }
        }
    }
    let lib_dir = require_pdfium!();
    let bytes = fs::read(repo_root().join("fixtures/pdf/form-page-fallback.pdf")).unwrap();
    let ws = TestWorkspace::from_bytes("form-page-fallback.pdf", &bytes);
    let mut sink = Vec::new();
    let mut ctx = ToolContext::new(
        "form-fallback",
        "pdf.extract_images",
        ws.paths,
        Limits::local(),
        &mut sink,
    )
    .with_library_dir(lib_dir);
    let pdfium = ctx.pdfium().unwrap();
    let doc = pdfium.load_pdf_from_byte_slice(&bytes, None).unwrap();
    let page = doc.pages().get(0).unwrap();
    let mut images = Vec::new();
    image_data(page.objects().iter(), &mut images);
    assert_eq!(images, vec![b"B".to_vec()]);
    let params = ExtractImagesParams {
        min_px: 1,
        ..ExtractImagesParams::default()
    };
    let extracted = extract_images::extract_document_images(&mut ctx, &params).unwrap();
    assert_eq!(extracted.images.first().unwrap().object_id, Some((7, 0)));
}

#[test]
fn accepts_name_like_bytes_inside_image_stream() {
    let lib_dir = require_pdfium!();
    let ws = TestWorkspace::from_fixture("fixtures/pdf/name-like-image-bytes.pdf");
    let mut sink = Vec::new();
    let mut ctx = ToolContext::new(
        "image-bytes",
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
    let result = extract_images::extract_document_images(&mut ctx, &params)
        .expect("image bytes are not names");
    assert_eq!(result.images.len(), 1);
}

#[test]
fn accepts_name_like_text_in_trailing_comment() {
    let lib_dir = require_pdfium!();
    let mut input = fs::read(repo_root().join("fixtures/pdf/canonical-control.pdf")).unwrap();
    input.extend_from_slice(b"\n% /");
    input.extend(std::iter::repeat_n(b'a', 300));
    input.push(b'\n');
    let ws = TestWorkspace::from_bytes("comment.pdf", &input);
    let mut sink = Vec::new();
    let mut ctx = ToolContext::new(
        "comment",
        "pdf.extract_images",
        ws.paths,
        Limits::local(),
        &mut sink,
    )
    .with_library_dir(lib_dir);
    extract_images::extract_document_images(&mut ctx, &ExtractImagesParams::default())
        .expect("a comment is not a PDF name");
}

#[test]
fn refuses_truncated_xobject_names_before_pdfium_page_load() {
    let lib_dir = require_pdfium!();
    for fixture in [
        "long-content-name.pdf",
        "escaped-resource-name.pdf",
        "compressed-resource-name.pdf",
        "compressed-content-name.pdf",
    ] {
        let path = repo_root().join("fixtures/pdf").join(fixture);
        let bytes = fs::read(path).unwrap();
        let ws = TestWorkspace::from_bytes(fixture, &bytes);
        let mut sink = Vec::new();
        let mut ctx = ToolContext::new(
            "long-name",
            "pdf.extract_images",
            ws.paths,
            Limits::local(),
            &mut sink,
        )
        .with_library_dir(&lib_dir);
        let pdfium = ctx.pdfium().expect("PDFium");
        let document = pdfium
            .load_pdf_from_byte_slice(&bytes, None)
            .expect("PDFium loads document");
        let page = document.pages().get(0).expect("PDFium parses page");
        assert_eq!(
            page.objects().len(),
            if fixture == "compressed-resource-name.pdf" {
                0
            } else {
                1
            },
            "PDFium parses {fixture}"
        );
        let error = extract_images::run(&mut ctx, ExtractImagesParams::default())
            .expect_err("ambiguous name is rejected");
        assert_eq!(error.code, ErrorCode::LimitExceeded);
        let reason =
            if fixture == "compressed-content-name.pdf" || fixture == "long-content-name.pdf" {
                "PDF content name exceeds the safe token length."
            } else {
                "PDF object name exceeds the safe token length."
            };
        assert_eq!(error.message, reason, "{fixture}");
    }
}

#[test]
fn refuses_repaired_cross_reference_before_loading_pdfium() {
    {
        let fixture = "repaired-xref-extraction.pdf";
        let ws = TestWorkspace::from_fixture(&format!("fixtures/pdf/{fixture}"));
        let mut sink = Vec::new();
        let mut ctx = ToolContext::new(
            "repaired-xref",
            "pdf.extract_images",
            ws.paths,
            Limits::local(),
            &mut sink,
        );
        let err = extract_images::run(&mut ctx, ExtractImagesParams::default())
            .expect_err("repaired cross-reference must be refused");
        assert_eq!(err.code, ErrorCode::LimitExceeded, "{fixture}");
        assert_eq!(
            err.message,
            "Image extraction cannot inspect a PDF with a repaired cross-reference table.",
            "{fixture}"
        );
    }
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
    assert_eq!(err.message, "Page tree node with Kids is not a Pages node.");
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
    let side = 128u32;
    let rgb = RgbImage::from_fn(side, side, |x, y| {
        image::Rgb([
            (x.wrapping_mul(71) ^ y.wrapping_mul(31)) as u8,
            (x.wrapping_mul(19) ^ y.wrapping_mul(97)) as u8,
            (x.wrapping_mul(43) ^ y.wrapping_mul(59)) as u8,
        ])
    });
    let mut jpeg = Vec::new();
    JpegEncoder::new_with_quality(&mut jpeg, 15)
        .encode_image(&rgb)
        .unwrap();
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
        objects.push(stream_object("/Type /XObject /Subtype /Image /Width 128 /Height 128 /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode", &jpeg));
        objects.push(stream_object("", b"q 128 0 0 128 0 0 cm /Im Do Q"));
    }
    let pdf = assemble_pdf(&objects);
    let ws = TestWorkspace::from_bytes("stream.pdf", &pdf);
    let mut sink = Observe {
        dir: ws.paths.work_dir.clone(),
        counts: Vec::new(),
        events: Vec::new(),
    };
    let limit = (pdf.len() * 2 + 128 * 128 * 4 + 128 * 128 * 3) as u64;
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
    let params = ExtractImagesParams {
        format: ExtractFormat::Png,
        ..ExtractImagesParams::default()
    };
    let err = extract_images::run(&mut ctx, params).expect_err("cumulative output must be bounded");
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

#[test]
fn canonical_output_matches_preflight_for_ambiguous_xrefs() {
    let lib_dir = require_pdfium!();
    let render = |fixture: &str| -> Result<Vec<u8>, ariad_engine_io::EngineFailure> {
        let ws = TestWorkspace::from_fixture(&format!("fixtures/pdf/{fixture}"));
        let mut sink = Vec::new();
        let mut ctx = ToolContext::new(
            "canonical",
            "pdf.extract_images",
            ws.paths,
            Limits::local(),
            &mut sink,
        )
        .with_library_dir(&lib_dir);
        extract_images::extract_document_images(&mut ctx, &ExtractImagesParams::default())?;
        let bytes = ctx.load_validated_pdf_bytes()?;
        let pdfium = ctx.pdfium()?;
        let doc = pdfium.load_pdf_from_byte_slice(&bytes, None).unwrap();
        let page = doc.pages().get(0).unwrap();
        let bitmap = page
            .render_with_config(&PdfRenderConfig::new().set_target_size(100, 100))
            .unwrap();
        Ok(bitmap.as_rgba_bytes())
    };
    let control = render("canonical-control.pdf").expect("control");
    let mut accepted = 0;
    for fixture in [
        "canonical-prev-cycle.pdf",
        "canonical-short-entries.pdf",
        "canonical-gen-precedence.pdf",
        "canonical-hybrid-root.pdf",
        "canonical-objstm-index.pdf",
    ] {
        match render(fixture) {
            Ok(pixels) => {
                accepted += 1;
                assert_eq!(pixels, control, "{fixture}");
            }
            Err(err) => assert!(
                matches!(
                    err.code,
                    ErrorCode::LimitExceeded | ErrorCode::EngineFailure | ErrorCode::InvalidRequest
                ),
                "{fixture}: {err:?}"
            ),
        }
    }
    assert!(
        accepted > 0,
        "at least one ambiguous input must reach PDFium"
    );
}

#[test]
#[ignore = "requires the external adversarial PDF corpus"]
fn adversarial_content_streams_are_refused_before_pdfium() {
    let lib_dir = require_pdfium!();
    let dir = std::env::var("ARIAD_POC_DIR").expect("ARIAD_POC_DIR");
    for name in [
        "round3-2a-inline-key-hex-render.pdf",
        "round3-2b-inline-key-hex-52mp-bomb.pdf",
        "round3-3-string-cr-do.pdf",
        "round3-4-max-objnum-rebuild.pdf",
    ] {
        let input = fs::read(std::path::Path::new(&dir).join(name)).unwrap();
        for extraction in [false, true] {
            let ws = TestWorkspace::from_bytes("input.pdf", &input);
            let mut sink = Vec::new();
            let mut ctx = ToolContext::new(
                "adversarial",
                "pdf.extract_images",
                ws.paths,
                Limits::local(),
                &mut sink,
            )
            .with_library_dir(&lib_dir);
            let result = if extraction {
                extract_images::extract_document_images(&mut ctx, &ExtractImagesParams::default())
                    .map(|_| ())
            } else {
                ctx.load_validated_pdf_bytes().map(|_| ())
            };
            let error = result.expect_err(name);
            assert_eq!(error.code, ErrorCode::LimitExceeded, "{name}: {error:?}");
        }
    }
}

#[test]
#[ignore = "requires the external adversarial PDF corpus"]
fn all_adversarial_pdfs_are_preflighted() {
    let dir = std::env::var("ARIAD_POC_DIR").expect("ARIAD_POC_DIR");
    let mut paths: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "pdf"))
        .collect();
    paths.sort();
    assert!(!paths.is_empty());
    for path in paths {
        let bytes = fs::read(&path).unwrap();
        let ws = TestWorkspace::from_bytes("input.pdf", &bytes);
        let mut sink = Vec::new();
        let mut ctx = ToolContext::new(
            "adversarial",
            "pdf.render",
            ws.paths,
            Limits::local(),
            &mut sink,
        );
        match ctx.load_validated_pdf_bytes() {
            Ok(_) => eprintln!("ACCEPTED {}", path.display()),
            Err(err) => {
                eprintln!("REFUSED {}: {}", path.display(), err.message);
                assert!(matches!(
                    err.code,
                    ErrorCode::LimitExceeded | ErrorCode::EngineFailure | ErrorCode::InvalidRequest
                ));
            }
        }
    }
}

#[test]
#[ignore = "requires the external adversarial PDF corpus"]
fn adversarial_canonical_graphs_are_refused_or_render_the_control() {
    let lib_dir = require_pdfium!();
    let dir = std::env::var("ARIAD_POC_DIR").expect("ARIAD_POC_DIR");
    let render = |path: &std::path::Path| -> Result<Vec<u8>, ariad_engine_io::EngineFailure> {
        let input = fs::read(path).unwrap();
        let ws = TestWorkspace::from_bytes("input.pdf", &input);
        let mut sink = Vec::new();
        let mut ctx = ToolContext::new(
            "poc",
            "pdf.extract_images",
            ws.paths,
            Limits::local(),
            &mut sink,
        )
        .with_library_dir(&lib_dir);
        extract_images::extract_document_images(&mut ctx, &ExtractImagesParams::default())?;
        let bytes = ctx.load_validated_pdf_bytes()?;
        let pdfium = ctx.pdfium()?;
        let doc = pdfium.load_pdf_from_byte_slice(&bytes, None).unwrap();
        let page = doc.pages().get(0).unwrap();
        Ok(page
            .render_with_config(&PdfRenderConfig::new().set_target_size(100, 100))
            .unwrap()
            .as_rgba_bytes())
    };
    let control = render(&repo_root().join("fixtures/pdf/canonical-control.pdf")).unwrap();
    for name in [
        "prev-cycle.pdf",
        "short-entries.pdf",
        "gen-precedence.pdf",
        "hybrid-root.pdf",
        "objstm-index.pdf",
        "deep-64.pdf",
        "deep-65.pdf",
        "inf-real.pdf",
        "neg-inf-real.pdf",
        "zero-ref.pdf",
        "bignum-only.pdf",
    ] {
        match render(&std::path::Path::new(&dir).join(name)) {
            Ok(pixels) => {
                assert!(
                    !matches!(
                        name,
                        "deep-64.pdf"
                            | "deep-65.pdf"
                            | "inf-real.pdf"
                            | "neg-inf-real.pdf"
                            | "zero-ref.pdf"
                    ),
                    "{name} must be refused"
                );
                assert_eq!(pixels, control, "{name}");
            }
            Err(err) => assert!(
                matches!(
                    err.code,
                    ErrorCode::LimitExceeded | ErrorCode::EngineFailure | ErrorCode::InvalidRequest
                ),
                "{name}: {err:?}"
            ),
        }
    }
}

#[test]
fn accepts_prefix_and_zero_previous_xref() {
    let lib_dir = require_pdfium!();
    let control = fs::read(repo_root().join("fixtures/pdf/canonical-control.pdf")).unwrap();
    let text = String::from_utf8_lossy(&control);
    let input = text.replace("/Root 2 0 R >>", "/Root 2 0 R /Prev 0 >>");
    assert_ne!(input.as_bytes(), control);
    // Preserve the binary header while modifying the ASCII trailer.
    let tail = &input.as_bytes()[input.find("xref\n").unwrap()..];
    let original_xref = control.windows(5).position(|v| v == b"xref\n").unwrap();
    let mut bytes = b" \r\n".to_vec();
    bytes.extend_from_slice(&control[..original_xref]);
    bytes.extend_from_slice(tail);
    let ws = TestWorkspace::from_bytes("prefix.pdf", &bytes);
    let mut sink = Vec::new();
    let mut ctx = ToolContext::new(
        "prefix",
        "pdf.extract_images",
        ws.paths,
        Limits::local(),
        &mut sink,
    )
    .with_library_dir(lib_dir);
    extract_images::extract_document_images(&mut ctx, &ExtractImagesParams::default())
        .expect("valid prefix and zero Prev");
}

#[test]
fn accepts_whitespace_before_object_header_at_xref_offset() {
    let lib_dir = require_pdfium!();
    let mut bytes = fs::read(repo_root().join("fixtures/pdf/canonical-control.pdf")).unwrap();
    let xref = bytes.windows(5).position(|w| w == b"xref\n").unwrap();
    let marker = b"5 1\n";
    let start = xref
        + bytes[xref..]
            .windows(marker.len())
            .position(|w| w == marker)
            .unwrap()
        + marker.len();
    let offset: usize = std::str::from_utf8(&bytes[start..start + 10])
        .unwrap()
        .parse()
        .unwrap();
    assert!(bytes[offset - 1].is_ascii_whitespace());
    bytes[start..start + 10].copy_from_slice(format!("{:010}", offset - 1).as_bytes());
    let ws = TestWorkspace::from_bytes("whitespace-offset.pdf", &bytes);
    let mut sink = Vec::new();
    let mut ctx = ToolContext::new(
        "whitespace-offset",
        "pdf.extract_images",
        ws.paths,
        Limits::local(),
        &mut sink,
    )
    .with_library_dir(lib_dir);
    extract_images::extract_document_images(&mut ctx, &ExtractImagesParams::default())
        .expect("whitespace at object offset");
}

#[test]
#[ignore = "requires the external adversarial PDF corpus"]
fn content_container_corpus_is_refused_with_pixel_cap() {
    let lib_dir = require_pdfium!();
    let dir = std::env::var("ARIAD_POC_DIR").expect("ARIAD_POC_DIR");
    for name in [
        "poc_a.pdf",
        "poc_b.pdf",
        "poc_a85.pdf",
        "poc_alias.pdf",
        "poc_ff1.pdf",
        "poc_ff2.pdf",
        "pat.pdf",
        "annot.pdf",
        "smask.pdf",
    ] {
        let input = fs::read(std::path::Path::new(&dir).join(name)).unwrap();
        for extraction in [false, true] {
            let ws = TestWorkspace::from_bytes("input.pdf", &input);
            let mut sink = Vec::new();
            let limits = Limits {
                max_asset_bytes: Some(100_000_000),
                ..Limits::local()
            };
            let mut ctx =
                ToolContext::new("content", "pdf.extract_images", ws.paths, limits, &mut sink)
                    .with_library_dir(&lib_dir);
            let result = if extraction {
                extract_images::extract_document_images(&mut ctx, &ExtractImagesParams::default())
                    .map(|_| ())
            } else {
                ctx.load_validated_pdf_bytes().map(|_| ())
            };
            let error = result.expect_err(name);
            assert_eq!(error.code, ErrorCode::LimitExceeded, "{name}");
        }
    }
}

#[test]
fn page_content_resources_are_removed_from_canonical_output() {
    let input = assemble_pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] /Resources << >> /Contents 4 0 R >>"
            .to_vec(),
        stream_object("/Resources << /Pattern << >> >>", b"q Q"),
    ]);
    let ws = TestWorkspace::from_bytes("input.pdf", &input);
    let mut sink = Vec::new();
    let mut ctx = ToolContext::new("page", "pdf.render", ws.paths, Limits::local(), &mut sink);
    let output = ctx.load_validated_pdf_bytes().unwrap();
    let doc = lopdf::Document::load_mem(&output).unwrap();
    assert!(
        doc.get_object((4, 0))
            .unwrap()
            .as_stream()
            .unwrap()
            .dict
            .get(b"Resources")
            .is_err()
    );
}

#[test]
fn page_content_resource_overrides_do_not_hide_resource_work() {
    for variant in 0..4 {
        let pattern = "/Type /Pattern /PatternType 1 /PaintType 1 /TilingType 1 /BBox [0 0 1 1] /XStep 1 /YStep 1 /Resources << >>";
        let form = "/Type /XObject /Subtype /Form /BBox [0 0 10 10]";
        let mut work = b"q Q ".repeat(600);
        if variant == 1 {
            work.extend_from_slice(b"BI /W 1 /H 1 /BPC 8 /CS /G ID x EI ");
        }
        let repeats = match variant {
            2 => b"/S gs ".repeat(101),
            3 => b"/F Do ".to_vec(),
            _ => b"/P scn ".repeat(101),
        };
        let resources = match variant {
            2 => "<< /ExtGState << /S << /SMask << /G 5 0 R >> >> >> >>",
            3 => "<< /XObject << /F 5 0 R >> /Pattern << /P 6 0 R >> >>",
            _ => "<< /Pattern << /P 5 0 R >> >>",
        };
        let override_resources = match variant {
            2 => "/Resources << /ExtGState << >> >>",
            3 => "/Resources << /XObject << >> >>",
            _ => "/Resources << /Pattern << >> >>",
        };
        let object5 = match variant {
            2 => stream_object(form, &work),
            3 => stream_object(form, &b"/P scn ".repeat(101)),
            _ => stream_object(pattern, &work),
        };
        let page = format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] /Resources {resources} /Contents 4 0 R >>"
        );
        let mut objects = vec![
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
            page.into_bytes(),
            stream_object(override_resources, &repeats),
            object5,
        ];
        if variant == 3 {
            objects.push(stream_object(pattern, &work));
        }
        let ws = TestWorkspace::from_bytes("input.pdf", &assemble_pdf(&objects));
        let mut sink = Vec::new();
        let mut ctx = ToolContext::new("page", "pdf.render", ws.paths, Limits::local(), &mut sink);
        let err = ctx
            .load_validated_pdf_bytes()
            .expect_err("resource work exceeds limit");
        assert_eq!(err.code, ErrorCode::LimitExceeded, "variant {variant}");
        assert!(
            err.message.contains("Content execution cost"),
            "variant {variant}: {err:?}"
        );
    }
}

#[test]
fn type3_glyph_resource_dependencies_require_an_explicit_scope() {
    let cases: &[&[u8]] = &[
        b"0 0 d0 /X Do",
        b"0 0 d0 /S sh",
        b"0 0 d0 /G gs",
        b"0 0 d0 /P scn",
        b"0 0 d0 /P SCN",
        b"0 0 d0 BT /F 1 Tf ET",
        b"0 0 d0 BI /W 1 /H 1 /BPC 8 /CS /G ID x EI",
    ];
    for content in cases {
        for (font_resources, proc_resources, refuse) in [
            ("", "", true),
            ("/Resources << >>", "", false),
            ("", "/Resources << >>", false),
        ] {
            let font = format!(
                "<< /Type /Font /Subtype /Type3 /FontBBox [0 0 1 1] /FontMatrix [1 0 0 1 0 0] /CharProcs << /A 5 0 R >> /Encoding << /Type /Encoding /Differences [65 /A] >> /FirstChar 65 /LastChar 65 /Widths [1] {font_resources} >>"
            );
            let input = assemble_pdf(&[
                b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
                b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
                b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] /Resources << /Font << /T 4 0 R >> >> /Contents 6 0 R >>".to_vec(),
                font.into_bytes(), stream_object(proc_resources, content), stream_object("", b"q Q"),
            ]);
            let ws = TestWorkspace::from_bytes("input.pdf", &input);
            let mut sink = Vec::new();
            let mut ctx =
                ToolContext::new("type3", "pdf.render", ws.paths, Limits::local(), &mut sink);
            let result = ctx.load_validated_pdf_bytes();
            if refuse {
                let err = result.expect_err("ambiguous glyph");
                assert_eq!(err.code, ErrorCode::LimitExceeded);
                assert_eq!(
                    err.message,
                    "Type3 glyph procedure depends on ambiguous page resources"
                );
            } else {
                result.expect("explicit glyph scope");
            }
        }
    }
}

#[test]
fn type3_glyph_rebinding_in_a_child_form_is_refused() {
    for (proc, child_resources) in [
        (
            b"0 0 d0 /P scn".as_slice(),
            "/Font << /T 4 0 R >> /Pattern << >>",
        ),
        (
            b"0 0 d0 /P scn".as_slice(),
            "/Font << /T 4 0 R >> /Pattern << /P 8 0 R >>",
        ),
        (b"0 0 d0 /X Do".as_slice(), "/XObject << /X 8 0 R >>"),
    ] {
        let input = assemble_pdf(&[
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] /Resources << /Font << /T 4 0 R >> /XObject << /F 7 0 R >> >> /Contents 6 0 R >>".to_vec(),
            b"<< /Type /Font /Subtype /Type3 /FontBBox [0 0 1 1] /FontMatrix [1 0 0 1 0 0] /CharProcs << /A 5 0 R >> /Encoding << /Type /Encoding /Differences [65 /A] >> /FirstChar 65 /LastChar 65 /Widths [1] >>".to_vec(),
            stream_object("", proc), stream_object("", b"BT /T 1 Tf /F Do (A) Tj ET"),
            stream_object(&format!("/Type /XObject /Subtype /Form /BBox [0 0 1 1] /Resources << {child_resources} >>"), b"BT /T 1 Tf ET"),
            if child_resources.contains("/Pattern << /P") {
                stream_object("/Type /Pattern /PatternType 1 /PaintType 1 /TilingType 1 /BBox [0 0 1 1] /XStep 1 /YStep 1", b"q Q")
            } else { stream_object("/Type /XObject /Subtype /Form /BBox [0 0 1 1]", b"q Q") },
        ]);
        let ws = TestWorkspace::from_bytes("input.pdf", &input);
        let mut sink = Vec::new();
        let mut ctx = ToolContext::new("glyph", "pdf.render", ws.paths, Limits::local(), &mut sink);
        let err = ctx
            .load_validated_pdf_bytes()
            .expect_err("ambiguous glyph scope");
        assert_eq!(err.code, ErrorCode::LimitExceeded);
        assert_eq!(
            err.message,
            "Type3 glyph procedure depends on ambiguous page resources"
        );
    }
}

#[test]
fn type3_resource_scopes_charge_executed_glyphs() {
    for name in [
        "type3-charproc-resource-budget.pdf",
        "type3-inherited-font-budget.pdf",
    ] {
        let ws = TestWorkspace::from_fixture(&format!("fixtures/pdf/{name}"));
        let mut sink = Vec::new();
        let mut ctx = ToolContext::new("scope", "pdf.render", ws.paths, Limits::local(), &mut sink);
        let err = ctx.load_validated_pdf_bytes().expect_err(name);
        assert_eq!(err.code, ErrorCode::LimitExceeded, "{name}: {err:?}");
        assert!(
            err.message.contains("Content execution cost"),
            "{name}: {err:?}"
        );
    }
}

#[test]
#[ignore = "requires external resource-scope regression corpus"]
fn resource_scope_corpus_is_refused() {
    let lib_dir = require_pdfium!();
    let dir = std::env::var("ARIAD_POC_DIR").expect("ARIAD_POC_DIR");
    let mut count = 0;
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy();
        if !name.starts_with("r5-poc") || !name.ends_with(".pdf") {
            continue;
        }
        count += 1;
        let bytes = fs::read(&path).unwrap();
        for extraction in [false, true] {
            let ws = TestWorkspace::from_bytes("input.pdf", &bytes);
            let mut sink = Vec::new();
            let mut ctx =
                ToolContext::new("scope", "pdf.render", ws.paths, Limits::local(), &mut sink)
                    .with_library_dir(&lib_dir);
            let result = if extraction {
                extract_images::extract_document_images(&mut ctx, &ExtractImagesParams::default())
                    .map(|_| ())
            } else {
                ctx.load_validated_pdf_bytes().map(|_| ())
            };
            let err = result.expect_err(&name);
            assert_eq!(err.code, ErrorCode::LimitExceeded, "{name}: {err:?}");
        }
    }
    assert_eq!(count, 8);
}

#[test]
#[ignore = "requires the external resource-scope corpus"]
fn page_and_glyph_resource_corpus_is_refused() {
    let lib_dir = require_pdfium!();
    let dir = std::env::var("ARIAD_POC_DIR").expect("ARIAD_POC_DIR");
    let mut paths: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "pdf"))
        .collect();
    paths.sort();
    assert_eq!(paths.len(), 14);
    for path in paths {
        let bytes = fs::read(&path).unwrap();
        for extraction in [false, true] {
            let ws = TestWorkspace::from_bytes("input.pdf", &bytes);
            let mut sink = Vec::new();
            let mut ctx =
                ToolContext::new("scope", "pdf.render", ws.paths, Limits::local(), &mut sink)
                    .with_library_dir(&lib_dir);
            let result = if extraction {
                extract_images::extract_document_images(&mut ctx, &ExtractImagesParams::default())
                    .map(|_| ())
            } else {
                ctx.load_validated_pdf_bytes().map(|_| ())
            };
            let err = result.expect_err(&path.display().to_string());
            assert_eq!(
                err.code,
                ErrorCode::LimitExceeded,
                "{}: {err:?}",
                path.display()
            );
        }
    }
}

fn pdf_with_content(content: &[u8]) -> Vec<u8> {
    assemble_pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] /Resources << >> /Contents 4 0 R >>"
            .to_vec(),
        stream_object("", content),
    ])
}

#[test]
fn indirect_annotation_appearance_without_classification_is_refused() {
    let input = assemble_pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] /Resources << >> /Contents 4 0 R /Annots 5 0 R >>".to_vec(),
        stream_object("", b"q Q"),
        b"[ << /Type /Annot /Subtype /Stamp /Rect [0 0 10 10] /AP << /N 6 0 R >> >> ]".to_vec(),
        stream_object("", b"q Q"),
    ]);
    let ws = TestWorkspace::from_bytes("appearance.pdf", &input);
    let mut sink = Vec::new();
    let mut ctx = ToolContext::new(
        "appearance",
        "pdf.render",
        ws.paths,
        Limits::local(),
        &mut sink,
    );
    let err = ctx
        .load_validated_pdf_bytes()
        .expect_err("unclassified appearance must be refused");
    assert_eq!(err.code, ErrorCode::LimitExceeded);
}

#[test]
fn nested_form_pattern_and_mask_depth_is_enforced_before_pdfium() {
    use lopdf::{Document, Object, Stream, dictionary};
    let lib_dir = require_pdfium!();
    for depth in [8, 9] {
        let mut doc = Document::with_version("1.4");
        let pages = doc.new_object_id();
        let leaf_dict = if depth % 3 == 1 {
            dictionary! { "Type" => "Pattern", "PatternType" => 1, "PaintType" => 1, "TilingType" => 1, "BBox" => vec![0.into(),0.into(),10.into(),10.into()], "XStep" => 10, "YStep" => 10 }
        } else {
            dictionary! { "Type" => "XObject", "Subtype" => "Form", "BBox" => vec![0.into(),0.into(),10.into(),10.into()] }
        };
        let leaf = doc.add_object(Stream::new(leaf_dict, b"0 0 1 1 re f".to_vec()));
        let mut next = leaf;
        for i in (1..depth).rev() {
            let bbox = vec![0.into(), 0.into(), 10.into(), 10.into()];
            let (dict, content) = match i % 3 {
                0 => (
                    dictionary! { "Type" => "XObject", "Subtype" => "Form", "BBox" => bbox, "Resources" => dictionary! { "Pattern" => dictionary! { "P" => Object::Reference(next) } } },
                    b"/Pattern cs /P scn 0 0 1 1 re f".to_vec(),
                ),
                1 => (
                    dictionary! { "Type" => "Pattern", "PatternType" => 1, "PaintType" => 1, "TilingType" => 1, "BBox" => bbox, "XStep" => 10, "YStep" => 10, "Resources" => dictionary! { "XObject" => dictionary! { "F" => Object::Reference(next) } } },
                    b"/F Do".to_vec(),
                ),
                _ => (
                    dictionary! { "Type" => "XObject", "Subtype" => "Form", "BBox" => bbox, "Resources" => dictionary! { "ExtGState" => dictionary! { "GS" => dictionary! { "Type" => "ExtGState", "SMask" => dictionary! { "S" => "Alpha", "G" => Object::Reference(next) } } } } },
                    b"/GS gs".to_vec(),
                ),
            };
            next = doc.add_object(Stream::new(dict, content));
        }
        // The outer resource operation matches the kind of its target.
        let root = b"/Pattern cs /P scn 0 0 1 1 re f".to_vec();
        let content = doc.add_object(Stream::new(dictionary! {}, root));
        let page = doc.add_object(dictionary! { "Type" => "Page", "Parent" => Object::Reference(pages), "MediaBox" => vec![0.into(),0.into(),10.into(),10.into()], "Contents" => Object::Reference(content), "Resources" => dictionary! { "Pattern" => dictionary! { "P" => Object::Reference(next) } } });
        doc.objects.insert(pages, Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => vec![Object::Reference(page)], "Count" => 1 }));
        let catalog = doc
            .add_object(dictionary! { "Type" => "Catalog", "Pages" => Object::Reference(pages) });
        doc.trailer.set("Root", Object::Reference(catalog));
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();
        let ws = TestWorkspace::from_bytes("depth.pdf", &bytes);
        let mut sink = Vec::new();
        let mut ctx = ToolContext::new("depth", "pdf.render", ws.paths, Limits::local(), &mut sink)
            .with_library_dir(&lib_dir);
        let result = ctx.load_validated_pdf_bytes();
        if depth == 9 {
            assert_eq!(result.unwrap_err().code, ErrorCode::LimitExceeded);
        } else {
            let bytes = result.expect("eight levels must be accepted");
            let pdfium = ctx.pdfium().unwrap();
            let doc = pdfium.load_pdf_from_byte_slice(&bytes, None).unwrap();
            doc.pages()
                .get(0)
                .unwrap()
                .render_with_config(&PdfRenderConfig::new().set_target_size(10, 10))
                .unwrap();
        }
    }
}

#[test]
fn inline_filter_payload_boundaries_match_pdfium() {
    use std::io::Write;
    let lib_dir = require_pdfium!();
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&[0; 4]).unwrap();
    let flate = encoder.finish().unwrap();
    let mut trailing = flate.clone();
    trailing.extend_from_slice(b"XYZ");
    let samples: Vec<(&str, Vec<u8>, bool)> = vec![
        ("/Filter /FlateDecode", flate, false),
        ("/Filter /FlateDecode", trailing, true),
        ("/Filter /RunLengthDecode", vec![253, 0, 128], false),
        (
            "/Filter /LZWDecode /DecodeParms << /EarlyChange 0 >>",
            vec![0x80, 0, 0, 0, 0, 4, 4],
            false,
        ),
        (
            "/Filter /LZWDecode /DecodeParms << /EarlyChange 1 >>",
            vec![0x80, 0, 0, 0, 0, 4, 4],
            false,
        ),
        ("/Filter /ASCIIHexDecode", b"00 00\n00 00 >".to_vec(), false),
        ("/Filter /ASCII85Decode", b"z~>".to_vec(), false),
    ];
    let render = |pdfium: &Pdfium, bytes: &[u8]| {
        let doc = pdfium.load_pdf_from_byte_slice(bytes, None).unwrap();
        doc.pages()
            .get(0)
            .unwrap()
            .render_with_config(&PdfRenderConfig::new().set_target_size(40, 40))
            .unwrap()
            .as_rgba_bytes()
    };
    for (filter, data, must_refuse) in samples {
        let mut content = format!("q 1 0 0 1 20 20 cm BI /Width 2 /Height 2 /BitsPerComponent 8 /ColorSpace /DeviceGray {filter} ID ").into_bytes();
        content.extend_from_slice(&data);
        content.extend_from_slice(b" EI Q 0 0 0 rg 1 1 8 8 re f");
        let input = pdf_with_content(&content);
        let ws = TestWorkspace::from_bytes("inline.pdf", &input);
        let mut sink = Vec::new();
        let mut ctx =
            ToolContext::new("inline", "pdf.render", ws.paths, Limits::local(), &mut sink)
                .with_library_dir(&lib_dir);
        let canonical = ctx.load_validated_pdf_bytes();
        if must_refuse {
            assert_eq!(
                canonical.unwrap_err().code,
                ErrorCode::LimitExceeded,
                "{filter}"
            );
            continue;
        }
        let canonical = canonical.unwrap_or_else(|err| panic!("{filter}: {err:?}"));
        let pdfium = ctx.pdfium().unwrap();
        let control = pdf_with_content(b"0 0 0 rg 1 1 8 8 re f");
        let expected = render(&pdfium, &control);
        assert_eq!(render(&pdfium, &input), expected, "original {filter}");
        assert_eq!(render(&pdfium, &canonical), expected, "canonical {filter}");
    }
}

fn zlib_compress(data: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(data).expect("zlib write");
    encoder.finish().expect("zlib finish")
}

fn pdf_with_page_resources(
    resources: &str,
    content: &[u8],
    extra_objects: Vec<Vec<u8>>,
) -> Vec<u8> {
    let mut objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 20 20] /Resources << {resources} >> /Contents 4 0 R >>"
        )
        .into_bytes(),
        stream_object("", content),
    ];
    objects.extend(extra_objects);
    assemble_pdf(&objects)
}

#[test]
fn inline_resource_colorspace_renders_and_extracts_like_device_rgb() {
    let rgb_samples = b"\xff\x00\x00\x00\xff\x00\x00\x00\xff\xff\xff\x00";
    let control_content = [
        b"q 10 0 0 10 2 2 cm BI /W 2 /H 2 /BPC 8 /CS /RGB ID ".as_slice(),
        rgb_samples.as_slice(),
        b" EI Q".as_slice(),
    ]
    .concat();
    let control_pdf = pdf_with_page_resources("", &control_content, vec![]);

    for (key, res, extra) in [
        ("/CS", "/ColorSpace << /Cs8 /DeviceRGB >>", vec![]),
        (
            "/ColorSpace",
            "/ColorSpace << /Cs8 5 0 R >>",
            vec![b"/DeviceRGB".to_vec()],
        ),
    ] {
        let mut content =
            format!("q 10 0 0 10 2 2 cm BI /W 2 /H 2 /BPC 8 {key} /Cs8 ID ").into_bytes();
        content.extend_from_slice(rgb_samples);
        content.extend_from_slice(b" EI Q BI /W 1 /H 1 /BPC 8 /CS /G ID \x80 EI");
        let input = pdf_with_page_resources(res, &content, extra.clone());
        let ws = TestWorkspace::from_bytes("inline_cs.pdf", &input);
        let mut sink = Vec::new();
        let mut ctx = ToolContext::new(
            "inline-cs",
            "pdf.render",
            ws.paths,
            Limits::local(),
            &mut sink,
        );
        let canonical = ctx
            .load_validated_pdf_bytes()
            .expect("resource-named inline colorspace should canonicalize");
        let parsed = lopdf::Document::load_mem(&canonical).expect("valid canonical pdf");
        let page_id = *parsed.get_pages().values().next().unwrap();
        let stream_bytes = parsed.get_page_content(page_id);
        let text = String::from_utf8_lossy(&stream_bytes);
        assert!(
            text.contains("/ColorSpace /Cs8 "),
            "resource name should stay /Cs8: {text}"
        );
        assert!(
            text.contains("/ColorSpace /DeviceGray "),
            "abbreviated /G should still expand to /DeviceGray: {text}"
        );

        let Ok(lib_dir) = std::env::var("ARIAD_TEST_PDFIUM_DIR") else {
            continue;
        };
        if lib_dir.is_empty() {
            continue;
        }

        let control_ws = TestWorkspace::from_bytes("control.pdf", &control_pdf);
        let mut control_sink = Vec::new();
        let mut control_ctx = ToolContext::new(
            "control",
            "pdf.extract_images",
            control_ws.paths,
            Limits::local(),
            &mut control_sink,
        )
        .with_library_dir(&lib_dir);

        let single_inline = [
            format!("q 10 0 0 10 2 2 cm BI /W 2 /H 2 /BPC 8 {key} /Cs8 ID ").into_bytes(),
            rgb_samples.to_vec(),
            b" EI Q".to_vec(),
        ]
        .concat();
        let single_input = pdf_with_page_resources(res, &single_inline, extra.clone());

        let test_ws = TestWorkspace::from_bytes("resource.pdf", &single_input);
        let mut test_sink = Vec::new();
        let mut test_ctx = ToolContext::new(
            "resource",
            "pdf.extract_images",
            test_ws.paths,
            Limits::local(),
            &mut test_sink,
        )
        .with_library_dir(&lib_dir);

        let params = ExtractImagesParams {
            min_px: 1,
            format: ExtractFormat::Png,
            ..ExtractImagesParams::default()
        };
        let pdfium = control_ctx.pdfium().unwrap();
        let expected_doc = pdfium.load_pdf_from_byte_slice(&control_pdf, None).unwrap();
        let expected_rgba = expected_doc
            .pages()
            .get(0)
            .unwrap()
            .render_with_config(&PdfRenderConfig::new().set_target_size(20, 20))
            .unwrap()
            .as_rgba_bytes();

        let single_ws = TestWorkspace::from_bytes("single.pdf", &single_input);
        let mut single_sink = Vec::new();
        let mut single_ctx = ToolContext::new(
            "single",
            "pdf.render",
            single_ws.paths,
            Limits::local(),
            &mut single_sink,
        )
        .with_library_dir(&lib_dir);
        let single_canonical = single_ctx.load_validated_pdf_bytes().unwrap();
        let actual_doc = pdfium
            .load_pdf_from_byte_slice(&single_canonical, None)
            .unwrap();
        let actual_rgba = actual_doc
            .pages()
            .get(0)
            .unwrap()
            .render_with_config(&PdfRenderConfig::new().set_target_size(20, 20))
            .unwrap()
            .as_rgba_bytes();
        assert_eq!(
            actual_rgba, expected_rgba,
            "rendered page must match /DeviceRGB control (case `{key}` `{res}`)"
        );

        if res.contains("<< /Cs8 /DeviceRGB >>") {
            let expected_extract =
                extract_images::extract_document_images(&mut control_ctx, &params)
                    .expect("control extract");
            let actual_extract = extract_images::extract_document_images(&mut test_ctx, &params)
                .unwrap_or_else(|e| panic!("resource extract ({key} {res}): {e:?}"));
            assert_eq!(actual_extract.images[0].width, 2);
            assert_eq!(actual_extract.images[0].height, 2);
            let expected_png = &expected_extract.images[0].bytes;
            let actual_png = &actual_extract.images[0].bytes;
            assert_eq!(
                image::load_from_memory(actual_png).unwrap().to_rgba8(),
                image::load_from_memory(expected_png).unwrap().to_rgba8(),
                "extracted inline image pixels must match /DeviceRGB control"
            );
        }
    }
}

#[test]
fn inline_indexed_resource_and_base_colorspaces() {
    // Wait: inline image indexes 2x2 image using 2 palette entries (red, green)
    let indexed_samples = b"\x00\x01\x01\x00";
    let control_content = [
        b"q 10 0 0 10 2 2 cm BI /W 2 /H 2 /BPC 8 /CS [/Indexed /DeviceRGB 1 <FF0000 00FF00>] ID "
            .as_slice(),
        indexed_samples.as_slice(),
        b" EI Q".as_slice(),
    ]
    .concat();
    let control_pdf = pdf_with_page_resources("", &control_content, vec![]);

    type IndexedCase<'a> = (&'a str, Vec<u8>, Vec<Vec<u8>>, &'a str);
    let cases: Vec<IndexedCase<'_>> = vec![
        (
            "/ColorSpace << /CsIdx [/Indexed /DeviceRGB 1 5 0 R] >>",
            [
                b"q 10 0 0 10 2 2 cm BI /W 2 /H 2 /BPC 8 /CS /CsIdx ID ".as_slice(),
                indexed_samples.as_slice(),
                b" EI Q".as_slice(),
            ]
            .concat(),
            vec![stream_object("", b"\xff\x00\x00\x00\xff\x00")],
            "/ColorSpace /CsIdx ",
        ),
        (
            "/ColorSpace << /CsIdx [/Indexed 5 0 R 1 6 0 R] >>",
            [
                b"q 10 0 0 10 2 2 cm BI /W 2 /H 2 /BPC 8 /CS /CsIdx ID ".as_slice(),
                indexed_samples.as_slice(),
                b" EI Q".as_slice(),
            ]
            .concat(),
            vec![
                b"/DeviceRGB".to_vec(),
                stream_object("", b"\xff\x00\x00\x00\xff\x00"),
            ],
            "/ColorSpace /CsIdx ",
        ),
        (
            "",
            [
                b"q 10 0 0 10 2 2 cm BI /W 2 /H 2 /BPC 8 /CS [/I /DeviceRGB 1 <FF0000 00FF00>] ID "
                    .as_slice(),
                indexed_samples.as_slice(),
                b" EI Q".as_slice(),
            ]
            .concat(),
            vec![],
            "/ColorSpace [ /Indexed /DeviceRGB ",
        ),
        (
            "",
            [
                b"q 10 0 0 10 2 2 cm BI /W 2 /H 2 /BPC 8 /CS [/I /RGB 1 <FF0000 00FF00>] ID "
                    .as_slice(),
                indexed_samples.as_slice(),
                b" EI Q".as_slice(),
            ]
            .concat(),
            vec![],
            "/ColorSpace [ /Indexed /RGB ",
        ),
    ];

    for (res, content, extra, expected_snippet) in cases {
        let input = pdf_with_page_resources(res, &content, extra.clone());
        let ws = TestWorkspace::from_bytes("indexed.pdf", &input);
        let mut sink = Vec::new();
        let mut ctx = ToolContext::new(
            "indexed",
            "pdf.render",
            ws.paths,
            Limits::local(),
            &mut sink,
        );
        let canonical = ctx
            .load_validated_pdf_bytes()
            .unwrap_or_else(|e| panic!("failed {expected_snippet}: {e:?}"));
        let parsed = lopdf::Document::load_mem(&canonical).unwrap();
        let page_id = *parsed.get_pages().values().next().unwrap();
        let stream_bytes = parsed.get_page_content(page_id);
        let text = String::from_utf8_lossy(&stream_bytes);
        assert!(
            text.contains(expected_snippet),
            "missing `{expected_snippet}` in: {text}"
        );

        let Ok(lib_dir) = std::env::var("ARIAD_TEST_PDFIUM_DIR") else {
            continue;
        };
        if lib_dir.is_empty() {
            continue;
        }
        let render_ws = TestWorkspace::from_bytes("render.pdf", &canonical);
        let mut render_sink = Vec::new();
        let render_ctx = ToolContext::new(
            "render",
            "pdf.render",
            render_ws.paths,
            Limits::local(),
            &mut render_sink,
        )
        .with_library_dir(&lib_dir);
        let pdfium = render_ctx.pdfium().unwrap();
        let expected_doc = pdfium.load_pdf_from_byte_slice(&control_pdf, None).unwrap();
        let actual_doc = pdfium.load_pdf_from_byte_slice(&canonical, None).unwrap();
        let expected_rgba = expected_doc
            .pages()
            .get(0)
            .unwrap()
            .render_with_config(&PdfRenderConfig::new().set_target_size(20, 20))
            .unwrap()
            .as_rgba_bytes();
        let actual_rgba = actual_doc
            .pages()
            .get(0)
            .unwrap()
            .render_with_config(&PdfRenderConfig::new().set_target_size(20, 20))
            .unwrap()
            .as_rgba_bytes();
        assert_eq!(
            actual_rgba, expected_rgba,
            "rendered indexed mismatch for `{res}` (`{expected_snippet}`)"
        );
    }
}

#[test]
fn inline_resource_colorspace_follows_execution_budget_scopes() {
    // 1. Inherited from /Pages /Resources down to Page without /Resources
    let inherited_page = assemble_pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 /Resources << /ColorSpace << /Cs8 /DeviceRGB >> >> >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] /Contents 4 0 R >>".to_vec(),
        stream_object("", b"BI /W 1 /H 1 /BPC 8 /CS /Cs8 ID abc EI"),
    ]);
    // 2. Form inherits when no /Resources; overrides when /ColorSpace present; falls back to Page when /Resources omits /ColorSpace
    let form_scopes = assemble_pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] /Resources << /XObject << /FInherit 5 0 R /FOuter 6 0 R >> /ColorSpace << /Cs1 /DeviceGray /CsRgb /DeviceRGB >> >> /Contents 4 0 R >>".to_vec(),
        stream_object("", b"/FInherit Do /FOuter Do"),
        // 5: inherits Page CsRgb (3 bytes)
        stream_object("/Type /XObject /Subtype /Form /BBox [0 0 10 10]", b"BI /W 1 /H 1 /BPC 8 /CS /CsRgb ID abc EI"),
        // 6: overrides Cs1 to DeviceRGB (3 bytes) and invokes 7 which omits /ColorSpace so 7 MUST fall back to Page Cs1 (DeviceGray, 1 byte)
        stream_object("/Type /XObject /Subtype /Form /BBox [0 0 10 10] /Resources << /XObject << /FInner 7 0 R >> /ColorSpace << /Cs1 /DeviceRGB >> >>", b"BI /W 1 /H 1 /BPC 8 /CS /Cs1 ID abc EI /FInner Do"),
        // 7: has /Resources << /Font << >> >> (omits /ColorSpace) -> falls back to Page Cs1 (1 byte = x)
        stream_object("/Type /XObject /Subtype /Form /BBox [0 0 10 10] /Resources << /Font << >> >>", b"BI /W 1 /H 1 /BPC 8 /CS /Cs1 ID x EI"),
    ]);
    // 3. Tiling Pattern (PatternType 1), Annotation AP, ExtGState SMask falling back to physical_page, and Type3 CharProc
    let pattern_ap_smask_type3 = assemble_pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] /Resources << /ColorSpace << /CsPage /DeviceGray /CsPhys /DeviceRGB >> /XObject << /FOuter 5 0 R >> /Pattern << /P1 6 0 R >> /Font << /T3 7 0 R >> >> /Annots [8 0 R] /Contents 4 0 R >>".to_vec(),
        stream_object("", b"/FOuter Do /P1 scn BT /T3 1 Tf (A) Tj ET"),
        // 5: Outer Form overrides /CsPhys to /DeviceGray, invokes /GS1 whose /SMask /G (10 0 R) MUST use physical_page CsPhys (/DeviceRGB = 3 bytes)
        stream_object("/Type /XObject /Subtype /Form /BBox [0 0 10 10] /Resources << /ColorSpace << /CsPhys /DeviceGray >> /ExtGState << /GS1 << /SMask << /S /Alpha /G 10 0 R >> >> >> >>", b"/GS1 gs"),
        // 6: PatternType 1 with no /Resources inherits Page CsPage (1 byte)
        stream_object("/Type /Pattern /PatternType 1 /PaintType 1 /TilingType 1 /BBox [0 0 10 10] /XStep 10 /YStep 10", b"BI /W 1 /H 1 /BPC 8 /CS /CsPage ID x EI"),
        // 7: Type3 Font with /Resources << /ColorSpace << /CsFont /DeviceRGB >> >> and CharProc /A = 11 0 R
        b"<< /Type /Font /Subtype /Type3 /FontBBox [0 0 1 1] /FontMatrix [1 0 0 1 0 0] /CharProcs << /A 11 0 R >> /Encoding << /Type /Encoding /Differences [65 /A] >> /FirstChar 65 /LastChar 65 /Widths [1] /Resources << /ColorSpace << /CsFont /DeviceRGB >> >> >>".to_vec(),
        // 8: Annotation with /AP /N 9 0 R
        b"<< /Type /Annot /Subtype /Stamp /Rect [0 0 10 10] /AP << /N 9 0 R >> >>".to_vec(),
        // 9: AP stream with own override /CsAp
        stream_object("/Type /XObject /Subtype /Form /BBox [0 0 10 10] /Resources << /ColorSpace << /CsAp /DeviceRGB >> >>", b"BI /W 1 /H 1 /BPC 8 /CS /CsAp ID abc EI"),
        // 10: SMask Group Form without /Resources -> physical_page CsPhys (DeviceRGB = 3 bytes)
        stream_object("/Type /XObject /Subtype /Form /BBox [0 0 10 10]", b"BI /W 1 /H 1 /BPC 8 /CS /CsPhys ID rgb EI"),
        // 11: Type3 CharProc without /Resources -> inherits Font CsFont (DeviceRGB = 3 bytes)
        stream_object("", b"0 0 d0 BI /W 1 /H 1 /BPC 8 /CS /CsFont ID rgb EI"),
    ]);
    // 4. Page /Contents stream ignoring its own /Resources (CsPage IS on Page, even if /Contents has conflicting /CsPage)
    let contents_ignores_own_resources = assemble_pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] /Resources << /ColorSpace << /Cs8 /DeviceRGB >> >> /Contents 4 0 R >>".to_vec(),
        stream_object("/Resources << /ColorSpace << /Cs8 /DeviceGray >> >>", b"BI /W 1 /H 1 /BPC 8 /CS /Cs8 ID rgb EI"),
    ]);

    for (label, bytes) in [
        ("inherited_page", inherited_page),
        ("form_scopes", form_scopes),
        ("pattern_ap_smask_type3", pattern_ap_smask_type3),
        (
            "contents_ignores_own_resources",
            contents_ignores_own_resources,
        ),
    ] {
        let ws = TestWorkspace::from_bytes("scope.pdf", &bytes);
        let mut sink = Vec::new();
        let mut ctx = ToolContext::new(label, "pdf.render", ws.paths, Limits::local(), &mut sink);
        ctx.load_validated_pdf_bytes()
            .unwrap_or_else(|e| panic!("{label} should pass: {e:?}"));
    }

    // Contents stream /Resources alone (NOT on Page) must fail closed
    let content_only_res = assemble_pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] /Contents 4 0 R >>".to_vec(),
        stream_object(
            "/Resources << /ColorSpace << /Cs8 /DeviceRGB >> >>",
            b"BI /W 1 /H 1 /BPC 8 /CS /Cs8 ID rgb EI",
        ),
    ]);
    let ws = TestWorkspace::from_bytes("content_only.pdf", &content_only_res);
    let mut sink = Vec::new();
    let mut ctx = ToolContext::new(
        "content-only",
        "pdf.render",
        ws.paths,
        Limits::local(),
        &mut sink,
    );
    assert_eq!(
        ctx.load_validated_pdf_bytes().unwrap_err().code,
        ErrorCode::LimitExceeded,
        "page /Contents stream /Resources must be ignored"
    );
}

#[test]
fn shared_stream_across_multiple_scopes_is_bounded_and_scoped() {
    // 200x150 = 30_000 px: DeviceGray decoded_len = 30_000; DeviceRGB decoded_len = 90_000.
    let gray_payload = zlib_compress(&vec![0u8; 200 * 150]);
    let shared_form = [
        b"BI /W 200 /H 150 /BPC 8 /CS /Cs /F /Fl ID ".as_slice(),
        gray_payload.as_slice(),
        b" EI".as_slice(),
    ]
    .concat();

    let build_two_page = |p1_ops: usize, p2_ops: usize, p2_cs: &str| {
        let mut p1_body = b"/F Do ".to_vec();
        p1_body.extend_from_slice(&b"q Q ".repeat(p1_ops));
        let mut p2_body = b"/F Do ".to_vec();
        p2_body.extend_from_slice(&b"q Q ".repeat(p2_ops));
        assemble_pdf(&[
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            b"<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] /Resources << /XObject << /F 7 0 R >> /ColorSpace << /Cs /DeviceGray >> >> /Contents 5 0 R >>".to_vec(),
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] /Resources << /XObject << /F 7 0 R >> {p2_cs} >> /Contents 6 0 R >>"
            )
            .into_bytes(),
            stream_object("", &p1_body),
            stream_object("", &p2_body),
            stream_object("/Type /XObject /Subtype /Form /BBox [0 0 10 10]", &shared_form),
        ])
    };

    // Valid: Page 1 has 20_000 ops (20_000 * 2 = 40_000 + 30_002 < 100_000); Page 2 has 1 op (2 + 90_002 < 100_000)
    let both_fit = build_two_page(20_000, 1, "/ColorSpace << /Cs /DeviceRGB >>");
    {
        let ws = TestWorkspace::from_bytes("both_fit.pdf", &both_fit);
        let mut sink = Vec::new();
        let mut ctx = ToolContext::new(
            "both-fit",
            "pdf.render",
            ws.paths,
            Limits::local(),
            &mut sink,
        );
        ctx.load_validated_pdf_bytes()
            .expect("both pages within per-scope budget");
    }

    // Over budget on Page 2 ONLY because Cs -> DeviceRGB (90_002 + 10_000 > 100_000), while Page 1 Cs -> DeviceGray fit!
    let p2_over = build_two_page(1, 5_500, "/ColorSpace << /Cs /DeviceRGB >>");
    {
        let ws = TestWorkspace::from_bytes("p2_over.pdf", &p2_over);
        let mut sink = Vec::new();
        let mut ctx = ToolContext::new(
            "p2-over",
            "pdf.render",
            ws.paths,
            Limits::local(),
            &mut sink,
        );
        let err = ctx
            .load_validated_pdf_bytes()
            .expect_err("shared form must not undercharge Page 2 RGB budget");
        assert_eq!(
            err.code,
            ErrorCode::LimitExceeded,
            "unexpected error: {err:?}"
        );
    }

    // Over budget specifically at page invocation fan-out when Form fits single-pass preflight:
    // 100x100 = 10_000 Gray, 30_000 RGB; 4 invocations -> 40_008 on Page 1 vs 120_008 on Page 2.
    {
        let small_payload = zlib_compress(&vec![0u8; 100 * 100]);
        let small_form = [
            b"BI /W 100 /H 100 /BPC 8 /CS /Cs /F /Fl ID ".as_slice(),
            small_payload.as_slice(),
            b" EI".as_slice(),
        ]
        .concat();
        let fanout = assemble_pdf(&[
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            b"<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] /Resources << /XObject << /F 7 0 R >> /ColorSpace << /Cs /DeviceGray >> >> /Contents 5 0 R >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] /Resources << /XObject << /F 7 0 R >> /ColorSpace << /Cs /DeviceRGB >> >> /Contents 6 0 R >>".to_vec(),
            stream_object("", b"/F Do /F Do /F Do /F Do"),
            stream_object("", b"/F Do /F Do /F Do /F Do"),
            stream_object("/Type /XObject /Subtype /Form /BBox [0 0 10 10]", &small_form),
        ]);
        let ws = TestWorkspace::from_bytes("fanout_over.pdf", &fanout);
        let mut sink = Vec::new();
        let mut ctx = ToolContext::new(
            "fanout-over",
            "pdf.render",
            ws.paths,
            Limits::local(),
            &mut sink,
        );
        let err = ctx
            .load_validated_pdf_bytes()
            .expect_err("4 invocations of 30_000 RGB form must exceed 100_000 page work");
        assert_eq!(
            err.code,
            ErrorCode::LimitExceeded,
            "unexpected error: {err:?}"
        );
    }

    // Missing /Cs on Page 2 even though Page 1 defines /Cs -> fails closed!
    let p2_missing = build_two_page(1, 1, "");
    {
        let ws = TestWorkspace::from_bytes("p2_missing.pdf", &p2_missing);
        let mut sink = Vec::new();
        let mut ctx = ToolContext::new(
            "p2-missing",
            "pdf.render",
            ws.paths,
            Limits::local(),
            &mut sink,
        );
        assert_eq!(
            ctx.load_validated_pdf_bytes().unwrap_err().code,
            ErrorCode::LimitExceeded
        );
    }
}

#[test]
fn rejects_unresolvable_cyclic_and_malformed_inline_colorspaces() {
    type RejectCase<'a> = (&'a str, &'a str, &'a [u8], Vec<Vec<u8>>);
    let cases: Vec<RejectCase<'_>> = vec![
        (
            "missing_name",
            "/ColorSpace << /Other /DeviceGray >>",
            b"BI /W 1 /H 1 /BPC 8 /CS /Missing /F /AHx ID 00> EI",
            vec![],
        ),
        (
            "self_ref_cycle",
            "/ColorSpace << /Cs 5 0 R >>",
            b"BI /W 1 /H 1 /BPC 8 /CS /Cs /F /AHx ID 00> EI",
            vec![b"5 0 R".to_vec()],
        ),
        (
            "two_step_ref_cycle",
            "/ColorSpace << /Cs 5 0 R >>",
            b"BI /W 1 /H 1 /BPC 8 /CS /Cs /F /AHx ID 00> EI",
            vec![b"6 0 R".to_vec(), b"5 0 R".to_vec()],
        ),
        (
            "indexed_container_cycle",
            "/ColorSpace << /Cs 5 0 R >>",
            b"BI /W 1 /H 1 /BPC 8 /CS /Cs /F /AHx ID 00> EI",
            vec![b"[/Indexed 5 0 R 1 <0001>]".to_vec()],
        ),
        (
            "name_alias_cycle",
            "/ColorSpace << /Cs1 /Cs2 /Cs2 /Cs1 >>",
            b"BI /W 1 /H 1 /BPC 8 /CS /Cs1 /F /AHx ID 00> EI",
            vec![],
        ),
        (
            "nested_indexed_direct",
            "",
            b"BI /W 1 /H 1 /BPC 8 /CS [/Indexed [/Indexed /DeviceRGB 1 <000000FFFFFF>] 1 <0001>] /F /AHx ID 00> EI",
            vec![],
        ),
        (
            "nested_indexed_via_resource",
            "/ColorSpace << /CsBase [/Indexed /DeviceRGB 1 <000000FFFFFF>] >>",
            b"BI /W 1 /H 1 /BPC 8 /CS [/Indexed /CsBase 1 <0001>] /F /AHx ID 00> EI",
            vec![],
        ),
        (
            "indexed_pattern_base",
            "",
            b"BI /W 1 /H 1 /BPC 8 /CS [/Indexed /Pattern 1 <0001>] /F /AHx ID 00> EI",
            vec![],
        ),
        (
            "direct_pattern",
            "",
            b"BI /W 1 /H 1 /BPC 8 /CS /Pattern /F /AHx ID 00> EI",
            vec![],
        ),
        (
            "pattern_array",
            "",
            b"BI /W 1 /H 1 /BPC 8 /CS [/Pattern /DeviceRGB] /F /AHx ID 00> EI",
            vec![],
        ),
        (
            "indexed_unresolvable_base",
            "/ColorSpace << /CsOther /DeviceRGB >>",
            b"BI /W 1 /H 1 /BPC 8 /CS [/Indexed /MissingBase 1 <000000FFFFFF>] /F /AHx ID 00> EI",
            vec![],
        ),
        (
            "indexed_hival_negative",
            "",
            b"BI /W 1 /H 1 /BPC 8 /CS [/Indexed /DeviceRGB -1 <000000>] /F /AHx ID 00> EI",
            vec![],
        ),
        (
            "indexed_hival_over_255",
            "",
            b"BI /W 1 /H 1 /BPC 8 /CS [/Indexed /DeviceRGB 256 <000000>] /F /AHx ID 00> EI",
            vec![],
        ),
        (
            "indexed_missing_lookup",
            "",
            b"BI /W 1 /H 1 /BPC 8 /CS [/Indexed /DeviceRGB 1] /F /AHx ID 00> EI",
            vec![],
        ),
        (
            "indexed_int_lookup",
            "",
            b"BI /W 1 /H 1 /BPC 8 /CS [/Indexed /DeviceRGB 1 123] /F /AHx ID 00> EI",
            vec![],
        ),
        (
            "indexed_indirect_ref_in_inline_dict",
            "",
            b"BI /W 1 /H 1 /BPC 8 /CS [/Indexed /DeviceRGB 1 5 0 R] /F /AHx ID 00> EI",
            vec![b"<000000FFFFFF>".to_vec()],
        ),
        (
            "cs_integer",
            "",
            b"BI /W 1 /H 1 /BPC 8 /CS 123 /F /AHx ID 00> EI",
            vec![],
        ),
        (
            "cs_string",
            "",
            b"BI /W 1 /H 1 /BPC 8 /CS (DeviceRGB) /F /AHx ID 00> EI",
            vec![],
        ),
        (
            "cs_dict",
            "",
            b"BI /W 1 /H 1 /BPC 8 /CS << /A 1 >> /F /AHx ID 00> EI",
            vec![],
        ),
        (
            "cs_empty_array",
            "",
            b"BI /W 1 /H 1 /BPC 8 /CS [] /F /AHx ID 00> EI",
            vec![],
        ),
        (
            "cs_indirect_ref",
            "",
            b"BI /W 1 /H 1 /BPC 8 /CS 5 0 R /F /AHx ID 00> EI",
            vec![b"/DeviceRGB".to_vec()],
        ),
        (
            "indexed_inline_I_csbase",
            "/ColorSpace << /CsBase /DeviceRGB >>",
            b"BI /W 2 /H 2 /BPC 8 /CS [/I /CsBase 1 <FF0000 00FF00>] ID \x00\x01\x01\x00 EI",
            vec![],
        ),
        (
            "indexed_inline_I_csbase_abbr",
            "/ColorSpace << /CsBase /RGB >>",
            b"BI /W 2 /H 2 /BPC 8 /CS [/I /CsBase 1 <FF0000 00FF00>] ID \x00\x01\x01\x00 EI",
            vec![],
        ),
        (
            "indexed_inline_full_csbase",
            "/ColorSpace << /CsBase /DeviceRGB >>",
            b"BI /W 2 /H 2 /BPC 8 /CS [/Indexed /CsBase 1 <FF0000 00FF00>] ID \x00\x01\x01\x00 EI",
            vec![],
        ),
        (
            "indexed_res_csidx_csbase",
            "/ColorSpace << /CsBase /DeviceRGB /CsIdx [/Indexed /CsBase 1 5 0 R] >>",
            b"BI /W 2 /H 2 /BPC 8 /CS /CsIdx ID \x00\x01\x01\x00 EI",
            vec![stream_object("", b"\xff\x00\x00\x00\xff\x00")],
        ),
        (
            "indexed_indirect_resname_base",
            "/ColorSpace << /CsBase /DeviceRGB /CsIdx [/Indexed 5 0 R 1 6 0 R] >>",
            b"BI /W 2 /H 2 /BPC 8 /CS /CsIdx ID \x00\x01\x01\x00 EI",
            vec![b"/CsBase".to_vec(), stream_object("", b"\xff\x00\x00\x00\xff\x00")],
        ),
        (
            "indexed_inline_nested_calrgb",
            "",
            b"BI /W 1 /H 1 /BPC 8 /CS [/Indexed [/CalRGB << /WhitePoint [1 1 1] >>] 0 <FF0000>] ID \x00 EI",
            vec![],
        ),
        (
            "alias_two_hop_device_gray",
            "/ColorSpace << /Cs1 /Cs2 /Cs2 /DeviceGray >>",
            b"BI /W 1 /H 1 /BPC 8 /CS /Cs1 ID \x00 EI",
            vec![],
        ),
        (
            "alias_indirect_to_resname",
            "/ColorSpace << /Cs1 5 0 R /Cs2 /DeviceRGB >>",
            b"BI /W 1 /H 1 /BPC 8 /CS /Cs1 ID \xff\x00\x00 EI",
            vec![b"/Cs2".to_vec()],
        ),
        (
            "separation_res_alternate",
            "/ColorSpace << /CsAlt /DeviceRGB /CsSep [/Separation /Spot /CsAlt 5 0 R] >>",
            b"BI /W 1 /H 1 /BPC 8 /CS /CsSep ID \xff EI",
            vec![b"<< /FunctionType 2 /Domain [0 1] /C0 [1 1 1] /C1 [1 0 0] /N 1 >>".to_vec()],
        ),
    ];

    for (label, res, content, extra) in cases {
        let input = pdf_with_page_resources(res, content, extra);
        let ws = TestWorkspace::from_bytes("bad.pdf", &input);
        let mut sink = Vec::new();
        let mut ctx = ToolContext::new(label, "pdf.render", ws.paths, Limits::local(), &mut sink);
        let err = ctx
            .load_validated_pdf_bytes()
            .expect_err(&format!("case `{label}` must fail closed"));
        assert_eq!(
            err.code,
            ErrorCode::LimitExceeded,
            "case `{label}`: {err:?}"
        );
    }
}

#[test]
fn rejects_resolved_inline_colorspace_over_limits() {
    // Part A: max_asset_bytes cap via 16-bit 80x80 (6_400 px <= max_pixels 10_000):
    // 1-component DeviceGray = 12_800 bytes <= 40_000; 4-component DeviceCMYK = 51_200 bytes > 40_000.
    let payload_16 = zlib_compress(&vec![0u8; 80 * 80 * 2]);
    let content_16 = [
        b"BI /W 80 /H 80 /BPC 16 /CS /Cs /F /Fl ID ".as_slice(),
        payload_16.as_slice(),
        b" EI".as_slice(),
    ]
    .concat();
    let gray_pdf =
        pdf_with_page_resources("/ColorSpace << /Cs /DeviceGray >>", &content_16, vec![]);
    let cmyk_pdf =
        pdf_with_page_resources("/ColorSpace << /Cs /DeviceCMYK >>", &content_16, vec![]);
    let mut limits = Limits::local();
    limits.max_asset_bytes = Some(40_000);

    let ws_gray = TestWorkspace::from_bytes("gray.pdf", &gray_pdf);
    let mut sink_gray = Vec::new();
    let mut ctx_gray = ToolContext::new(
        "gray",
        "pdf.render",
        ws_gray.paths,
        limits.clone(),
        &mut sink_gray,
    );
    ctx_gray
        .load_validated_pdf_bytes()
        .expect("1-component 16-bit DeviceGray fits 40_000 byte cap");

    let ws_cmyk = TestWorkspace::from_bytes("cmyk.pdf", &cmyk_pdf);
    let mut sink_cmyk = Vec::new();
    let mut ctx_cmyk =
        ToolContext::new("cmyk", "pdf.render", ws_cmyk.paths, limits, &mut sink_cmyk);
    let err = ctx_cmyk
        .load_validated_pdf_bytes()
        .expect_err("4-component 16-bit DeviceCMYK (51_200 bytes) must exceed cap");
    assert_eq!(err.code, ErrorCode::LimitExceeded);

    // Part B: page_work 100_000 ceiling via 8-bit 120x250 (30_000 px):
    // DeviceGray = 30_000 <= 100_000; DeviceCMYK = 120_000 > 100_000.
    let payload_8 = zlib_compress(&vec![0u8; 120 * 250]);
    let content_8 = [
        b"BI /W 120 /H 250 /BPC 8 /CS /Cs /F /Fl ID ".as_slice(),
        payload_8.as_slice(),
        b" EI".as_slice(),
    ]
    .concat();
    let work_gray =
        pdf_with_page_resources("/ColorSpace << /Cs /DeviceGray >>", &content_8, vec![]);
    let work_cmyk =
        pdf_with_page_resources("/ColorSpace << /Cs /DeviceCMYK >>", &content_8, vec![]);

    let ws_wgray = TestWorkspace::from_bytes("wgray.pdf", &work_gray);
    let mut sink_wgray = Vec::new();
    let mut ctx_wgray = ToolContext::new(
        "wgray",
        "pdf.render",
        ws_wgray.paths,
        Limits::local(),
        &mut sink_wgray,
    );
    ctx_wgray
        .load_validated_pdf_bytes()
        .expect("DeviceGray 30_000 page work fits 100_000 limit");

    let ws_wcmyk = TestWorkspace::from_bytes("wcmyk.pdf", &work_cmyk);
    let mut sink_wcmyk = Vec::new();
    let mut ctx_wcmyk = ToolContext::new(
        "wcmyk",
        "pdf.render",
        ws_wcmyk.paths,
        Limits::local(),
        &mut sink_wcmyk,
    );
    let err = ctx_wcmyk
        .load_validated_pdf_bytes()
        .expect_err("DeviceCMYK 120_000 page work must exceed 100_000 limit");
    assert_eq!(err.code, ErrorCode::LimitExceeded);
}

#[test]
fn direct_type3_fonts_in_same_scope_keep_distinct_budgets() {
    let build_pdf = |f1_w: usize, f1_h: usize, f2: Option<(usize, usize)>, page_stream: &[u8]| {
        let gray_payload = zlib_compress(&vec![0u8; f1_w * f1_h]);
        let glyph_gray = [
            format!("0 0 d0 BI /W {f1_w} /H {f1_h} /BPC 8 /CS /DeviceGray /F /Fl ID ").into_bytes(),
            gray_payload,
            b" EI".to_vec(),
        ]
        .concat();
        let (fonts, extras) = if let Some((f2_w, f2_h)) = f2 {
            let cmyk_payload = zlib_compress(&vec![0u8; f2_w * f2_h * 4]);
            let glyph_cmyk = [
                format!("0 0 d0 BI /W {f2_w} /H {f2_h} /BPC 8 /CS /Cs8 /F /Fl ID ").into_bytes(),
                cmyk_payload,
                b" EI".to_vec(),
            ]
            .concat();
            (
                "/Font << /F1 << /Type /Font /Subtype /Type3 /FontBBox [0 0 1 1] /FontMatrix [1 0 0 1 0 0] /CharProcs << /A 5 0 R >> /Encoding << /Type /Encoding /Differences [65 /A] >> /FirstChar 65 /LastChar 65 /Widths [1] >> /F2 << /Type /Font /Subtype /Type3 /FontBBox [0 0 1 1] /FontMatrix [1 0 0 1 0 0] /CharProcs << /B 6 0 R >> /Encoding << /Type /Encoding /Differences [66 /B] >> /FirstChar 66 /LastChar 66 /Widths [1] >> >>".to_string(),
                vec![
                    stream_object("/Resources << >>", &glyph_gray),
                    stream_object("/Resources << >>", &glyph_cmyk),
                ],
            )
        } else {
            (
                "/Font << /F1 << /Type /Font /Subtype /Type3 /FontBBox [0 0 1 1] /FontMatrix [1 0 0 1 0 0] /CharProcs << /A 5 0 R >> /Encoding << /Type /Encoding /Differences [65 /A] >> /FirstChar 65 /LastChar 65 /Widths [1] >> >>".to_string(),
                vec![stream_object("/Resources << >>", &glyph_gray)],
            )
        };
        let mut objects = vec![
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] /Resources << /ColorSpace << /Cs8 /DeviceCMYK >> {fonts} >> /Contents 4 0 R >>"
            )
            .into_bytes(),
            stream_object("", page_stream),
        ];
        objects.extend(extras);
        assemble_pdf(&objects)
    };

    // Within-budget variants:
    // (a) only /F1 (100x200 Gray, 2 glyphs)
    // (b) both direct /F1 (50x100 Gray = 5_000) and /F2 (50x100 CMYK = 20_000), 2 glyphs
    for (label, pdf) in [
        (
            "only_f1_fits",
            build_pdf(100, 200, None, b"BT /F1 1 Tf () Tj ET"),
        ),
        (
            "both_smaller_f1_f2_fit",
            build_pdf(50, 100, Some((50, 100)), b"BT /F1 1 Tf () Tj ET"),
        ),
    ] {
        let ws = TestWorkspace::from_bytes("t3_ok.pdf", &pdf);
        let mut sink = Vec::new();
        let mut ctx = ToolContext::new(label, "pdf.render", ws.paths, Limits::local(), &mut sink);
        ctx.load_validated_pdf_bytes()
            .unwrap_or_else(|e| panic!("{label} should fit per-page budget: {e:?}"));
    }

    // Over-budget variants:
    // (a) Exact 100x200 Gray + 100x200 CMYK with 2 glyphs
    let lead_over = build_pdf(
        100,
        200,
        Some((100, 200)),
        b"BT /F1 1 Tf (A) Tj /F2 1 Tf (B) Tj ET",
    );
    // (b) Single-draw budget check on /F1 + /F2 (50x100 Gray + 100x200 CMYK, 2 glyphs = 160_000 > 100_000)
    let budget_over = build_pdf(50, 100, Some((100, 200)), b"BT /F1 1 Tf () Tj ET");

    for (label, pdf) in [
        ("lead_both_100x200", lead_over),
        ("two_glyphs_160k_over_budget", budget_over),
    ] {
        let ws = TestWorkspace::from_bytes("t3_over.pdf", &pdf);
        let mut sink = Vec::new();
        let mut ctx = ToolContext::new(label, "pdf.render", ws.paths, Limits::local(), &mut sink);
        let err = ctx
            .load_validated_pdf_bytes()
            .expect_err(&format!("{label} must fail closed when exceeding 100_000"));
        assert_eq!(err.code, ErrorCode::LimitExceeded, "{label}: {err:?}");
    }
}

#[test]
fn multi_page_shared_form_extgstate_memoizes_and_distinct_colorspace_enforces_document_work() {
    let build_multi_page = |pages: usize, repeats: usize, distinct_cs: bool| {
        let form_ops = b"q Q ".repeat(repeats);
        let form_id = 3usize;
        let first_page_id = 4usize;
        let mut objects: Vec<Vec<u8>> = Vec::with_capacity(3 + pages * 2);
        objects.push(b"<< /Type /Catalog /Pages 2 0 R >>".to_vec());
        let kids: String = (0..pages)
            .map(|i| format!("{} 0 R", first_page_id + i * 2))
            .collect::<Vec<_>>()
            .join(" ");
        objects.push(format!("<< /Type /Pages /Kids [{kids}] /Count {pages} >>").into_bytes());
        objects.push(stream_object(
            "/Type /XObject /Subtype /Form /BBox [0 0 10 10]",
            &form_ops,
        ));
        for i in 0..pages {
            let page_id = first_page_id + i * 2;
            let content_id = page_id + 1;
            let extra_res = if distinct_cs {
                format!("/ColorSpace << /Cs{i} /DeviceRGB >>")
            } else {
                format!("/ExtGState << /GS{i} << /CA 1 >> >>")
            };
            objects.push(
                format!(
                    "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] /Resources << /XObject << /F {form_id} 0 R >> {extra_res} >> /Contents {content_id} 0 R >>"
                )
                .into_bytes(),
            );
            objects.push(stream_object("", b"/F Do"));
        }
        assemble_pdf(&objects)
    };

    // 210 pages sharing a 4_000-op Form under distinct ExtGState scopes memoize canonical rewrites.
    let memo_pdf = build_multi_page(210, 2_000, false);
    {
        let ws = TestWorkspace::from_bytes("memo210.pdf", &memo_pdf);
        let mut sink = Vec::new();
        let mut ctx = ToolContext::new(
            "memo-210",
            "pdf.render",
            ws.paths,
            Limits::local(),
            &mut sink,
        );
        ctx.load_validated_pdf_bytes()
            .expect("210 pages with distinct ExtGState scopes should succeed");
    }

    // 250 pages sharing a 4_500-op Form across distinct ColorSpace scopes exceed 1_000_000 document rewrite work.
    let cs_over_pdf = build_multi_page(250, 2_250, true);
    {
        let ws = TestWorkspace::from_bytes("docwork250.pdf", &cs_over_pdf);
        let mut sink = Vec::new();
        let mut ctx = ToolContext::new(
            "docwork-250",
            "pdf.render",
            ws.paths,
            Limits::local(),
            &mut sink,
        );
        let err = ctx
            .load_validated_pdf_bytes()
            .expect_err("250 distinct ColorSpace scopes must fail closed at document work cap");
        assert_eq!(err.code, ErrorCode::LimitExceeded);

        let page1 = PageRange::parse("1").unwrap();
        let mut sink_p1 = Vec::new();
        let ws_p1 = TestWorkspace::from_bytes("docwork250_p1.pdf", &cs_over_pdf);
        let mut ctx_p1 = ToolContext::new(
            "docwork-250-p1",
            "pdf.render",
            ws_p1.paths,
            Limits::local(),
            &mut sink_p1,
        );
        let err_p1 = ctx_p1
            .load_validated_pdf_bytes_for_pages(Some(&page1))
            .expect_err(
                "document-wide canonical rewrite work must still fail when rendering page 1",
            );
        assert_eq!(err_p1.code, ErrorCode::LimitExceeded);
    }
}

#[test]
fn rejects_transformed_huge_inline_image_with_unresolvable_or_cal_colorspace() {
    let lead_repro = pdf_with_page_resources(
        "/ColorSpace << /CsBase /DeviceRGB >>",
        b"q 20000 0 0 20000 0 0 cm BI /W 1 /H 1 /BPC 8 /CS [/I /CsBase 0 <FF0000>] ID \x00 EI Q",
        vec![],
    );
    {
        let ws = TestWorkspace::from_bytes("lead_repro_render.pdf", &lead_repro);
        let mut sink = Vec::new();
        let mut ctx = ToolContext::new(
            "lead-repro-render",
            "pdf.render",
            ws.paths,
            Limits::local(),
            &mut sink,
        );
        let err = ctx
            .load_validated_pdf_bytes()
            .expect_err("Lead repro must fail closed in pdf.render preflight");
        assert_eq!(err.code, ErrorCode::LimitExceeded);
    }

    let cal_indirect = vec![b"[/CalRGB << /WhitePoint [0.9505 1 1.089] >>]".to_vec()];
    let huge_cal = pdf_with_page_resources(
        "/ColorSpace << /CsCal 5 0 R >>",
        b"q 20000 0 0 20000 0 0 cm BI /W 1 /H 1 /BPC 8 /CS /CsCal ID abc EI Q",
        cal_indirect.clone(),
    );
    {
        let ws = TestWorkspace::from_bytes("huge_cal_preflight.pdf", &huge_cal);
        let mut sink = Vec::new();
        let mut ctx = ToolContext::new(
            "huge-cal-preflight",
            "pdf.render",
            ws.paths,
            Limits::local(),
            &mut sink,
        );
        ctx.load_validated_pdf_bytes()
            .expect("1x1 /CsCal image should be preflight-valid for canonicalization");
    }

    let Ok(lib_dir) = std::env::var("ARIAD_TEST_PDFIUM_DIR") else {
        return;
    };
    if lib_dir.is_empty() {
        return;
    }

    let params = ExtractImagesParams {
        min_px: 1,
        format: ExtractFormat::Png,
        ..ExtractImagesParams::default()
    };

    // 1. Lead repro under extract_images
    {
        let ws = TestWorkspace::from_bytes("lead_repro_extract.pdf", &lead_repro);
        let mut sink = Vec::new();
        let mut ctx = ToolContext::new(
            "lead-repro-extract",
            "pdf.extract_images",
            ws.paths,
            Limits::local(),
            &mut sink,
        )
        .with_library_dir(&lib_dir);
        let err = extract_images::extract_document_images(&mut ctx, &params)
            .expect_err("Lead repro must fail closed in pdf.extract_images");
        assert_eq!(err.code, ErrorCode::LimitExceeded);
    }

    // 2. Preflight-valid 1x1 /CsCal scaled 20000x20000 -> exceeds pixel cap before get_processed_bitmap
    {
        let ws = TestWorkspace::from_bytes("huge_cal_extract.pdf", &huge_cal);
        let mut sink = Vec::new();
        let mut ctx = ToolContext::new(
            "huge-cal-extract",
            "pdf.extract_images",
            ws.paths,
            Limits::local(),
            &mut sink,
        )
        .with_library_dir(&lib_dir);
        let err = extract_images::extract_document_images(&mut ctx, &params)
            .expect_err("20000x20000 transformed /CsCal must fail before get_processed_bitmap");
        assert_eq!(err.code, ErrorCode::LimitExceeded);
    }

    // 3. Transformed size within MAX_IMAGE_PIXELS (2000x2000 = 4M px) but 16 MB RGBA > max_asset_bytes (4 MB)
    let med_cal = pdf_with_page_resources(
        "/ColorSpace << /CsCal 5 0 R >>",
        b"q 2000 0 0 2000 0 0 cm BI /W 1 /H 1 /BPC 8 /CS /CsCal ID abc EI Q",
        cal_indirect.clone(),
    );
    {
        let mut limits = Limits::local();
        limits.max_asset_bytes = Some(4_000_000);
        let ws = TestWorkspace::from_bytes("med_cal_asset_cap.pdf", &med_cal);
        let mut sink = Vec::new();
        let mut ctx = ToolContext::new(
            "med-cal-asset-cap",
            "pdf.extract_images",
            ws.paths,
            limits,
            &mut sink,
        )
        .with_library_dir(&lib_dir);
        let err = extract_images::extract_document_images(&mut ctx, &params)
            .expect_err("2000x2000 RGBA (16 MB) must exceed 4 MB max_asset_bytes");
        assert_eq!(err.code, ErrorCode::LimitExceeded);
    }

    // 4. Normal 10x10 /CsCal extracts cleanly
    let small_cal = pdf_with_page_resources(
        "/ColorSpace << /CsCal 5 0 R >>",
        b"q 10 0 0 10 2 2 cm BI /W 1 /H 1 /BPC 8 /CS /CsCal ID \xff\x00\x00 EI Q",
        cal_indirect,
    );
    {
        let ws = TestWorkspace::from_bytes("small_cal_extract.pdf", &small_cal);
        let mut sink = Vec::new();
        let mut ctx = ToolContext::new(
            "small-cal-extract",
            "pdf.extract_images",
            ws.paths,
            Limits::local(),
            &mut sink,
        )
        .with_library_dir(&lib_dir);
        let outcome = extract_images::extract_document_images(&mut ctx, &params)
            .expect("10x10 /CsCal image within limits should extract");
        assert_eq!(outcome.images.len(), 1);
    }
}

#[test]
fn extract_images_does_not_double_charge_document_work_across_pages() {
    let lib_dir = require_pdfium!();
    let jpeg_bytes = make_test_jpeg(24, 24);
    let pages = 80usize;
    let first_page_id = 4usize;

    let mut objects: Vec<Vec<u8>> = Vec::with_capacity(3 + pages * 2);
    objects.push(b"<< /Type /Catalog /Pages 2 0 R >>".to_vec());
    let kids: String = (0..pages)
        .map(|i| format!("{} 0 R", first_page_id + i * 2))
        .collect::<Vec<_>>()
        .join(" ");
    objects.push(format!("<< /Type /Pages /Kids [{kids}] /Count {pages} >>").into_bytes());
    objects.push(stream_object(
        "/Type /XObject /Subtype /Image /Width 24 /Height 24 /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode",
        &jpeg_bytes,
    ));

    // 80 pages * 7,500 PDF operators = 600,000 ops total (60% of MAX_DOCUMENT_WORK = 1_000_000,
    // with < MAX_PAGE_WORK = 100_000 per page). Double-charging in extract_document_images_with
    // would reach 1,200,000 > 1,000_000 and fail with LimitExceeded.
    let filler_7500 = b"q Q ".repeat(3_750);
    let mut last_page_content = b"q Q ".repeat(3_748);
    last_page_content.extend_from_slice(b"q 24 0 0 24 50 500 cm /Im1 Do Q");

    for i in 0..pages {
        let page_id = first_page_id + i * 2;
        let content_id = page_id + 1;
        let (resources, payload): (&str, &[u8]) = if i + 1 == pages {
            (
                "/Resources << /XObject << /Im1 3 0 R >> >> ",
                &last_page_content,
            )
        } else {
            ("", &filler_7500)
        };
        objects.push(
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] {resources}/Contents {content_id} 0 R >>"
            )
            .into_bytes(),
        );
        objects.push(stream_object("", payload));
    }

    let pdf_bytes = assemble_pdf(&objects);
    let ws = TestWorkspace::from_bytes("doc_work_60pct.pdf", &pdf_bytes);
    let mut sink = Vec::new();
    let mut ctx = ToolContext::new(
        "doc-work-60pct",
        "pdf.extract_images",
        ws.paths,
        Limits::local(),
        &mut sink,
    )
    .with_library_dir(lib_dir);

    let params = ExtractImagesParams {
        pages: PageRange::all(),
        format: ExtractFormat::Original,
        min_px: 16,
    };
    let outcome = extract_images::extract_document_images(&mut ctx, &params)
        .expect("80 pages * 7,500 ops (600,000 total) must not exceed MAX_DOCUMENT_WORK");
    assert_eq!(outcome.pages_inspected, pages);
    assert_eq!(outcome.images.len(), 1);
    assert_eq!(outcome.images[0].page, u32::try_from(pages).unwrap());
    assert_eq!(outcome.images[0].bytes, jpeg_bytes);
}

#[test]
fn extract_images_resolves_only_nearest_inherited_resources_dictionary() {
    let lib_dir = require_pdfium!();
    let jpeg_bytes = make_test_jpeg(24, 24);
    let objects = vec![
        // 1: Catalog
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        // 2: Pages node declaring /ImAncestor in /Resources /XObject
        b"<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 /Resources << /XObject << /ImAncestor 5 0 R >> >> >>".to_vec(),
        // 3: Page 1 defines its own /Resources with /Font and no /XObject -> shadows /Pages /Resources
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> >> >> /Contents 6 0 R >>".to_vec(),
        // 4: Page 2 omits /Resources -> inherits /Pages /Resources as a whole
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 7 0 R >>".to_vec(),
        // 5: 24x24 JPEG image XObject
        stream_object(
            "/Type /XObject /Subtype /Image /Width 24 /Height 24 /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode",
            &jpeg_bytes,
        ),
        // 6: Page 1 invokes /ImAncestor Do
        stream_object("", b"q 24 0 0 24 50 500 cm /ImAncestor Do Q"),
        // 7: Page 2 invokes /ImAncestor Do
        stream_object("", b"q 24 0 0 24 50 500 cm /ImAncestor Do Q"),
    ];
    let pdf_bytes = assemble_pdf(&objects);

    // Page 1 defines its own /Resources without /XObject -> /ImAncestor must not be extracted.
    {
        let ws = TestWorkspace::from_bytes("nearest_resources_p1.pdf", &pdf_bytes);
        let mut sink = Vec::new();
        let mut ctx = ToolContext::new(
            "nearest-resources-p1",
            "pdf.extract_images",
            ws.paths,
            Limits::local(),
            &mut sink,
        )
        .with_library_dir(&lib_dir);
        let params = ExtractImagesParams {
            pages: PageRange::parse("1").expect("valid page 1"),
            format: ExtractFormat::Original,
            min_px: 16,
        };
        let outcome = extract_images::extract_document_images(&mut ctx, &params)
            .expect("page 1 extraction succeeds with 0 images");
        assert_eq!(outcome.pages_inspected, 1);
        assert!(
            outcome.images.is_empty(),
            "page 1 /Resources without /XObject must shadow /Pages /Resources, got {} images",
            outcome.images.len()
        );
    }

    // Page 2 omits /Resources -> inherits /Pages /Resources as a whole and extracts /ImAncestor.
    {
        let ws = TestWorkspace::from_bytes("nearest_resources_p2.pdf", &pdf_bytes);
        let mut sink = Vec::new();
        let mut ctx = ToolContext::new(
            "nearest-resources-p2",
            "pdf.extract_images",
            ws.paths,
            Limits::local(),
            &mut sink,
        )
        .with_library_dir(&lib_dir);
        let params = ExtractImagesParams {
            pages: PageRange::parse("2").expect("valid page 2"),
            format: ExtractFormat::Original,
            min_px: 16,
        };
        let outcome = extract_images::extract_document_images(&mut ctx, &params)
            .expect("page 2 extraction succeeds");
        assert_eq!(outcome.pages_inspected, 1);
        assert_eq!(outcome.images.len(), 1);
        assert_eq!(outcome.images[0].page, 2);
        assert_eq!(outcome.images[0].bytes, jpeg_bytes);
    }

    // Inspecting all pages extracts only Page 2's image.
    {
        let ws = TestWorkspace::from_bytes("nearest_resources_all.pdf", &pdf_bytes);
        let mut sink = Vec::new();
        let mut ctx = ToolContext::new(
            "nearest-resources-all",
            "pdf.extract_images",
            ws.paths,
            Limits::local(),
            &mut sink,
        )
        .with_library_dir(&lib_dir);
        let params = ExtractImagesParams {
            pages: PageRange::all(),
            format: ExtractFormat::Original,
            min_px: 16,
        };
        let outcome = extract_images::extract_document_images(&mut ctx, &params)
            .expect("full document extraction succeeds");
        assert_eq!(outcome.pages_inspected, 2);
        assert_eq!(outcome.images.len(), 1);
        assert_eq!(outcome.images[0].page, 2);
        assert_eq!(outcome.images[0].bytes, jpeg_bytes);
    }
}
