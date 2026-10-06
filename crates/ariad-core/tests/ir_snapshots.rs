mod support;

use std::{collections::BTreeMap, fs};

use ariad_core::{
    limits::Limits,
    reader::{html, markdown},
};
use serde_json::Value;

#[test]
fn markdown_fixture_irs_are_snapshotted_and_validate_against_the_schema() {
    let root = support::repository_root()
        .canonicalize()
        .expect("canonicalize repository root");
    let fixtures = support::fixtures::fixtures_for("md->docx", "0")
        .expect("load Markdown-to-DOCX phase 0 fixtures");
    assert!(
        fixtures.len() >= 20,
        "expected at least 20 phase 0 md->docx fixtures, found {}",
        fixtures.len()
    );

    let fixture_by_path: BTreeMap<_, _> = fixtures
        .iter()
        .map(|fixture| {
            let path = root
                .join(&fixture.path)
                .canonicalize()
                .expect("canonicalize manifest fixture path");
            (path, fixture)
        })
        .collect();
    let schema: Value = serde_json::from_str(
        &fs::read_to_string(support::ir_schema_path()).expect("read generated IR schema"),
    )
    .expect("parse generated IR schema");
    let validator = jsonschema::validator_for(&schema).expect("compile generated IR schema");
    let mut matched = 0;

    // The three-argument form is required because this glob walks above tests/.
    insta::glob!("../../../fixtures", "md/*.md", |path| {
        matched += 1;
        let canonical_path = path.canonicalize().expect("canonicalize Markdown fixture");
        let fixture = fixture_by_path.get(&canonical_path).unwrap_or_else(|| {
            panic!(
                "Markdown fixture {} is not listed for md->docx phase 0",
                path.display()
            )
        });
        let markdown_text = fs::read_to_string(path).expect("read UTF-8 Markdown fixture");
        let result = markdown::read(&markdown_text, &Limits::local())
            .unwrap_or_else(|error| panic!("parse fixture {}: {error}", fixture.id));
        let value = serde_json::to_value(&result.document).expect("serialize fixture IR");
        let errors = validator
            .iter_errors(&value)
            .map(|error| error.to_string())
            .collect::<Vec<_>>();
        assert!(
            errors.is_empty(),
            "IR for fixture {} does not validate:\n{}",
            fixture.id,
            errors.join("\n")
        );

        let snapshot_name = format!("{}.ir", path.file_stem().unwrap().to_string_lossy());
        insta::with_settings!({
            snapshot_path => "../../../fixtures/golden",
            prepend_module_to_snapshot => false,
            omit_expression => true,
            snapshot_suffix => "",
        }, {
            insta::assert_json_snapshot!(snapshot_name, value);
        });
    });

    assert_eq!(
        matched,
        fixtures.len(),
        "all manifest fixtures for md->docx phase 0 must be snapshotted"
    );
    assert!(matched >= 20, "at least 20 Markdown fixtures must match");
}

#[test]
fn html_fixture_irs_are_snapshotted_and_validate_against_the_schema() {
    let root = support::repository_root()
        .canonicalize()
        .expect("canonicalize repository root");
    let fixtures = support::fixtures::fixtures_for("html->md", "1a")
        .expect("load HTML-to-Markdown phase 1a fixtures");
    assert_eq!(
        fixtures.len(),
        6,
        "expected exactly 6 phase 1a html->md fixtures, found {}",
        fixtures.len()
    );

    let schema: Value = serde_json::from_str(
        &fs::read_to_string(support::ir_schema_path()).expect("read generated IR schema"),
    )
    .expect("parse generated IR schema");
    let validator = jsonschema::validator_for(&schema).expect("compile generated IR schema");

    for fixture in &fixtures {
        let path = root.join(&fixture.path);
        let html_bytes =
            fs::read(&path).unwrap_or_else(|error| panic!("read fixture {}: {error}", fixture.id));
        let result = html::read(&html_bytes, &Limits::local())
            .unwrap_or_else(|error| panic!("parse fixture {}: {error}", fixture.id));
        let value = serde_json::to_value(&result.document).expect("serialize fixture IR");
        let errors = validator
            .iter_errors(&value)
            .map(|error| error.to_string())
            .collect::<Vec<_>>();
        assert!(
            errors.is_empty(),
            "IR for fixture {} does not validate:\n{}",
            fixture.id,
            errors.join("\n")
        );

        let snapshot_name = format!("{}.ir", fixture.id);
        insta::with_settings!({
            snapshot_path => "../../../fixtures/golden",
            prepend_module_to_snapshot => false,
            omit_expression => true,
            snapshot_suffix => "",
        }, {
            insta::assert_json_snapshot!(snapshot_name, value);
        });
    }
}
