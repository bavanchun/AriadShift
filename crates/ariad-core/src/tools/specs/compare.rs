//! Tool specification for `pdf.compare`.

use crate::tools::{
    InputArity, Tier, ToolGroup, ToolRuntime, ToolSpec,
    params::{CompareParams, schema_for_params},
};

/// Allowed output artifact formats for `pdf.compare`.
pub const PDF_COMPARE_OUTPUT_FORMATS: &[&str] = &["json", "pdf"];

const RUNTIMES: &[ToolRuntime] = &[ToolRuntime::Wasm, ToolRuntime::Local, ToolRuntime::Cloud];

/// Specification for `pdf.compare`.
pub const PDF_COMPARE_SPEC: ToolSpec = ToolSpec {
    id: "pdf.compare",
    version: "1",
    group: ToolGroup::Compare,
    summary: "Compare two PDF documents semantically or visually with annotated difference report.",
    inputs: InputArity::Many { min: 2, max: 2 },
    input_formats: &["pdf"],
    output_format: "json",
    output_formats: PDF_COMPARE_OUTPUT_FORMATS,
    tier: Tier::Free,
    runtimes: RUNTIMES,
    engine: "ariad-pdf",
    params_schema: schema_for_params::<CompareParams>,
    secret_params: &[],
};

/// Returns the tool specifications registered by this module.
#[must_use]
pub fn specs() -> Vec<ToolSpec> {
    vec![PDF_COMPARE_SPEC]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compare_specification_is_valid() {
        let specs = specs();
        assert_eq!(specs.len(), 1);
        crate::tools::validate_specs(&specs).unwrap();
        assert_eq!(specs[0].id, "pdf.compare");
        assert_eq!(specs[0].group, ToolGroup::Compare);
        assert!(specs[0].allows_artifact_format("json", &serde_json::json!({})));
        assert!(specs[0].allows_artifact_format("pdf", &serde_json::json!({})));
        assert!(!specs[0].allows_artifact_format("png", &serde_json::json!({})));

        assert_eq!(
            specs[0].engine_for_params(&serde_json::json!({})),
            "ariad-pdf"
        );
        assert_eq!(
            specs[0].engine_for_params(&serde_json::json!({"mode": "text"})),
            "ariad-pdf"
        );
        assert_eq!(
            specs[0].engine_for_params(&serde_json::json!({"mode": "visual"})),
            "pdfium"
        );
        assert_eq!(
            specs[0].engine_for_params(&serde_json::json!({"mode": "both"})),
            "pdfium"
        );
    }
}
