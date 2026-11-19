//! Checks of an engine's stdout against the engine protocol.

use std::fs;

use serde_json::Value;

use super::fixtures::repository_root;

/// Parses engine stdout as protocol events and checks every line against the protocol schema,
/// that exactly one `result` arrives, and that it comes last.
pub fn validate_stdout(stdout: &[u8]) -> Vec<Value> {
    let schema: Value = serde_json::from_slice(
        &fs::read(repository_root().join("schemas/engine-protocol.v1.json"))
            .expect("read protocol schema"),
    )
    .expect("parse protocol schema");
    let validator = jsonschema::validator_for(&schema).expect("compile protocol schema");
    let text = std::str::from_utf8(stdout).expect("engine stdout is UTF-8");
    let values = text
        .lines()
        .map(|line| serde_json::from_str(line).expect("engine line is JSON"))
        .collect::<Vec<Value>>();
    assert!(!values.is_empty(), "engine must emit a protocol event");
    for value in &values {
        let errors = validator
            .iter_errors(value)
            .map(|error| error.to_string())
            .collect::<Vec<_>>();
        assert!(
            errors.is_empty(),
            "protocol schema errors: {}",
            errors.join("\n")
        );
    }
    let results = values
        .iter()
        .enumerate()
        .filter(|(_, value)| value["type"] == "result")
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    assert_eq!(results.len(), 1, "exactly one result is required");
    assert_eq!(results[0], values.len() - 1, "result must come last");
    values
}
