//! Tool specifications for page geometry and stream tools (`pdf.rotate`, `pdf.crop`,
//! `pdf.scale`, `pdf.single_page`, `pdf.rotate_content`, `pdf.decompress`).

use crate::tools::{
    InputArity, Tier, ToolGroup, ToolRuntime, ToolSpec,
    params::{
        crop::CropParams, decompress::DecompressParams, rotate::RotateParams,
        rotate_content::RotateContentParams, scale::ScaleParams, schema_for_params,
        single_page::SinglePageParams,
    },
};

/// Returns the tool specifications registered by this module.
#[must_use]
pub fn specs() -> Vec<ToolSpec> {
    vec![
        ToolSpec {
            id: "pdf.rotate",
            version: "1",
            group: ToolGroup::Organize,
            summary: "Rotate selected PDF pages by a multiple of 90 degrees without re-rendering.",
            inputs: InputArity::One,
            input_formats: &["pdf"],
            output_format: "pdf",
            tier: Tier::Free,
            runtimes: &[ToolRuntime::Wasm, ToolRuntime::Local],
            engine: "ariad-pdf",
            params_schema: schema_for_params::<RotateParams>,
            secret_params: &[],
        },
        ToolSpec {
            id: "pdf.crop",
            version: "1",
            group: ToolGroup::Organize,
            summary: "Set the CropBox or MediaBox of selected PDF pages in points.",
            inputs: InputArity::One,
            input_formats: &["pdf"],
            output_format: "pdf",
            tier: Tier::Free,
            runtimes: &[ToolRuntime::Wasm, ToolRuntime::Local],
            engine: "ariad-pdf",
            params_schema: schema_for_params::<CropParams>,
            secret_params: &[],
        },
        ToolSpec {
            id: "pdf.scale",
            version: "1",
            group: ToolGroup::Organize,
            summary: "Scale selected PDF pages to a target paper size using Form XObjects.",
            inputs: InputArity::One,
            input_formats: &["pdf"],
            output_format: "pdf",
            tier: Tier::Free,
            runtimes: &[ToolRuntime::Wasm, ToolRuntime::Local],
            engine: "ariad-pdf",
            params_schema: schema_for_params::<ScaleParams>,
            secret_params: &[],
        },
        ToolSpec {
            id: "pdf.single_page",
            version: "1",
            group: ToolGroup::Organize,
            summary: "Concatenate all pages of a PDF document vertically into a single continuous page.",
            inputs: InputArity::One,
            input_formats: &["pdf"],
            output_format: "pdf",
            tier: Tier::Free,
            runtimes: &[ToolRuntime::Wasm, ToolRuntime::Local],
            engine: "ariad-pdf",
            params_schema: schema_for_params::<SinglePageParams>,
            secret_params: &[],
        },
        ToolSpec {
            id: "pdf.rotate_content",
            version: "1",
            group: ToolGroup::Organize,
            summary: "Rotate PDF page content by an arbitrary angle using a Form XObject and transform matrix.",
            inputs: InputArity::One,
            input_formats: &["pdf"],
            output_format: "pdf",
            tier: Tier::Free,
            runtimes: &[ToolRuntime::Wasm, ToolRuntime::Local],
            engine: "ariad-pdf",
            params_schema: schema_for_params::<RotateContentParams>,
            secret_params: &[],
        },
        ToolSpec {
            id: "pdf.decompress",
            version: "1",
            group: ToolGroup::Optimize,
            summary: "Decompress PDF streams and expand object streams within decoded-size limits.",
            inputs: InputArity::One,
            input_formats: &["pdf"],
            output_format: "pdf",
            tier: Tier::Free,
            runtimes: &[ToolRuntime::Wasm, ToolRuntime::Local],
            engine: "ariad-pdf",
            params_schema: schema_for_params::<DecompressParams>,
            secret_params: &[],
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::validate_specs;

    #[test]
    fn organize_geometry_specs_are_valid_and_complete() {
        let list = specs();
        assert_eq!(list.len(), 6);
        assert!(validate_specs(&list).is_ok());

        let ids: Vec<&str> = list.iter().map(|s| s.id).collect();
        assert_eq!(
            ids,
            vec![
                "pdf.rotate",
                "pdf.crop",
                "pdf.scale",
                "pdf.single_page",
                "pdf.rotate_content",
                "pdf.decompress",
            ]
        );
    }
}
