mod support;

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use support::fixtures::{fixtures_for, load_manifest, validate_manifest};

struct RemoveOnDrop(PathBuf);

impl Drop for RemoveOnDrop {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

#[test]
fn fixture_manifest_is_complete_and_valid() {
    let manifest = load_manifest().expect("fixture manifest should parse");
    let errors = validate_manifest(&manifest);
    assert!(
        errors.is_empty(),
        "fixture manifest validation failed:\n{}",
        errors.join("\n")
    );

    let mut invalid_manifest = manifest.clone();
    invalid_manifest.fixture[1].id = invalid_manifest.fixture[0].id.clone();
    invalid_manifest.fixture[2].sha256 = "0".repeat(64);
    invalid_manifest.fixture[3].license = "CC-BY-SA-4.0".to_owned();
    invalid_manifest.fixture[4]
        .tags
        .push("unknown-tag".to_owned());
    invalid_manifest.fixture.truncate(49);
    let errors = validate_manifest(&invalid_manifest);
    assert!(
        errors
            .iter()
            .any(|error| error.contains("duplicate fixture id"))
    );
    assert!(
        errors
            .iter()
            .any(|error| error.contains("disallowed license CC-BY-SA-4.0"))
    );
    assert!(
        errors
            .iter()
            .any(|error| error.contains("SHA-256 mismatch"))
    );
    assert!(
        errors
            .iter()
            .any(|error| error.contains("unknown tag unknown-tag"))
    );
    assert!(
        errors
            .iter()
            .any(|error| error.contains("at least 50 are required"))
    );

    let root = support::fixtures::repository_root();
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock should be after the Unix epoch")
        .as_nanos();
    let relative_path = format!("fixtures/unlisted-check-{}-{nonce}.tmp", std::process::id());
    let path = root.join(&relative_path);
    let _cleanup = RemoveOnDrop(path.clone());
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .expect("temporary unlisted fixture path should be available");
    file.write_all(b"temporary checker probe")
        .expect("temporary probe should be written");
    drop(file);
    let errors = validate_manifest(&manifest);
    assert!(errors.iter().any(|error| error.contains(&relative_path)));
}

#[test]
fn fixtures_for_selects_the_requested_route_and_phase() {
    let fixtures = fixtures_for("md->docx", "0").expect("fixture manifest should parse");
    assert_eq!(fixtures.len(), 28);
    assert!(
        fixtures
            .iter()
            .all(|fixture| fixture.routes.iter().any(|route| route == "md->docx"))
    );
    assert!(fixtures.iter().all(|fixture| fixture.phase == "0"));
}
