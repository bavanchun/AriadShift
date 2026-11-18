//! `pdf.tables_to_csv` and PDF -> XLSX/CSV without the Docling pack: the in-process reader
//! detects tables drawn with ruling lines and tables whose columns are aligned with whitespace.

mod support;

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

use serde_json::Value;
use tempfile::{TempDir, tempdir};
use zip::ZipArchive;

fn pdf(name: &str) -> PathBuf {
    support::fixtures::repository_root()
        .join("fixtures/pdf")
        .join(name)
}

/// A CLI command with no Docling pack reachable from the environment or the data directory.
fn no_docling_cli(data: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ashift"));
    command
        .env("ASHIFT_DATA_DIR", data)
        .env_remove("ASHIFT_PACK_DOCLING")
        .env_remove("ASHIFT_DOCLING_ENGINE")
        .env_remove("ASHIFT_PLAN");
    command
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn run_tool(data: &Path, input: &Path, out_dir: &Path) -> Output {
    no_docling_cli(data)
        .args(["tool", "pdf.tables_to_csv"])
        .arg(input)
        .arg("-o")
        .arg(out_dir)
        .arg("--json")
        .output()
        .unwrap()
}

/// `(cell reference, text or number)` for every cell of an XLSX worksheet in file order, and the
/// merged ranges. The writer's output is deterministic, so plain scanning is enough here.
fn sheet_cells(xlsx: &Path, sheet: &str) -> (Vec<(String, String)>, Vec<String>) {
    let mut archive = ZipArchive::new(fs::File::open(xlsx).unwrap()).unwrap();
    let mut xml = String::new();
    std::io::Read::read_to_string(&mut archive.by_name(sheet).unwrap(), &mut xml).unwrap();

    fn between<'a>(text: &'a str, start: &str, end: &str) -> Option<&'a str> {
        let from = text.find(start)? + start.len();
        let length = text[from..].find(end)?;
        Some(&text[from..from + length])
    }

    let cells = xml
        .split("<c r=\"")
        .skip(1)
        .filter_map(|cell| {
            let (reference, rest) = cell.split_once('"').expect("cell reference");
            // An empty cell with a style is self-closing and has no value.
            let cell = &rest[..rest.find("</c>")?];
            let value = between(cell, "<t xml:space=\"preserve\">", "</t>")
                .or_else(|| between(cell, "<v>", "</v>"))
                .expect("cell value");
            Some((reference.to_owned(), value.to_owned()))
        })
        .collect();
    let merges = xml
        .split("<mergeCell ref=\"")
        .skip(1)
        .map(|merge| merge.split_once('"').expect("merge range").0.to_owned())
        .collect();
    (cells, merges)
}

fn cells(expected: &[(&str, &str)]) -> Vec<(String, String)> {
    expected
        .iter()
        .map(|(reference, text)| ((*reference).to_owned(), (*text).to_owned()))
        .collect()
}

fn report(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).expect("json report")
}

fn warning_messages(output: &Output, code: &str) -> Vec<String> {
    report(output)["warnings"]
        .as_array()
        .expect("warnings array")
        .iter()
        .filter(|warning| warning["code"] == code)
        .map(|warning| warning["message"].as_str().unwrap().to_owned())
        .collect()
}

fn warning_codes(output: &Output) -> Vec<String> {
    let report = report(output);
    report["warnings"]
        .as_array()
        .expect("warnings array")
        .iter()
        .map(|warning| warning["code"].as_str().unwrap().to_owned())
        .collect()
}

fn fresh() -> (TempDir, PathBuf, PathBuf) {
    let directory = tempdir().unwrap();
    let data = directory.path().join("empty-data");
    let out = directory.path().join("out");
    fs::create_dir_all(&out).unwrap();
    (directory, data, out)
}

#[test]
fn tables_to_csv_extracts_ruled_tables_without_docling() {
    let (_directory, data, out) = fresh();
    let output = run_tool(&data, &pdf("ruled-tables.pdf"), &out);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        fs::read_to_string(out.join("ruled-tables.table-001.csv")).unwrap(),
        "Region,Units,Revenue\r\nNorth,120,\"4,500\"\r\nCentral region,95,\"3,210\"\r\nEast,143,\"5,008\"\r\n"
    );
    assert_eq!(
        fs::read_to_string(out.join("ruled-tables.table-002.csv")).unwrap(),
        "Plan,,Actual\r\nTeam A,10,12\r\n,11,13\r\n"
    );
    assert!(
        !warning_codes(&output).contains(&"styles_dropped".to_owned()),
        "CSV has no styling, so that notice would be noise"
    );
    // The built-in reader ran, and the report says so.
    let report = report(&output);
    assert_eq!(report["engine"], "ariad-pdf");
    assert_eq!(
        warning_messages(&output, "docling_fallback"),
        [
            "The Docling engine pack is not installed; tables were detected from ruling lines and from columns aligned with whitespace, and other layouts stay text. Install Docling for layout-aware table detection."
        ]
    );
}

