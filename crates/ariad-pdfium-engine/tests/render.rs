//! Integration tests for `pdf.render` and `pdf.to_images`.

use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use ariad_core::{
    limits::Limits,
    protocol::{ErrorCode, v2::EventV2},
    tools::{
        page_range::PageRange,
        params::{Background, Combine, RasterFormat, RenderParams, ToImagesParams},
    },
};
use ariad_engine_io::RunToolPaths;
use ariad_pdfium_engine::{
    output::{CancelFlag, ToolContext},
    render,
};
use image::{ImageFormat, RgbaImage};
use zip::ZipArchive;

const PDFIUM_DIR_ENV: &str = "ARIAD_TEST_PDFIUM_DIR";

fn pdfium_dir() -> Option<PathBuf> {
    let raw = std::env::var_os(PDFIUM_DIR_ENV)?;
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
                eprintln!("skipping test because {PDFIUM_DIR_ENV} is not set");
                return;
            }
        }
    };
}

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/pdf-tools")
}

fn encrypted_seed_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fuzz/seeds/pdf_open_bounded/encrypted-empty-password.pdf")
}

fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

fn golden_path(page: u8, os: &str) -> PathBuf {
    let suffix = if os == "linux" {
        String::new()
    } else {
        format!(".{os}")
    };
    golden_dir().join(format!("pd-vi-qd-1922-2026.page-{page:03}{suffix}.png"))
}

fn replace_fixed_len(haystack: &[u8], needle: &[u8], replacement: &[u8]) -> Vec<u8> {
    assert_eq!(needle.len(), replacement.len());
    let mut out = haystack.to_vec();
    let mut found = false;
    let mut i = 0;
    while i + needle.len() <= out.len() {
        if &out[i..i + needle.len()] == needle {
            out[i..i + needle.len()].copy_from_slice(replacement);
            found = true;
            i += needle.len();
        } else {
            i += 1;
        }
    }
    assert!(found, "needle present in PDF bytes");
    out
}

struct TestWorkspace {
    _temp: tempfile::TempDir,
    paths: RunToolPaths,
}

fn setup_workspace(input_file_name: &str, input_bytes: &[u8]) -> TestWorkspace {
    let temp = tempfile::tempdir().expect("create tempdir");
    let root = temp.path().canonicalize().expect("canonicalize tempdir");
    let in_dir = root.join("in");
    let out_dir = root.join("out");
    let work_dir = root.join("work");
    fs::create_dir_all(&in_dir).expect("create in dir");
    fs::create_dir_all(&out_dir).expect("create out dir");
    fs::create_dir_all(&work_dir).expect("create work dir");

    let input_path = in_dir.join(input_file_name);
    fs::write(&input_path, input_bytes).expect("write input PDF");

    TestWorkspace {
        _temp: temp,
        paths: RunToolPaths {
            workspace: root,
            inputs: vec![input_path],
            output_dir: out_dir,
            work_dir,
        },
    }
}

fn setup_fixture_workspace(fixture_name: &str) -> TestWorkspace {
    let bytes = fs::read(fixtures_dir().join(fixture_name)).expect("read fixture PDF");
    setup_workspace(fixture_name, &bytes)
}

fn parse_events(sink: &[u8]) -> Vec<EventV2> {
    sink.split(|b| *b == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice::<EventV2>(line).expect("parse EventV2 JSON"))
        .collect()
}

fn artifact_events(events: &[EventV2]) -> Vec<(PathBuf, String)> {
    events
        .iter()
        .filter_map(|ev| match ev {
            EventV2::Artifact { path, format } => Some((PathBuf::from(path), format.clone())),
            _ => None,
        })
        .collect()
}

