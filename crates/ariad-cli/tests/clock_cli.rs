//! Clock consistency across recipe execution and publication.
#![cfg(unix)]
use serde_json::json;
use std::{fs, path::PathBuf};
use tempfile::TempDir;

fn pdf_fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/pdf-tools")
        .join(name)
}

#[cfg(unix)]
#[test]
fn recipe_stamp_and_output_name_share_the_utc_epoch() {
    let tmp = TempDir::new().unwrap();
    let recipe = tmp.path().join("stamp.json");
    fs::write(&recipe, serde_json::to_vec(&json!({
        "version": "ariad-recipe/1",
        "steps": [{"tool": "pdf.stamp", "params": {"preset": "custom", "text": "Date {date}", "pages": "1"}}],
        "naming": "{stem}-{date}.pdf"
    })).unwrap()).unwrap();
    let output = tmp.path().join("out");
    fs::create_dir(&output).unwrap();
    let invoke = |epoch: &str| {
        std::process::Command::new("sh")
            .args([
                "-c",
                "SOURCE_DATE_EPOCH=$1 exec \"$2\" recipe run \"$3\" \"$4\" -o \"$5\" --yes --json",
                "test",
                epoch,
                env!("CARGO_BIN_EXE_ashift"),
            ])
            .arg(&recipe)
            .arg(pdf_fixture("mixed-sizes.pdf"))
            .arg(&output)
            .output()
            .unwrap()
    };
    let result = invoke("1700000000");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let pdf = fs::read(output.join("mixed-sizes-2023-11-14.pdf")).unwrap();
    let parsed = ariad_core::reader::pdf::read(&pdf, &ariad_core::limits::Limits::local()).unwrap();
    assert!(
        serde_json::to_string(&parsed.document)
            .unwrap()
            .contains("Date 2023-11-14")
    );
    let invalid = invoke("invalid");
    assert!(!invalid.status.success());
    assert!(String::from_utf8_lossy(&invalid.stderr).contains("invalid SOURCE_DATE_EPOCH"));
}
