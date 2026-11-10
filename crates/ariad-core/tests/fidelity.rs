use ariad_core::fidelity::{self, FidelityBasis, Observations};
use ariad_core::ir::Document;
use serde_json::Value;

fn assert_subset(actual: &Value, expected: &Value) {
    match expected {
        Value::Object(fields) => {
            for (key, value) in fields {
                assert_subset(&actual[key], value);
            }
        }
        Value::Number(value) => assert!(
            (actual.as_f64().unwrap() - value.as_f64().unwrap()).abs() <= 1e-12,
            "{actual} != {expected}"
        ),
        _ => assert_eq!(actual, expected),
    }
}

#[test]
fn fidelity_python_rust_vectors_agree() {
    let vectors: Value = serde_json::from_str(include_str!(
        "../../../bench/results/fidelity-v2/conformance.json"
    ))
    .unwrap();
    for vector in vectors["vectors"].as_array().unwrap() {
        let reference: Observations = serde_json::from_value(vector["reference"].clone()).unwrap();
        let hypothesis: Observations =
            serde_json::from_value(vector["hypothesis"].clone()).unwrap();
        let report = fidelity::score(&reference, &hypothesis, FidelityBasis::IndependentReference);
        assert_subset(&serde_json::to_value(report).unwrap(), &vector["expected"]);
    }
    for vector in vectors["ir_vectors"].as_array().unwrap() {
        let reference: Document = serde_json::from_value(vector["reference"].clone()).unwrap();
        let hypothesis: Document = serde_json::from_value(vector["hypothesis"].clone()).unwrap();
        if let Some(expected) = vector.get("reference_observations") {
            assert_subset(
                &serde_json::to_value(fidelity::observe(&reference).unwrap()).unwrap(),
                expected,
            );
        }
        let report =
            fidelity::compare(&reference, &hypothesis, FidelityBasis::IndependentReference);
        assert_subset(&serde_json::to_value(report).unwrap(), &vector["expected"]);
    }
}

#[test]
fn observation_extraction_bounds_depth_before_semantic_walk() {
    let mut inline = serde_json::json!({"type":"text", "text":"abc"});
    for _ in 0..70 {
        inline = serde_json::json!({"type":"strong", "content":[inline]});
    }
    let document: Document = serde_json::from_value(serde_json::json!({
        "body":[{"type":"paragraph", "id":"a", "content":[inline]}]
    }))
    .unwrap();
    assert_eq!(
        fidelity::observe(&document).unwrap_err(),
        "observation_limit"
    );
}