/// Compares `actual` against `golden` requiring identical dimensions and >= 99.5% of pixels
/// within a per-channel delta of `<= 2`.
fn assert_matches_golden(actual: &RgbaImage, golden: &RgbaImage, label: &str) {
    assert_eq!(
        actual.dimensions(),
        golden.dimensions(),
        "{label}: dimensions mismatch (actual {:?} vs golden {:?})",
        actual.dimensions(),
        golden.dimensions()
    );
    let total_pixels = (actual.width() as usize) * (actual.height() as usize);
    assert!(total_pixels > 0, "{label}: empty image");

    let mut within_tolerance = 0usize;
    for (p1, p2) in actual.pixels().zip(golden.pixels()) {
        let dr = p1[0].abs_diff(p2[0]);
        let dg = p1[1].abs_diff(p2[1]);
        let db = p1[2].abs_diff(p2[2]);
        let da = p1[3].abs_diff(p2[3]);
        if dr <= 2 && dg <= 2 && db <= 2 && da <= 2 {
            within_tolerance += 1;
        }
    }
    let ratio = (within_tolerance as f64) / (total_pixels as f64);
    assert!(
        ratio >= 0.995,
        "{label}: only {:.4}% of pixels matched golden within <=2 channel tolerance (expected >= 99.5%)",
        ratio * 100.0
    );
}

#[test]
fn golden_renders_match_embedded_font_document_pages_1_and_2() {
    let lib_dir = require_pdfium!();
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/pdf/pd-vi-qd-1922-2026.pdf");
    let ws = setup_workspace(
        "pd-vi-qd-1922-2026.pdf",
        &fs::read(fixture).expect("read embedded-font fixture"),
    );
    let mut sink = Vec::new();
    let mut ctx = ToolContext::new(
        "job-golden",
        "pdf.render",
        ws.paths.clone(),
        Limits::local(),
        &mut sink,
    )
    .with_library_dir(lib_dir);

    let params = RenderParams {
        pages: "1-2".parse::<PageRange>().expect("valid page range"),
        dpi: 72,
        format: RasterFormat::Png,
        quality: None,
        background: Background::White,
        annotations: true,
        combine: Combine::None,
    };

    let metrics = render::run(&mut ctx, params)
        .expect("render succeeds")
        .expect("metrics present");
    assert_eq!(metrics.get("pages").and_then(|v| v.as_u64()), Some(2));

    let events = parse_events(&sink);
    let artifacts = artifact_events(&events);
    assert_eq!(artifacts.len(), 2);

    let golden_paths = [
        golden_path(1, std::env::consts::OS),
        golden_path(2, std::env::consts::OS),
    ];

    if std::env::var_os("ARIAD_UPDATE_GOLDENS").is_some_and(|value| value == "1") {
        fs::create_dir_all(golden_dir()).expect("create golden dir");
        for ((actual_path, _), golden_path) in artifacts.iter().zip(golden_paths.iter()) {
            fs::copy(actual_path, golden_path).expect("write golden PNG");
        }
        return;
    }

    for (idx, ((actual_path, fmt), golden_path)) in
        artifacts.iter().zip(golden_paths.iter()).enumerate()
    {
        assert_eq!(fmt, "png");
        let actual_bytes = fs::read(actual_path).expect("read rendered PNG");
        let actual_img = image::load_from_memory_with_format(&actual_bytes, ImageFormat::Png)
            .expect("decode rendered PNG")
            .to_rgba8();

        let golden_bytes = fs::read(golden_path).unwrap_or_else(|e| {
            panic!("missing or unreadable {} golden {}: {e}; generate it with ARIAD_UPDATE_GOLDENS=1 cargo test --locked -p ariad-pdfium-engine --test render", std::env::consts::OS, golden_path.display())
        });
        let golden_img = image::load_from_memory_with_format(&golden_bytes, ImageFormat::Png)
            .expect("decode golden PNG")
            .to_rgba8();

        assert_matches_golden(&actual_img, &golden_img, &format!("page {}", idx + 1));
    }
}