#[test]
fn tables_to_csv_extracts_tables_without_ruling_lines_without_docling() {
    let (_directory, data, out) = fresh();
    let output = run_tool(&data, &pdf("unruled-tables.pdf"), &out);
    assert!(output.status.success(), "{}", stderr(&output));
    let mut files: Vec<_> = fs::read_dir(&out)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    files.sort();
    assert_eq!(
        files,
        [
            "unruled-tables.table-001.csv",
            "unruled-tables.table-002.csv",
            "unruled-tables.table-003.csv"
        ]
    );
    assert_eq!(
        fs::read_to_string(out.join("unruled-tables.table-001.csv")).unwrap(),
        "Instrument,Range,Error,Unit\r\nCaliper,150,0.02,mm\r\nMicrometer,25,0.004,mm\r\nDial gauge,10,0.01,mm\r\nHeight gauge,300,0.03,mm\r\nDepth rod,200,0.05,mm\r\n"
    );
    assert_eq!(
        fs::read_to_string(out.join("unruled-tables.table-002.csv")).unwrap(),
        "Region,Spring,Summer,Autumn\r\nNorth,12,18,\r\nSouth,20,25,14\r\nEast,9,,11\r\nWest,15,22,13\r\n"
    );
    assert_eq!(report(&output)["engine"], "ariad-pdf");
    assert_eq!(
        warning_messages(&output, "docling_fallback"),
        [
            "The Docling engine pack is not installed; tables were detected from ruling lines and from columns aligned with whitespace, and other layouts stay text. Install Docling for layout-aware table detection."
        ]
    );
}

#[test]
fn a_lone_table_is_named_without_a_number() {
    let (_directory, data, out) = fresh();
    let output = run_tool(&data, &pdf("en-map-room.pdf"), &out);
    assert!(output.status.success(), "{}", stderr(&output));
    let files: Vec<_> = fs::read_dir(&out)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(files, ["en-map-room.table.csv"]);
}

#[test]
fn tables_to_csv_without_ruled_tables_fails_like_before() {
    let (_directory, data, out) = fresh();
    let output = run_tool(&data, &pdf("pd-en-gao-08-35-highlights.pdf"), &out);
    assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
    assert!(
        stderr(&output).contains("no tables found in document"),
        "{}",
        stderr(&output)
    );
    assert_eq!(fs::read_dir(&out).unwrap().count(), 0);
}

