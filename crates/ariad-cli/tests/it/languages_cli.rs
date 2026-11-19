//! `ashift engines languages --json` output shapes, against an empty model cache.
use serde_json::Value;
use std::process::{Command, Output};
use tempfile::TempDir;

fn run(data: &TempDir, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ashift"))
        .env("ASHIFT_DATA_DIR", data.path())
        .args(["engines", "languages"])
        .args(args)
        .output()
        .expect("run ashift")
}

fn json(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "stdout is not JSON ({error}): {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

#[test]
fn list_json_is_an_array_of_model_statuses() {
    let data = TempDir::new().unwrap();
    let output = run(&data, &["list", "--json"]);
    assert_eq!(output.status.code(), Some(0));
    let models = json(&output);
    let models = models.as_array().expect("a JSON array");
    assert!(!models.is_empty());
    for model in models {
        assert!(model["language"].is_string());
        assert_eq!(
            model["installed"], false,
            "empty cache has nothing installed"
        );
        assert!(model["size"].as_u64().unwrap() > 0);
        assert_eq!(model.as_object().unwrap().len(), 3);
    }
    assert!(models.iter().any(|model| model["language"] == "fra"));
}

#[test]
fn list_human_output_is_unchanged_and_not_json() {
    let data = TempDir::new().unwrap();
    let output = run(&data, &["list"]);
    assert_eq!(output.status.code(), Some(0));
    let text = String::from_utf8(output.stdout).unwrap();
    let line = text.lines().find(|l| l.starts_with("fra\t")).unwrap();
    assert!(line.starts_with("fra\tavailable\t") && line.ends_with(" bytes"));
}

#[test]
fn remove_json_reports_the_model_as_not_installed() {
    let data = TempDir::new().unwrap();
    let output = run(&data, &["remove", "fra", "--json"]);
    assert_eq!(output.status.code(), Some(0));
    let model = json(&output);
    assert_eq!(model["language"], "fra");
    assert_eq!(model["installed"], false);
    assert!(model["size"].as_u64().unwrap() > 0);
}

#[test]
fn remove_without_json_prints_nothing() {
    let data = TempDir::new().unwrap();
    let output = run(&data, &["remove", "fra"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
}

#[test]
fn unknown_language_fails_with_usage_error_and_no_stdout_in_json_mode() {
    let data = TempDir::new().unwrap();
    for action in ["install", "remove"] {
        let output = run(&data, &[action, "zzz", "--json"]);
        assert_ne!(output.status.code(), Some(0), "{action}");
        assert!(output.stdout.is_empty(), "{action}: errors go to stderr");
        assert!(String::from_utf8_lossy(&output.stderr).contains("unknown OCR language"));
    }
}