#[test]
fn golden_paths_are_specific_to_each_supported_os() {
    assert!(golden_path(1, "linux").ends_with("pd-vi-qd-1922-2026.page-001.png"));
    assert!(golden_path(1, "macos").ends_with("pd-vi-qd-1922-2026.page-001.macos.png"));
    assert!(golden_path(2, "windows").ends_with("pd-vi-qd-1922-2026.page-002.windows.png"));
}

#[test]
fn render_supports_all_five_raster_formats_and_quality() {
    let lib_dir = require_pdfium!();

    let cases = [
        (RasterFormat::Png, None, ImageFormat::Png),
        (RasterFormat::Jpeg, Some(80), ImageFormat::Jpeg),
        (RasterFormat::Webp, Some(90), ImageFormat::WebP),
        (RasterFormat::Bmp, None, ImageFormat::Bmp),
        (RasterFormat::Tiff, None, ImageFormat::Tiff),
    ];

    for (raster_fmt, quality, img_fmt) in cases {
        let ws = setup_fixture_workspace("multipage-20.pdf");
        let mut sink = Vec::new();
        let mut ctx = ToolContext::new(
            "job-fmt",
            "pdf.render",
            ws.paths.clone(),
            Limits::local(),
            &mut sink,
        )
        .with_library_dir(&lib_dir);

        let params = RenderParams {
            pages: "1".parse::<PageRange>().expect("page 1"),
            dpi: 72,
            format: raster_fmt,
            quality,
            background: Background::White,
            annotations: true,
            combine: Combine::None,
        };

        let metrics = render::run(&mut ctx, params)
            .unwrap_or_else(|e| panic!("render {:?} failed: {e:?}", raster_fmt))
            .expect("metrics");
        assert_eq!(
            metrics.get("format").and_then(|v| v.as_str()),
            Some(raster_fmt.as_str())
        );

        let events = parse_events(&sink);
        let artifacts = artifact_events(&events);
        assert_eq!(artifacts.len(), 1);
        assert_eq!(artifacts[0].1, raster_fmt.as_str());

        let raw = fs::read(&artifacts[0].0).expect("read output image");
        let decoded = image::load_from_memory_with_format(&raw, img_fmt)
            .unwrap_or_else(|e| panic!("decode {:?} failed: {e}", raster_fmt));
        assert_eq!(decoded.width(), 595);
        assert_eq!(decoded.height(), 842);
    }

    // Verify that quality on a lossless format is rejected with InvalidRequest.
    let ws = setup_fixture_workspace("multipage-20.pdf");
    let mut sink = Vec::new();
    let mut ctx = ToolContext::new(
        "job-bad-quality",
        "pdf.render",
        ws.paths,
        Limits::local(),
        &mut sink,
    )
    .with_library_dir(lib_dir);
    let bad_params = RenderParams {
        pages: "1".parse().unwrap(),
        dpi: 72,
        format: RasterFormat::Png,
        quality: Some(80),
        background: Background::White,
        annotations: true,
        combine: Combine::None,
    };
    let err = render::run(&mut ctx, bad_params).unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidRequest);
}

