#![cfg(all(feature = "pdf", feature = "typst"))]

mod support;

use std::{fs, path::Path, process::Command};

use ariad_core::{limits::Limits, reader::pdf, writer::markdown};
use serde_json::Value;
use tempfile::tempdir;

fn pandoc_path() -> std::path::PathBuf {
    if let Some(path) = std::env::var_os("ASHIFT_PANDOC") {
        return std::path::PathBuf::from(path);
    }
    let name = if cfg!(windows) {
        "pandoc.exe"
    } else {
        "pandoc"
    };
    support::fixtures::repository_root()
        .join(".tools/pandoc/bin")
        .join(name)
}

fn engine_probe_path() -> std::path::PathBuf {
    let probe = Path::new(env!("CARGO_BIN_EXE_ashift"))
        .with_file_name(format!("engine-probe{}", std::env::consts::EXE_SUFFIX));
    if probe.exists() {
        return probe;
    }
    let repo_probe = support::fixtures::repository_root().join("target/debug/engine-probe");
    if repo_probe.exists() {
        return repo_probe;
    }
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let mut cmd = std::process::Command::new(cargo);
    cmd.args([
        "build",
        "-p",
        "ariad-host",
        "--bin",
        "engine-probe",
        "--features",
        "ariad-host/test-probe",
    ]);
    if let Some(target_dir) = std::env::var_os("CARGO_TARGET_DIR") {
        cmd.env("CARGO_TARGET_DIR", target_dir);
    }
    let _ = cmd.status();
    if probe.exists() { probe } else { repo_probe }
}

fn locate_libreoffice() -> Option<std::path::PathBuf> {
    match ariad_host::engines::libreoffice::locate() {
        Ok(soffice) => Some(soffice.path),
        Err(error) => {
            eprintln!(
                "skipping: LibreOffice 24.2 or newer is not installed ({error:?}); set ASHIFT_SOFFICE to run this test"
            );
            None
        }
    }
}

fn cli() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ashift"));
    command.env("SOURCE_DATE_EPOCH", "1700000000");
    command.env("ASHIFT_PANDOC", pandoc_path());
    command
}

fn plan(input: &Path, to: &str) -> Value {
    let result = cli()
        .arg("plan")
        .arg(input)
        .args(["--to", to, "--json"])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    serde_json::from_slice(&result.stdout).unwrap()
}

