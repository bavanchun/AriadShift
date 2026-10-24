//! The real Docling engine, driven the way the host drives it. These tests need the models, the
//! OCR data and Tesseract 5, so they are ignored by default. `just docling-engine-test` runs
//! them against the development environment that `just docling-dev` sets up; the engine pack
//! workflow runs them in CI. `docling_routes.rs` covers the same routes with a stand-in engine.

mod support;

use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

use serde_json::{Value, json};
use tempfile::tempdir;

fn repository() -> PathBuf {
    support::fixtures::repository_root()
}

fn fixture(relative: &str) -> PathBuf {
    repository().join("fixtures").join(relative)
}

/// The engine command line: the caller's override, else the development environment.
fn engine_command() -> String {
    if let Ok(command) = std::env::var("ASHIFT_DOCLING_ENGINE") {
        return command;
    }
    let python = repository()
        .join(".tools/docling/python-env")
        .join(if cfg!(windows) {
            "Scripts/python.exe"
        } else {
            "bin/python"
        });
    format!("\"{}\" -I -m ariad_docling", python.display())
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
    repository().join(".tools/pandoc/bin").join(name)
}

/// The engine process with the cleared environment the host gives it.
fn engine_process() -> Command {
    let words = ariad_host::engines::docling::split_command_line(&engine_command())
        .expect("a usable engine command line");
    let mut command = Command::new(&words[0]);
    command.args(&words[1..]).env_clear();
    for name in ["PATH", "SYSTEMROOT"] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    command
}

fn run_engine(lines: &[Value]) -> Output {
    let mut child = engine_process()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start the docling engine");
    let mut payload = Vec::new();
    for line in lines {
        serde_json::to_writer(&mut payload, line).unwrap();
        payload.push(b'\n');
    }
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&payload)
        .expect("write engine requests");
    child.wait_with_output().expect("wait for the engine")
}

fn cli(data_dir: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ashift"));
    command
        .env("ASHIFT_PANDOC", pandoc_path())
        .env("ASHIFT_DOCLING_ENGINE", engine_command())
        .env("ASHIFT_DATA_DIR", data_dir)
        .env("SOURCE_DATE_EPOCH", "1700000000")
        .env_remove("ASHIFT_PACK_DOCLING");
    command
}

fn convert(input: &Path, to: &str) -> (tempfile::TempDir, PathBuf, Value) {
    let directory = tempdir().unwrap();
    let output = directory.path().join(format!("converted.{to}"));
    let result = cli(&directory.path().join("data"))
        .arg("convert")
        .arg(input)
        .args(["--to", to, "--json", "-o"])
        .arg(&output)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report["route"][0]["engine"], "docling", "{report}");
    (directory, output, report)
}

/// The text of a converted file: Markdown as is, DOCX from its `w:t` runs.
fn text_of(path: &Path) -> String {
    if path
        .extension()
        .is_some_and(|extension| extension == "docx")
    {
        let mut archive = zip::ZipArchive::new(fs::File::open(path).unwrap()).unwrap();
        let mut xml = String::new();
        std::io::Read::read_to_string(&mut archive.by_name("word/document.xml").unwrap(), &mut xml)
            .unwrap();
        let mut runs = Vec::new();
        let mut rest = xml.as_str();
        while let Some(start) = rest.find("<w:t") {
            let after = &rest[start..];
            let open_end = after.find('>').unwrap();
            let close = after.find("</w:t>").unwrap();
            runs.push(after[open_end + 1..close].to_owned());
            rest = &after[close + 6..];
        }
        runs.join(" ")
    } else {
        fs::read_to_string(path).unwrap()
    }
}

fn normalized(text: &str) -> Vec<char> {
    text.chars()
        .filter(|character| !matches!(character, '#' | '*' | '_'))
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .collect()
}