#[test]
fn background_transparent_vs_white_and_jpeg_transparent_rejection() {
    let lib_dir = require_pdfium!();

    // Render page 1 with transparent background.
    let ws_trans = setup_fixture_workspace("multipage-20.pdf");
    let mut sink_trans = Vec::new();
    let mut ctx_trans = ToolContext::new(
        "job-trans",
        "pdf.render",
        ws_trans.paths.clone(),
        Limits::local(),
        &mut sink_trans,
    )
    .with_library_dir(&lib_dir);

    render::run(
        &mut ctx_trans,
        RenderParams {
            pages: "1".parse().unwrap(),
            dpi: 72,
            format: RasterFormat::Png,
            quality: None,
            background: Background::Transparent,
            annotations: true,
            combine: Combine::None,
        },
    )
    .expect("transparent PNG render succeeds");

    let trans_artifacts = artifact_events(&parse_events(&sink_trans));
    let trans_img = image::load_from_memory_with_format(
        &fs::read(&trans_artifacts[0].0).unwrap(),
        ImageFormat::Png,
    )
    .unwrap()
    .to_rgba8();

    // Top-left margin pixel (0, 0) is unpainted and must be transparent (alpha < 255, specifically 0),
    // while ink pixels on the page have alpha > 0.
    assert_eq!(
        trans_img.get_pixel(0, 0)[3],
        0,
        "unpainted margin pixel on transparent render must have alpha == 0"
    );
    assert!(
        trans_img.pixels().any(|p| p[3] == 255),
        "rendered text/content pixels must be opaque"
    );

    // Render page 1 with white background.
    let ws_white = setup_fixture_workspace("multipage-20.pdf");
    let mut sink_white = Vec::new();
    let mut ctx_white = ToolContext::new(
        "job-white",
        "pdf.render",
        ws_white.paths.clone(),
        Limits::local(),
        &mut sink_white,
    )
    .with_library_dir(&lib_dir);

    render::run(
        &mut ctx_white,
        RenderParams {
            pages: "1".parse().unwrap(),
            dpi: 72,
            format: RasterFormat::Png,
            quality: None,
            background: Background::White,
            annotations: true,
            combine: Combine::None,
        },
    )
    .expect("white PNG render succeeds");

    let white_artifacts = artifact_events(&parse_events(&sink_white));
    let white_img = image::load_from_memory_with_format(
        &fs::read(&white_artifacts[0].0).unwrap(),
        ImageFormat::Png,
    )
    .unwrap()
    .to_rgba8();

    assert_eq!(white_img.get_pixel(0, 0).0, [255, 255, 255, 255]);
    assert!(
        white_img.pixels().all(|p| p[3] == 255),
        "every pixel on white background render must have alpha == 255"
    );

    // Transparent + JPEG must fail with InvalidRequest.
    let ws_jpeg = setup_fixture_workspace("multipage-20.pdf");
    let mut sink_jpeg = Vec::new();
    let mut ctx_jpeg = ToolContext::new(
        "job-jpeg-trans",
        "pdf.render",
        ws_jpeg.paths,
        Limits::local(),
        &mut sink_jpeg,
    )
    .with_library_dir(lib_dir);

    let err = render::run(
        &mut ctx_jpeg,
        RenderParams {
            pages: "1".parse().unwrap(),
            dpi: 72,
            format: RasterFormat::Jpeg,
            quality: None,
            background: Background::Transparent,
            annotations: true,
            combine: Combine::None,
        },
    )
    .unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidRequest);
}

#[test]
fn pdf_to_images_defaults_to_300_dpi_and_supports_vertical_combine() {
    let lib_dir = require_pdfium!();

    // Verify ToImagesParams default DPI is 300.
    let default_to_images = ToImagesParams::default();
    assert_eq!(default_to_images.dpi, 300);
    let converted: RenderParams = default_to_images.into();
    assert_eq!(converted.dpi, 300);

    // Test vertical combine of 3 pages from multipage-20.pdf at 72 DPI.
    let ws = setup_fixture_workspace("multipage-20.pdf");
    let mut sink = Vec::new();
    let mut ctx = ToolContext::new(
        "job-combine",
        "pdf.to_images",
        ws.paths.clone(),
        Limits::local(),
        &mut sink,
    )
    .with_library_dir(lib_dir);

    let to_images = ToImagesParams {
        pages: "1-3".parse().unwrap(),
        dpi: 72,
        format: RasterFormat::Png,
        quality: None,
        background: Background::White,
        annotations: true,
        combine: Combine::Vertical,
    };

    let metrics = render::run(&mut ctx, to_images.into())
        .expect("vertical combine succeeds")
        .expect("metrics");
    assert_eq!(
        metrics.get("combined").and_then(|v| v.as_bool()),
        Some(true)
    );
    assert_eq!(metrics.get("pages").and_then(|v| v.as_u64()), Some(3));

    let artifacts = artifact_events(&parse_events(&sink));
    assert_eq!(artifacts.len(), 1);
    assert_eq!(artifacts[0].1, "png");

    let combined_img = image::load_from_memory_with_format(
        &fs::read(&artifacts[0].0).expect("read combined PNG"),
        ImageFormat::Png,
    )
    .expect("decode combined PNG");

    // Each page at 72 DPI is 595 x 842 px, so 3 vertically combined pages = 595 x (842 * 3 = 2526).
    assert_eq!(combined_img.width(), 595);
    assert_eq!(combined_img.height(), 842 * 3);
}

