//! Integration tests for `pdf.compare` CLI tool.

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};
use tempfile::TempDir;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .into()
}

fn command() -> Command {
    Command::new(env!("CARGO_BIN_EXE_ashift"))
}

fn pdfium_pack_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("ASHIFT_PACK_PDFIUM") {
        let path = PathBuf::from(dir);
        if path.join("pack.toml").is_file() {
            return Some(path);
        }
    }
    let coord = PathBuf::from("/home/vchun/Codes/01-AriadShift/.run/coord/pdfium-pack");
    if coord.join("pack.toml").is_file() {
        return Some(coord);
    }
    let candidate = root().join("target/packs/pdfium");
    if candidate.join("pack.toml").is_file() {
        return Some(candidate);
    }
    None
}

#[test]
fn text_mode_runs_in_process_and_detects_seeded_changes() {
    let before = root().join("fixtures/pdf-tools/compare-before.pdf");
    let after = root().join("fixtures/pdf-tools/compare-after.pdf");

    let output = command()
        .args([
            "tool",
            "pdf.compare",
            before.to_str().unwrap(),
            after.to_str().unwrap(),
            "--param",
            "mode=text",
        ])
        .output()
        .expect("run ashift tool pdf.compare");

    assert!(
        output.status.success(),
        "pdf.compare text mode failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout_str = String::from_utf8_lossy(&output.stdout);
    let report: serde_json::Value =
        serde_json::from_str(&stdout_str).expect("valid JSON stdout report");

    let summary = &report["summary"];
    assert_eq!(summary["pages_before"], 2);
    assert_eq!(summary["pages_after"], 3);
    assert_eq!(summary["pages_compared"], 3);
    assert_eq!(summary["pages_identical"], 1, "page 2 must be identical");
    assert_eq!(summary["pages_changed"], 2, "pages 1 and 3 must be changed");
    assert!(
        summary["text_changes"].as_u64().unwrap_or(0) > 0,
        "text changes must be found"
    );

    let pages = report["pages"].as_array().expect("pages array");
    assert_eq!(pages.len(), 3);

    // Page 1: changed with replace, delete, move
    let p1 = &pages[0];
    assert_eq!(p1["page_number"], 1);
    assert_eq!(p1["status"], "changed");
    let p1_changes = p1["text_changes"].as_array().expect("p1 text changes");
    assert!(!p1_changes.is_empty());

    let kinds: Vec<&str> = p1_changes
        .iter()
        .filter_map(|c| c["kind"].as_str())
        .collect();
    assert!(
        kinds.contains(&"replace"),
        "must detect text edit (replace): {kinds:?}"
    );
    assert!(
        kinds.contains(&"delete"),
        "must detect deleted text: {kinds:?}"
    );
    assert!(kinds.contains(&"move"), "must detect moved text: {kinds:?}");

    for change in p1_changes {
        let bbox = change["bbox"].as_array().expect("bbox array");
        assert_eq!(bbox.len(), 4, "bbox must have 4 coordinates");
        let w = bbox[2].as_f64().unwrap();
        let h = bbox[3].as_f64().unwrap();
        assert!(w > 0.0 && h > 0.0, "bbox dimensions must be positive");
    }

    // Page 2: unchanged
    let p2 = &pages[1];
    assert_eq!(p2["page_number"], 2);
    assert_eq!(p2["status"], "identical");
    assert!(p2["text_changes"].as_array().unwrap().is_empty());
    assert!(p2["visual_changes"].as_array().unwrap().is_empty());

    // Page 3: added
    let p3 = &pages[2];
    assert_eq!(p3["page_number"], 3);
    assert_eq!(p3["status"], "added");
    assert!(p3["page_before"].is_null());
    assert_eq!(p3["page_after"], 3);
}

#[test]
fn output_target_single_pdf_and_json_and_directory() {
    let before = root().join("fixtures/pdf-tools/compare-before.pdf");
    let after = root().join("fixtures/pdf-tools/compare-after.pdf");
    let temp = TempDir::new().unwrap();

    // 1. Single PDF output: writes annotated PDF
    let out_pdf = temp.path().join("report.pdf");
    let output = command()
        .args([
            "tool",
            "pdf.compare",
            before.to_str().unwrap(),
            after.to_str().unwrap(),
            "-o",
            out_pdf.to_str().unwrap(),
        ])
        .output()
        .expect("run compare with -o report.pdf");
    assert!(
        output.status.success(),
        "single pdf output failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(out_pdf.is_file(), "report.pdf must be created");

    // Verify annotated PDF has annotations on page 1 with lopdf
    let doc = lopdf::Document::load(&out_pdf).expect("load report.pdf");
    let pages = doc.get_pages();
    let page_1_id = pages.get(&1).expect("page 1 exists");
    let page_1_obj = doc.get_object(*page_1_id).expect("page 1 obj");
    let page_1_dict = page_1_obj.as_dict().expect("page 1 dict");
    let annots = page_1_dict
        .get(b"Annots")
        .expect("page 1 has Annots dictionary entry");
    let annot_array = match annots {
        lopdf::Object::Array(arr) => arr.clone(),
        lopdf::Object::Reference(r) => doc.get_object(*r).unwrap().as_array().unwrap().clone(),
        _ => panic!("unexpected Annots object type"),
    };
    assert!(
        !annot_array.is_empty(),
        "page 1 must contain change annotations"
    );

    // 2. Single JSON output: writes report.json
    let out_json = temp.path().join("report.json");
    let output = command()
        .args([
            "tool",
            "pdf.compare",
            before.to_str().unwrap(),
            after.to_str().unwrap(),
            "-o",
            out_json.to_str().unwrap(),
        ])
        .output()
        .expect("run compare with -o report.json");
    assert!(
        output.status.success(),
        "single json output failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(out_json.is_file(), "report.json must be created");
    let json_content = fs::read_to_string(&out_json).unwrap();
    let parsed: serde_json::Value =
        serde_json::from_str(&json_content).expect("valid report.json content");
    assert_eq!(parsed["summary"]["pages_identical"], 1);

    // 3. Directory output: writes both .compared.json and .compared.pdf
    let out_dir = temp.path().join("out_dir");
    fs::create_dir_all(&out_dir).unwrap();
    let output = command()
        .args([
            "tool",
            "pdf.compare",
            before.to_str().unwrap(),
            after.to_str().unwrap(),
            "-o",
            out_dir.to_str().unwrap(),
        ])
        .output()
        .expect("run compare with -o out_dir/");
    assert!(
        output.status.success(),
        "dir output failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let published_json = out_dir.join("compare-before.compared.json");
    let published_pdf = out_dir.join("compare-before.compared.pdf");
    assert!(
        published_json.is_file(),
        "compare-before.compared.json must be published"
    );
    assert!(
        published_pdf.is_file(),
        "compare-before.compared.pdf must be published"
    );
}

#[test]
fn visual_mode_with_pdfium_detects_image_tint() {
    let pack_dir = match pdfium_pack_dir() {
        Some(p) => p,
        None => {
            if std::env::var_os("REQUIRE_PDFIUM").is_some() {
                panic!("REQUIRE_PDFIUM set but no PDFium pack found");
            }
            eprintln!("skipping: visual mode test requires pdfium pack");
            return;
        }
    };

    let before = root().join("fixtures/pdf-tools/compare-before.pdf");
    let after = root().join("fixtures/pdf-tools/compare-after.pdf");

    let output = command()
        .env("ASHIFT_PACK_PDFIUM", &pack_dir)
        .args([
            "tool",
            "pdf.compare",
            before.to_str().unwrap(),
            after.to_str().unwrap(),
            "--param",
            "mode=visual",
        ])
        .output()
        .expect("run ashift tool pdf.compare mode=visual");

    assert!(
        output.status.success(),
        "pdf.compare visual mode failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout_str = String::from_utf8_lossy(&output.stdout);
    let report: serde_json::Value =
        serde_json::from_str(&stdout_str).expect("valid JSON stdout report");

    let summary = &report["summary"];
    assert_eq!(summary["pages_before"], 2);
    assert_eq!(summary["pages_after"], 3);
    assert_eq!(summary["pages_compared"], 3);
    assert_eq!(summary["pages_identical"], 1, "page 2 must be identical");
    assert_eq!(summary["pages_changed"], 2);
    assert!(
        summary["visual_changes"].as_u64().unwrap_or(0) > 0,
        "visual changes must be detected for image tint"
    );

    let pages = report["pages"].as_array().expect("pages array");
    let p1 = &pages[0];
    assert_eq!(p1["page_number"], 1);
    assert_eq!(p1["status"], "changed");
    let p1_vis = p1["visual_changes"].as_array().expect("p1 visual changes");
    assert!(!p1_vis.is_empty(), "image tint change must be found");

    let p2 = &pages[1];
    assert_eq!(p2["page_number"], 2);
    assert_eq!(p2["status"], "identical");
    assert!(p2["visual_changes"].as_array().unwrap().is_empty());

    let p3 = &pages[2];
    assert_eq!(p3["page_number"], 3);
    assert_eq!(p3["status"], "added");
}

#[test]
fn visual_mode_without_pdfium_fails_with_exit_5() {
    let before = root().join("fixtures/pdf-tools/compare-before.pdf");
    let after = root().join("fixtures/pdf-tools/compare-after.pdf");
    let temp = TempDir::new().unwrap();
    let empty_data = temp.path().join("empty-data");
    fs::create_dir_all(&empty_data).unwrap();

    let output = command()
        .env("ASHIFT_DATA_DIR", &empty_data)
        .env_remove("ASHIFT_PACK_PDFIUM")
        .env_remove("ARIAD_TEST_PDFIUM_DIR")
        .args([
            "tool",
            "pdf.compare",
            before.to_str().unwrap(),
            after.to_str().unwrap(),
            "--param",
            "mode=visual",
        ])
        .output()
        .expect("run compare without pdfium pack");

    assert_eq!(
        output.status.code(),
        Some(5),
        "visual mode without pack must exit with code 5; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("pdfium") && stderr.contains("ashift engines install pdfium"),
        "stderr must name pdfium and its install hint, got: {stderr}"
    );
}

#[test]
fn wrong_input_arity_is_rejected_with_exit_10() {
    let before = root().join("fixtures/pdf-tools/compare-before.pdf");

    // 1 input
    let out_1 = command()
        .args(["tool", "pdf.compare", before.to_str().unwrap()])
        .output()
        .expect("run compare with 1 input");
    assert_eq!(
        out_1.status.code(),
        Some(10),
        "1 input must be rejected with exit code 10; stderr: {}",
        String::from_utf8_lossy(&out_1.stderr)
    );

    // 3 inputs
    let out_3 = command()
        .args([
            "tool",
            "pdf.compare",
            before.to_str().unwrap(),
            before.to_str().unwrap(),
            before.to_str().unwrap(),
        ])
        .output()
        .expect("run compare with 3 inputs");
    assert_eq!(
        out_3.status.code(),
        Some(10),
        "3 inputs must be rejected with exit code 10; stderr: {}",
        String::from_utf8_lossy(&out_3.stderr)
    );
}
