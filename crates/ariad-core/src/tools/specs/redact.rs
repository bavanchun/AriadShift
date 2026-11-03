//! Redaction tool specifications.

use crate::tools::{
    InputArity, Tier, ToolGroup, ToolRuntime, ToolSpec,
    params::{
        redact::{RedactParams, RedactVerifyParams},
        schema_for_params,
    },
    specs::ocr_scan::HOST_COMPOSITE_ENGINE,
};
use schemars::{JsonSchema, Schema};

fn spec<P: JsonSchema>(
    id: &'static str,
    summary: &'static str,
    group: ToolGroup,
    inputs: InputArity,
    input_formats: &'static [&'static str],
    output_formats: &'static [&'static str],
    engine: &'static str,
) -> ToolSpec {
    let schema: fn() -> Schema = schema_for_params::<P>;
    ToolSpec {
        id,
        version: "1",
        group,
        summary,
        inputs,
        input_formats,
        output_format: output_formats[0],
        output_formats,
        tier: Tier::Free,
        runtimes: &[ToolRuntime::Local],
        engine,
        params_schema: schema,
        secret_params: &[],
    }
}

/// Public redaction tool specifications.
#[must_use]
pub fn specs() -> Vec<ToolSpec> {
    vec![
        spec::<RedactParams>(
            "pdf.redact",
            "True object-stream and raster redaction with verification report.",
            ToolGroup::Redact,
            InputArity::One,
            &["pdf"],
            &["pdf"],
            HOST_COMPOSITE_ENGINE,
        ),
        spec::<RedactVerifyParams>(
            "pdf.redact_verify",
            "Independently verify document redaction against requested targets.",
            ToolGroup::Redact,
            InputArity::One,
            &["pdf"],
            &["json"],
            HOST_COMPOSITE_ENGINE,
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redact_specs_are_valid() {
        let list = specs();
        assert_eq!(list.len(), 2);
        crate::tools::validate_specs(&list).unwrap();
        assert_eq!(list[0].id, "pdf.redact");
        assert_eq!(list[0].output_format, "pdf");
        assert_eq!(list[1].id, "pdf.redact_verify");
        assert_eq!(list[1].output_format, "json");
    }
}