#[test]
fn more_than_50_pages_emits_single_zip_with_51_valid_png_entries() {
    let lib_dir = require_pdfium!();
    let ws = setup_fixture_workspace("large-1000-pages.pdf");
    let mut sink = Vec::new();
    let mut ctx = ToolContext::new(
        "job-zip",
        "pdf.render",
        ws.paths.clone(),
        Limits::local(),
        &mut sink,
    )
    .with_library_dir(lib_dir);

    let params = RenderParams {
        pages: "1-51".parse().unwrap(),
        dpi: 36,
        format: RasterFormat::Png,
        quality: None,
        background: Background::White,
        annotations: true,
        combine: Combine::None,
    };

    let metrics = render::run(&mut ctx, params)
        .expect("51-page render succeeds")
        .expect("metrics");
    assert_eq!(metrics.get("pages").and_then(|v| v.as_u64()), Some(51));
    assert_eq!(metrics.get("zipped").and_then(|v| v.as_bool()), Some(true));

    let artifacts = artifact_events(&parse_events(&sink));
    assert_eq!(artifacts.len(), 1, "must emit a single .zip artifact");
    assert_eq!(artifacts[0].1, "zip");

    let zip_file = fs::File::open(&artifacts[0].0).expect("open zip artifact");
    let mut archive = ZipArchive::new(zip_file).expect("read zip archive");
    assert_eq!(archive.len(), 51, "zip must contain 51 page entries");

    for i in 0..51 {
        let mut entry = archive.by_index(i).expect("zip entry");
        let expected_name = format!("large-1000-pages.page-{:03}.png", i + 1);
        assert_eq!(entry.name().expect("entry name"), expected_name);
        let mut png_bytes = Vec::new();
        entry.read_to_end(&mut png_bytes).expect("read entry bytes");
        let decoded = image::load_from_memory_with_format(&png_bytes, ImageFormat::Png)
            .expect("decode PNG inside zip");
        assert!(decoded.width() > 0 && decoded.height() > 0);
    }
}

