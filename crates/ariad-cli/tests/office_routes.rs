//! Office and ODF routes through the system LibreOffice.
//!
//! Tests that need LibreOffice skip with a message when no supported `soffice` is installed;
//! the Linux CI job installs it so they run there.

mod support;

use std::{
    fs,
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
