//! `ashift convert` with several inputs.

mod support;

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

use serde_json::Value;
use tempfile::tempdir;

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

fn convert(args: &[&Path], extra: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ashift"))
        .env("ASHIFT_PANDOC", pandoc_path())
        .env("SOURCE_DATE_EPOCH", "1700000000")
        .arg("convert")
        .args(args)
        .args(extra)
        .output()
        .expect("start ashift")
}

fn write(path: &Path, text: &str) -> PathBuf {
    fs::write(path, text).expect("write input");
    path.to_path_buf()
}

fn stdout_lines(output: &Output) -> Vec<String> {
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::to_owned)
        .collect()
}

#[test]
fn several_inputs_convert_into_an_output_directory() {
    let directory = tempdir().unwrap();
    let first = write(&directory.path().join("first.md"), "# First\n");
    let second = write(&directory.path().join("second.v2.md"), "# Second\n");
    let out = directory.path().join("out");
    fs::create_dir(&out).unwrap();

    let output = convert(
        &[&first, &second],
        &["--to", "docx", "-o", out.to_str().unwrap()],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let expected = [out.join("first.docx"), out.join("second.v2.docx")];
    assert_eq!(
        stdout_lines(&output),
        expected
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>(),
        "one line per input, in input order"
    );
    for path in &expected {
        assert!(path.is_file(), "{} was written", path.display());
    }
}

#[test]
fn several_inputs_default_to_their_own_directories() {
    let directory = tempdir().unwrap();
    let nested = directory.path().join("nested");
    fs::create_dir(&nested).unwrap();
    let first = write(&directory.path().join("a.md"), "# A\n");
    let second = write(&nested.join("b.md"), "# B\n");
    let output = convert(&[&first, &second], &["--to", "html"]);
    assert!(output.status.success());
    assert!(directory.path().join("a.html").is_file());
    assert!(nested.join("b.html").is_file());
}

#[test]
fn a_mix_of_successes_and_failures_exits_with_the_first_failure() {
    let directory = tempdir().unwrap();
    let good = write(&directory.path().join("good.md"), "# Good\n");
    let unsupported = write(&directory.path().join("notes.txt"), "plain text\n");
    let existing = write(&directory.path().join("taken.md"), "# Taken\n");
    fs::write(directory.path().join("taken.docx"), b"keep me").unwrap();
    let last = write(&directory.path().join("last.md"), "# Last\n");

    let output = convert(&[&good, &unsupported, &existing, &last], &["--to", "docx"]);
    assert_eq!(
        output.status.code(),
        Some(3),
        "the first failure was an unsupported route"
    );
    assert_eq!(
        stdout_lines(&output),
        vec![
            directory.path().join("good.docx").display().to_string(),
            directory.path().join("last.docx").display().to_string(),
        ],
        "later inputs still convert after a failure"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(&format!(
            "ashift: {}: unsupported conversion route",
            unsupported.display()
        )),
        "{stderr}"
    );
    assert!(
        stderr.contains(&format!(
            "ashift: {}: the destination already exists",
            existing.display()
        )),
        "{stderr}"
    );
    assert_eq!(
        fs::read(directory.path().join("taken.docx")).unwrap(),
        b"keep me"
    );
}

#[test]
fn json_reports_one_document_with_an_array_of_results() {
    let directory = tempdir().unwrap();
    let good = write(&directory.path().join("good.md"), "# Good\n");
    let unsupported = write(&directory.path().join("notes.txt"), "plain text\n");
    let output = convert(&[&good, &unsupported], &["--to", "html", "--json"]);
    assert_eq!(output.status.code(), Some(3));
    let document: Value = serde_json::from_slice(&output.stdout).expect("one JSON document");
    let results = document["results"].as_array().expect("results array");
    assert_eq!(results.len(), 2);

    assert_eq!(results[0]["input"], good.display().to_string());
    assert_eq!(results[0]["ok"], true);
    assert_eq!(
        results[0]["output"],
        directory.path().join("good.html").display().to_string()
    );
    assert!(results[0]["route"].is_array());
    assert!(results[0]["warnings"].is_array());
    assert!(results[0]["elapsed_ms"].is_u64());
    assert!(results[0].get("error").is_none());

    assert_eq!(results[1]["input"], unsupported.display().to_string());
    assert_eq!(results[1]["ok"], false);
    assert_eq!(results[1]["error"]["code"], "unsupported_route");
    assert_eq!(results[1]["error"]["exit_code"], 3);
    assert!(
        results[1]["error"]["message"]
            .as_str()
            .unwrap()
            .contains("unsupported")
    );
    assert!(results[1].get("output").is_none());
}

#[test]
fn several_inputs_need_an_existing_output_directory() {
    let directory = tempdir().unwrap();
    let first = write(&directory.path().join("a.md"), "# A\n");
    let second = write(&directory.path().join("b.md"), "# B\n");
    let output = convert(
        &[&first, &second],
        &[
            "--to",
            "html",
            "-o",
            directory.path().join("missing").to_str().unwrap(),
        ],
    );
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("-o must be an existing directory"));
    assert!(!directory.path().join("a.html").exists());
}

#[test]
fn inputs_that_would_share_a_destination_are_refused_before_converting() {
    let directory = tempdir().unwrap();
    for name in ["one", "two"] {
        fs::create_dir(directory.path().join(name)).unwrap();
    }
    let first = write(&directory.path().join("one/doc.md"), "# One\n");
    let second = write(&directory.path().join("two/doc.md"), "# Two\n");
    let out = directory.path().join("out");
    fs::create_dir(&out).unwrap();
    let output = convert(
        &[&first, &second],
        &["--to", "html", "-o", out.to_str().unwrap()],
    );
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("several inputs would write"));
    assert!(
        !out.join("doc.html").exists(),
        "nothing converts when destinations collide"
    );
}
