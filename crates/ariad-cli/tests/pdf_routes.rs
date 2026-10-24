#![cfg(all(feature = "pdf", feature = "typst"))]

mod support;

use std::{fs, path::Path, process::Command};

use ariad_core::{limits::Limits, reader::pdf, writer::markdown};
use serde_json::Value;
use tempfile::tempdir;

fn cli() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ashift"));
    command.env("SOURCE_DATE_EPOCH", "1700000000");
    command
}

fn convert(input: &Path, to: &str, output: &Path) -> Value {
    let result = cli()
        .arg("convert")
        .arg(input)
        .args(["--to", to, "--profile", "fast", "--json", "-o"])
        .arg(output)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    serde_json::from_slice(&result.stdout).unwrap()
}

#[test]
fn generated_digital_pdfs_convert_to_markdown_goldens() {
    let root = support::fixtures::repository_root();
    let dir = tempdir().unwrap();
    for name in ["vi-garden-route", "en-map-room"] {
        let output = dir.path().join(format!("{name}.md"));
        let report = convert(
            &root.join(format!("fixtures/pdf/{name}.pdf")),
            "md",
            &output,
        );
        assert_eq!(report["route"][0]["engine"], "pdf");
        assert_eq!(report["route"][1]["engine"], "ariad-core");
        let text = fs::read_to_string(output).unwrap();
        insta::assert_snapshot!(name, text);
    }
}

#[test]
fn markdown_to_pdf_reads_back_with_vietnamese_text_and_fixed_dates() {
    let dir = tempdir().unwrap();
    let input = dir.path().join("notes.md");
    fs::write(
        &input,
        "# Khu vườn đọc sách\n\nTưới cây vào sáng thứ bảy.\n\nBến sông và mái hiên.\n",
    )
    .unwrap();
    let first = dir.path().join("first.pdf");
    let second = dir.path().join("second.pdf");
    let report = convert(&input, "pdf", &first);
    assert_eq!(report["route"][1]["engine"], "typst");
    convert(&input, "pdf", &second);
    let bytes = fs::read(first).unwrap();
    assert_eq!(bytes, fs::read(second).unwrap());
    let read = pdf::read(&bytes, &Limits::local()).unwrap();
    let stats = pdf::PdfStats::from_document(&read.document).unwrap();
    assert_eq!(
        (stats.pages, stats.text_pages, stats.image_only_pages),
        (1, 1, 0)
    );
    assert!(!stats.needs_ocr());
    let text = markdown::write(&read.document).content;
    for expected in [
        "Khu vườn đọc sách",
        "Tưới cây vào sáng thứ bảy.",
        "Bến sông và mái hiên.",
    ] {
        assert!(text.contains(expected), "missing {expected}: {text}");
    }
    assert!(bytes.windows(10).any(|window| window == b"D:20231114"));
}

#[test]
fn inspect_image_only_pdf_reports_ocr_and_page_facts() {
    let root = support::fixtures::repository_root();
    let result = cli()
        .arg("inspect")
        .arg(root.join("fixtures/pdf/vi-garden-image-only.pdf"))
        .arg("--json")
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report["page_count"], 1);
    assert_eq!(report["pdf"]["pages"], 1);
    assert_eq!(report["pdf"]["text_pages"], 0);
    assert_eq!(report["pdf"]["image_only_pages"], 1);
    assert_eq!(report["pdf"]["needs_ocr"], true);
    assert!(
        report["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|warning| warning["code"] == "ocr_required")
    );
}

#[test]
fn json_is_a_document_target_distinct_from_the_json_report_flag() {
    let dir = tempdir().unwrap();
    let input = dir.path().join("notes.md");
    fs::write(&input, "# A heading\n\nIR round trip.\n").unwrap();
    let json = dir.path().join("notes.json");
    let report = convert(&input, "json", &json);
    assert_eq!(report["route"].as_array().unwrap().len(), 1);
    let document: Value = serde_json::from_slice(&fs::read(&json).unwrap()).unwrap();
    assert_eq!(document["version"], ariad_core::ir::IR_VERSION);
    let md = dir.path().join("round-trip.md");
    convert(&json, "md", &md);
    assert_eq!(
        fs::read_to_string(md).unwrap(),
        "# A heading\n\nIR round trip.\n\n"
    );
}

#[test]
fn batch_pdf_routes_use_the_shared_executor_and_canonical_extensions() {
    let root = support::fixtures::repository_root();
    let directory = tempdir().unwrap();
    let names = ["vi-garden-route", "en-map-room"];
    let pdfs: Vec<_> = names
        .iter()
        .map(|name| root.join(format!("fixtures/pdf/{name}.pdf")))
        .collect();
    let result = cli()
        .arg("convert")
        .args(&pdfs)
        .args(["--to", "json", "--json", "-o"])
        .arg(directory.path())
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report: Value = serde_json::from_slice(&result.stdout).unwrap();
    for (index, name) in names.iter().enumerate() {
        assert_eq!(report["results"][index]["ok"], true);
        assert_eq!(report["results"][index]["route"][0]["engine"], "pdf");
        let json = directory.path().join(format!("{name}.json"));
        assert_eq!(
            report["results"][index]["output"],
            json.to_string_lossy().as_ref()
        );
        assert!(json.is_file());
    }
    let jsons: Vec<_> = names
        .iter()
        .map(|name| directory.path().join(format!("{name}.json")))
        .collect();
    let result = cli()
        .arg("convert")
        .args(jsons)
        .args(["--to", "pdf", "--profile", "fast", "--json"])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report: Value = serde_json::from_slice(&result.stdout).unwrap();
    for (index, name) in names.iter().enumerate() {
        assert_eq!(report["results"][index]["ok"], true);
        assert_eq!(report["results"][index]["route"][0]["engine"], "typst");
        let bytes = fs::read(directory.path().join(format!("{name}.pdf"))).unwrap();
        assert!(pdf::read(&bytes, &Limits::local()).is_ok());
    }
}
