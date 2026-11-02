//! Integration tests for `pdf.thumbnails`.

use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use ariad_core::{
    limits::Limits,
    protocol::{ErrorCode, v2::EventV2},
    tools::{page_range::PageRange, params::ThumbnailsParams},
};
use ariad_engine_io::RunToolPaths;
use ariad_pdfium_engine::{
    output::{CancelFlag, ToolContext},
    thumbnails::{self, ThumbnailEntry},
};
use image::ImageFormat;

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

#[test]
fn thumbnails_preserve_aspect_ratio_and_emit_matching_json_index() {
    let lib_dir = require_pdfium!();
    // `mixed-sizes.pdf` has 3 pages:
    // Page 1: A4 portrait (595.28 x 841.89 pt) -> at max_px=256: 181 x 256
    // Page 2: Letter portrait (612.0 x 792.0 pt) -> at max_px=256: 198 x 256
    // Page 3: A3 landscape (1190.55 x 841.89 pt) -> at max_px=256: 256 x 181
    let ws = setup_fixture_workspace("mixed-sizes.pdf");
    let mut sink = Vec::new();
    let mut ctx = ToolContext::new(
        "job-thumbs",
        "pdf.thumbnails",
        ws.paths.clone(),
        Limits::local(),
        &mut sink,
    )
    .with_library_dir(lib_dir);

    let params = ThumbnailsParams {
        pages: PageRange::all(),
        max_px: 256,
    };

    let metrics = thumbnails::run(&mut ctx, params)
        .expect("thumbnails run succeeds")
        .expect("metrics present");
    assert_eq!(metrics.get("pages").and_then(|v| v.as_u64()), Some(3));
    assert_eq!(metrics.get("max_px").and_then(|v| v.as_u64()), Some(256));

    let events = parse_events(&sink);
    let artifacts = artifact_events(&events);
    // 3 PNG artifacts + 1 JSON index artifact = 4 artifacts.
    assert_eq!(artifacts.len(), 4);

    for (path, fmt) in &artifacts[0..3] {
        assert_eq!(fmt, "png");
        assert!(path.is_file());
    }
    let (index_path, index_fmt) = &artifacts[3];
    assert_eq!(index_fmt, "json");
    assert_eq!(
        index_path.file_name().and_then(|s| s.to_str()),
        Some("mixed-sizes.thumbnails.json")
    );

    let index_raw = fs::read(index_path).expect("read thumbnails index JSON");
    let entries: Vec<ThumbnailEntry> =
        serde_json::from_slice(&index_raw).expect("deserialize Vec<ThumbnailEntry>");
    assert_eq!(entries.len(), 3);

    let expected_dims = [(1, 181, 256), (2, 198, 256), (3, 256, 181)];
    for (entry, (expected_page, expected_w, expected_h)) in entries.iter().zip(expected_dims.iter())
    {
        assert_eq!(entry.page, *expected_page);
        assert_eq!(entry.width, *expected_w);
        assert_eq!(entry.height, *expected_h);
        assert_eq!(
            entry.width.max(entry.height),
            256,
            "longest side of page {} must equal max_px (256)",
            entry.page
        );

        let png_path = ws.paths.output_dir.join(&entry.file);
        let png_bytes = fs::read(&png_path).expect("read thumbnail PNG");
        let decoded = image::load_from_memory_with_format(&png_bytes, ImageFormat::Png)
            .expect("decode thumbnail PNG");
        assert_eq!(decoded.width(), entry.width);
        assert_eq!(decoded.height(), entry.height);
    }
}

