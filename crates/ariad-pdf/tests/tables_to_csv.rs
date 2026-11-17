//! `pdf.tables_to_csv` over the in-process ruled-table detector.

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
fn a_document_without_ruled_tables_has_no_tables() {
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