/// Edit distance between `actual` and `expected` as a share of the expected length.
fn character_error_rate(actual: &str, expected: &str) -> f64 {
    let (actual, expected) = (normalized(actual), normalized(expected));
    let mut previous: Vec<usize> = (0..=expected.len()).collect();
    for (row, left) in actual.iter().enumerate() {
        let mut current = vec![row + 1];
        for (column, right) in expected.iter().enumerate() {
            let substitution = previous[column] + usize::from(left != right);
            current.push(
                substitution
                    .min(previous[column + 1] + 1)
                    .min(current[column] + 1),
            );
        }
        previous = current;
    }
    previous[expected.len()] as f64 / expected.len() as f64
}

fn assert_vietnamese_scan(input: &str, to: &str) {
    let (_directory, output, report) = convert(&fixture(input), to);
    assert_eq!(report["route"][0]["to"], "docling+json");
    let text = text_of(&output);
    let truth = fs::read_to_string(fixture("scan/truth/vi-garden-300dpi.txt")).unwrap();
    let rate = character_error_rate(&text, &truth);
    assert!(rate <= 0.08, "character error rate {rate:.3}:\n{text}");
    assert!(text.contains("Khu vườn đọc sách"), "{text}");
}

#[test]
#[ignore = "needs the docling engine from `just docling-dev`"]
fn the_engine_conforms_to_the_protocol() {
    let described =
        run_engine(&[json!({"protocol": "ariad-engine/1", "job": "describe-1", "op": "describe"})]);
    assert!(described.status.success());
    let events = support::protocol::validate_stdout(&described.stdout);
    assert_eq!(events.len(), 2);
    let capabilities = &events[0];
    assert_eq!(capabilities["type"], "capabilities");
    assert_eq!(capabilities["engine"], "docling");
    assert_eq!(capabilities["supports_session"], true);
    assert_eq!(capabilities["tool"]["status"], "found", "{capabilities}");
    assert_eq!(capabilities["routes"].as_array().unwrap().len(), 8);
    for model in [
        "docling-layout-heron",
        "tableformer",
        "rapidocr",
        "tessdata-vie",
    ] {
        assert!(
            capabilities["models"]
                .as_array()
                .unwrap()
                .contains(&json!(model)),
            "{capabilities}"
        );
    }

    // Requests that are not valid or not for this engine get one typed result.
    for (request, code) in [
        (
            json!({"protocol": "ariad-engine/9", "job": "x", "op": "describe"}),
            "invalid_request",
        ),
        (json!({}), "invalid_request"),
    ] {
        let rejected = run_engine(&[request]);
        let events = support::protocol::validate_stdout(&rejected.stdout);
        assert_eq!(events.last().unwrap()["error"]["code"], code);
    }

    // A conversion with progress, a warning and an artifact inside the output directory. The
    // Vietnamese hint with RapidOCR makes the engine warn without running OCR.
    let directory = tempdir().unwrap();
    for name in ["in", "out", "tmp"] {
        fs::create_dir(directory.path().join(name)).unwrap();
    }
    let input = directory.path().join("in/en-map-room.pdf");
    fs::copy(fixture("pdf/en-map-room.pdf"), &input).unwrap();
    let converted = run_engine(&[json!({
        "protocol": "ariad-engine/1",
        "job": "convert-1",
        "op": "convert",
        "input": {"path": input, "format": "pdf"},
        "output": {"dir": directory.path().join("out"), "format": "docling+json"},
        "work_dir": directory.path().join("tmp"),
        "options": {"ocr": "rapidocr", "lang": ["vi"]},
        "limits": {"max_nesting_depth": 64}
    })]);
    assert!(
        converted.status.success(),
        "{}",
        String::from_utf8_lossy(&converted.stderr)
    );
    let events = support::protocol::validate_stdout(&converted.stdout);
    assert!(events.iter().any(|event| event["type"] == "progress"));
    assert!(
        events
            .iter()
            .any(|event| event["type"] == "warning" && event["code"] == "ocr_language_unsupported")
    );
    let artifact = events
        .iter()
        .find(|event| event["type"] == "artifact")
        .expect("an artifact event");
    assert_eq!(artifact["format"], "docling+json");
    let path = PathBuf::from(artifact["path"].as_str().unwrap());
    assert!(path.starts_with(directory.path().join("out")), "{path:?}");
    let document: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert_eq!(document["schema_name"], "DoclingDocument");
    assert_eq!(events.last().unwrap()["ok"], true);
}

