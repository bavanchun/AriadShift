//! Tool specifications for multi-page layout and imposition tools (`pdf.nup`, `pdf.booklet`,
//! `pdf.overlay`, `pdf.divide_pages`, `pdf.posterize`).

use crate::tools::{
    InputArity, Tier, ToolGroup, ToolRuntime, ToolSpec,
    params::{
        booklet::BookletParams, divide_pages::DividePagesParams, nup::NupParams,
        overlay::OverlayParams, posterize::PosterizeParams, schema_for_params,
    },
};

/// Returns the tool specifications registered by this module.
#[must_use]
pub fn specs() -> Vec<ToolSpec> {
    vec![
        ToolSpec {
            id: "pdf.nup",
            version: "1",
            group: ToolGroup::Organize,
            summary: "Place multiple PDF pages onto each output sheet in an N-up grid layout.",
            inputs: InputArity::One,
            input_formats: &["pdf"],
            output_format: "pdf",
            output_formats: &[],
            tier: Tier::Free,
            runtimes: &[ToolRuntime::Wasm, ToolRuntime::Local],
            engine: "ariad-pdf",
            params_schema: schema_for_params::<NupParams>,
            secret_params: &[],
        },
        ToolSpec {
            id: "pdf.booklet",
            version: "1",
            group: ToolGroup::Organize,
            summary: "Impose PDF pages into saddle-stitch booklet spreads padded to a multiple of 4.",
            inputs: InputArity::One,
            input_formats: &["pdf"],
            output_format: "pdf",
            output_formats: &[],
            tier: Tier::Free,
            runtimes: &[ToolRuntime::Wasm, ToolRuntime::Local],
            engine: "ariad-pdf",
            params_schema: schema_for_params::<BookletParams>,
            secret_params: &[],
        },
        ToolSpec {
            id: "pdf.overlay",
            version: "1",
            group: ToolGroup::Organize,
            summary: "Overlay or underlay pages from a second PDF onto selected pages of a base PDF.",
            inputs: InputArity::many(2, 2),
            input_formats: &["pdf"],
            output_format: "pdf",
            output_formats: &[],
            tier: Tier::Free,
            runtimes: &[ToolRuntime::Wasm, ToolRuntime::Local],
            engine: "ariad-pdf",
            params_schema: schema_for_params::<OverlayParams>,
            secret_params: &[],
        },
        ToolSpec {
            id: "pdf.divide_pages",
            version: "1",
            group: ToolGroup::Organize,
            summary: "Subdivide each selected PDF page into a cols by rows grid of cropped cell pages.",
            inputs: InputArity::One,
            input_formats: &["pdf"],
            output_format: "pdf",
            output_formats: &[],
            tier: Tier::Free,
            runtimes: &[ToolRuntime::Wasm, ToolRuntime::Local],
            engine: "ariad-pdf",
            params_schema: schema_for_params::<DividePagesParams>,
            secret_params: &[],
        },
        ToolSpec {
            id: "pdf.posterize",
            version: "1",
            group: ToolGroup::Organize,
            summary: "Enlarge selected PDF pages across multiple tiled output sheets with overlap and crop marks.",
            inputs: InputArity::One,
            input_formats: &["pdf"],
            output_format: "pdf",
            output_formats: &[],
            tier: Tier::Free,
            runtimes: &[ToolRuntime::Wasm, ToolRuntime::Local],
            engine: "ariad-pdf",
            params_schema: schema_for_params::<PosterizeParams>,
            secret_params: &[],
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::validate_specs;

    #[test]
    fn organize_layout_specs_are_valid_and_complete() {
        let list = specs();
        assert_eq!(list.len(), 5);
        assert!(validate_specs(&list).is_ok());

        let ids: Vec<&str> = list.iter().map(|s| s.id).collect();
        assert_eq!(
            ids,
            vec![
                "pdf.nup",
                "pdf.booklet",
                "pdf.overlay",
                "pdf.divide_pages",
                "pdf.posterize",
            ]
        );
        for spec in &list {
            assert_eq!(spec.group, ToolGroup::Organize);
            assert_eq!(spec.tier, Tier::Free);
            assert_eq!(spec.engine, "ariad-pdf");
            assert_eq!(spec.runtimes, &[ToolRuntime::Wasm, ToolRuntime::Local]);
            assert_eq!(spec.input_formats, &["pdf"]);
            assert_eq!(spec.output_format, "pdf");
        }
        assert_eq!(list[2].inputs, InputArity::many(2, 2));
    }
}
