//! Tool specifications for document-level structure and metadata tools (`pdf.metadata`,
//! `pdf.outline`, `pdf.attachments`, `pdf.page_labels`, `pdf.viewer_prefs`, `pdf.portfolio`,
//! `pdf.layers`).

use schemars::{Schema, generate::SchemaSettings};

use crate::tools::{
    InputArity, Tier, ToolGroup, ToolRuntime, ToolSpec,
    params::{
        AttachmentsParams, LayersParams, MetadataParams, OutlineParams, PageLabelsParams,
        PortfolioParams, ViewerPrefsParams,
    },
};

fn metadata_params_schema() -> Schema {
    SchemaSettings::draft2020_12()
        .into_generator()
        .into_root_schema_for::<MetadataParams>()
}

fn outline_params_schema() -> Schema {
    SchemaSettings::draft2020_12()
        .into_generator()
        .into_root_schema_for::<OutlineParams>()
}

fn attachments_params_schema() -> Schema {
    SchemaSettings::draft2020_12()
        .into_generator()
        .into_root_schema_for::<AttachmentsParams>()
}

fn page_labels_params_schema() -> Schema {
    SchemaSettings::draft2020_12()
        .into_generator()
        .into_root_schema_for::<PageLabelsParams>()
}

fn viewer_prefs_params_schema() -> Schema {
    SchemaSettings::draft2020_12()
        .into_generator()
        .into_root_schema_for::<ViewerPrefsParams>()
}

fn portfolio_params_schema() -> Schema {
    SchemaSettings::draft2020_12()
        .into_generator()
        .into_root_schema_for::<PortfolioParams>()
}

fn layers_params_schema() -> Schema {
    SchemaSettings::draft2020_12()
        .into_generator()
        .into_root_schema_for::<LayersParams>()
}

const RUNTIMES_WASM_LOCAL: &[ToolRuntime] = &[ToolRuntime::Wasm, ToolRuntime::Local];
const INPUT_FORMATS_PDF: &[&str] = &["pdf"];

/// Static specification for `pdf.metadata`.
pub const PDF_METADATA_SPEC: ToolSpec = ToolSpec {
    id: "pdf.metadata",
    version: "1",
    group: ToolGroup::Organize,
    summary: "Set, remove, or strip PDF /Info dictionary and XMP metadata fields.",
    inputs: InputArity::One,
    input_formats: INPUT_FORMATS_PDF,
    output_format: "pdf",
    tier: Tier::Free,
    runtimes: RUNTIMES_WASM_LOCAL,
    engine: "ariad-pdf",
    params_schema: metadata_params_schema,
    secret_params: &[],
};

/// Static specification for `pdf.outline`.
pub const PDF_OUTLINE_SPEC: ToolSpec = ToolSpec {
    id: "pdf.outline",
    version: "1",
    group: ToolGroup::Organize,
    summary: "Export, import, clear, or generate PDF outline bookmarks from headings.",
    inputs: InputArity::One,
    input_formats: INPUT_FORMATS_PDF,
    output_format: "pdf",
    tier: Tier::Free,
    runtimes: RUNTIMES_WASM_LOCAL,
    engine: "ariad-pdf",
    params_schema: outline_params_schema,
    secret_params: &[],
};

/// Static specification for `pdf.attachments`.
pub const PDF_ATTACHMENTS_SPEC: ToolSpec = ToolSpec {
    id: "pdf.attachments",
    version: "1",
    group: ToolGroup::Organize,
    summary: "List, add, extract, or remove embedded file attachments in a PDF.",
    inputs: InputArity::Many { min: 1, max: 100 },
    input_formats: INPUT_FORMATS_PDF,
    output_format: "pdf",
    tier: Tier::Free,
    runtimes: RUNTIMES_WASM_LOCAL,
    engine: "ariad-pdf",
    params_schema: attachments_params_schema,
    secret_params: &[],
};

/// Static specification for `pdf.page_labels`.
pub const PDF_PAGE_LABELS_SPEC: ToolSpec = ToolSpec {
    id: "pdf.page_labels",
    version: "1",
    group: ToolGroup::Organize,
    summary: "Rewrite the PDF /PageLabels number tree across 1-based page ranges.",
    inputs: InputArity::One,
    input_formats: INPUT_FORMATS_PDF,
    output_format: "pdf",
    tier: Tier::Free,
    runtimes: RUNTIMES_WASM_LOCAL,
    engine: "ariad-pdf",
    params_schema: page_labels_params_schema,
    secret_params: &[],
};

/// Static specification for `pdf.viewer_prefs`.
pub const PDF_VIEWER_PREFS_SPEC: ToolSpec = ToolSpec {
    id: "pdf.viewer_prefs",
    version: "1",
    group: ToolGroup::Organize,
    summary: "Configure PDF catalog /ViewerPreferences, /PageLayout, /PageMode, and /OpenAction.",
    inputs: InputArity::One,
    input_formats: INPUT_FORMATS_PDF,
    output_format: "pdf",
    tier: Tier::Free,
    runtimes: RUNTIMES_WASM_LOCAL,
    engine: "ariad-pdf",
    params_schema: viewer_prefs_params_schema,
    secret_params: &[],
};

/// Static specification for `pdf.portfolio`.
pub const PDF_PORTFOLIO_SPEC: ToolSpec = ToolSpec {
    id: "pdf.portfolio",
    version: "1",
    group: ToolGroup::Organize,
    summary: "Create, list, or extract a PDF portfolio (/Collection) with embedded files.",
    inputs: InputArity::Many { min: 1, max: 100 },
    input_formats: INPUT_FORMATS_PDF,
    output_format: "pdf",
    tier: Tier::Free,
    runtimes: RUNTIMES_WASM_LOCAL,
    engine: "ariad-pdf",
    params_schema: portfolio_params_schema,
    secret_params: &[],
};

/// Static specification for `pdf.layers`.
pub const PDF_LAYERS_SPEC: ToolSpec = ToolSpec {
    id: "pdf.layers",
    version: "1",
    group: ToolGroup::Organize,
    summary: "List, toggle default visibility, or remove Optional Content Group (OCG) layers.",
    inputs: InputArity::One,
    input_formats: INPUT_FORMATS_PDF,
    output_format: "pdf",
    tier: Tier::Free,
    runtimes: RUNTIMES_WASM_LOCAL,
    engine: "ariad-pdf",
    params_schema: layers_params_schema,
    secret_params: &[],
};

/// Returns the tool specifications registered by this module.
#[must_use]
pub fn specs() -> Vec<ToolSpec> {
    vec![
        PDF_METADATA_SPEC,
        PDF_OUTLINE_SPEC,
        PDF_ATTACHMENTS_SPEC,
        PDF_PAGE_LABELS_SPEC,
        PDF_VIEWER_PREFS_SPEC,
        PDF_PORTFOLIO_SPEC,
        PDF_LAYERS_SPEC,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::validate_specs;

    #[test]
    fn organize_document_specs_are_valid() {
        let list = specs();
        assert_eq!(list.len(), 7);
        validate_specs(&list).expect("organize_document specs must pass registry validation");
        let ids: Vec<&str> = list.iter().map(|s| s.id).collect();
        assert_eq!(
            ids,
            vec![
                "pdf.metadata",
                "pdf.outline",
                "pdf.attachments",
                "pdf.page_labels",
                "pdf.viewer_prefs",
                "pdf.portfolio",
                "pdf.layers",
            ]
        );
    }
}