#[test]
#[ignore = "needs the docling engine from `just docling-dev`"]
fn scanned_vietnamese_image_converts_to_markdown() {
    assert_vietnamese_scan("scan/vi-garden-300dpi.png", "md");
}

#[test]
#[ignore = "needs the docling engine from `just docling-dev`"]
fn scanned_vietnamese_image_converts_to_docx() {
    assert_vietnamese_scan("scan/vi-garden-300dpi.png", "docx");
}

#[test]
#[ignore = "needs the docling engine from `just docling-dev`"]
fn scanned_vietnamese_pdf_converts_to_markdown() {
    assert_vietnamese_scan("scan/vi-garden-300dpi.pdf", "md");
}

#[test]
#[ignore = "needs the docling engine from `just docling-dev`"]
fn scanned_vietnamese_pdf_converts_to_docx() {
    assert_vietnamese_scan("scan/vi-garden-300dpi.pdf", "docx");
}

#[test]
#[ignore = "needs the docling engine from `just docling-dev`"]
fn a_digital_pdf_keeps_its_table() {
    let (_directory, output, _) = convert(&fixture("pdf/en-map-room.pdf"), "md");
    let markdown = fs::read_to_string(output).unwrap();
    assert!(
        markdown.contains("## Notes from the Map Room"),
        "{markdown}"
    );
    let rows: Vec<&str> = markdown
        .lines()
        .filter(|line| line.starts_with('|'))
        .collect();
    // A header row, the separator and at least two body rows, all with two columns.
    assert!(rows.len() >= 4, "{markdown}");
    assert!(
        rows.iter().all(|row| row.matches('|').count() == 3),
        "{markdown}"
    );
    assert!(rows[1].contains("---"), "{markdown}");
    assert!(markdown.contains("| Archive item"), "{markdown}");
}

#[test]
#[ignore = "needs the docling engine from `just docling-dev`"]
fn presentations_and_workbooks_convert_to_markdown() {
    let (_directory, output, _) = convert(&fixture("pptx/vi-garden-slides.pptx"), "md");
    assert!(
        fs::read_to_string(output)
            .unwrap()
            .contains("Lịch chăm vườn")
    );
    let (_directory, output, _) = convert(&fixture("xlsx/en-repair-log.xlsx"), "md");
    let markdown = fs::read_to_string(output).unwrap();
    assert!(markdown.contains("Desk lamp"), "{markdown}");
    assert!(
        markdown.lines().any(|line| line.starts_with('|')),
        "{markdown}"
    );
}

#[cfg(unix)]
mod cancellation {
    use std::{
        process::Child,
        thread,
        time::{Duration, Instant},
    };

    use super::*;

    /// Kills the converter on every exit path of the test.
    struct KillOnDrop(Child);

    impl Drop for KillOnDrop {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    /// `(pid, command name)` of every descendant of `root`.
    fn descendants(root: u32) -> Vec<(u32, String)> {
        let output = Command::new("ps")
            .args(["-axo", "pid=,ppid=,comm="])
            .output()
            .expect("run ps");
        let table: Vec<(u32, u32, String)> = String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| {
                let mut fields = line.split_whitespace();
                Some((
                    fields.next()?.parse().ok()?,
                    fields.next()?.parse().ok()?,
                    fields.next()?.to_owned(),
                ))
            })
            .collect();
        let mut family = vec![root];
        let mut found = Vec::new();
        loop {
            let before = family.len();
            for (pid, parent, name) in &table {
                if family.contains(parent) && !family.contains(pid) {
                    family.push(*pid);
                    found.push((*pid, name.clone()));
                }
            }
            if family.len() == before {
                return found;
            }
        }
    }