#[test]
fn pixel_cap_refuses_single_page_and_vertical_combine_before_oversized_allocation() {
    let lib_dir = require_pdfium!();

    // 1. Single page exceeding pixel cap derived from Limits (`max_asset_bytes = 200_000` -> 50,000 px).
    // At 72 DPI, page 1 is 595 x 842 = 500,990 px > 50,000 px.
    let ws = setup_fixture_workspace("multipage-20.pdf");
    let mut sink = Vec::new();
    let tight_limits = Limits {
        max_asset_bytes: Some(200_000),
        ..Limits::local()
    };
    let mut ctx = ToolContext::new(
        "job-px-cap",
        "pdf.render",
        ws.paths,
        tight_limits,
        &mut sink,
    )
    .with_library_dir(&lib_dir);

    let err = render::run(
        &mut ctx,
        RenderParams {
            pages: "1".parse().unwrap(),
            dpi: 72,
            format: RasterFormat::Png,
            quality: None,
            background: Background::White,
            annotations: true,
            combine: Combine::None,
        },
    )
    .unwrap_err();
    assert_eq!(err.code, ErrorCode::LimitExceeded);

    // 2. Vertical combine where each page fits (500,990 px <= 1,000,000 px cap),
    // but stacking 3 pages (1,502,970 px) exceeds the cap.
    let ws_comb = setup_fixture_workspace("multipage-20.pdf");
    let mut sink_comb = Vec::new();
    let combine_limits = Limits {
        max_asset_bytes: Some(4_000_000), // 1,000,000 pixels max
        ..Limits::local()
    };
    let mut ctx_comb = ToolContext::new(
        "job-comb-cap",
        "pdf.to_images",
        ws_comb.paths,
        combine_limits,
        &mut sink_comb,
    )
    .with_library_dir(&lib_dir);

    let comb_err = render::run(
        &mut ctx_comb,
        RenderParams {
            pages: "1-3".parse().unwrap(),
            dpi: 72,
            format: RasterFormat::Png,
            quality: None,
            background: Background::White,
            annotations: true,
            combine: Combine::Vertical,
        },
    )
    .unwrap_err();
    assert_eq!(comb_err.code, ErrorCode::LimitExceeded);

    // 3. Huge-MediaBox PDF exceeding the absolute MAX_IMAGE_PIXELS (16384x16384) under Limits::local().
    let base_pdf = fs::read(fixtures_dir().join("encrypted-aes256.plaintext.pdf")).unwrap();
    let huge_bytes = replace_fixed_len(
        &base_pdf,
        b"/MediaBox [ 0 0 595.28 841.89 ]",
        b"/MediaBox [ 0 0 10000  10000  ]",
    );

    let ws_huge = setup_workspace("huge.pdf", &huge_bytes);
    let mut sink_huge = Vec::new();
    let mut ctx_huge = ToolContext::new(
        "job-huge",
        "pdf.render",
        ws_huge.paths,
        Limits::local(),
        &mut sink_huge,
    )
    .with_library_dir(lib_dir);

    // 10,000 pt at 150 DPI = 20,833 x 20,833 = 434,013,889 px > MAX_IMAGE_PIXELS (268,435,456).
    let huge_err = render::run(&mut ctx_huge, RenderParams::default()).unwrap_err();
    assert_eq!(huge_err.code, ErrorCode::LimitExceeded);
}

#[test]
fn cancel_and_timeout_abort_cleanly() {
    let lib_dir = require_pdfium!();

    // 1. Cancelled flag aborts with EngineFailure.
    let ws_cancel = setup_fixture_workspace("multipage-20.pdf");
    let mut sink_cancel = Vec::new();
    let cancel = CancelFlag::new();
    cancel.cancel();
    let mut ctx_cancel = ToolContext::new(
        "job-cancel",
        "pdf.render",
        ws_cancel.paths,
        Limits::local(),
        &mut sink_cancel,
    )
    .with_library_dir(&lib_dir)
    .with_cancel(cancel);

    let err_cancel = render::run(&mut ctx_cancel, RenderParams::default()).unwrap_err();
    assert_eq!(err_cancel.code, ErrorCode::EngineFailure);
    assert!(err_cancel.message.contains("cancelled"));

    // 2. Expired deadline aborts with LimitExceeded.
    let ws_timeout = setup_fixture_workspace("multipage-20.pdf");
    let mut sink_timeout = Vec::new();
    let mut ctx_timeout = ToolContext::new(
        "job-timeout",
        "pdf.render",
        ws_timeout.paths,
        Limits::local(),
        &mut sink_timeout,
    )
    .with_library_dir(lib_dir)
    .with_deadline(Instant::now() - Duration::from_millis(10));

    let err_timeout = render::run(&mut ctx_timeout, RenderParams::default()).unwrap_err();
    assert_eq!(err_timeout.code, ErrorCode::LimitExceeded);
    assert!(err_timeout.message.contains("time limit"));
}

