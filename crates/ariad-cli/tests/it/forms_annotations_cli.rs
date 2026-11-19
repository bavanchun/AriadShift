//! CLI publication and format contracts for form-data and annotations.
use assert_cmd::Command;
use std::{
    fs,
    path::{Path, PathBuf},
};
use tempfile::TempDir;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .into()
}
fn command() -> Command {
    Command::new(env!("CARGO_BIN_EXE_ashift"))
}

#[test]
fn batch_form_import_publishes_atomically_and_requires_a_directory() {
    let tmp = TempDir::new().unwrap();
    let input = root().join("fixtures/pdf-forms/forms-all-types.pdf");
    let csv = tmp.path().join("data.csv");
    fs::write(&csv, b"name\nAn\nBinh\n").unwrap();
    let output = tmp.path().join("out");
    fs::create_dir(&output).unwrap();
    let first = output.join("forms-all-types.filled-001.pdf");
    let second = output.join("forms-all-types.filled-002.pdf");
    fs::write(&second, b"existing").unwrap();
    let run = |target: &Path| {
        command()
            .args(["tool", "pdf.forms_import"])
            .arg(&input)
            .arg(&csv)
            .arg("-o")
            .arg(target)
            .output()
            .unwrap()
    };
    let blocked = run(&output);
    assert_eq!(
        blocked.status.code(),
        Some(6),
        "{}",
        String::from_utf8_lossy(&blocked.stderr)
    );
    assert!(!first.exists());
    assert_eq!(fs::read(&second).unwrap(), b"existing");
    fs::remove_file(&second).unwrap();
    let success = run(&output);
    assert!(
        success.status.success(),
        "{}",
        String::from_utf8_lossy(&success.stderr)
    );
    assert!(first.is_file() && second.is_file());
    let bad = run(&tmp.path().join("single.pdf"));
    assert_eq!(
        bad.status.code(),
        Some(2),
        "{}",
        String::from_utf8_lossy(&bad.stderr)
    );
}

#[test]
fn annotations_list_and_forms_export_choose_requested_output_formats() {
    let tmp = TempDir::new().unwrap();
    let input = root().join("fixtures/pdf-forms/forms-all-types.pdf");
    let output = command()
        .args(["tool", "pdf.annotations"])
        .arg(&input)
        .args(["--param", "action=list", "--json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(serde_json::from_slice::<serde_json::Value>(&output.stdout).is_ok());
    let path = tmp.path().join("data.xfdf");
    let output = command()
        .args(["tool", "pdf.forms_export"])
        .arg(&input)
        .args(["--param", "format=xfdf", "-o"])
        .arg(&path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(fs::read_to_string(path).unwrap().contains("<xfdf"));
}

#[test]
fn batch_names_follow_the_selected_column_and_are_sanitized() {
    let tmp = TempDir::new().unwrap();
    let input = root().join("fixtures/pdf-forms/forms-all-types.pdf");
    let csv = tmp.path().join("data.csv");
    fs::write(&csv, b"filename,name\n../An,An\nAN,Binh\n").unwrap();
    let output = tmp.path().join("out");
    fs::create_dir(&output).unwrap();
    let result = command()
        .args(["tool", "pdf.forms_import"])
        .arg(input)
        .arg(csv)
        .args(["--param", "name_column=filename", "-o"])
        .arg(&output)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(output.join("An.pdf").is_file());
    assert!(output.join("AN-2.pdf").is_file());
    assert!(!tmp.path().join("An.pdf").exists());
}