fn convert(input: &Path, to: &str, output: &Path) -> Value {
    let result = cli()
        .arg("convert")
        .arg(input)
        .args(["--to", to, "--json", "-o"])
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

// Stock Ubuntu denies the namespaces sandboxed LibreOffice needs. The default Markdown route
// must not depend on it.
#[cfg(target_os = "linux")]
#[test]
fn default_markdown_to_pdf_never_needs_a_blocked_libreoffice() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempdir().unwrap();
    let soffice = dir.path().join("soffice");
    fs::write(&soffice, "#!/bin/sh\nexit 1\n").unwrap();
    fs::set_permissions(&soffice, fs::Permissions::from_mode(0o755)).unwrap();
    let input = dir.path().join("notes.md");
    fs::write(&input, "# Notes\n\nPlain text.\n").unwrap();
    let output = dir.path().join("notes.pdf");
    let result = cli()
        .env("ASHIFT_SANDBOX", "best-effort")
        .env("ASHIFT_SOFFICE", &soffice)
        .env("ARIAD_TEST_SANDBOX_SUPPORT", "namespace-denied")
        .arg("convert")
        .arg(&input)
        .args(["--to", "pdf", "--json", "-o"])
        .arg(&output)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report: Value = serde_json::from_slice(&result.stdout).unwrap();
    let engines: Vec<_> = report["route"]
        .as_array()
        .unwrap()
        .iter()
        .map(|step| step["engine"].as_str().unwrap())
        .collect();
    assert_eq!(engines, ["ariad-core", "typst"]);
    assert!(pdf::read(&fs::read(output).unwrap(), &Limits::local()).is_ok());
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
            std::fs::canonicalize(report["results"][index]["output"].as_str().unwrap()).unwrap(),
            std::fs::canonicalize(&json).unwrap()
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
        .args(["--to", "pdf", "--json"])
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

#[test]
fn every_pdf_to_text_family_row_is_plannable_and_converts() {
    let root = support::fixtures::repository_root();
    let input = root.join("fixtures/pdf/en-map-room.pdf");
    let dir = tempdir().unwrap();

    let targets = ["docx", "odt", "rtf", "epub", "html", "md", "txt", "json"];
    for target in targets {
        let expected_to = ariad_core::format::Format::from_user_name(target)
            .unwrap()
            .id();
        let plan_report = plan(&input, target);
        let steps = plan_report["route"].as_array().expect("route array");
        assert_eq!(steps.first().unwrap()["from"], "pdf");
        assert_eq!(steps.last().unwrap()["to"], expected_to);

        let output = dir.path().join(format!("en-map-room.{target}"));
        let report = convert(&input, target, &output);
        assert!(output.is_file(), "output file for {target} should exist");
        let len = fs::metadata(&output).unwrap().len();
        assert!(len > 0, "output file for {target} should be non-empty");

        if target == "rtf" {
            let rtf_bytes = fs::read(&output).unwrap();
            assert!(
                rtf_bytes.starts_with(b"{\\rtf1"),
                "RTF output must be a full document starting with {{\\rtf1"
            );
            assert!(
                rtf_bytes.ends_with(b"}\n") || rtf_bytes.ends_with(b"}"),
                "RTF output must be a closed document ending with }}"
            );
        }

        let route = report["route"].as_array().expect("route array");
        assert_eq!(route.first().unwrap()["from"], "pdf");
        assert_eq!(route.last().unwrap()["to"], expected_to);
    }
}

#[test]
fn odt_and_rtf_outputs_open_in_libreoffice() {
    let Some(soffice) = locate_libreoffice() else {
        eprintln!(
            "skipping: LibreOffice 24.2 or newer is not installed; set ASHIFT_SOFFICE to run this test"
        );
        return;
    };
    let root = support::fixtures::repository_root();
    let input = root.join("fixtures/pdf/en-map-room.pdf");
    let dir = tempdir().unwrap();

    let odt_output = dir.path().join("en-map-room.odt");
    convert(&input, "odt", &odt_output);
    assert!(odt_output.is_file());

    let rtf_output = dir.path().join("en-map-room.rtf");
    convert(&input, "rtf", &rtf_output);
    assert!(rtf_output.is_file());

    // Verify both open in LibreOffice and convert to PDF headless into distinct output directories
    let odt_out_dir = dir.path().join("soffice_odt_out");
    fs::create_dir_all(&odt_out_dir).unwrap();

    let result = Command::new(&soffice)
        .args([
            "--headless",
            "--convert-to",
            "pdf",
            "--outdir",
            odt_out_dir.to_str().unwrap(),
        ])
        .arg(&odt_output)
        .output()
        .expect("execute soffice for ODT");
    assert!(
        result.status.success(),
        "soffice failed on ODT: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let odt_pdf = odt_out_dir.join("en-map-room.pdf");
    assert!(odt_pdf.is_file(), "ODT converted PDF should exist");

    let rtf_out_dir = dir.path().join("soffice_rtf_out");
    fs::create_dir_all(&rtf_out_dir).unwrap();

    let result = Command::new(&soffice)
        .args([
            "--headless",
            "--convert-to",
            "pdf",
            "--outdir",
            rtf_out_dir.to_str().unwrap(),
        ])
        .arg(&rtf_output)
        .output()
        .expect("execute soffice for RTF");
    assert!(
        result.status.success(),
        "soffice failed on RTF: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let rtf_pdf = rtf_out_dir.join("en-map-room.pdf");
    assert!(rtf_pdf.is_file(), "RTF converted PDF should exist");

    // Assert converted PDFs have text containing a known string from the fixture
    let extract_text = |pdf_path: &Path| -> String {
        if let Ok(output) = Command::new("pdftotext").arg(pdf_path).arg("-").output()
            && output.status.success()
        {
            return String::from_utf8_lossy(&output.stdout).into_owned();
        }
        let bytes = fs::read(pdf_path).unwrap();
        let doc =
            ariad_core::reader::pdf::read(&bytes, &Limits::local()).expect("read converted PDF");
        let write_out = ariad_core::writer::text::write(&doc.document);
        write_out.content
    };

    let odt_text = extract_text(&odt_pdf);
    assert!(
        odt_text.contains("Map Room"),
        "ODT-converted PDF text must contain fixture string 'Map Room'"
    );

    let rtf_text = extract_text(&rtf_pdf);
    assert!(
        rtf_text.contains("Map Room"),
        "RTF-converted PDF text must contain fixture string 'Map Room'"
    );
}

#[test]
fn odt_package_stamping_honours_source_date_epoch_determinism() {
    let root = support::fixtures::repository_root();
    let input = root.join("fixtures/pdf/en-map-room.pdf");
    let dir = tempdir().unwrap();
    let out1 = dir.path().join("first.odt");
    let out2 = dir.path().join("second.odt");

    convert(&input, "odt", &out1);
    // Sleep across ZIP timestamp resolution boundary (2 seconds) to verify determinism
    std::thread::sleep(std::time::Duration::from_millis(2500));
    convert(&input, "odt", &out2);

    let bytes1 = fs::read(&out1).unwrap();
    let bytes2 = fs::read(&out2).unwrap();
    assert_eq!(
        bytes1, bytes2,
        "ODT output must be byte-identical across runs under SOURCE_DATE_EPOCH"
    );

    // Verify meta.xml contains fixed timestamp from SOURCE_DATE_EPOCH=1700000000 (2023-11-14T22:13:20Z)
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(&bytes1)).unwrap();
    let mut meta = String::new();
    std::io::Read::read_to_string(&mut archive.by_name("meta.xml").unwrap(), &mut meta).unwrap();
    assert!(meta.contains("<meta:creation-date>2023-11-14T22:13:20Z</meta:creation-date>"));
    assert!(meta.contains("<dc:date>2023-11-14T22:13:20Z</dc:date>"));
}

#[test]
fn docling_scan_path_maps_tables_to_ir_tables() {
    let root = support::fixtures::repository_root();
    let docling_fixture = root.join("fixtures/docling/en-map-room.docling.json");
    let input_pdf = root.join("fixtures/pdf/en-map-room.pdf");
    let dir = tempdir().unwrap();

    let ir_output = dir.path().join("en-map-room.docling.ir.json");
    let mut command = cli();
    command.env("ASHIFT_SANDBOX", "off");
    command.env(
        "ASHIFT_DOCLING_ENGINE",
        format!(
            "\"{}\" docling \"{}\"",
            engine_probe_path().display(),
            docling_fixture.display()
        ),
    );
    let result = command
        .args(["convert", "--engine", "docling"])
        .arg(&input_pdf)
        .args(["--to", "json", "--json", "-o"])
        .arg(&ir_output)
        .output()
        .expect("run ashift with docling engine probe");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report["route"][0]["engine"], "docling");
    assert_eq!(report["route"][1]["engine"], "ariad-core");

    let doc: ariad_core::ir::Document =
        serde_json::from_slice(&fs::read(&ir_output).unwrap()).unwrap();
    let mut found_table = false;
    for block in &doc.body {
        if let ariad_core::ir::Block::Table {
            columns,
            head,
            body,
            ..
        } = block
        {
            found_table = true;
            assert!(!columns.is_empty(), "IR table should have columns");
            assert!(
                !head.is_empty() || !body.is_empty(),
                "IR table should have rows"
            );
        }
    }
    assert!(
        found_table,
        "Docling scan path must map tables to IR Block::Table"
    );

    // Also convert to txt and verify table formatting
    let txt_output = dir.path().join("from_docling.txt");
    convert(&ir_output, "txt", &txt_output);
    let txt_content = fs::read_to_string(txt_output).unwrap();
    assert!(
        txt_content.contains('|'),
        "TXT table output must contain pipe delimiters"
    );

    // Optional check for real Docling pack when installed/configured
    if ariad_host::engines::docling::resolve().is_none() {
        eprintln!(
            "skipping: docling pack is not installed or configured; set ASHIFT_PACK_DOCLING or ASHIFT_DOCLING_ENGINE to run this test"
        );
    }
}