#[test]
fn javascript_action_pdf_renders_without_executing_javascript() {
    let lib_dir = require_pdfium!();
    let js_pdf_bytes = fs::read(fixtures_dir().join("javascript-action.pdf")).unwrap();

    // Confirm the fixture actually carries document-level JavaScript (/OpenAction and /Names /JavaScript).
    let info = ariad_pdf::info::run(&js_pdf_bytes, &Limits::local())
        .expect("inspect javascript-action.pdf");
    assert!(
        info.has_javascript,
        "javascript-action.pdf fixture must contain embedded JavaScript actions"
    );

    let ws = setup_workspace("javascript-action.pdf", &js_pdf_bytes);
    let mut sink = Vec::new();
    let mut ctx = ToolContext::new(
        "job-js",
        "pdf.render",
        ws.paths.clone(),
        Limits::local(),
        &mut sink,
    )
    .with_library_dir(&lib_dir);

    let params = RenderParams {
        pages: PageRange::all(),
        dpi: 72,
        format: RasterFormat::Png,
        quality: None,
        background: Background::White,
        annotations: true,
        combine: Combine::None,
    };

    render::run(&mut ctx, params).expect("rendering javascript-action.pdf must succeed");

    let events = parse_events(&sink);
    // Verify no warnings or unexpected events occurred.
    assert!(
        !events.iter().any(|e| matches!(e, EventV2::Warning { .. })),
        "no warnings expected during javascript-action.pdf render"
    );
    let artifacts = artifact_events(&events);
    assert_eq!(artifacts.len(), 1);

    let img = image::load_from_memory_with_format(
        &fs::read(&artifacts[0].0).expect("read rendered PNG"),
        ImageFormat::Png,
    )
    .expect("decode PNG")
    .to_rgba8();

    // Ensure the image is non-blank (contains dark text pixels on white background).
    let non_white_pixels = img
        .pixels()
        .filter(|p| p[0] < 240 || p[1] < 240 || p[2] < 240)
        .count();
    assert!(
        non_white_pixels > 100,
        "rendered javascript-action.pdf must be non-blank (got {non_white_pixels} non-white pixels)"
    );

    // Neutralize /OpenAction, /JavaScript, and /JS in-place (preserving exact byte offsets in xref)
    // and assert the rendered bitmap is pixel-for-pixel identical.
    let without_open = replace_fixed_len(&js_pdf_bytes, b"/OpenAction 4 0 R", b"                 ");
    let without_js_name = replace_fixed_len(&without_open, b"/JavaScript", b"/NoOpScript");
    let stripped_bytes = replace_fixed_len(&without_js_name, b"/JS (", b"/NO (");

    let stripped_info =
        ariad_pdf::info::run(&stripped_bytes, &Limits::local()).expect("inspect stripped PDF");
    assert!(!stripped_info.has_javascript);

    let ws_clean = setup_workspace("clean.pdf", &stripped_bytes);
    let mut sink_clean = Vec::new();
    let mut ctx_clean = ToolContext::new(
        "job-clean",
        "pdf.render",
        ws_clean.paths.clone(),
        Limits::local(),
        &mut sink_clean,
    )
    .with_library_dir(lib_dir);

    render::run(
        &mut ctx_clean,
        RenderParams {
            pages: PageRange::all(),
            dpi: 72,
            format: RasterFormat::Png,
            quality: None,
            background: Background::White,
            annotations: true,
            combine: Combine::None,
        },
    )
    .expect("render stripped PDF");
    let clean_artifacts = artifact_events(&parse_events(&sink_clean));
    let clean_img = image::load_from_memory_with_format(
        &fs::read(&clean_artifacts[0].0).unwrap(),
        ImageFormat::Png,
    )
    .unwrap()
    .to_rgba8();
    assert_eq!(img.as_raw(), clean_img.as_raw());
}

