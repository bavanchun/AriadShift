//! Integration tests for `ariad_pdf::image_to_pdf`.

use std::{
    sync::{Arc, atomic::AtomicBool},
    time::{Duration, Instant},
};

use ariad_core::limits::Limits;
use ariad_pdf::{
    PdfError, ToolCtx,
    image_to_pdf::{self, ImageToPdfParams, Orientation, PageSize},
    info,
};
use lopdf::{Document, Object, Stream, dictionary};

fn make_minimal_jpeg(width: u16, height: u16, components: u8) -> Vec<u8> {
    let mut out = Vec::new();
    // SOI
    out.extend_from_slice(b"\xFF\xD8");
    // APP0 (JFIF)
    out.extend_from_slice(b"\xFF\xE0\x00\x10JFIF\x00\x01\x01\x01\x00H\x00H\x00\x00");
    // SOF0 (Baseline DCT)
    let sof_len = 8 + 3 * u16::from(components);
    out.extend_from_slice(&[0xFF, 0xC0]);
    out.extend_from_slice(&sof_len.to_be_bytes());
    out.push(8); // 8-bit precision
    out.extend_from_slice(&height.to_be_bytes());
    out.extend_from_slice(&width.to_be_bytes());
    out.push(components);
    for id in 1..=components {
        out.extend_from_slice(&[id, 0x11, 0x00]);
    }
    // SOS (Start of Scan)
    let sos_len = 6 + 2 * u16::from(components);
    out.extend_from_slice(&[0xFF, 0xDA]);
    out.extend_from_slice(&sos_len.to_be_bytes());
    out.push(components);
    for id in 1..=components {
        out.extend_from_slice(&[id, 0x00]);
    }
    out.extend_from_slice(&[0x00, 0x3F, 0x00]);
    // Minimal entropy-coded payload + EOI
    out.extend_from_slice(&[0x7F, 0x00, 0xFF, 0xD9]);
    out
}

fn png_chunk(chunk_type: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(12 + data.len());
    let len = u32::try_from(data.len()).expect("chunk length fits u32");
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(chunk_type);
    out.extend_from_slice(data);
    // CRC placeholder (4 bytes)
    out.extend_from_slice(&0_u32.to_be_bytes());
    out
}

fn make_png(
    width: u32,
    height: u32,
    bit_depth: u8,
    color_type: u8,
    raw_scanlines: &[u8],
    plte: Option<&[u8]>,
    trns: Option<&[u8]>,
) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"\x89PNG\r\n\x1a\n");

    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.push(bit_depth);
    ihdr.push(color_type);
    ihdr.push(0); // compression
    ihdr.push(0); // filter
    ihdr.push(0); // interlace = none
    out.extend_from_slice(&png_chunk(b"IHDR", &ihdr));

    if let Some(p) = plte {
        out.extend_from_slice(&png_chunk(b"PLTE", p));
    }
    if let Some(t) = trns {
        out.extend_from_slice(&png_chunk(b"tRNS", t));
    }

    let mut stream = Stream::new(dictionary! {}, raw_scanlines.to_vec());
    stream.compress().expect("compress IDAT");
    out.extend_from_slice(&png_chunk(b"IDAT", &stream.content));
    out.extend_from_slice(&png_chunk(b"IEND", &[]));
    out
}

fn make_rgba_png(width: u32, height: u32) -> Vec<u8> {
    let row_bytes = (width as usize) * 4;
    let mut raw = Vec::with_capacity((height as usize) * (row_bytes + 1));
    for y in 0..height {
        // Cycle through PNG filter types 0..=4 across rows to exercise all unfilter paths
        let filter = (y % 5) as u8;
        raw.push(filter);
        for x in 0..width {
            raw.extend_from_slice(&[
                ((x * 17) & 0xFF) as u8,
                ((y * 31) & 0xFF) as u8,
                (((x + y) * 13) & 0xFF) as u8,
                if (x + y) % 2 == 0 { 128 } else { 255 },
            ]);
        }
    }
    make_png(width, height, 8, 6, &raw, None, None)
}

