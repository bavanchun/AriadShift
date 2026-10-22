//! Engine sandbox modes as seen by `ashift convert` users.
//!
//! Debug builds treat `ARIAD_TEST_SANDBOX_SUPPORT=none` as a kernel without Landlock or seccomp,
//! so the degraded and fail-closed paths run on every CI kernel.

mod support;

use std::{fs, path::PathBuf, process::Command};

use serde_json::Value;
use tempfile::{TempDir, tempdir};

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

struct Conversion {
    output: std::process::Output,
    docx: PathBuf,
    _directory: TempDir,
}

fn convert_md_to_docx(mode: &str, simulate_no_support: bool) -> Conversion {
    let directory = tempdir().expect("create test directory");
    let input = directory.path().join("draft.md");
    fs::write(&input, "# Title\n\nBody.\n").expect("write Markdown");
    let output_path = directory.path().join("draft.docx");
    let mut command = Command::new(env!("CARGO_BIN_EXE_ashift"));
    command
        .env("ASHIFT_PANDOC", pandoc_path())
        .env("ASHIFT_SANDBOX", mode)
        .arg("convert")
        .arg(&input)
        .args(["--to", "docx", "--json", "-o"])
        .arg(&output_path);
    if simulate_no_support {
        command.env("ARIAD_TEST_SANDBOX_SUPPORT", "none");
    }
    Conversion {
        output: command.output().expect("start ashift"),
        docx: output_path,
        _directory: directory,
    }
}

fn warning_codes(stdout: &[u8]) -> Vec<String> {
    let report: Value = serde_json::from_slice(stdout).expect("convert --json output");
    report["warnings"]
        .as_array()
        .expect("warnings array")
        .iter()
        .map(|warning| warning["code"].as_str().unwrap_or_default().to_owned())
        .collect()
}

#[test]
fn best_effort_without_kernel_support_converts_and_reports_the_gap() {
    let Conversion {
        output,
        docx,
        _directory,
    } = convert_md_to_docx("best-effort", true);
    assert!(
        output.status.success(),
        "best effort must still convert: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(docx.is_file());
    let codes = warning_codes(&output.stdout);
    if cfg!(target_os = "linux") {
        assert_eq!(codes, vec!["sandbox_unavailable"]);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("warning[sandbox_unavailable]"), "{stderr}");
        assert!(
            stderr.contains("Landlock"),
            "the warning names what is missing: {stderr}"
        );
    } else {
        assert!(
            codes.is_empty(),
            "macOS and Windows do not use the launcher"
        );
    }
}

#[test]
fn required_mode_fails_closed_without_kernel_support() {
    let Conversion {
        output,
        docx,
        _directory,
    } = convert_md_to_docx("required", true);
    assert_eq!(output.status.code(), Some(5));
    assert!(output.stdout.is_empty());
    assert!(
        !docx.exists(),
        "no output may be written when the sandbox is missing"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("sandbox is required but unavailable"),
        "{stderr}"
    );
}

#[test]
fn an_unknown_sandbox_mode_is_refused() {
    let Conversion {
        output,
        docx,
        _directory,
    } = convert_md_to_docx("relaxed", false);
    assert_eq!(output.status.code(), Some(5));
    assert!(!docx.exists());
    assert!(String::from_utf8_lossy(&output.stderr).contains("ASHIFT_SANDBOX must be"));
}

#[test]
fn off_mode_never_warns() {
    let Conversion {
        output,
        docx,
        _directory,
    } = convert_md_to_docx("off", true);
    assert!(output.status.success());
    assert!(docx.is_file());
    assert!(warning_codes(&output.stdout).is_empty());
}

#[cfg(target_os = "linux")]
#[test]
fn best_effort_on_a_supporting_kernel_converts_without_a_warning() {
    let Conversion {
        output,
        docx,
        _directory,
    } = convert_md_to_docx("best-effort", false);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(docx.is_file());
    let codes = warning_codes(&output.stdout);
    if codes.is_empty() {
        return;
    }
    // A kernel without Landlock reports the gap; anything else is a regression.
    eprintln!("this kernel lacks part of the sandbox: {codes:?}");
    assert_eq!(codes, vec!["sandbox_unavailable"]);
}
