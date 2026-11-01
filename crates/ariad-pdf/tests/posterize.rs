use std::{fs, path::PathBuf};

use ariad_core::{
    limits::Limits,
    tools::{
        page_range::PageRange,
        params::{
            common::PaperSize,
            posterize::{PosterizeParams, PosterizeTiles},
        },
    },
    writer::markdown,
};
use ariad_pdf::{
    InputDoc, PdfError, REPAIRED_XREF_WARNING, ToolCtx, document, info,
    tools::{posterize, run_tool},
};
use serde_json::{Value, json};

fn fixture_bytes(name: &str) -> Vec<u8> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf();
    fs::read(root.join("fixtures/pdf-tools").join(name)).expect("read pdf-tools fixture")
}

fn extract_text(pdf_bytes: &[u8], limits: &Limits) -> String {
    let read_out = ariad_core::reader::pdf::read(pdf_bytes, limits).expect("extract text from pdf");
    markdown::write(&read_out.document).content
}

fn assert_json_snapshot(snap_name: &str, value: &Value) {
    let snap_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots");
    fs::create_dir_all(&snap_dir).expect("create snapshots dir");
    let snap_path = snap_dir.join(format!("{snap_name}.snap"));
    let body = serde_json::to_string_pretty(value).expect("serialize snapshot json");
    let full = format!(
        "---\nsource: crates/ariad-pdf/tests/posterize.rs\nexpression: info_json\n---\n{body}\n"
    );
    if std::env::var_os("UPDATE_SNAPSHOTS").is_some() || !snap_path.exists() {
        fs::write(&snap_path, &full).expect("write snapshot");
    }
    let existing = fs::read_to_string(&snap_path).expect("read snapshot");
    assert_eq!(existing, full, "snapshot mismatch for {snap_name}");
}

#[test]
fn posterize_scale_2_tiles_3x2_crop_marks_determinism_and_snapshot() {
    let ctx = ToolCtx::default();
    let limits = Limits::local();
    let raw = fixture_bytes("annotations.pdf");
    let input = InputDoc::new("annotations.pdf", raw);

    // 1. scale = 2.0 on A4 -> 4 A4 sheets with overlap_mm = 10.0 and crop_marks = true
    let params_scale = json!({
        "pages": "all",
        "sheet": "A4",
        "scale": 2.0,
        "overlap_mm": 10.0,
        "crop_marks": true
    });
    let out_scale = run_tool(
        "pdf.posterize",
        std::slice::from_ref(&input),
        &params_scale,
        &ctx,
    )
    .expect("run posterize scale 2.0");
    let bytes_scale = &out_scale.artifacts[0].bytes;
    let doc_scale = document::open(bytes_scale, &limits).expect("open posterized pdf");

    let tile1_id = doc_scale.inner().page_iter().next().unwrap();
    let tile1_bytes = doc_scale.inner().get_page_content(tile1_id);
    let tile1_stream = String::from_utf8_lossy(&tile1_bytes);
    assert!(
        tile1_stream.contains("q 0.25 w 0 0 0 RG"),
        "expected crop marks stroke in tile content stream, got: {tile1_stream}"
    );

    let preview_dir = std::env::temp_dir().join("ariad-p03-s4-preview");
    fs::create_dir_all(&preview_dir).expect("create preview dir");
    fs::write(preview_dir.join("posterize.pdf"), bytes_scale).expect("write preview posterize.pdf");

    let info_scale = info::run(bytes_scale, &limits).expect("inspect posterize");
    assert_eq!(info_scale.page_count, 4);
    for page in &info_scale.pages {
        assert!((page.width_pt - 595.276).abs() < 0.05);
        assert!((page.height_pt - 841.89).abs() < 0.05);
    }
    let info_json = serde_json::to_value(&info_scale).unwrap();
    assert_json_snapshot("posterize_single_page_info", &info_json);

    let extracted = extract_text(bytes_scale, &limits);
    assert!(
        !extracted.trim().is_empty(),
        "expected extracted text from posterized tiles"
    );

    // Determinism check
    let out_scale_2 = run_tool(
        "pdf.posterize",
        std::slice::from_ref(&input),
        &params_scale,
        &ctx,
    )
    .expect("run posterize second time");
    assert_eq!(
        bytes_scale, &out_scale_2.artifacts[0].bytes,
        "pdf.posterize must be byte-deterministic"
    );

    // 2. tiles = { cols: 3, rows: 2 }, crop_marks = false -> 6 sheets without crop marks
    let params_tiles = json!({
        "pages": "1",
        "sheet": "A4",
        "tiles": { "cols": 3, "rows": 2 },
        "overlap_mm": 12.0,
        "crop_marks": false
    });
    let out_tiles = run_tool(
        "pdf.posterize",
        std::slice::from_ref(&input),
        &params_tiles,
        &ctx,
    )
    .expect("run posterize tiles 3x2");
    let bytes_tiles = &out_tiles.artifacts[0].bytes;
    let doc_tiles = document::open(bytes_tiles, &limits).expect("open 3x2 tiles");
    let t1_id = doc_tiles.inner().page_iter().next().unwrap();
    let t1_bytes = doc_tiles.inner().get_page_content(t1_id);
    let t1_stream = String::from_utf8_lossy(&t1_bytes);
    assert!(
        !t1_stream.contains("q 0.25 w 0 0 0 RG"),
        "crop_marks=false must not draw crop mark ticks"
    );
    let info_tiles = info::run(bytes_tiles, &limits).expect("inspect 3x2 tiles");
    assert_eq!(info_tiles.page_count, 6);
}

