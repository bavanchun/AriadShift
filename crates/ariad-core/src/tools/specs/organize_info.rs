//! Tool specifications for the `organize` group (`pdf.info`).

use crate::tools::{
    InputArity, Tier, ToolGroup, ToolRuntime, ToolSpec,
    params::{info::InfoParams, schema_for_params},
};

/// Returns the tool specifications registered by this module.
#[must_use]
pub fn specs() -> Vec<ToolSpec> {
    vec![ToolSpec {
        id: "pdf.info",
        version: "1",
        group: ToolGroup::Organize,
        summary: "Inspect PDF version, page count, encryption status, geometry, metadata, and structural features.",
        inputs: InputArity::One,
        input_formats: &["pdf"],
        output_format: "json",
        output_formats: &[],
        tier: Tier::Free,
        runtimes: &[ToolRuntime::Wasm, ToolRuntime::Local],
        engine: "ariad-pdf",
        params_schema: schema_for_params::<InfoParams>,
        secret_params: &[],
    }]
}