fn make_rgb_png(width: u32, height: u32) -> Vec<u8> {
    let row_bytes = (width as usize) * 3;
    let mut raw = Vec::with_capacity((height as usize) * (row_bytes + 1));
    for y in 0..height {
        raw.push(0); // filter None
        for x in 0..width {
            raw.extend_from_slice(&[(x & 0xFF) as u8, (y & 0xFF) as u8, 200]);
        }
    }
    make_png(width, height, 8, 2, &raw, None, None)
}

#[test]
fn jpeg_passthrough_embeds_dct_decode_without_reencoding() {
    let jpeg = make_minimal_jpeg(320, 200, 3);
    let pdf_bytes = image_to_pdf::run(
        [jpeg.as_slice()],
        &ImageToPdfParams::default(),
        &Limits::local(),
    )
    .expect("JPEG to PDF succeeds");

    let doc = Document::load_mem(&pdf_bytes).expect("valid PDF");
    let mut found_jpeg = false;
    for object in doc.objects.values() {
        let Ok(stream) = object.as_stream() else {
            continue;
        };
        if stream
            .dict
            .get(b"Subtype")
            .ok()
            .and_then(|o| o.as_name().ok())
            == Some(b"Image".as_slice())
        {
            assert_eq!(
                stream
                    .dict
                    .get(b"Filter")
                    .ok()
                    .and_then(|o| o.as_name().ok()),
                Some(b"DCTDecode".as_slice())
            );
            assert_eq!(
                stream
                    .dict
                    .get(b"ColorSpace")
                    .ok()
                    .and_then(|o| o.as_name().ok()),
                Some(b"DeviceRGB".as_slice())
            );
            assert_eq!(
                stream.dict.get(b"Width").and_then(Object::as_i64).ok(),
                Some(320)
            );
            assert_eq!(
                stream.dict.get(b"Height").and_then(Object::as_i64).ok(),
                Some(200)
            );
            assert_eq!(stream.content, jpeg, "JPEG stream must not be re-encoded");
            found_jpeg = true;
        }
    }
    assert!(found_jpeg, "expected an Image XObject with DCTDecode");
}

#[test]
fn png_with_alpha_embeds_flate_image_and_smask_xobject() {
    let rgba_png = make_rgba_png(16, 12);
    let pdf_bytes = image_to_pdf::run(
        [rgba_png.as_slice()],
        &ImageToPdfParams::default(),
        &Limits::local(),
    )
    .expect("RGBA PNG to PDF succeeds");

    let doc = Document::load_mem(&pdf_bytes).expect("valid PDF");
    let mut main_img_checked = false;
    for object in doc.objects.values() {
        let Ok(stream) = object.as_stream() else {
            continue;
        };
        if stream
            .dict
            .get(b"Subtype")
            .ok()
            .and_then(|o| o.as_name().ok())
            == Some(b"Image".as_slice())
            && stream.dict.has(b"SMask")
        {
            assert_eq!(
                stream
                    .dict
                    .get(b"Filter")
                    .ok()
                    .and_then(|o| o.as_name().ok()),
                Some(b"FlateDecode".as_slice())
            );
            assert_eq!(
                stream
                    .dict
                    .get(b"ColorSpace")
                    .ok()
                    .and_then(|o| o.as_name().ok()),
                Some(b"DeviceRGB".as_slice())
            );
            let smask_id = stream
                .dict
                .get(b"SMask")
                .expect("SMask entry")
                .as_reference()
                .expect("SMask reference");
            let smask_stream = doc
                .get_object(smask_id)
                .expect("SMask object exists")
                .as_stream()
                .expect("SMask is a stream");
            assert_eq!(
                smask_stream
                    .dict
                    .get(b"ColorSpace")
                    .ok()
                    .and_then(|o| o.as_name().ok()),
                Some(b"DeviceGray".as_slice())
            );
            assert_eq!(
                smask_stream
                    .dict
                    .get(b"Filter")
                    .ok()
                    .and_then(|o| o.as_name().ok()),
                Some(b"FlateDecode".as_slice())
            );
            let decoded_alpha = smask_stream
                .decompressed_content()
                .expect("decompress SMask");
            assert_eq!(decoded_alpha.len(), 16 * 12);
            main_img_checked = true;
        }
    }
    assert!(main_img_checked, "expected main image XObject with /SMask");
}

