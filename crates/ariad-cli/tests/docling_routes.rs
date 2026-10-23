//! Docling routes through the CLI. A probe binary stands in for the Python engine, so these run
//! on every OS without models; the real engine is covered by `docling_engine.rs`.

mod support;

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

use serde_json::Value;
use tempfile::tempdir;

fn repository() -> PathBuf {
    support::fixtures::repository_root()
}

fn engine_probe_path() -> PathBuf {
    Path::new(env!("CARGO_BIN_EXE_ashift"))
        .with_file_name(format!("engine-probe{}", std::env::consts::EXE_SUFFIX))
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

fn cli() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ashift"));
    command
        .env("ASHIFT_PANDOC", pandoc_path())
        .env("SOURCE_DATE_EPOCH", "1700000000")
        // No docling pack from the machine running the tests.
        .env("ASHIFT_DATA_DIR", data_dir_path())
        .env_remove("ASHIFT_PACK_DOCLING")
        .env_remove("ASHIFT_DOCLING_ENGINE");
    command
}

fn data_dir_path() -> PathBuf {
    Path::new(env!("CARGO_TARGET_TMPDIR")).join("docling-routes-data")
}

/// A CLI whose Docling engine is the probe, answering with `fixture` (or `-` for a missing tool).
fn cli_with_engine(fixture: &str) -> Command {
    let mut command = cli();
    // This fixture-replay probe reads repository files outside a job workspace.
    // Real-engine and pack tests exercise the sandbox policy.
    command.env("ASHIFT_SANDBOX", "off");
    command.env(
        "ASHIFT_DOCLING_ENGINE",
        format!(
            "\"{}\" docling \"{fixture}\"",
            engine_probe_path().display()
        ),
    );
    command
}

