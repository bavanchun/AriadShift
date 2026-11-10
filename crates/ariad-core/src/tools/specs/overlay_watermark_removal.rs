//! Specifications for watermark candidate detection and explicit removal.

use crate::tools::{
    InputArity, Tier, ToolGroup, ToolRuntime, ToolSpec,
    params::{
        WatermarkDetectParams, WatermarkRemoveParams, WatermarkRemoveRasterParams,
        schema_for_params,
    },
};
use schemars::{Schema, generate::SchemaSettings};

fn detect_schema() -> Schema {
    SchemaSettings::draft2020_12()
        .into_generator()
        .into_root_schema_for::<WatermarkDetectParams>()
}
fn remove_schema() -> Schema {
    let mut schema = SchemaSettings::draft2020_12()
        .into_generator()
        .into_root_schema_for::<WatermarkRemoveParams>();
    if let Some(property) = schema
        .as_object_mut()
        .and_then(|schema| schema.get_mut("properties"))
        .and_then(|properties| properties.get_mut("candidates"))
        .and_then(serde_json::Value::as_object_mut)
    {
        property.insert("uniqueItems".into(), serde_json::Value::Bool(true));
    }
    schema
}
/// Pure-Rust watermark inspection and explicit removal tools.
#[must_use]
pub fn specs() -> Vec<ToolSpec> {
    vec![
        ToolSpec {
            id: "pdf.watermark_detect",
            version: "1",
            group: ToolGroup::Overlay,
            summary: "Find possible watermarks for review.",
            inputs: InputArity::One,
            input_formats: &["pdf"],
            output_format: "json",
            output_formats: &[],
            params_schema: detect_schema,
            tier: Tier::Free,
            runtimes: &[ToolRuntime::Wasm, ToolRuntime::Local],
            engine: "ariad-pdf",
            secret_params: &[],
        },
        ToolSpec {
            id: "pdf.watermark_remove",
            version: "1",
            group: ToolGroup::Overlay,
            summary: "Remove explicitly selected watermark candidates.",
            inputs: InputArity::One,
            input_formats: &["pdf"],
            output_format: "pdf",
            output_formats: &[],
            params_schema: remove_schema,
            tier: Tier::Free,
            runtimes: &[ToolRuntime::Wasm, ToolRuntime::Local],
            engine: "ariad-pdf",
            secret_params: &[],
        },
        ToolSpec {
            id: "pdf.watermark_remove_raster",
            version: "1",
            group: ToolGroup::Overlay,
            summary: "Preview or remove raster watermarks with inpainting.",
            inputs: InputArity::One,
            input_formats: &["pdf"],
            output_format: "pdf",
            output_formats: &[],
            tier: Tier::Free,
            runtimes: &[ToolRuntime::Local],
            engine: "inpaint",
            params_schema: schema_for_params::<WatermarkRemoveRasterParams>,
            secret_params: &[],
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_watermark_contracts_are_typed() {
        let specs = specs();
        assert_eq!(specs.len(), 3);
        for spec in &specs[..2] {
            assert_eq!(spec.group, ToolGroup::Overlay);
            assert_eq!(spec.engine, "ariad-pdf");
            assert_eq!(spec.input_formats, &["pdf"]);
            assert!(spec.runtimes.contains(&ToolRuntime::Wasm));
            assert!(spec.runtimes.contains(&ToolRuntime::Local));
        }
        assert_eq!(specs[0].output_format, "json");
        assert_eq!(specs[1].output_format, "pdf");
        let schema = serde_json::to_value((specs[1].params_schema)()).unwrap();
        assert_eq!(schema["properties"]["candidates"]["uniqueItems"], true);
        assert_eq!(schema["properties"]["candidates"]["minItems"], 1);
    }
}