#[test]
fn combines_multiple_images_into_multipage_pdf_and_is_deterministic() {
    let img1 = make_minimal_jpeg(400, 300, 3);
    let img2 = make_rgb_png(200, 500);
    let img3 = make_rgba_png(120, 120);
    let inputs = [img1.as_slice(), img2.as_slice(), img3.as_slice()];

    let params = ImageToPdfParams {
        page: PageSize::FitImage,
        margin_pt: 10.0,
        orientation: Orientation::Auto,
    };
    let first = image_to_pdf::run(inputs, &params, &Limits::local()).expect("first run");
    let second = image_to_pdf::run(inputs, &params, &Limits::local()).expect("second run");
    assert_eq!(first, second, "output PDF bytes must be deterministic");

    let report = info::run(&first, &Limits::local()).expect("inspect generated PDF");
    assert_eq!(report.page_count, 3);
    assert_eq!(report.pages[0].width_pt, 420.0);
    assert_eq!(report.pages[0].height_pt, 320.0);
    assert_eq!(report.pages[0].orientation, "landscape");
    assert!(!report.pages[0].blank);
    assert_eq!(report.pages[0].image_only, Some(true));

    assert_eq!(report.pages[1].width_pt, 220.0);
    assert_eq!(report.pages[1].height_pt, 520.0);
    assert_eq!(report.pages[1].orientation, "portrait");

    assert_eq!(report.pages[2].width_pt, 140.0);
    assert_eq!(report.pages[2].height_pt, 140.0);
    assert_eq!(report.pages[2].orientation, "square");
}

#[test]
fn supports_standard_paper_sizes_margin_and_auto_orientation() {
    let wide_img = make_minimal_jpeg(800, 400, 3);
    let tall_img = make_rgb_png(300, 600);

    let params = ImageToPdfParams {
        page: PageSize::A4,
        margin_pt: 36.0,
        orientation: Orientation::Auto,
    };
    let pdf_bytes = image_to_pdf::run(
        [wide_img.as_slice(), tall_img.as_slice()],
        &params,
        &Limits::local(),
    )
    .expect("A4 auto orientation succeeds");

    let report = info::run(&pdf_bytes, &Limits::local()).expect("inspect A4 PDF");
    assert_eq!(report.page_count, 2);
    // Page 1 is a wide image -> landscape A4 (841.89 x 595.28)
    assert_eq!(report.pages[0].paper, "A4");
    assert_eq!(report.pages[0].orientation, "landscape");
    assert_eq!(report.pages[0].width_pt, 841.89);
    assert_eq!(report.pages[0].height_pt, 595.28);

    // Page 2 is a tall image -> portrait A4 (595.28 x 841.89)
    assert_eq!(report.pages[1].paper, "A4");
    assert_eq!(report.pages[1].orientation, "portrait");
    assert_eq!(report.pages[1].width_pt, 595.28);
    assert_eq!(report.pages[1].height_pt, 841.89);
}

#[test]
fn enforces_pixel_cap_and_input_size_limits() {
    // 20_000 x 20_000 = 400_000_000 > MAX_IMAGE_PIXELS (268_435_456)
    let huge_jpeg = make_minimal_jpeg(20_000, 20_000, 3);
    let err = image_to_pdf::run(
        [huge_jpeg.as_slice()],
        &ImageToPdfParams::default(),
        &Limits::local(),
    )
    .expect_err("oversized pixel count must fail");
    assert!(matches!(err, PdfError::LimitExceeded(_)), "{err:?}");
    assert_eq!(err.cli_error_code(), "limit_exceeded");

    // Input byte limit exceeded
    let small_jpeg = make_minimal_jpeg(64, 64, 1);
    let tight_limits = Limits {
        max_input_bytes: Some(10),
        ..Limits::local()
    };
    let err = image_to_pdf::run(
        [small_jpeg.as_slice()],
        &ImageToPdfParams::default(),
        &tight_limits,
    )
    .expect_err("input byte limit must fail");
    assert!(matches!(err, PdfError::LimitExceeded(_)), "{err:?}");
}

