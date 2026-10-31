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

#[test]
fn engine_protocol_schema_matches_the_generated_request_schema() {
    let path = support::engine_protocol_schema_path();
    let expected = support::generated_engine_protocol_schema();

    if std::env::var("ARIAD_BLESS_SCHEMAS").as_deref() == Ok("1") {
        fs::create_dir_all(path.parent().expect("schema path has a parent"))
            .expect("create schema directory");
        fs::write(&path, expected).expect("write generated engine protocol schema");
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
        "engine protocol schema drifted. Run ARIAD_BLESS_SCHEMAS=1 cargo test -p ariad-core --test schema_drift to update it."
    );
}

#[test]
#[ignore]
fn regenerate_engine_protocol_v2_schema() {
    let path = support::engine_protocol_v2_schema_path();
    let expected = support::generated_engine_protocol_v2_schema();
    fs::create_dir_all(path.parent().expect("schema path has a parent"))
        .expect("create schema directory");
    fs::write(&path, expected).expect("write generated engine protocol v2 schema");
}

#[test]
fn engine_protocol_v2_schema_matches_the_generated_request_schema() {
    let path = support::engine_protocol_v2_schema_path();
    let expected = support::generated_engine_protocol_v2_schema();

    if std::env::var("ARIAD_BLESS_SCHEMAS").as_deref() == Ok("1") {
        fs::create_dir_all(path.parent().expect("schema path has a parent"))
            .expect("create schema directory");
        fs::write(&path, expected).expect("write generated engine protocol v2 schema");
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
        "engine protocol v2 schema drifted. Run ARIAD_BLESS_SCHEMAS=1 cargo test -p ariad-core --test schema_drift to update it."
    );
}

#[test]
fn engine_protocol_v2_schema_validates_messages_and_rejects_invalid_requests() {
    let schema_path = support::engine_protocol_v2_schema_path();
    let schema_text = fs::read_to_string(&schema_path).expect("read engine-protocol.v2.json");
    let schema_val: serde_json::Value =
        serde_json::from_str(&schema_text).expect("parse engine-protocol.v2.json");
    assert_eq!(schema_val["x-status"], "draft");
    let validator =
        jsonschema::validator_for(&schema_val).expect("compile engine-protocol.v2.json validator");

    let valid_run_tool = serde_json::json!({
        "protocol": "ariad-engine/2",
        "job": "job_01KXYZ",
        "op": "run_tool",
        "tool": "pdf.rotate",
        "inputs": [{"path": "/work/in/0.pdf", "format": "pdf"}],
        "output": {"dir": "/work/out"},
        "params": {"pages": "1-3,5", "angle": 90},
        "work_dir": "/work/tmp",
        "limits": {"max_pages": null, "timeout_s": null, "max_memory_mb": null, "max_nesting_depth": 64}
    });
    assert!(
        validator.is_valid(&valid_run_tool),
        "valid run_tool must pass v2 schema"
    );

    let valid_capabilities = serde_json::json!({
        "type": "capabilities",
        "protocol": "ariad-engine/2",
        "engine": "pdfium",
        "version": "0.1.0",
        "tool": {"name": "pdfium", "version": "158.0.8125.0", "status": "found"},
        "license": "BSD-3-Clause AND Apache-2.0",
        "routes": [],
        "tools": [{"id": "pdf.rotate", "version": "1"}],
        "enforces_memory_limit": false,
        "supports_session": false
    });
    assert!(
        validator.is_valid(&valid_capabilities),
        "valid v2 capabilities must pass v2 schema"
    );

    let mut unknown_op = valid_run_tool.clone();
    unknown_op["op"] = serde_json::json!("unknown_op");
    assert!(
        !validator.is_valid(&unknown_op),
        "v2 schema must reject unknown op"
    );

    let mut empty_inputs = valid_run_tool.clone();
    empty_inputs["inputs"] = serde_json::json!([]);
    assert!(
        !validator.is_valid(&empty_inputs),
        "v2 schema must reject empty inputs"
    );
}

#[test]
fn capabilities_schema_matches_the_generated_capabilities_schema() {
    let path = support::capabilities_schema_path();
    let expected = support::generated_capabilities_schema();

    if std::env::var("ARIAD_BLESS_SCHEMAS").as_deref() == Ok("1") {
        fs::create_dir_all(path.parent().expect("schema path has a parent"))
            .expect("create schema directory");
        fs::write(&path, expected).expect("write generated capabilities schema");
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
        "capabilities schema drifted. Run ARIAD_BLESS_SCHEMAS=1 cargo test -p ariad-core --test schema_drift to update it."
    );
}

#[test]
fn embedded_capabilities_parse_and_validate() {
    let caps = ariad_core::planner::embedded();
    assert_eq!(caps.version, "ariad-capabilities/0");
    assert!(!caps.edges.is_empty());
    for edge in &caps.edges {
        assert_ne!(edge.from, edge.to, "no self loops");
        assert!(!edge.engine.trim().is_empty(), "engine must not be empty");
        assert!(!edge.runtime.is_empty(), "runtime must not be empty");
        assert!(!edge.license.trim().is_empty(), "license must not be empty");
    }
    caps.validate()
        .expect("embedded capabilities must be valid");
}

