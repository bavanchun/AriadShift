//! PDF optimize tool specifications.
use crate::tools::{
    InputArity, Tier, ToolGroup, ToolRuntime, ToolSpec,
    params::{CompressParams, GrayscaleParams, LinearizeParams, RepairParams, schema_for_params},
};
/// Returns the tool specifications in this group.
#[must_use]
pub fn specs() -> Vec<ToolSpec> {
    vec![
        ToolSpec {
            id: "pdf.compress",
            version: "1",
            group: ToolGroup::Optimize,
            summary: "Compress a PDF document.",
            inputs: InputArity::One,
            input_formats: &["pdf"],
            output_format: "pdf",
            output_formats: &[],
            tier: Tier::Free,
            runtimes: &[ToolRuntime::Local, ToolRuntime::Cloud],
            engine: "ariad-pdf",
            params_schema: schema_for_params::<CompressParams>,
            secret_params: &[],
        },
        ToolSpec {
            id: "pdf.grayscale",
            version: "1",
            group: ToolGroup::Optimize,
            summary: "Convert a PDF document to grayscale.",
            inputs: InputArity::One,
            input_formats: &["pdf"],
            output_format: "pdf",
            output_formats: &[],
            tier: Tier::Free,
            runtimes: &[ToolRuntime::Local, ToolRuntime::Cloud],
            engine: "ariad-pdf",
            params_schema: schema_for_params::<GrayscaleParams>,
            secret_params: &[],
        },
        ToolSpec {
            id: "pdf.repair",
            version: "1",
            group: ToolGroup::Optimize,
            summary: "Repair a damaged PDF document.",
            inputs: InputArity::One,
            input_formats: &["pdf"],
            output_format: "pdf",
            output_formats: &[],
            tier: Tier::Free,
            runtimes: &[ToolRuntime::Local, ToolRuntime::Cloud],
            engine: "qpdf",
            params_schema: schema_for_params::<RepairParams>,
            secret_params: &[],
        },
        ToolSpec {
            id: "pdf.linearize",
            version: "1",
            group: ToolGroup::Optimize,
            summary: "Optimize a PDF document for progressive loading.",
            inputs: InputArity::One,
            input_formats: &["pdf"],
            output_format: "pdf",
            output_formats: &[],
            tier: Tier::Free,
            runtimes: &[ToolRuntime::Local, ToolRuntime::Cloud],
            engine: "qpdf",
            params_schema: schema_for_params::<LinearizeParams>,
            secret_params: &[],
        },
    ]
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn specifications_are_valid_and_cloud_eligible() {
        let specs = specs();
        crate::tools::validate_specs(&specs).unwrap();
        assert!(
            specs
                .iter()
                .all(|s| s.runtimes.contains(&ToolRuntime::Cloud))
        );
        for spec in specs {
            let schema = serde_json::to_value(spec.resolved_params_schema()).unwrap();
            for field in spec.secret_params {
                assert_eq!(schema["properties"][field]["x-ariad-secret"], true);
            }
        }
    }
}