#[test]
fn decodes_valid_webp_tiff_and_bmp_inputs_into_pdf_pages() {
    use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
    use std::io::Cursor;

    let mut rgba = RgbaImage::new(24, 16);
    for (x, y, pixel) in rgba.enumerate_pixels_mut() {
        *pixel = Rgba([
            ((x * 9) & 0xFF) as u8,
            ((y * 15) & 0xFF) as u8,
            180,
            if (x + y) % 3 == 0 { 160 } else { 255 },
        ]);
    }
    let dyn_img = DynamicImage::ImageRgba8(rgba);

    let mut webp = Vec::new();
    dyn_img
        .write_to(&mut Cursor::new(&mut webp), ImageFormat::WebP)
        .expect("encode WebP");

    let mut tiff = Vec::new();
    dyn_img
        .write_to(&mut Cursor::new(&mut tiff), ImageFormat::Tiff)
        .expect("encode TIFF");

    let mut bmp = Vec::new();
    dyn_img
        .to_rgb8()
        .write_to(&mut Cursor::new(&mut bmp), ImageFormat::Bmp)
        .expect("encode BMP");

    let pdf_bytes = image_to_pdf::run(
        [webp.as_slice(), tiff.as_slice(), bmp.as_slice()],
        &ImageToPdfParams::default(),
        &Limits::local(),
    )
    .expect("WebP, TIFF and BMP to PDF succeeds");

    let report = info::run(&pdf_bytes, &Limits::local()).expect("inspect generated PDF");
    assert_eq!(report.page_count, 3);
    for page in &report.pages {
        assert_eq!(page.width_pt, 24.0);
        assert_eq!(page.height_pt, 16.0);
        assert!(!page.blank);
        assert_eq!(page.image_only, Some(true));
    }
}

#[test]
fn rejects_unsupported_and_corrupt_formats_with_invalid_input() {
    let heic = b"\x00\x00\x00\x18ftypheic\x00\x00\x00\x00heicmif1";
    let webp = b"RIFF\x20\x00\x00\x00WEBPVP8 ";
    let tiff = b"II*\x00\x08\x00\x00\x00";
    let bmp = b"BM\x36\x00\x00\x00\x00\x00\x00\x00";
    let unknown = b"not an image at all";
    let truncated_png = b"\x89PNG\r\n\x1a\ntruncated";

    for sample in [
        heic.as_slice(),
        webp.as_slice(),
        tiff.as_slice(),
        bmp.as_slice(),
        unknown.as_slice(),
        truncated_png.as_slice(),
    ] {
        let err = image_to_pdf::run([sample], &ImageToPdfParams::default(), &Limits::local())
            .expect_err("unsupported or corrupt input must be rejected");
        assert!(matches!(err, PdfError::InvalidParams(_)), "{err:?}");
        assert_eq!(err.cli_error_code(), "invalid_input");
    }

    // Empty input slice must also fail with InvalidParams
    let empty: [&[u8]; 0] = [];
    let err = image_to_pdf::run(empty, &ImageToPdfParams::default(), &Limits::local())
        .expect_err("empty image list must be rejected");
    assert_eq!(err.cli_error_code(), "invalid_input");
}

#[test]
fn respects_cancellation_and_timeout_in_tool_ctx() {
    let jpeg = make_minimal_jpeg(32, 32, 3);
    let cancel = Arc::new(AtomicBool::new(true));
    let ctx = ToolCtx::with_cancel(cancel);
    let err = image_to_pdf::run_with_ctx(
        [jpeg.as_slice()],
        &ImageToPdfParams::default(),
        &Limits::local(),
        &ctx,
    )
    .expect_err("cancelled context must abort");
    assert!(err.is_cancelled());

    let expired_ctx = ToolCtx::with_deadline(Instant::now() - Duration::from_secs(1));
    let err = image_to_pdf::run_with_ctx(
        [jpeg.as_slice()],
        &ImageToPdfParams::default(),
        &Limits::local(),
        &expired_ctx,
    )
    .expect_err("expired deadline must abort");
    assert!(err.is_timed_out());
}