#[test]
fn embedded_capabilities_validates_against_committed_schema() {
    let schema_path = support::capabilities_schema_path();
    let schema_text = fs::read_to_string(&schema_path).expect("read capabilities schema");
    let schema_val: serde_json::Value =
        serde_json::from_str(&schema_text).expect("parse capabilities schema JSON");
    let validator =
        jsonschema::validator_for(&schema_val).expect("compile capabilities schema validator");

    let embedded_json = include_str!("../data/capabilities.json");
    let embedded_val: serde_json::Value =
        serde_json::from_str(embedded_json).expect("parse embedded capabilities.json");

    let validation_errors: Vec<String> = validator
        .iter_errors(&embedded_val)
        .map(|e| format!("{}: {}", e.instance_path(), e))
        .collect();
    assert!(
        validation_errors.is_empty(),
        "embedded capabilities.json must validate against committed capabilities.v0.json: {validation_errors:?}"
    );
}

#[test]
fn capabilities_schema_rejects_out_of_range_values_and_unknown_fields() {
    let schema_path = support::capabilities_schema_path();
    let schema_text = fs::read_to_string(&schema_path).expect("read capabilities schema");
    let schema_val: serde_json::Value =
        serde_json::from_str(&schema_text).expect("parse capabilities schema JSON");
    let validator =
        jsonschema::validator_for(&schema_val).expect("compile capabilities schema validator");

    let valid_json = include_str!("../data/capabilities.json");
    let base_val: serde_json::Value =
        serde_json::from_str(valid_json).expect("parse embedded capabilities.json");

    // 1. Unknown field in root
    let mut unknown_root = base_val.clone();
    unknown_root["bogus_field"] = serde_json::json!("forbidden");
    assert!(
        !validator.is_valid(&unknown_root),
        "schema must reject unknown root fields"
    );

    // 2. Unknown field in edge
    let mut unknown_edge = base_val.clone();
    unknown_edge["edges"][0]["extra"] = serde_json::json!(123);
    assert!(
        !validator.is_valid(&unknown_edge),
        "schema must reject unknown edge fields"
    );

    // 3. Out-of-range fidelity (> 1.0)
    let mut bad_fidelity = base_val.clone();
    bad_fidelity["edges"][0]["metrics"] = serde_json::json!({
        "fidelity": 1.5,
        "editability": 0.8,
        "p50_ms": 100.0,
        "peak_mem_mb": 50.0,
        "samples": 10
    });
    assert!(
        !validator.is_valid(&bad_fidelity),
        "schema must reject fidelity > 1.0"
    );

    // 4. Out-of-range samples (0)
    let mut zero_samples = base_val.clone();
    zero_samples["edges"][0]["metrics"] = serde_json::json!({
        "fidelity": 0.9,
        "editability": 0.8,
        "p50_ms": 100.0,
        "peak_mem_mb": 50.0,
        "samples": 0
    });
    assert!(
        !validator.is_valid(&zero_samples),
        "schema must reject samples < 1"
    );

    // 5. Negative p50_ms
    let mut neg_p50 = base_val.clone();
    neg_p50["edges"][0]["metrics"] = serde_json::json!({
        "fidelity": 0.9,
        "editability": 0.8,
        "p50_ms": -10.0,
        "peak_mem_mb": 50.0,
        "samples": 5
    });
    assert!(
        !validator.is_valid(&neg_p50),
        "schema must reject negative p50_ms"
    );

    // 6. Unknown runtime
    let mut bad_runtime = base_val.clone();
    bad_runtime["edges"][0]["runtime"] = serde_json::json!(["cloud_orbit"]);
    assert!(
        !validator.is_valid(&bad_runtime),
        "schema must reject unrecognized runtime enum variant"
    );

    // 7. Duplicate runtime (uniqueItems: true)
    let mut dup_runtime = base_val.clone();
    dup_runtime["edges"][0]["runtime"] = serde_json::json!(["local", "local"]);
    assert!(
        !validator.is_valid(&dup_runtime),
        "schema must reject duplicate runtimes in an edge"
    );
}

#[test]
fn capabilities_serde_rejects_missing_metrics_key() {
    let json_missing_metrics = r#"{
        "from": "markdown",
        "to": "ariad-ir+json",
        "engine": "ariad-core",
        "runtime": ["wasm", "local"],
        "license": "Apache-2.0"
    }"#;
    let res: Result<ariad_core::planner::CapabilityEdge, _> =
        serde_json::from_str(json_missing_metrics);
    assert!(res.is_err(), "serde must reject edge missing 'metrics' key");

    // But explicit null is accepted
    let json_null_metrics = r#"{
        "from": "markdown",
        "to": "ariad-ir+json",
        "engine": "ariad-core",
        "runtime": ["wasm", "local"],
        "license": "Apache-2.0",
        "metrics": null
    }"#;
    let res_null: Result<ariad_core::planner::CapabilityEdge, _> =
        serde_json::from_str(json_null_metrics);
    assert!(res_null.is_ok(), "serde must accept 'metrics': null");
}

#[test]
fn frozen_ir_schema_requires_the_current_version_and_retains_the_legacy_schema() {
    let schema: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(support::ir_schema_path()).unwrap()).unwrap();
    assert_eq!(schema["x-status"], "final");
    let validator = jsonschema::validator_for(&schema).unwrap();
    let mut document = serde_json::to_value(ariad_core::ir::Document::default()).unwrap();
    assert!(validator.is_valid(&document));
    document["version"] = serde_json::json!("ariad-ir/0");
    assert!(!validator.is_valid(&document));
    let legacy: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(support::repository_root().join("schemas/ir.v0.json")).unwrap(),
    )
    .unwrap();
    assert!(
        jsonschema::validator_for(&legacy)
            .unwrap()
            .is_valid(&document)
    );
    document.as_object_mut().unwrap().remove("version");
    assert!(!validator.is_valid(&document));
}
