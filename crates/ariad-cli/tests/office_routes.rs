//! Office and ODF routes through the system LibreOffice.
//!
//! Tests that need LibreOffice skip with a message when no supported `soffice` is installed;
//! the Linux CI job installs it so they run there.

mod support;

use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Output},
};

use serde_json::Value;
use tempfile::tempdir;
use zip::ZipArchive;

fn cli() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ashift"));
    command
        .env("ASHIFT_PANDOC", pandoc_path())
        .env("SOURCE_DATE_EPOCH", "1700000000");
    command
}

fn pandoc_path() -> PathBuf {
    if let Some(path) = std::env::var_os("ASHIFT_PANDOC") {
        return PathBuf::from(path);
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

fn fixture(name: &str) -> PathBuf {
    support::fixtures::repository_root()
        .join("fixtures/office")
        .join(name)
}

fn libreoffice_available() -> bool {
    match ariad_host::engines::libreoffice::locate() {
        Ok(_) => true,
        Err(error) => {
            eprintln!(
                "skipping: LibreOffice 24.2 or newer is not installed ({error:?}); set ASHIFT_SOFFICE to run this test"
            );
            false
        }
    }
}

fn convert_json(input: &Path, target: &str, output: &Path) -> Output {
    cli()
        .arg("convert")
        .arg(input)
        .args(["--to", target, "--json", "-o"])
        .arg(output)
        .output()
        .expect("start ashift")
}

fn route_of(output: &Output) -> Vec<String> {
    assert!(
        output.status.success(),
        "conversion failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).expect("convert --json output");
    report["route"]
        .as_array()
        .expect("route array")
        .iter()
        .map(|step| {
            format!(
                "{}-{}->{}",
                step["from"].as_str().unwrap(),
                step["engine"].as_str().unwrap(),
                step["to"].as_str().unwrap()
            )
        })
        .collect()
}

fn zip_has(path: &Path, entry: &str) -> bool {
    let file = fs::File::open(path).expect("open output");
    let mut archive = ZipArchive::new(file).expect("output is a ZIP package");
    archive.by_name(entry).is_ok()
}

#[test]
fn legacy_word_documents_reach_markdown_through_docx() {
    if !libreoffice_available() {
        return;
    }
    let directory = tempdir().unwrap();
    for name in [
        "vi-garden-notes.doc",
        "vi-garden-notes.rtf",
        "vi-garden-notes.odt",
    ] {
        let output = directory.path().join(format!("{name}.md"));
        let result = convert_json(&fixture(name), "md", &output);
        let extension = name.rsplit('.').next().unwrap();
        assert_eq!(
            route_of(&result),
            [
                format!("{extension}-libreoffice->docx"),
                "docx-pandoc->ariad-ir+json".to_owned(),
                "ariad-ir+json-ariad-core->markdown".to_owned(),
            ],
            "{name}"
        );
        let markdown = fs::read_to_string(&output).unwrap();
        assert!(
            markdown.contains("# Khu vườn đọc sách"),
            "{name}: {markdown}"
        );
        assert!(markdown.contains("## Lịch chăm cây"), "{name}: {markdown}");
        assert!(markdown.contains("Tưới cây vào sáng thứ bảy"), "{name}");
        assert!(markdown.contains("Bến sông"), "{name}");
        assert!(markdown.contains("mái hiên"), "{name}");
    }
}

#[test]
fn legacy_and_odf_documents_convert_to_ooxml() {
    if !libreoffice_available() {
        return;
    }
    let directory = tempdir().unwrap();
    let cases = [
        ("vi-garden-notes.doc", "docx", "word/document.xml"),
        ("vi-garden-notes.rtf", "docx", "word/document.xml"),
        ("vi-garden-notes.odt", "docx", "word/document.xml"),
        ("vi-route-sheet.xls", "xlsx", "xl/workbook.xml"),
        ("vi-route-sheet.ods", "xlsx", "xl/workbook.xml"),
        ("en-map-room-slides.ppt", "pptx", "ppt/presentation.xml"),
        ("en-map-room-slides.odp", "pptx", "ppt/presentation.xml"),
    ];
    for (name, target, part) in cases {
        let output = directory.path().join(format!("{name}.{target}"));
        let result = convert_json(&fixture(name), target, &output);
        let extension = name.rsplit('.').next().unwrap();
        assert_eq!(
            route_of(&result),
            [format!("{extension}-libreoffice->{target}")],
            "{name}"
        );
        assert!(zip_has(&output, part), "{name} -> {target} lacks {part}");
    }
}

#[test]
fn office_and_odf_documents_convert_to_pdf_directly() {
    if !libreoffice_available() {
        return;
    }
    let directory = tempdir().unwrap();
    for name in [
        "vi-garden-notes.doc",
        "vi-garden-notes.odt",
        "vi-route-sheet.xlsx",
        "vi-route-sheet.ods",
        "en-map-room-slides.pptx",
        "en-map-room-slides.odp",
    ] {
        let output = directory.path().join(format!("{name}.pdf"));
        let result = convert_json(&fixture(name), "pdf", &output);
        let extension = name.rsplit('.').next().unwrap();
        assert_eq!(
            route_of(&result),
            [format!("{extension}-libreoffice->pdf")],
            "{name}"
        );
        let bytes = fs::read(&output).unwrap();
        assert!(bytes.starts_with(b"%PDF-"), "{name} did not produce a PDF");
    }
}

#[test]
fn a_missing_libreoffice_names_the_engine_and_docx_falls_back_to_typst() {
    let directory = tempdir().unwrap();
    let missing = directory.path().join("no-soffice-here");

    let slides = cli()
        .env("ASHIFT_SOFFICE", &missing)
        .arg("convert")
        .arg(fixture("en-map-room-slides.pptx"))
        .args(["--to", "pdf", "-o"])
        .arg(directory.path().join("slides.pdf"))
        .output()
        .unwrap();
    assert_eq!(slides.status.code(), Some(5));
    let stderr = String::from_utf8_lossy(&slides.stderr);
    assert!(stderr.contains("libreoffice"), "{stderr}");
    assert!(stderr.contains("ASHIFT_SOFFICE"), "{stderr}");
    assert!(!directory.path().join("slides.pdf").exists());

    let report = cli()
        .env("ASHIFT_SOFFICE", &missing)
        .arg("convert")
        .arg(support::fixtures::repository_root().join("fixtures/docx/vi-styled-report.docx"))
        .args(["--to", "pdf", "--json", "-o"])
        .arg(directory.path().join("report.pdf"))
        .output()
        .unwrap();
    assert_eq!(
        route_of(&report),
        ["docx-pandoc->ariad-ir+json", "ariad-ir+json-typst->pdf"]
    );
}

#[cfg(target_os = "linux")]
#[test]
fn configured_libreoffice_symlink_in_system_temp_still_converts() {
    let Ok(soffice) = ariad_host::engines::libreoffice::locate() else {
        eprintln!("skipping: LibreOffice >=24.2 is not installed");
        return;
    };
    let directory = tempdir().unwrap();
    let alias = directory.path().join("soffice");
    std::os::unix::fs::symlink(fs::canonicalize(soffice.path).unwrap(), &alias).unwrap();
    let output = directory.path().join("slides.pdf");
    let mut command = cli();
    Command::env(&mut command, "ASHIFT_SOFFICE", alias);
    let result = command
        .arg("convert")
        .arg(fixture("en-map-room-slides.pptx"))
        .args(["--to", "pdf", "--json", "-o"])
        .arg(&output)
        .output()
        .unwrap();
    assert_eq!(route_of(&result), ["pptx-libreoffice->pdf"]);
    assert!(fs::read(output).unwrap().starts_with(b"%PDF-"));
}

#[test]
fn engines_and_doctor_report_libreoffice_as_optional() {
    let directory = tempdir().unwrap();
    let missing = directory.path().join("no-soffice-here");
    let engines = cli()
        .env("ASHIFT_SOFFICE", &missing)
        .args(["engines", "--json"])
        .output()
        .unwrap();
    assert!(engines.status.success());
    let rows: Value = serde_json::from_slice(&engines.stdout).unwrap();
    let office = rows
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == "libreoffice")
        .expect("libreoffice row");
    assert_eq!(office["status"], "missing");
    assert_eq!(office["license"], "MPL-2.0");
    assert!(
        office["routes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|route| route == "doc -> docx")
    );

    let doctor = cli()
        .env("ASHIFT_SOFFICE", &missing)
        .args(["doctor", "--json"])
        .output()
        .unwrap();
    let report: Value = serde_json::from_slice(&doctor.stdout).unwrap();
    let check = report["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|check| check["id"] == "engine_libreoffice")
        .expect("libreoffice doctor check");
    assert_eq!(check["ok"], false);
    assert_eq!(check["required"], false);
    // A missing optional engine never fails doctor on its own.
    assert!(
        doctor.status.success(),
        "{}",
        String::from_utf8_lossy(&doctor.stderr)
    );
}

#[cfg(unix)]
mod lifecycle {
    use std::{
        ffi::OsString,
        fs,
        os::unix::fs::PermissionsExt,
        thread,
        time::{Duration, Instant},
    };

    use ariad_core::limits::Limits;
    use ariad_host::convert::{ConvertError, convert_custom};
    use tokio_util::sync::CancellationToken;

    /// A test tool that holds a descendant alive while the real LibreOffice adapter runs.
    fn probe(directory: &std::path::Path) -> std::path::PathBuf {
        let path = directory.join("soffice-probe");
        let heartbeat = directory.join("heartbeat");
        let pid = directory.join("probe-pid");
        fs::write(&path, format!(
            "#!/bin/sh\nif [ \"$1\" = --version ]; then echo 'LibreOffice 26.8.0.3'; exit 0; fi\necho $$ > '{}'\n(while :; do echo beat >> '{}'; sleep 0.05; done) &\nworker=$!\ntrap 'kill \"$worker\"; wait \"$worker\"; exit 0' TERM\nwait \"$worker\"\n",
            pid.display(), heartbeat.display(),
        )).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[test]
    fn cancellation_and_timeout_stop_libreoffice_descendants() {
        // The subprocess environment avoids modifying the test process's global environment.
        for timeout in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let tool = probe(directory.path());
            let heartbeat = directory.path().join("heartbeat");
            let token = CancellationToken::new();
            let cancel = token.clone();
            let input = super::fixture("en-map-room-slides.pptx");
            let output = directory.path().join("output.pdf");
            let invocation = directory.path().join("ashift-probe");
            // Use a custom invocation for lifecycle observation: its heartbeat is outside
            // the job workspace. Built-in engine tests cover the actual sandbox launcher.
            fs::write(
                &invocation,
                format!(
                    "#!/bin/sh\nexport ASHIFT_SOFFICE='{}'\nexec '{}' __engine libreoffice\n",
                    tool.display(),
                    env!("CARGO_BIN_EXE_ashift"),
                ),
            )
            .unwrap();
            fs::set_permissions(&invocation, fs::Permissions::from_mode(0o755)).unwrap();
            let worker = thread::spawn(move || {
                convert_custom(
                    &input,
                    &output,
                    "pdf",
                    &invocation,
                    &[OsString::from("serve")],
                    false,
                    Limits {
                        timeout_s: timeout.then_some(2),
                        ..Limits::local()
                    },
                    cancel,
                    |_| {},
                )
            });
            let deadline = Instant::now() + Duration::from_secs(30);
            while !heartbeat.exists() && !worker.is_finished() && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(20));
            }
            let started = heartbeat.exists();
            if !timeout || !started {
                token.cancel();
            }
            let result = worker.join().unwrap();
            let before = fs::metadata(&heartbeat).map(|m| m.len()).unwrap_or(0);
            thread::sleep(Duration::from_millis(250));
            let after = fs::metadata(&heartbeat).map(|m| m.len()).unwrap_or(0);
            // Clean up a regressed adapter's escaped group before asserting the failure.
            if after != before
                && let Ok(pid) = fs::read_to_string(directory.path().join("probe-pid"))
                && let Ok(pid) = pid.trim().parse::<u32>()
                && pid > 1
            {
                let _ = std::process::Command::new("kill")
                    .args(["-TERM", "--", &format!("-{pid}")])
                    .status();
            }
            assert!(started, "adapter never started the probe");
            assert_eq!(after, before, "LibreOffice descendant survived the runner");
            assert_eq!(
                result.err(),
                Some(if timeout {
                    ConvertError::LimitExceeded
                } else {
                    ConvertError::Interrupted
                })
            );
            assert!(!directory.path().join("output.pdf").exists());
        }
    }
}

#[test]
fn pdf_converts_to_pptx_via_libreoffice() {
    if !libreoffice_available() {
        return;
    }
    let directory = tempdir().unwrap();
    let input = support::fixtures::repository_root().join("fixtures/pdf-forms-detect/table.pdf");
    let output = directory.path().join("table.pptx");
    let result = convert_json(&input, "pptx", &output);
    assert_eq!(
        route_of(&result),
        ["pdf-libreoffice->pptx"],
        "pdf -> pptx route"
    );
    assert!(zip_has(&output, "ppt/presentation.xml"));
}

#[test]
fn odg_converts_to_pdf_via_libreoffice() {
    if !libreoffice_available() {
        return;
    }
    let directory = tempdir().unwrap();
    let odg_path = directory.path().join("test_drawing.odg");
    write_dummy_odg(&odg_path);

    let output = directory.path().join("test_drawing.pdf");
    let result = convert_json(&odg_path, "pdf", &output);
    assert_eq!(
        route_of(&result),
        ["odg-libreoffice->pdf"],
        "odg -> pdf route"
    );
    let bytes = fs::read(&output).unwrap();
    assert!(bytes.starts_with(b"%PDF-"), "odg did not produce a PDF");
}

fn write_dummy_odg(path: &Path) {
    let file = fs::File::create(path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let stored =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    zip.start_file("mimetype", stored).unwrap();
    zip.write_all(b"application/vnd.oasis.opendocument.graphics")
        .unwrap();

    let deflated = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    zip.start_file("META-INF/manifest.xml", deflated).unwrap();
    zip.write_all(
        br#"<?xml version="1.0" encoding="UTF-8"?>
<manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0" manifest:version="1.3">
  <manifest:file-entry manifest:full-path="/" manifest:version="1.3" manifest:media-type="application/vnd.oasis.opendocument.graphics"/>
  <manifest:file-entry manifest:full-path="content.xml" manifest:media-type="text/xml"/>
</manifest:manifest>"#,
    )
    .unwrap();

    zip.start_file("content.xml", deflated).unwrap();
    zip.write_all(
        br#"<?xml version="1.0" encoding="UTF-8"?>
<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0" office:version="1.3">
  <office:body>
    <office:drawing>
      <draw:page draw:name="page1">
        <draw:custom-shape svg:width="8cm" svg:height="4cm" svg:x="1cm" svg:y="1cm"/>
      </draw:page>
    </office:drawing>
  </office:body>
</office:document-content>"#,
    )
    .unwrap();
    zip.finish().unwrap();
}

#[test]
fn pages_family_routes_are_plannable() {
    let directory = tempdir().unwrap();
    for fmt in ["odg", "pub", "vsd", "wpd", "wps"] {
        let input_file = directory.path().join(format!("sample.{fmt}"));
        if fmt == "odg" {
            write_dummy_odg(&input_file);
        } else {
            fs::write(&input_file, b"dummy content").unwrap();
        }
        let out = cli()
            .args([
                "plan",
                input_file.to_str().unwrap(),
                "--to",
                "pdf",
                "--json",
            ])
            .output()
            .expect("start ashift plan");
        assert!(
            out.status.success(),
            "planning failed for {fmt} -> pdf: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let plan: Value = serde_json::from_slice(&out.stdout).expect("parse plan JSON");
        assert_eq!(plan["route"][0]["engine"], "libreoffice");
    }

    let pdf_file = support::fixtures::repository_root().join("fixtures/pdf-forms-detect/table.pdf");
    let pptx_plan = cli()
        .args(["plan", pdf_file.to_str().unwrap(), "--to", "pptx", "--json"])
        .output()
        .expect("start ashift plan");
    assert!(
        pptx_plan.status.success(),
        "planning failed for pdf -> pptx: {}",
        String::from_utf8_lossy(&pptx_plan.stderr)
    );
    let plan: Value = serde_json::from_slice(&pptx_plan.stdout).expect("parse plan JSON");
    assert_eq!(plan["route"][0]["engine"], "libreoffice");
}

#[test]
fn xlsx_created_from_ir_with_tables_opens_in_libreoffice() {
    if !libreoffice_available() {
        return;
    }
    let directory = tempdir().unwrap();
    let ir_json = r#"{
      "version": "ariad-ir/1",
      "meta": {},
      "body": [
        {
          "type": "table",
          "caption": [{"type": "text", "text": "First Table"}],
          "columns": [{"align": "left"}, {"align": "right"}],
          "head": [
            [
              {"rowspan": 1, "colspan": 1, "blocks": [{"type": "paragraph", "content": [{"type": "text", "text": "Item"}]}]},
              {"rowspan": 1, "colspan": 1, "blocks": [{"type": "paragraph", "content": [{"type": "text", "text": "Price"}]}]}
            ]
          ],
          "body": [
            [
              {"rowspan": 1, "colspan": 1, "blocks": [{"type": "paragraph", "content": [{"type": "text", "text": "Apple"}]}]},
              {"rowspan": 1, "colspan": 1, "blocks": [{"type": "paragraph", "content": [{"type": "text", "text": "1.50"}]}]}
            ]
          ]
        },
        {
          "type": "table",
          "caption": [{"type": "text", "text": "Second Table"}],
          "columns": [{"align": "left"}],
          "head": [],
          "body": [
            [
              {"rowspan": 1, "colspan": 1, "blocks": [{"type": "paragraph", "content": [{"type": "text", "text": "Only Cell"}]}]}
            ]
          ]
        }
      ]
    }"#;
    let ir_file = directory.path().join("document.ir.json");
    fs::write(&ir_file, ir_json).unwrap();

    let xlsx_file = directory.path().join("document.xlsx");
    let result = convert_json(&ir_file, "xlsx", &xlsx_file);
    assert_eq!(
        route_of(&result),
        ["ariad-ir+json-ariad-core->xlsx"],
        "ir -> xlsx route"
    );
    assert!(zip_has(&xlsx_file, "xl/worksheets/sheet1.xml"));
    assert!(zip_has(&xlsx_file, "xl/worksheets/sheet2.xml"));
    assert!(
        !zip_has(&xlsx_file, "xl/worksheets/sheet3.xml"),
        "expected exactly 2 sheets"
    );

    let wb_xml = {
        let file = fs::File::open(&xlsx_file).unwrap();
        let mut archive = ZipArchive::new(file).unwrap();
        let mut f = archive.by_name("xl/workbook.xml").unwrap();
        let mut s = String::new();
        std::io::Read::read_to_string(&mut f, &mut s).unwrap();
        s
    };
    assert!(wb_xml.contains("name=\"First Table\""));
    assert!(wb_xml.contains("name=\"Second Table\""));

    let soffice = ariad_host::engines::libreoffice::locate().unwrap();
    let lo_pdf = directory.path().join("document.pdf");
    let lo_run = Command::new(&soffice.path)
        .args(["--headless", "--convert-to", "pdf", "--outdir"])
        .arg(directory.path())
        .arg(&xlsx_file)
        .output()
        .expect("run soffice conversion");
    assert!(
        lo_run.status.success(),
        "LibreOffice failed to open/convert generated XLSX"
    );
    assert!(
        lo_pdf.exists(),
        "LibreOffice failed to produce PDF from generated XLSX"
    );
}

fn docling_available() -> bool {
    if std::env::var_os("ASHIFT_PACK_DOCLING").is_some()
        || std::env::var_os("ASHIFT_DOCLING_ENGINE").is_some()
    {
        return true;
    }
    match ariad_host::engines::resolve_pack("docling") {
        Ok(_) => true,
        Err(_) => {
            eprintln!(
                "skipping: docling pack not installed; set ASHIFT_PACK_DOCLING or ASHIFT_DOCLING_ENGINE to run this test"
            );
            false
        }
    }
}

#[test]
fn pages_family_conversion_attempts_or_explicit_skips() {
    if !libreoffice_available() {
        return;
    }
    // ODG is tested by odg_converts_to_pdf_via_libreoffice.
    // For pub, vsd, wpd, wps: verify if real non-copyleft fixtures exist.
    // If not committed due to repository licensing rules, skip with explicit explanation.
    for fmt in ["pub", "vsd", "wpd", "wps"] {
        let fixture_path =
            support::fixtures::repository_root().join(format!("fixtures/office/sample.{fmt}"));
        if !fixture_path.exists() {
            eprintln!(
                "skipping: real test fixture for {fmt} not committed due to repository licensing rules \
                 (public DLP/libwpd/libwps suites use LGPL-2.1/MPL copyleft test assets excluded by AGENTS.md); \
                 see local-gate note in result file"
            );
            continue;
        }
        let directory = tempdir().unwrap();
        let output = directory.path().join(format!("output_{fmt}.pdf"));
        let result = convert_json(&fixture_path, "pdf", &output);
        assert_eq!(route_of(&result), [format!("{fmt}-libreoffice->pdf")]);
        assert!(output.exists());
    }
}

#[test]
fn pdf_to_xlsx_and_csv_route_planning_and_conversion_with_docling() {
    if !docling_available() {
        return;
    }
    let pdf_fixture =
        support::fixtures::repository_root().join("fixtures/pdf-forms-detect/table.pdf");
    let directory = tempdir().unwrap();

    // 1. Plan pdf -> xlsx: must select docling engine
    let plan_xlsx = cli()
        .args([
            "plan",
            pdf_fixture.to_str().unwrap(),
            "--to",
            "xlsx",
            "--json",
        ])
        .output()
        .expect("plan pdf -> xlsx");
    assert!(plan_xlsx.status.success());
    let plan: Value = serde_json::from_slice(&plan_xlsx.stdout).unwrap();
    assert_eq!(
        plan["route"][0]["engine"], "docling",
        "pdf -> xlsx must plan via docling tables"
    );

    // 2. Convert pdf -> xlsx: route must run through docling
    let xlsx_out = directory.path().join("table.xlsx");
    let conv_xlsx = cli()
        .args([
            "convert",
            pdf_fixture.to_str().unwrap(),
            "--to",
            "xlsx",
            "--json",
            "-o",
            xlsx_out.to_str().unwrap(),
        ])
        .output()
        .expect("convert pdf -> xlsx");
    assert!(conv_xlsx.status.success());
    assert!(xlsx_out.exists());
    assert!(zip_has(&xlsx_out, "xl/worksheets/sheet1.xml"));

    // 3. Plan pdf -> csv: must select docling engine
    let plan_csv = cli()
        .args([
            "plan",
            pdf_fixture.to_str().unwrap(),
            "--to",
            "csv",
            "--json",
        ])
        .output()
        .expect("plan pdf -> csv");
    assert!(plan_csv.status.success());
    let plan: Value = serde_json::from_slice(&plan_csv.stdout).unwrap();
    assert_eq!(
        plan["route"][0]["engine"], "docling",
        "pdf -> csv must plan via docling tables"
    );

    // 4. Convert pdf -> csv: route must run through docling
    let csv_out = directory.path().join("table.csv");
    let conv_csv = cli()
        .args([
            "convert",
            pdf_fixture.to_str().unwrap(),
            "--to",
            "csv",
            "--json",
            "-o",
            csv_out.to_str().unwrap(),
        ])
        .output()
        .expect("convert pdf -> csv");
    assert!(conv_csv.status.success());
    assert!(csv_out.exists());
}

#[test]
fn pdf_to_xlsx_without_docling_falls_back_to_text_dump_with_warning() {
    let pdf_fixture =
        support::fixtures::repository_root().join("fixtures/pdf-forms-detect/table.pdf");
    let directory = tempdir().unwrap();
    let xlsx_out = directory.path().join("fallback.xlsx");

    // Convert forcing built-in PDF reader (--engine pdf)
    let conv = cli()
        .args([
            "convert",
            pdf_fixture.to_str().unwrap(),
            "--engine",
            "pdf",
            "--to",
            "xlsx",
            "--json",
            "-o",
            xlsx_out.to_str().unwrap(),
        ])
        .output()
        .expect("convert with native pdf");
    assert!(conv.status.success());
    let report: Value = serde_json::from_slice(&conv.stdout).unwrap();
    let warnings = report["warnings"].as_array().expect("warnings array");
    assert!(
        warnings.iter().any(|w| w["code"] == "no_tables_found"),
        "fallback text dump must issue no_tables_found warning: {warnings:?}"
    );
    assert!(xlsx_out.exists());
    assert!(zip_has(&xlsx_out, "xl/worksheets/sheet1.xml"));
}