#[test]
fn posterize_error_cases_and_broken_xref() {
    let ctx = ToolCtx::default();
    let limits = Limits::local();
    let raw = fixture_bytes("annotations.pdf");
    let input = InputDoc::new("annotations.pdf", raw.clone());
    let src = document::open(&raw, &limits).expect("open annotations.pdf");

    // Setting both scale and tiles must be rejected
    let both_err = run_tool(
        "pdf.posterize",
        std::slice::from_ref(&input),
        &json!({
            "scale": 2.0,
            "tiles": { "cols": 2, "rows": 2 }
        }),
        &ctx,
    )
    .expect_err("both scale and tiles must fail");
    assert!(matches!(both_err, PdfError::InvalidParams(_)));

    // Invalid scale <= 1.0 via struct
    let bad_scale = PosterizeParams {
        pages: PageRange::all(),
        sheet: PaperSize::default(),
        scale: Some(1.0),
        tiles: None,
        overlap_mm: 10.0,
        crop_marks: true,
    };
    assert!(matches!(
        posterize::run(std::slice::from_ref(&src), &bad_scale, &ctx),
        Err(PdfError::InvalidParams(_))
    ));

    // Invalid tiles = 0 via struct
    let bad_tiles = PosterizeParams {
        pages: PageRange::all(),
        sheet: PaperSize::default(),
        scale: None,
        tiles: Some(PosterizeTiles { cols: 0, rows: 2 }),
        overlap_mm: 10.0,
        crop_marks: true,
    };
    assert!(matches!(
        posterize::run(std::slice::from_ref(&src), &bad_tiles, &ctx),
        Err(PdfError::InvalidParams(_))
    ));

    // Wrong input count
    assert!(matches!(
        posterize::run(&[], &PosterizeParams::default(), &ctx),
        Err(PdfError::InvalidParams(_))
    ));

    // broken-xref.pdf repair
    let broken_raw = fixture_bytes("broken-xref.pdf");
    let broken_input = InputDoc::new("broken-xref.pdf", broken_raw);
    let out = run_tool(
        "pdf.posterize",
        &[broken_input],
        &json!({"scale": 2.0}),
        &ctx,
    )
    .expect("posterize on broken-xref.pdf");
    assert_eq!(out.warnings, vec![REPAIRED_XREF_WARNING.to_owned()]);
    let reopened = document::open(&out.artifacts[0].bytes, &limits).expect("reopen repaired");
    assert!(!reopened.repaired_xref());
}
