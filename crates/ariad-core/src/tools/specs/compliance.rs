//! Tool specifications for PDF/A and PDF/UA compliance validation.

use crate::tools::{
    InputArity, Tier, ToolGroup, ToolRuntime, ToolSpec,
    params::{ValidatePdfUaParams, ValidatePdfaParams, schema_for_params},
};

/// Returns the tool specifications registered by this module.
#[must_use]
pub fn specs() -> Vec<ToolSpec> {
    vec![
        ToolSpec {
            id: "pdf.validate_pdfa",
            version: "1",
            group: ToolGroup::Compare,
            summary: "Validate a PDF document against PDF/A profiles using veraPDF.",
            inputs: InputArity::One,
            input_formats: &["pdf"],
            output_format: "json",
            output_formats: &[],
            tier: Tier::Free,
            runtimes: &[ToolRuntime::Local],
            engine: "verapdf",
            params_schema: schema_for_params::<ValidatePdfaParams>,
            secret_params: &[],
        },
        ToolSpec {
            id: "pdf.validate_pdfua",
            version: "1",
            group: ToolGroup::Compare,
            summary: "Validate a PDF document against PDF/UA accessibility profiles using veraPDF.",
            inputs: InputArity::One,
            input_formats: &["pdf"],
            output_format: "json",
            output_formats: &[],
            tier: Tier::Free,
            runtimes: &[ToolRuntime::Local],
            engine: "verapdf",
            params_schema: schema_for_params::<ValidatePdfUaParams>,
            secret_params: &[],
        },
    ]
}
