//! Tool specification for `web.to_pdf`.

use crate::tools::{
    InputArity, Tier, ToolGroup, ToolRuntime, ToolSpec,
    params::{WebToPdfParams, schema_for_params},
    specs::ocr_scan::HOST_COMPOSITE_ENGINE,
};

/// Allowed artifact formats for `web.to_pdf`.
pub const WEB_TO_PDF_OUTPUT_FORMATS: &[&str] = &["pdf"];

/// Returns the tool specifications registered by this module.
#[must_use]
pub fn specs() -> Vec<ToolSpec> {
    vec![ToolSpec {
        id: "web.to_pdf",
        version: "1",
        group: ToolGroup::Convert,
        summary: "Fetch a web page securely and convert it offline to PDF.",
        inputs: InputArity::None,
        input_formats: &[],
        output_format: "pdf",
        output_formats: WEB_TO_PDF_OUTPUT_FORMATS,
        tier: Tier::Free,
        runtimes: &[ToolRuntime::Local],
        engine: HOST_COMPOSITE_ENGINE,
        params_schema: schema_for_params::<WebToPdfParams>,
        secret_params: &[],
    }]
}