#[test]
fn tables_to_csv_on_a_scan_still_needs_docling() {
    let (_directory, data, out) = fresh();
    let output = run_tool(&data, &pdf("vi-garden-image-only.pdf"), &out);
    assert_eq!(output.status.code(), Some(5), "{}", stderr(&output));
    assert!(
        stderr(&output).contains("docling engine is not available"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn pdf_to_xlsx_puts_each_ruled_table_on_its_own_sheet_without_docling() {
    let (directory, data, _) = fresh();
    let xlsx = directory.path().join("tables.xlsx");
    let output = no_docling_cli(&data)
        .arg("convert")
        .arg(pdf("ruled-tables.pdf"))
        .args(["--to", "xlsx", "--json", "-o"])
        .arg(&xlsx)
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    let codes = warning_codes(&output);
    assert!(codes.contains(&"docling_fallback".to_owned()), "{codes:?}");
    assert!(!codes.contains(&"no_tables_found".to_owned()), "{codes:?}");

    let (first, first_merges) = sheet_cells(&xlsx, "xl/worksheets/sheet1.xml");
    assert_eq!(
        first,
        cells(&[
            ("A1", "Region"),
            ("B1", "Units"),
            ("C1", "Revenue"),
            ("A2", "North"),
            ("B2", "120"),
            ("C2", "4,500"),
            ("A3", "Central region"),
            ("B3", "95"),
            ("C3", "3,210"),
            ("A4", "East"),
            ("B4", "143"),
            ("C4", "5,008"),
        ])
    );
    assert!(first_merges.is_empty());
    let (second, second_merges) = sheet_cells(&xlsx, "xl/worksheets/sheet2.xml");
    assert_eq!(
        second,
        cells(&[
            ("A1", "Plan"),
            ("C1", "Actual"),
            ("A2", "Team A"),
            ("B2", "10"),
            ("C2", "12"),
            ("B3", "11"),
            ("C3", "13"),
        ])
    );
    assert_eq!(second_merges, ["A1:B1", "A2:A3"]);
    let mut archive = ZipArchive::new(fs::File::open(&xlsx).unwrap()).unwrap();
    assert!(archive.by_name("xl/worksheets/sheet3.xml").is_err());
}

#[test]
fn pdf_to_xlsx_puts_tables_without_ruling_lines_on_their_own_sheets_too() {
    let (directory, data, _) = fresh();
    let xlsx = directory.path().join("unruled.xlsx");
    let output = no_docling_cli(&data)
        .arg("convert")
        .arg(pdf("unruled-tables.pdf"))
        .args(["--to", "xlsx", "--json", "-o"])
        .arg(&xlsx)
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    let codes = warning_codes(&output);
    assert!(codes.contains(&"docling_fallback".to_owned()), "{codes:?}");
    assert!(!codes.contains(&"no_tables_found".to_owned()), "{codes:?}");

    let (second, merges) = sheet_cells(&xlsx, "xl/worksheets/sheet2.xml");
    assert_eq!(
        second,
        cells(&[
            ("A1", "Region"),
            ("B1", "Spring"),
            ("C1", "Summer"),
            ("D1", "Autumn"),
            ("A2", "North"),
            ("B2", "12"),
            ("C2", "18"),
            ("A3", "South"),
            ("B3", "20"),
            ("C3", "25"),
            ("D3", "14"),
            ("A4", "East"),
            ("B4", "9"),
            ("D4", "11"),
            ("A5", "West"),
            ("B5", "15"),
            ("C5", "22"),
            ("D5", "13"),
        ])
    );
    assert!(merges.is_empty());
    let mut archive = ZipArchive::new(fs::File::open(&xlsx).unwrap()).unwrap();
    assert!(archive.by_name("xl/worksheets/sheet3.xml").is_ok());
    assert!(archive.by_name("xl/worksheets/sheet4.xml").is_err());

    // Once tables are found, only the tables are kept: no sentence, title or caption of the page
    // appears anywhere in the workbook. The table cells do appear, so the scan reads real text.
    let mut workbook = String::new();
    for index in 0..archive.len() {
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut archive.by_index(index).unwrap(), &mut bytes).unwrap();
        workbook.push_str(&String::from_utf8_lossy(&bytes));
    }
    assert!(workbook.contains("Caliper") && workbook.contains("Gamma"));
    for prose in [
        "The committee reviewed the proposal and noted that every district would receive a share",
        "Members asked for a clearer account",
        "Measurement summary",
        "Table 1: Instruments and their stated error.",
        "Two columns",
    ] {
        assert!(
            !workbook.contains(prose),
            "prose kept in the workbook: {prose}"
        );
    }
}

#[test]
fn pdf_to_xlsx_without_ruled_tables_is_still_a_text_dump() {
    let (directory, data, _) = fresh();
    let xlsx = directory.path().join("prose.xlsx");
    let output = no_docling_cli(&data)
        .arg("convert")
        .arg(pdf("pd-en-gao-08-35-highlights.pdf"))
        .args(["--to", "xlsx", "--json", "-o"])
        .arg(&xlsx)
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(
        warning_codes(&output).contains(&"no_tables_found".to_owned()),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let (cells, merges) = sheet_cells(&xlsx, "xl/worksheets/sheet1.xml");
    assert!(!cells.is_empty() && merges.is_empty());
}

#[test]
fn pdf_to_csv_keeps_ruled_tables_apart() {
    let (directory, data, _) = fresh();
    let csv = directory.path().join("tables.csv");
    let output = no_docling_cli(&data)
        .arg("convert")
        .arg(pdf("ruled-tables.pdf"))
        .args(["--to", "csv", "-o"])
        .arg(&csv)
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        fs::read_to_string(&csv).unwrap(),
        "Region,Units,Revenue\r\nNorth,120,\"4,500\"\r\nCentral region,95,\"3,210\"\r\nEast,143,\"5,008\"\r\n\r\n\
         Plan,,Actual\r\nTeam A,10,12\r\n,11,13\r\n"
    );
}

#[test]
fn pdf_to_csv_keeps_tables_without_ruling_lines_apart() {
    let (directory, data, _) = fresh();
    let csv = directory.path().join("unruled.csv");
    let output = no_docling_cli(&data)
        .arg("convert")
        .arg(pdf("unruled-tables.pdf"))
        .args(["--to", "csv", "-o"])
        .arg(&csv)
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    // One block per detected table, a blank line between them, and nothing of the prose.
    let text = fs::read_to_string(&csv).unwrap();
    assert_eq!(
        text,
        "Instrument,Range,Error,Unit\r\nCaliper,150,0.02,mm\r\nMicrometer,25,0.004,mm\r\nDial gauge,10,0.01,mm\r\nHeight gauge,300,0.03,mm\r\nDepth rod,200,0.05,mm\r\n\r\n\
         Region,Spring,Summer,Autumn\r\nNorth,12,18,\r\nSouth,20,25,14\r\nEast,9,,11\r\nWest,15,22,13\r\n\r\n\
         Name,Size,Speed,Score\r\nAlpha,12,340,0.91\r\nBeta,48,210,0.87\r\nGamma,96,120,0.95\r\nDelta,7,410,0.78\r\n"
    );
    assert_eq!(
        text.matches("\r\n\r\n").count(),
        2,
        "three tables, two separators"
    );
}

/// `pdf.tables_to_csv` through the CLI is the route that extracts one file per table; the
/// conversion to XLSX is the route that falls back to a text dump. Both must find no table on a
/// PDF whose pages only look tabular.
#[test]
fn layouts_that_only_look_like_tables_give_no_tables_through_the_cli() {
    let (directory, data, out) = fresh();
    let output = run_tool(&data, &pdf("unruled-negatives.pdf"), &out);
    assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
    assert!(
        stderr(&output).contains("no tables found in document"),
        "{}",
        stderr(&output)
    );
    assert_eq!(fs::read_dir(&out).unwrap().count(), 0);

    let xlsx = directory.path().join("negatives.xlsx");
    let output = no_docling_cli(&data)
        .arg("convert")
        .arg(pdf("unruled-negatives.pdf"))
        .args(["--to", "xlsx", "--json", "-o"])
        .arg(&xlsx)
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(
        warning_codes(&output).contains(&"no_tables_found".to_owned()),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    // The text dump is one sheet without merged cells.
    let (dump, merges) = sheet_cells(&xlsx, "xl/worksheets/sheet1.xml");
    assert!(!dump.is_empty() && merges.is_empty());
    let mut archive = ZipArchive::new(fs::File::open(&xlsx).unwrap()).unwrap();
    assert!(archive.by_name("xl/worksheets/sheet2.xml").is_err());
}

#[test]
fn docling_wins_over_the_in_process_detector_when_it_is_installed() {
    let directory = tempdir().unwrap();
    let out = directory.path().join("out");
    fs::create_dir_all(&out).unwrap();
    let docling_fixture =
        support::fixtures::repository_root().join("fixtures/docling/en-map-room.docling.json");
    let output = no_docling_cli(&directory.path().join("empty-data"))
        .env("ASHIFT_SANDBOX", "off")
        .env(
            "ASHIFT_DOCLING_ENGINE",
            format!(
                "\"{}\" docling \"{}\"",
                support::engine_probe_path().display(),
                docling_fixture.display()
            ),
        )
        .args(["tool", "pdf.tables_to_csv"])
        .arg(pdf("ruled-tables.pdf"))
        .arg("-o")
        .arg(&out)
        .arg("--json")
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    // Docling ran, so the report names it and carries no built-in reader notice.
    assert_eq!(report(&output)["engine"], "docling");
    assert!(
        !warning_codes(&output).contains(&"docling_fallback".to_owned()),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let files: Vec<_> = fs::read_dir(&out)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(files, ["ruled-tables.table.csv"], "one Docling table");
    let csv = fs::read_to_string(out.join("ruled-tables.table.csv")).unwrap();
    assert!(csv.contains("Archive item"), "{csv}");
    assert!(
        !csv.contains("Region"),
        "the ruled-table detector must not run: {csv}"
    );
}
