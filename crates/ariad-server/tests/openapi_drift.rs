//! The committed OpenAPI document matches the code.

use std::fs;

const REGENERATE: &str = "run `just openapi` and commit schemas/openapi.v1.json";

#[test]
fn openapi_document_is_committed_and_current() {
    let path = ariad_server::api::openapi_document_path();
    let generated = ariad_server::api::openapi_json().expect("serialize the OpenAPI document");
    let committed = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("could not read {}: {error}; {REGENERATE}", path.display()));
    assert!(
        committed.replace("\r\n", "\n") == generated,
        "schemas/openapi.v1.json is stale: {REGENERATE}"
    );
}

#[test]
fn openapi_document_describes_the_v1_contract() {
    let json = ariad_server::api::openapi_json().expect("serialize");
    let value: serde_json::Value = serde_json::from_str(&json).expect("parse");
    assert!(
        value["openapi"]
            .as_str()
            .is_some_and(|v| v.starts_with("3.1"))
    );
    assert_eq!(value["info"]["title"], "AriadShift API");

    let schemes = &value["components"]["securitySchemes"];
    assert_eq!(schemes["bearer"]["type"], "http");
    assert_eq!(schemes["bearer"]["scheme"], "bearer");
    assert_eq!(schemes["bearer"]["bearerFormat"], "JWT");
    assert!(
        schemes["bearer"]["description"]
            .as_str()
            .is_some_and(|d| d.contains("JWKS"))
    );
    assert_eq!(schemes["api_key"]["type"], "apiKey");
    assert_eq!(schemes["api_key"]["in"], "header");
    assert_eq!(schemes["api_key"]["name"], "x-api-key");

    let paths = &value["paths"];
    let events = &paths["/v1/jobs/{id}/events"]["get"]["responses"]["200"]["content"];
    assert!(events["text/event-stream"].is_object(), "SSE events");
    let put = &paths["/v1/blobs/{token}"]["put"];
    assert_eq!(
        put["requestBody"]["content"]["application/octet-stream"]["schema"]["format"],
        "binary"
    );
    assert!(put.get("security").is_none());
    let get = &paths["/v1/blobs/{token}"]["get"]["responses"]["200"]["content"];
    assert!(get["application/octet-stream"].is_object());
    assert!(
        paths["/v1/jobs"]["post"]["responses"]["429"]["headers"]["retry-after"].is_object(),
        "rate-limited responses document Retry-After"
    );
}
