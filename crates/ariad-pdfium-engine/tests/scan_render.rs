//! Integration coverage for the internal scan renderer.
use ariad_core::{limits::Limits, tools::params::ScanRenderParams};
use ariad_engine_io::RunToolPaths;
use ariad_pdfium_engine::{output::ToolContext, scan::render};
use std::{fs, path::PathBuf};

#[test]
fn renders_grayscale_pages_and_metadata() {
    let Some(pdfium) = std::env::var_os("ARIAD_TEST_PDFIUM_DIR") else {
        assert!(
            std::env::var_os("ARIAD_REQUIRE_OCR_PACKS").is_none(),
            "ARIAD_TEST_PDFIUM_DIR is required"
        );
        eprintln!("skipping: ARIAD_TEST_PDFIUM_DIR is not set");
        return;
    };
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let input_dir = root.join("in");
    let output_dir = root.join("out");
    let work_dir = root.join("work");
    for dir in [&input_dir, &output_dir, &work_dir] {
        fs::create_dir(dir).unwrap();
    }
    let input = input_dir.join("input.pdf");
    fs::copy(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/pdf-tools/scan-mixed-text-vi.pdf"),
        &input,
    )
    .unwrap();
    let paths = RunToolPaths {
        workspace: root,
        inputs: vec![input],
        output_dir: output_dir.clone(),
        work_dir,
    };
    let mut events = Vec::new();
    let mut ctx = ToolContext::new("test", "scan.render", paths, Limits::default(), &mut events)
        .with_library_dir(pdfium);
    let params: ScanRenderParams =
        serde_json::from_value(serde_json::json!({"pages":"2-3", "dpi":72, "clean":true})).unwrap();
    render::run(&mut ctx, params).unwrap();
    let index: serde_json::Value =
        serde_json::from_slice(&fs::read(output_dir.join("scan.json")).unwrap()).unwrap();
    assert_eq!(index.as_array().unwrap().len(), 2);
    assert_eq!(index[0]["page"], 2);
    assert_eq!(index[1]["page"], 3);
    assert_eq!(index[0]["cleaned"], true);
    let image = image::open(output_dir.join("p0002.png")).unwrap();
    assert_eq!(image.color(), image::ColorType::L8);
    assert_eq!(
        u64::from(image.width()),
        index[0]["width_px"].as_u64().unwrap()
    );
}

#[test]
fn detects_the_three_scan_bed_regions() {
    let Some(pdfium) = std::env::var_os("ARIAD_TEST_PDFIUM_DIR") else {
        assert!(
            std::env::var_os("ARIAD_REQUIRE_OCR_PACKS").is_none(),
            "ARIAD_TEST_PDFIUM_DIR is required"
        );
        eprintln!("skipping: ARIAD_TEST_PDFIUM_DIR is not set");
        return;
    };
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let input_dir = root.join("in");
    let output_dir = root.join("out");
    let work_dir = root.join("work");
    for dir in [&input_dir, &output_dir, &work_dir] {
        fs::create_dir(dir).unwrap();
    }
    let input = input_dir.join("input.pdf");
    fs::copy(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/pdf-tools/scan-photos-3.pdf"),
        &input,
    )
    .unwrap();
    let paths = RunToolPaths {
        workspace: root,
        inputs: vec![input],
        output_dir: output_dir.clone(),
        work_dir,
    };
    let mut events = Vec::new();
    let mut ctx = ToolContext::new("test", "scan.render", paths, Limits::default(), &mut events)
        .with_library_dir(pdfium);
    let params: ScanRenderParams = serde_json::from_value(serde_json::json!({"dpi":150})).unwrap();
    render::run(&mut ctx, params).unwrap();
    let gray = image::open(output_dir.join("p0001.png"))
        .unwrap()
        .into_luma8();
    let regions = ariad_pdfium_engine::scan::regions::detect(&gray, 0.02, 0);
    assert_eq!(regions.len(), 3, "{regions:?}");
    let truth_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/pdf-tools/scan-photos-3.truth.json");
    let truth: serde_json::Value = serde_json::from_slice(&fs::read(truth_path).unwrap()).unwrap();
    let mut expected: Vec<(Vec<f64>, f64)> = truth["regions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|region| {
            (
                region["bbox_pt"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_f64().unwrap() * 150.0 / 72.0)
                    .collect(),
                region["angle_deg"].as_f64().unwrap(),
            )
        })
        .collect();
    expected.sort_by(|a, b| a.0[1].total_cmp(&b.0[1]).then(a.0[0].total_cmp(&b.0[0])));
    for (region, (bbox, angle)) in regions.iter().zip(expected) {
        for (actual, expected) in [region.x0, region.y0, region.x1, region.y1]
            .into_iter()
            .zip(bbox)
        {
            assert!((f64::from(actual) - expected).abs() <= 12.0, "{region:?}");
        }
        assert!(
            (f64::from(region.angle_deg) - angle).abs() < 1.5,
            "{region:?}, expected {angle}"
        );
    }
}
