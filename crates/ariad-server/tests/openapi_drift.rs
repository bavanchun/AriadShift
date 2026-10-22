//! The committed OpenAPI document matches the code.

use std::{fs, path::Path};

#[test]
fn openapi_document_is_committed_and_current() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/openapi.v1.json");
    let mut generated = ariad_server::api::openapi()
        .to_pretty_json()
        .expect("serialize the OpenAPI document");
    generated.push('\n');
    if std::env::var("ARIAD_BLESS_SCHEMAS").as_deref() == Ok("1") {
        fs::write(&path, &generated).expect("write schemas/openapi.v1.json");
        return;
    }
    let committed = fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "could not read {}: {error}. Run ARIAD_BLESS_SCHEMAS=1 cargo test -p ariad-server --test openapi_drift",
            path.display()
        )
    });
    assert!(
        committed.replace("\r\n", "\n") == generated,
        "the OpenAPI document drifted. Run ARIAD_BLESS_SCHEMAS=1 cargo test -p ariad-server --test openapi_drift to update it."
    );
}

#[test]
fn openapi_document_is_version_3_1() {
    let value: serde_json::Value =
        serde_json::from_str(&ariad_server::api::openapi().to_json().expect("serialize"))
            .expect("parse");
    assert!(
        value["openapi"]
            .as_str()
            .is_some_and(|v| v.starts_with("3.1"))
    );
    assert_eq!(value["info"]["title"], "AriadShift API");
    assert!(value["components"]["securitySchemes"]["bearer"].is_object());
    assert!(value["components"]["securitySchemes"]["api_key"].is_object());
}
