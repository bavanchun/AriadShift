//! Tool specifications for page-level organization tools (`pdf.merge`, `pdf.split`,
//! `pdf.extract_pages`, `pdf.delete_pages`).

use crate::tools::{
    InputArity, Tier, ToolGroup, ToolRuntime, ToolSpec,
    params::{
        delete_pages::DeletePagesParams, extract_pages::ExtractPagesParams, merge::MergeParams,
        schema_for_params, split::SplitParams,
    },
};

/// Returns the tool specifications registered by this module.
#[must_use]
pub fn specs() -> Vec<ToolSpec> {
    vec![
        ToolSpec {
            id: "pdf.merge",
            version: "1",
            group: ToolGroup::Organize,
            summary: "Merge two or more PDF documents into a single PDF.",
            inputs: InputArity::many(2, 500),
            input_formats: &["pdf"],
            output_format: "pdf",
            output_formats: &[],
            tier: Tier::Free,
            runtimes: &[ToolRuntime::Wasm, ToolRuntime::Local],
            engine: "ariad-pdf",
            params_schema: schema_for_params::<MergeParams>,
            secret_params: &[],
        },
        ToolSpec {
            id: "pdf.split",
            version: "1",
            group: ToolGroup::Organize,
            summary: "Split a PDF document into multiple part PDFs by page ranges, interval, bookmarks, size, or blank separators.",
            inputs: InputArity::One,
            input_formats: &["pdf"],
            output_format: "pdf",
            output_formats: &[],
            tier: Tier::Free,
            runtimes: &[ToolRuntime::Wasm, ToolRuntime::Local],
            engine: "ariad-pdf",
            params_schema: schema_for_params::<SplitParams>,
            secret_params: &[],
        },
        ToolSpec {
            id: "pdf.extract_pages",
            version: "1",
            group: ToolGroup::Organize,
            summary: "Extract selected pages from a PDF document into a new PDF.",
            inputs: InputArity::One,
            input_formats: &["pdf"],
            output_format: "pdf",
            output_formats: &[],
            tier: Tier::Free,
            runtimes: &[ToolRuntime::Wasm, ToolRuntime::Local],
            engine: "ariad-pdf",
            params_schema: schema_for_params::<ExtractPagesParams>,
            secret_params: &[],
        },
        ToolSpec {
            id: "pdf.delete_pages",
            version: "1",
            group: ToolGroup::Organize,
            summary: "Delete selected pages from a PDF document while keeping at least one remaining page.",
            inputs: InputArity::One,
            input_formats: &["pdf"],
            output_format: "pdf",
            output_formats: &[],
            tier: Tier::Free,
            runtimes: &[ToolRuntime::Wasm, ToolRuntime::Local],
            engine: "ariad-pdf",
            params_schema: schema_for_params::<DeletePagesParams>,
            secret_params: &[],
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::validate_specs;

    #[test]
    fn organize_pages_specs_are_valid() {
        let registered = specs();
        assert_eq!(registered.len(), 4);
        let ids: Vec<&str> = registered.iter().map(|s| s.id).collect();
        assert_eq!(
            ids,
            vec![
                "pdf.merge",
                "pdf.split",
                "pdf.extract_pages",
                "pdf.delete_pages"
            ]
        );
        validate_specs(&registered).expect("organize_pages specs must be valid");
    }
}
