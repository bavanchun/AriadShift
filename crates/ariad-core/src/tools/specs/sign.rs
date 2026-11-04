//! PDF digital signature tool specifications.

use crate::tools::{
    InputArity, Tier, ToolGroup, ToolRuntime, ToolSpec,
    params::{SignParams, SignRemoveParams, SignVerifyParams, schema_for_params},
};

/// Returns the tool specifications in this group.
#[must_use]
pub fn specs() -> Vec<ToolSpec> {
    vec![
        ToolSpec {
            id: "pdf.sign",
            version: "1",
            group: ToolGroup::Sign,
            summary: "Sign a PDF document with a PAdES digital signature.",
            inputs: InputArity::One,
            input_formats: &["pdf"],
            output_format: "pdf",
            output_formats: &[],
            tier: Tier::Free,
            runtimes: &[ToolRuntime::Local],
            engine: "sign",
            params_schema: schema_for_params::<SignParams>,
            secret_params: &["certificate", "password", "key_password"],
        },
        ToolSpec {
            id: "pdf.sign_verify",
            version: "1",
            group: ToolGroup::Sign,
            summary: "Verify digital signatures in a PDF document.",
            inputs: InputArity::One,
            input_formats: &["pdf"],
            output_format: "json",
            output_formats: &[],
            tier: Tier::Free,
            runtimes: &[ToolRuntime::Local],
            engine: "sign",
            params_schema: schema_for_params::<SignVerifyParams>,
            secret_params: &[],
        },
        ToolSpec {
            id: "pdf.sign_remove",
            version: "1",
            group: ToolGroup::Sign,
            summary: "Remove digital signatures from a PDF document.",
            inputs: InputArity::One,
            input_formats: &["pdf"],
            output_format: "pdf",
            output_formats: &[],
            tier: Tier::Free,
            runtimes: &[ToolRuntime::Local],
            engine: "sign",
            params_schema: schema_for_params::<SignRemoveParams>,
            secret_params: &[],
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn specifications_are_valid() {
        let specs = specs();
        crate::tools::validate_specs(&specs).unwrap();
        assert_eq!(specs.len(), 3);
        assert!(specs.iter().all(|s| s.engine == "sign"));
    }
}
