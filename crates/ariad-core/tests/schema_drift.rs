mod support;

use std::fs;

#[test]
fn ir_schema_matches_the_generated_document_schema() {
    let path = support::ir_schema_path();
    let expected = support::generated_ir_schema();

    if std::env::var("ARIAD_BLESS_SCHEMAS").as_deref() == Ok("1") {
        fs::create_dir_all(path.parent().expect("schema path has a parent"))
            .expect("create schema directory");
        fs::write(&path, expected).expect("write generated IR schema");
        return;
    }

    let actual = fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "could not read {}: {error}. Run ARIAD_BLESS_SCHEMAS=1 cargo test -p ariad-core --test schema_drift",
            path.display()
        )
    });
    assert_eq!(
        actual, expected,
        "IR schema drifted. Run ARIAD_BLESS_SCHEMAS=1 cargo test -p ariad-core --test schema_drift to update it."
    );
}
