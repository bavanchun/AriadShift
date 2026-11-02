//! Integration coverage for the internal scan renderer.
use ariad_core::{limits::Limits, tools::params::ScanRenderParams};
use ariad_engine_io::RunToolPaths;
use ariad_pdfium_engine::{output::ToolContext, scan::render};
use std::{fs, path::PathBuf};

#[test]
fn renders_grayscale_pages_and_metadata() {
    let Some(pdfium) = std::env::var_os("ARIAD_TEST_PDFIUM_DIR") else {
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