    fn alive(pid: u32) -> bool {
        Command::new("kill")
            .args(["-0", &pid.to_string()])
            .output()
            .is_ok_and(|output| output.status.success())
    }

    #[test]
    #[ignore = "needs the docling engine from `just docling-dev`"]
    fn cancelling_a_scan_conversion_stops_the_engine_and_tesseract() {
        let directory = tempdir().unwrap();
        let child = cli(&directory.path().join("data"))
            .arg("convert")
            .arg(fixture("scan/vi-garden-300dpi.png"))
            .args(["--to", "md", "-o"])
            .arg(directory.path().join("never.md"))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let pid = child.id();
        let mut guard = KillOnDrop(child);

        let started = Instant::now();
        let mut family = loop {
            let family = descendants(pid);
            if family.iter().any(|(_, name)| name.starts_with("tesseract")) {
                break family;
            }
            assert!(
                started.elapsed() < Duration::from_secs(300),
                "Tesseract never started"
            );
            thread::sleep(Duration::from_millis(50));
        };

        let signalled = Command::new("kill")
            .args(["-INT", &pid.to_string()])
            .status()
            .unwrap();
        assert!(signalled.success());
        let waited = Instant::now();
        let status = loop {
            if let Some(status) = guard.0.try_wait().unwrap() {
                break status;
            }
            assert!(
                waited.elapsed() < Duration::from_secs(60),
                "the converter did not stop"
            );
            thread::sleep(Duration::from_millis(50));
        };
        assert!(!status.success());

        // Anything the converter started after the last look counts too.
        family.extend(descendants(pid));
        thread::sleep(Duration::from_secs(1));
        let survivors: Vec<_> = family.iter().filter(|(pid, _)| alive(*pid)).collect();
        assert!(survivors.is_empty(), "still running: {survivors:?}");
        assert!(!directory.path().join("never.md").exists());
    }
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "needs the docling engine from `just docling-dev`"]
fn doctor_can_run_tesseract_with_data_in_a_custom_install_prefix() {
    use std::os::unix::fs::PermissionsExt;

    let directory = tempdir().unwrap();
    let bin = directory.path().join("prefix/bin");
    let tessdata = directory.path().join("prefix/share/tessdata");
    fs::create_dir_all(&bin).unwrap();
    fs::create_dir_all(&tessdata).unwrap();
    fs::write(tessdata.join("version"), "tesseract 5.5.3\n").unwrap();
    fs::write(
        directory.path().join("prefix/share/private"),
        "must stay unreadable",
    )
    .unwrap();
    fs::write(
        directory.path().join("prefix/private"),
        "must stay unreadable",
    )
    .unwrap();
    let program = bin.join("tesseract");
    // Like a relocated installation, the helper must read data outside the job workspace.
    fs::write(
        &program,
        "#!/bin/sh\nfor private in private share/private; do\nif cat \"${0%/bin/tesseract}/$private\" >/dev/null 2>&1; then exit 1; fi\ndone\ncat \"${0%/bin/tesseract}/share/tessdata/version\"\n",
    )
    .unwrap();
    fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
    let path = std::env::join_paths(
        std::iter::once(bin).chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
    )
    .unwrap();
    let mut command = cli(&directory.path().join("data"));
    Command::env(&mut command, "ASHIFT_SANDBOX", "required");
    let result = command
        .args(["doctor", "--json"])
        .env("PATH", path)
        .output()
        .unwrap();
    let report: Value = serde_json::from_slice(&result.stdout).unwrap();
    let checks = report["checks"].as_array().expect("doctor checks");
    let docling = checks
        .iter()
        .find(|check| check["id"] == "tesseract")
        .expect("Tesseract check");
    assert_eq!(docling["ok"], true, "{report}");
    assert!(
        docling["message"].as_str().unwrap().contains("Tesseract"),
        "{report}"
    );
}