#[test]
fn encrypted_and_malformed_inputs_fail_with_typed_errors() {
    let lib_dir = require_pdfium!();

    // 1. Owner-password-only AES-256 encrypted PDF (empty user password) fails pointing at `pdf.decrypt`.
    let owner_encrypted_bytes = fs::read(encrypted_seed_path()).expect("read encrypted seed PDF");
    let ws_owner = setup_workspace("owner-encrypted.pdf", &owner_encrypted_bytes);
    let mut sink_owner = Vec::new();
    let mut ctx_owner = ToolContext::new(
        "job-owner-enc",
        "pdf.render",
        ws_owner.paths,
        Limits::local(),
        &mut sink_owner,
    )
    .with_library_dir(&lib_dir);

    let owner_err = render::run(&mut ctx_owner, RenderParams::default()).unwrap_err();
    assert_eq!(owner_err.code, ErrorCode::EngineFailure);
    assert!(
        owner_err.message.contains("pdf.decrypt"),
        "expected owner-only encrypted error to mention 'pdf.decrypt', got: {}",
        owner_err.message
    );

    // 2. Non-empty user-password AES-256 encrypted PDF fails pointing at `pdf.decrypt`.
    let user_encrypted_bytes =
        replace_fixed_len(&owner_encrypted_bytes, b"/U <6d20654d", b"/U <ff20654d");
    let ws_enc = setup_workspace("encrypted.pdf", &user_encrypted_bytes);
    let mut sink_enc = Vec::new();
    let mut ctx_enc = ToolContext::new(
        "job-enc",
        "pdf.render",
        ws_enc.paths,
        Limits::local(),
        &mut sink_enc,
    )
    .with_library_dir(&lib_dir);

    let enc_err = render::run(&mut ctx_enc, RenderParams::default()).unwrap_err();
    assert_eq!(enc_err.code, ErrorCode::EngineFailure);
    assert!(
        enc_err.message.contains("pdf.decrypt"),
        "expected encrypted error to mention 'pdf.decrypt', got: {}",
        enc_err.message
    );

    // 3. Malformed / corrupt input fails with a typed error, never a panic.
    let ws_bad = setup_workspace("corrupt.pdf", b"%PDF-1.7\nnot a valid pdf body\n%%EOF");
    let mut sink_bad = Vec::new();
    let mut ctx_bad = ToolContext::new(
        "job-bad",
        "pdf.render",
        ws_bad.paths,
        Limits::local(),
        &mut sink_bad,
    )
    .with_library_dir(lib_dir);

    let bad_err = render::run(&mut ctx_bad, RenderParams::default()).unwrap_err();
    assert_eq!(bad_err.code, ErrorCode::EngineFailure);
}

#[test]
fn render_streams_each_page_and_rejects_cumulative_output_without_artifacts() {
    struct ObserveWrites {
        work_dir: PathBuf,
        counts: Vec<usize>,
        events: Vec<u8>,
    }
    impl std::io::Write for ObserveWrites {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.events.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            let event = self
                .events
                .split(|&b| b == b'\n')
                .rev()
                .nth(1)
                .unwrap_or_default();
            if event
                .windows(b"\"stage\":\"render\"".len())
                .any(|w| w == b"\"stage\":\"render\"")
            {
                self.counts.push(fs::read_dir(&self.work_dir)?.count());
            }
            Ok(())
        }
    }
    let lib_dir = require_pdfium!();
    let ws = setup_fixture_workspace("multipage-20.pdf");
    let mut sink = ObserveWrites {
        work_dir: ws.paths.work_dir.clone(),
        counts: Vec::new(),
        events: Vec::new(),
    };
    let limits = Limits {
        max_decompressed_bytes: 3_000_000,
        ..Limits::local()
    };
    let mut ctx = ToolContext::new("budget", "pdf.render", ws.paths.clone(), limits, &mut sink)
        .with_library_dir(lib_dir);
    let err = render::run(
        &mut ctx,
        RenderParams {
            pages: "1-3".parse().unwrap(),
            dpi: 72,
            format: RasterFormat::Bmp,
            quality: None,
            background: Background::White,
            annotations: true,
            combine: Combine::None,
        },
    )
    .unwrap_err();
    assert_eq!(err.code, ErrorCode::LimitExceeded);
    assert_eq!(sink.counts, vec![1]);
    assert!(artifact_events(&parse_events(&sink.events)).is_empty());
    assert_eq!(fs::read_dir(&ws.paths.output_dir).unwrap().count(), 0);
}