fn docling_fixture(name: &str) -> String {
    repository()
        .join("fixtures/docling")
        .join(name)
        .to_string_lossy()
        .into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn pdf_converts_to_markdown_through_the_engine_and_the_mapper() {
    let directory = tempdir().unwrap();
    let output = directory.path().join("map-room.md");
    let result = cli_with_engine(&docling_fixture("en-map-room.docling.json"))
        .args(["convert"])
        .arg(repository().join("fixtures/pdf/en-map-room.pdf"))
        .args(["--to", "md", "--json", "-o"])
        .arg(&output)
        .output()
        .unwrap();
    assert!(result.status.success(), "{}", stderr(&result));

    let report: Value = serde_json::from_slice(&result.stdout).unwrap();
    let engines: Vec<&str> = report["route"]
        .as_array()
        .unwrap()
        .iter()
        .map(|step| step["engine"].as_str().unwrap())
        .collect();
    assert_eq!(engines, ["docling", "ariad-core", "ariad-core"]);
    assert_eq!(report["route"][0]["to"], "docling+json");
    assert!(
        report["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|warning| warning["code"] == "ocr_fallback")
    );

    let markdown = fs::read_to_string(&output).unwrap();
    assert!(markdown.contains("## Notes from the Map Room"));
    assert!(markdown.contains("| Archive item"), "{markdown}");
}

#[test]
fn scanned_image_converts_to_docx_with_vietnamese_text() {
    let directory = tempdir().unwrap();
    let output = directory.path().join("garden.docx");
    let result = cli_with_engine(&docling_fixture("vi-garden-300dpi.docling.json"))
        .args(["convert"])
        .arg(repository().join("fixtures/scan/vi-garden-300dpi.png"))
        .args(["--to", "docx", "-o"])
        .arg(&output)
        .output()
        .unwrap();
    assert!(result.status.success(), "{}", stderr(&result));
    let mut archive = zip::ZipArchive::new(fs::File::open(&output).unwrap()).unwrap();
    let mut document = String::new();
    std::io::Read::read_to_string(
        &mut archive.by_name("word/document.xml").unwrap(),
        &mut document,
    )
    .unwrap();
    assert!(document.contains("Khu vườn đọc sách"));
}

#[test]
fn a_missing_engine_or_tool_exits_with_code_five_and_a_hint() {
    let directory = tempdir().unwrap();
    let source = repository().join("fixtures/scan/vi-garden-300dpi.png");

    let missing = cli()
        .args(["convert"])
        .arg(&source)
        .args(["--to", "md", "-o"])
        .arg(directory.path().join("a.md"))
        .output()
        .unwrap();
    assert_eq!(missing.status.code(), Some(5), "{}", stderr(&missing));
    assert!(stderr(&missing).contains("ASHIFT_DOCLING_ENGINE"));

    let no_tesseract = cli_with_engine("-")
        .args(["convert"])
        .arg(&source)
        .args(["--to", "md", "-o"])
        .arg(directory.path().join("b.md"))
        .output()
        .unwrap();
    assert_eq!(no_tesseract.status.code(), Some(5));
    assert!(stderr(&no_tesseract).contains("Tesseract"));
    assert!(!directory.path().join("b.md").exists());
}

#[test]
fn docling_json_converts_and_inspects_without_an_engine() {
    let directory = tempdir().unwrap();
    let source = docling_fixture("vi-route-table.docling.json");
    let output = directory.path().join("route.md");
    let result = cli()
        .args(["convert", &source, "--to", "md", "-o"])
        .arg(&output)
        .output()
        .unwrap();
    assert!(result.status.success(), "{}", stderr(&result));
    let markdown = fs::read_to_string(&output).unwrap();
    assert!(markdown.contains("Bảng hành trình ven sông"));
    assert!(markdown.contains("| **Điểm dừng**"), "{markdown}");

    let inspect = cli().args(["inspect", &source, "--json"]).output().unwrap();
    assert!(inspect.status.success(), "{}", stderr(&inspect));
    let report: Value = serde_json::from_slice(&inspect.stdout).unwrap();
    assert_eq!(report["format"], "docling+json");
    assert!(
        report["counts"]["tables"].as_u64().unwrap() >= 1,
        "{report}"
    );
}

#[test]
fn plan_and_engines_report_the_docling_route() {
    let plan = cli_with_engine(&docling_fixture("en-map-room.docling.json"))
        .args(["plan"])
        .arg(repository().join("fixtures/pdf/en-map-room.pdf"))
        .args(["--to", "docx", "--json"])
        .output()
        .unwrap();
    assert!(plan.status.success(), "{}", stderr(&plan));
    let text = String::from_utf8_lossy(&plan.stdout);
    assert!(text.contains("\"docling\""), "{text}");
    assert!(text.contains("docling+json"), "{text}");

    let engines = cli_with_engine("-")
        .args(["engines", "--json"])
        .output()
        .unwrap();
    assert!(engines.status.success(), "{}", stderr(&engines));
    let rows: Value = serde_json::from_slice(&engines.stdout).unwrap();
    let docling = rows
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == "docling")
        .expect("docling row");
    assert_eq!(docling["status"], "ready");
    assert_eq!(docling["version"], "2.142.0");
}

/// An unpacked docling pack whose engine is the probe answering with `fixture`, or reporting a
/// missing tool for `-`.
fn probe_pack(directory: &Path, fixture: &str) -> PathBuf {
    use sha2::{Digest, Sha256};

    let root = directory.join("docling-pack");
    fs::create_dir(&root).unwrap();
    let program = format!("engine-probe{}", std::env::consts::EXE_SUFFIX);
    fs::copy(engine_probe_path(), root.join(&program)).unwrap();
    let answer = if fixture == "-" {
        "-".to_owned()
    } else {
        fs::copy(fixture, root.join("document.docling.json")).unwrap();
        "{root}/document.docling.json".to_owned()
    };
    let bytes = fs::read(root.join(&program)).unwrap();
    let triple = ariad_host::packs::host_targets().remove(0);
    fs::write(
        root.join("pack.toml"),
        format!(
            r#"schema = "ariad-pack/1"
name = "docling"
version = "0.2.0"
license = "MIT"
requires_ashift = ">=0.1.0"

[[targets]]
triple = "{triple}"
entrypoint = ["{program}", "docling", "{answer}"]

[[targets.components]]
name = "{program}"
url = "{program}"
sha256 = "{digest}"
size = {size}
unpack = "file"
executable = true
"#,
            digest = hex::encode(Sha256::digest(&bytes)),
            size = bytes.len(),
        ),
    )
    .unwrap();
    root
}

#[test]
fn the_docling_pack_provides_the_engine() {
    let directory = tempdir().unwrap();
    let pack = probe_pack(
        directory.path(),
        &docling_fixture("en-map-room.docling.json"),
    );
    let output = directory.path().join("map-room.md");
    let result = cli()
        .env("ASHIFT_PACK_DOCLING", &pack)
        .args(["convert"])
        .arg(repository().join("fixtures/pdf/en-map-room.pdf"))
        .args(["--to", "md", "--json", "-o"])
        .arg(&output)
        .output()
        .unwrap();
    assert!(result.status.success(), "{}", stderr(&result));
    let report: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report["route"][0]["engine"], "docling");
    assert!(
        fs::read_to_string(&output)
            .unwrap()
            .contains("## Notes from the Map Room")
    );

    let engines = cli()
        .env("ASHIFT_PACK_DOCLING", &pack)
        .args(["engines", "--json"])
        .output()
        .unwrap();
    assert!(engines.status.success(), "{}", stderr(&engines));
    let rows: Value = serde_json::from_slice(&engines.stdout).unwrap();
    let docling = rows
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == "docling")
        .expect("docling row");
    assert_eq!(docling["status"], "ready");
    assert_eq!(docling["version"], "0.2.0");
    assert_eq!(docling["pack"]["source"], "override");
}

#[test]
fn the_development_override_wins_over_a_pack() {
    let directory = tempdir().unwrap();
    let pack = probe_pack(directory.path(), "-");
    let output = directory.path().join("map-room.md");
    let result = cli_with_engine(&docling_fixture("en-map-room.docling.json"))
        .env("ASHIFT_PACK_DOCLING", &pack)
        .args(["convert"])
        .arg(repository().join("fixtures/pdf/en-map-room.pdf"))
        .args(["--to", "md", "-o"])
        .arg(&output)
        .output()
        .unwrap();
    assert!(result.status.success(), "{}", stderr(&result));
}

#[cfg(unix)]
#[test]
fn an_artifact_that_links_out_of_the_workspace_is_refused() {
    let directory = tempdir().unwrap();
    let output = directory.path().join("map-room.md");
    let fixture = format!("symlink:{}", docling_fixture("en-map-room.docling.json"));
    let result = cli_with_engine(&fixture)
        .args(["convert"])
        .arg(repository().join("fixtures/pdf/en-map-room.pdf"))
        .args(["--to", "md", "-o"])
        .arg(&output)
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(1), "{}", stderr(&result));
    assert!(!output.exists());
}
