//! `pdf.tables_to_csv` over the in-process table detector.

use std::path::{Path, PathBuf};

use ariad_pdf::{
    PdfError, ToolCtx,
    tools::{InputDoc, ToolOutput, run_tool},
};
use serde_json::json;

fn fixture(name: &str) -> Vec<u8> {
    let path: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/pdf")
        .join(name);
    std::fs::read(path).expect("read fixture")
}

fn run(name: &str, stem: &str) -> Result<ToolOutput, PdfError> {
    run_tool(
        "pdf.tables_to_csv",
        &[InputDoc::new(stem, fixture(name))],
        &json!({}),
        &ToolCtx::new(),
    )
}

#[test]
fn ruled_tables_become_one_csv_each() {
    let output = run("ruled-tables.pdf", "ruled-tables.pdf").unwrap();
    let files: Vec<(&str, &str, String)> = output
        .artifacts
        .iter()
        .map(|artifact| {
            (
                artifact.name.as_str(),
                artifact.format.as_str(),
                String::from_utf8(artifact.bytes.clone()).unwrap(),
            )
        })
        .collect();
    assert_eq!(
        files,
        [
            (
                "ruled-tables.table-001.csv",
                "csv",
                "Region,Units,Revenue\r\nNorth,120,\"4,500\"\r\nCentral region,95,\"3,210\"\r\nEast,143,\"5,008\"\r\n"
                    .to_owned()
            ),
            (
                "ruled-tables.table-002.csv",
                "csv",
                "Plan,,Actual\r\nTeam A,10,12\r\n,11,13\r\n".to_owned()
            ),
        ]
    );
    // CSV carries no styling, so the reader's styles notice must not leak into the report.
    assert!(
        output
            .coded_warnings
            .iter()
            .all(|warning| warning.code != "styles_dropped"),
        "{:?}",
        output.coded_warnings
    );
}

#[test]
fn a_lone_table_is_named_without_a_number() {
    let output = run("en-map-room.pdf", "en-map-room.pdf").unwrap();
    let names: Vec<&str> = output.artifacts.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(names, ["en-map-room.table.csv"]);
}

#[test]
fn the_built_in_reader_says_so_in_a_warning() {
    let output = run("ruled-tables.pdf", "ruled-tables.pdf").unwrap();
    let fallback: Vec<&str> = output
        .coded_warnings
        .iter()
        .filter(|warning| warning.code == "docling_fallback")
        .map(|warning| warning.message.as_str())
        .collect();
    assert_eq!(
        fallback,
        [
            "The Docling engine pack is not installed; tables were detected from ruling lines and from columns aligned with whitespace, and other layouts stay text. Install Docling for layout-aware table detection."
        ]
    );
}

#[test]
fn tables_without_ruling_lines_become_one_csv_each_too() {
    let output = run("unruled-tables.pdf", "unruled-tables.pdf").unwrap();
    let files: Vec<(&str, String)> = output
        .artifacts
        .iter()
        .map(|artifact| {
            (
                artifact.name.as_str(),
                String::from_utf8(artifact.bytes.clone()).unwrap(),
            )
        })
        .collect();
    assert_eq!(
        files,
        [
            (
                "unruled-tables.table-001.csv",
                "Instrument,Range,Error,Unit\r\nCaliper,150,0.02,mm\r\nMicrometer,25,0.004,mm\r\nDial gauge,10,0.01,mm\r\nHeight gauge,300,0.03,mm\r\nDepth rod,200,0.05,mm\r\n"
                    .to_owned()
            ),
            (
                "unruled-tables.table-002.csv",
                "Region,Spring,Summer,Autumn\r\nNorth,12,18,\r\nSouth,20,25,14\r\nEast,9,,11\r\nWest,15,22,13\r\n"
                    .to_owned()
            ),
            (
                "unruled-tables.table-003.csv",
                "Name,Size,Speed,Score\r\nAlpha,12,340,0.91\r\nBeta,48,210,0.87\r\nGamma,96,120,0.95\r\nDelta,7,410,0.78\r\n"
                    .to_owned()
            ),
        ]
    );
}

#[test]
fn layouts_that_only_look_like_tables_are_not_extracted() {
    // Every page is aligned text that is not a table: a list of contents, a form, code, a
    // bibliography, a word list, a Yes and No matrix. None of them may become a CSV file.
    let error = run("unruled-negatives.pdf", "unruled-negatives.pdf").unwrap_err();
    assert_eq!(error, PdfError::NoTables);
}

#[test]
fn a_scan_needs_the_docling_engine() {
    let error = run("vi-garden-image-only.pdf", "vi-garden-image-only.pdf").unwrap_err();
    assert_eq!(error, PdfError::EngineRequired { engine: "docling" });
    assert_eq!(error.exit_code(), 5);
    assert_eq!(error.cli_error_code(), "tool_missing");
}

#[test]
fn a_document_without_tables_has_no_tables() {
    let error = run(
        "pd-en-gao-08-35-highlights.pdf",
        "pd-en-gao-08-35-highlights.pdf",
    )
    .unwrap_err();
    assert_eq!(error, PdfError::NoTables);
    assert_eq!(error.to_string(), "no tables found in document");
}

#[test]
fn unreadable_input_is_malformed() {
    let error = run_tool(
        "pdf.tables_to_csv",
        &[InputDoc::new("broken.pdf", b"not a pdf".to_vec())],
        &json!({}),
        &ToolCtx::new(),
    )
    .unwrap_err();
    assert_eq!(error, PdfError::Malformed);
}

#[test]
fn parameters_and_input_count_are_validated() {
    let input = || InputDoc::new("a.pdf", fixture("ruled-tables.pdf"));
    let unknown = run_tool(
        "pdf.tables_to_csv",
        &[input()],
        &json!({"unknown": 1}),
        &ToolCtx::new(),
    )
    .unwrap_err();
    assert!(matches!(unknown, PdfError::InvalidParams(_)), "{unknown:?}");
    let two = run_tool(
        "pdf.tables_to_csv",
        &[input(), input()],
        &json!({}),
        &ToolCtx::new(),
    )
    .unwrap_err();
    assert!(matches!(two, PdfError::InvalidParams(_)), "{two:?}");
}