#[test]
fn thumbnails_respect_page_range_and_custom_max_px() {
    let lib_dir = require_pdfium!();
    let ws = setup_fixture_workspace("multipage-20.pdf");
    let mut sink = Vec::new();
    let mut ctx = ToolContext::new(
        "job-thumbs-subset",
        "pdf.thumbnails",
        ws.paths.clone(),
        Limits::local(),
        &mut sink,
    )
    .with_library_dir(&lib_dir);

    let params = ThumbnailsParams {
        pages: "2,5".parse().expect("valid page range"),
        max_px: 128,
    };

    thumbnails::run(&mut ctx, params).expect("thumbnails subset succeeds");

    let artifacts = artifact_events(&parse_events(&sink));
    assert_eq!(artifacts.len(), 3); // 2 PNGs + 1 JSON index
    let entries: Vec<ThumbnailEntry> =
        serde_json::from_slice(&fs::read(&artifacts[2].0).unwrap()).unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].page, 2);
    assert_eq!(entries[0].file, "multipage-20.page-002.png");
    assert_eq!(entries[0].width.max(entries[0].height), 128);
    assert_eq!(entries[1].page, 5);
    assert_eq!(entries[1].file, "multipage-20.page-005.png");
    assert_eq!(entries[1].width.max(entries[1].height), 128);

    // Out-of-bounds max_px fails with InvalidRequest.
    let mut sink_bad = Vec::new();
    let mut ctx_bad = ToolContext::new(
        "job-thumbs-bad",
        "pdf.thumbnails",
        ws.paths,
        Limits::local(),
        &mut sink_bad,
    )
    .with_library_dir(lib_dir);

    let err = thumbnails::run(
        &mut ctx_bad,
        ThumbnailsParams {
            pages: PageRange::all(),
            max_px: 32,
        },
    )
    .unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidRequest);
}

#[test]
fn thumbnails_enforce_safety_limits_cancel_timeout_and_encryption() {
    let lib_dir = require_pdfium!();

    // 1. Pixel-cap refusal via tight max_asset_bytes.
    let ws_px = setup_fixture_workspace("multipage-20.pdf");
    let mut sink_px = Vec::new();
    let tight_limits = Limits {
        max_asset_bytes: Some(4_000), // 1,000 pixels max (< 90x128)
        ..Limits::local()
    };
    let mut ctx_px = ToolContext::new(
        "job-thumbs-px",
        "pdf.thumbnails",
        ws_px.paths,
        tight_limits,
        &mut sink_px,
    )
    .with_library_dir(&lib_dir);
    let px_err = thumbnails::run(
        &mut ctx_px,
        ThumbnailsParams {
            pages: "1".parse().unwrap(),
            max_px: 128,
        },
    )
    .unwrap_err();
    assert_eq!(px_err.code, ErrorCode::LimitExceeded);

    // 2. Cancel flag refusal.
    let ws_cancel = setup_fixture_workspace("multipage-20.pdf");
    let mut sink_cancel = Vec::new();
    let cancel = CancelFlag::new();
    cancel.cancel();
    let mut ctx_cancel = ToolContext::new(
        "job-thumbs-cancel",
        "pdf.thumbnails",
        ws_cancel.paths,
        Limits::local(),
        &mut sink_cancel,
    )
    .with_library_dir(&lib_dir)
    .with_cancel(cancel);
    let cancel_err = thumbnails::run(&mut ctx_cancel, ThumbnailsParams::default()).unwrap_err();
    assert_eq!(cancel_err.code, ErrorCode::EngineFailure);

    // 3. Expired deadline refusal.
    let ws_timeout = setup_fixture_workspace("multipage-20.pdf");
    let mut sink_timeout = Vec::new();
    let mut ctx_timeout = ToolContext::new(
        "job-thumbs-timeout",
        "pdf.thumbnails",
        ws_timeout.paths,
        Limits::local(),
        &mut sink_timeout,
    )
    .with_library_dir(&lib_dir)
    .with_deadline(Instant::now() - Duration::from_millis(5));
    let timeout_err = thumbnails::run(&mut ctx_timeout, ThumbnailsParams::default()).unwrap_err();
    assert_eq!(timeout_err.code, ErrorCode::LimitExceeded);

    // 4. Encrypted PDF refusal pointing at `pdf.decrypt`.
    let enc_bytes = fs::read(encrypted_seed_path()).expect("read encrypted seed PDF");
    let ws_enc = setup_workspace("encrypted.pdf", &enc_bytes);
    let mut sink_enc = Vec::new();
    let mut ctx_enc = ToolContext::new(
        "job-thumbs-enc",
        "pdf.thumbnails",
        ws_enc.paths,
        Limits::local(),
        &mut sink_enc,
    )
    .with_library_dir(lib_dir);
    let enc_err = thumbnails::run(&mut ctx_enc, ThumbnailsParams::default()).unwrap_err();
    assert_eq!(enc_err.code, ErrorCode::EngineFailure);
    assert!(enc_err.message.contains("pdf.decrypt"));
}
