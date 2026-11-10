//! Specifications for colour adjustment tools.

use crate::tools::{
    InputArity, Tier, ToolGroup, ToolRuntime, ToolSpec,
    params::{AdjustColorsParams, ReplaceColorParams},
};
use schemars::{Schema, generate::SchemaSettings};

fn replace_schema() -> Schema {
    SchemaSettings::draft2020_12()
        .into_generator()
        .into_root_schema_for::<ReplaceColorParams>()
}
fn adjust_schema() -> Schema {
    SchemaSettings::draft2020_12()
        .into_generator()
        .into_root_schema_for::<AdjustColorsParams>()
}

/// Colour tools run in the pure-Rust PDF engine.
#[must_use]
pub fn specs() -> Vec<ToolSpec> {
    vec![
        ToolSpec {
            id: "pdf.replace_color",
            version: "1",
            group: ToolGroup::Overlay,
            summary: "Replace text and vector colours on selected pages.",
            inputs: InputArity::One,
            input_formats: &["pdf"],
            output_format: "pdf",
            output_formats: &[],
            params_schema: replace_schema,
            tier: Tier::Free,
            runtimes: &[ToolRuntime::Wasm, ToolRuntime::Local, ToolRuntime::Cloud],
            engine: "ariad-pdf",
            secret_params: &[],
        },
        ToolSpec {
            id: "pdf.adjust_colors",
            version: "1",
            group: ToolGroup::Overlay,
            summary: "Adjust brightness, contrast, saturation and gamma.",
            inputs: InputArity::One,
            input_formats: &["pdf"],
            output_format: "pdf",
            output_formats: &[],
            params_schema: adjust_schema,
            tier: Tier::Free,
            runtimes: &[ToolRuntime::Wasm, ToolRuntime::Local, ToolRuntime::Cloud],
            engine: "ariad-pdf",
            secret_params: &[],
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn specs_have_expected_contract() {
        for spec in specs() {
            assert_eq!(spec.group, ToolGroup::Overlay);
            assert_eq!(spec.engine, "ariad-pdf");
            assert_eq!(spec.output_format, "pdf");
            assert_eq!(spec.input_formats, &["pdf"]);
            assert!(spec.runtimes.contains(&ToolRuntime::Wasm));
            assert!(spec.runtimes.contains(&ToolRuntime::Local));
        }
    }
}
