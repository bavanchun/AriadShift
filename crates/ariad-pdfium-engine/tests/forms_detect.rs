//! Evaluation of field proposals against synthetic ground-truth rectangles.

use ariad_core::{
    limits::Limits,
    tools::params::forms::{DetectedField, FormsDetectMode, FormsDetectParams},
};
use ariad_engine_io::RunToolPaths;
use ariad_pdfium_engine::{forms_detect, output::ToolContext};
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .into()
}
fn library() -> Option<PathBuf> {
    std::env::var_os("ARIAD_TEST_PDFIUM_DIR")
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
}
fn run(fixture: &str, mode: FormsDetectMode, lib: &Path) -> Value {
    let workspace = tempfile::tempdir().unwrap();
    let path = workspace.path().canonicalize().unwrap();
    for name in ["in", "out", "tmp"] {
        fs::create_dir(path.join(name)).unwrap();
    }
    let input = path.join("in/document.pdf");
    fs::copy(root().join(fixture), &input).unwrap();
    let paths = RunToolPaths {
        workspace: path.clone(),
        inputs: vec![input],
        output_dir: path.join("out"),
        work_dir: path.join("tmp"),
    };
    let mut events = Vec::new();
    let mut ctx = ToolContext::new(
        "job",
        "pdf.forms_detect",
        paths.clone(),
        Limits::local(),
        &mut events,
    )
    .with_library_dir(lib);
    let params: FormsDetectParams =
        serde_json::from_value(serde_json::json!({"mode":mode})).unwrap();
    forms_detect::run(&mut ctx, params).unwrap();
    serde_json::from_slice(
        &fs::read(paths.output_dir.join("document.detected-fields.json")).unwrap(),
    )
    .unwrap()
}
fn iou(a: [f64; 4], b: [f64; 4]) -> f64 {
    let area = |r: [f64; 4]| (r[2] - r[0]).max(0.0) * (r[3] - r[1]).max(0.0);
    let overlap = area([
        a[0].max(b[0]),
        a[1].max(b[1]),
        a[2].min(b[2]),
        a[3].min(b[3]),
    ]);
    overlap / (area(a) + area(b) - overlap).max(f64::EPSILON)
}
#[test]
fn evaluation_meets_precision_and_recall_targets() {
    let Some(lib) = library() else {
        eprintln!("skipping test: ARIAD_TEST_PDFIUM_DIR is not set");
        return;
    };
    let mut matched = 0usize;
    let mut proposed = 0usize;
    let mut expected = 0usize;
    for name in [
        "underlines",
        "boxes",
        "checkboxes",
        "table",
        "vietnamese",
        "existing-widgets",
        "rotated",
        "prose",
    ] {
        let fixture = format!("fixtures/pdf-forms-detect/{name}.pdf");
        let output = run(&fixture, FormsDetectMode::Vector, &lib);
        let proposals: Vec<DetectedField> =
            serde_json::from_value(output["fields"].clone()).unwrap();
        let truth: Value = serde_json::from_slice(
            &fs::read(root().join(format!("fixtures/pdf-forms-detect/{name}.truth.json"))).unwrap(),
        )
        .unwrap();
        let targets = truth["detected_fields"].as_array().unwrap();
        let mut used = vec![false; targets.len()];
        let mut hits = 0;
        for field in &proposals {
            assert!(field.proposal.validate().is_ok());
            let matching = targets
                .iter()
                .enumerate()
                .filter(|(i, truth)| {
                    !used[*i]
                        && truth["type"] == serde_json::to_value(field.proposal.field_type).unwrap()
                })
                .filter_map(|(i, truth)| {
                    let rect: [f64; 4] = serde_json::from_value(truth["rect"].clone()).ok()?;
                    let score = iou(field.proposal.rect, rect);
                    (score >= 0.5).then_some((i, score))
                })
                .max_by(|a, b| a.1.total_cmp(&b.1));
            if let Some((i, _)) = matching {
                used[i] = true;
                hits += 1;
            }
        }
        let precision = if proposals.is_empty() {
            1.0
        } else {
            hits as f64 / proposals.len() as f64
        };
        let recall = if targets.is_empty() {
            1.0
        } else {
            hits as f64 / targets.len() as f64
        };
        eprintln!(
            "{name}: matched={hits}, proposals={}, expected={}, precision={precision:.3}, recall={recall:.3}",
            proposals.len(),
            targets.len()
        );
        if name == "prose" {
            assert!(proposals.is_empty());
        }
        matched += hits;
        proposed += proposals.len();
        expected += targets.len();
    }
    let precision = matched as f64 / proposed.max(1) as f64;
    let recall = matched as f64 / expected.max(1) as f64;
    eprintln!(
        "overall: matched={matched}, proposals={proposed}, expected={expected}, precision={precision:.3}, recall={recall:.3}"
    );
    assert!(precision >= 0.7, "precision {precision}");
    assert!(recall >= 0.8, "recall {recall}");
}

#[test]
fn vector_scan_warns_and_auto_uses_raster_fallback() {
    let Some(lib) = library() else {
        eprintln!("skipping test: ARIAD_TEST_PDFIUM_DIR is not set");
        return;
    };
    let fixture = "fixtures/pdf-tools/scanned-skewed-vi.pdf";
    let vector = run(fixture, FormsDetectMode::Vector, &lib);
    assert!(
        vector["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "no_vector_content")
    );
    let auto = run(fixture, FormsDetectMode::Auto, &lib);
    assert!(
        auto["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "raster_detection")
    );
}
