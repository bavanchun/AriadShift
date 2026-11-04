//! Tool specification for `pdf.to_cbz`.

use crate::tools::{
    InputArity, Tier, ToolGroup, ToolRuntime, ToolSpec,
    params::{ToCbzParams, schema_for_params},
};

/// Host composite engine identifier.
pub const HOST_COMPOSITE_ENGINE: &str = "ariad-host";

/// Allowed artifact formats for `pdf.to_cbz`.
pub const TO_CBZ_OUTPUT_FORMATS: &[&str] = &["cbz"];

/// Returns the tool specifications registered by this module.
#[must_use]
pub fn specs() -> Vec<ToolSpec> {
    vec![ToolSpec {
        id: "pdf.to_cbz",
        version: "1",
        group: ToolGroup::Convert,
        summary: "Convert PDF pages into a comic book archive (CBZ).",
        inputs: InputArity::One,
        input_formats: &["pdf"],
        output_format: "cbz",
        output_formats: TO_CBZ_OUTPUT_FORMATS,
        tier: Tier::Free,
        runtimes: &[ToolRuntime::Local],
        engine: HOST_COMPOSITE_ENGINE,
        params_schema: schema_for_params::<ToCbzParams>,
        secret_params: &[],
    }]
}
