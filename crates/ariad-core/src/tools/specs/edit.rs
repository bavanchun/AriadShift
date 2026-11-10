//! Document reflow and stateless in-place page edit tool specifications.

use crate::tools::{
    InputArity, Tier, ToolGroup, ToolRuntime, ToolSpec,
    params::{
        EditApplyParams, EditExportDocumentParams, EditOpenDocumentParams, EditPageObjectsParams,
        schema_for_params,
    },
};

const WASM_LOCAL: &[ToolRuntime] = &[ToolRuntime::Wasm, ToolRuntime::Local];

pub const EDIT_OPEN_DOCUMENT_SPEC: ToolSpec = ToolSpec {
    id: "edit.open_document",
    version: "1",
    group: ToolGroup::Edit,
    summary: "Open a PDF, DOCX, HTML, or Markdown document into ariad-ir/2 with layout, block source map, and export fidelity preview.",
    inputs: InputArity::One,
    input_formats: &["pdf", "docx", "html", "md"],
    output_format: "json",
    output_formats: &[],
    tier: Tier::Free,
    runtimes: WASM_LOCAL,
    engine: "ariad-core",
    params_schema: schema_for_params::<EditOpenDocumentParams>,
    secret_params: &[],
};

pub const EDIT_EXPORT_DOCUMENT_SPEC: ToolSpec = ToolSpec {
    id: "edit.export_document",
    version: "1",
    group: ToolGroup::Edit,
    summary: "Export an edited ariad-ir/2 document to PDF, DOCX, HTML, or Markdown with measured fidelity score and block change list.",
    inputs: InputArity::Many { min: 1, max: 2 },
    input_formats: &["json", "pdf", "docx", "html", "md"],
    output_format: "pdf",
    output_formats: &["pdf", "docx", "html", "md"],
    tier: Tier::Free,
    runtimes: WASM_LOCAL,
    engine: "ariad-core",
    params_schema: schema_for_params::<EditExportDocumentParams>,
    secret_params: &[],
};

pub const EDIT_PAGE_OBJECTS_SPEC: ToolSpec = ToolSpec {
    id: "edit.page_objects",
    version: "1",
    group: ToolGroup::Edit,
    summary: "Inspect per-page PDF text runs, images, and vector paths with stable object IDs, bounding boxes, fonts, and colors for in-place editing.",
    inputs: InputArity::One,
    input_formats: &["pdf"],
    output_format: "json",
    output_formats: &[],
    tier: Tier::Pro,
    runtimes: WASM_LOCAL,
    engine: "pdfium",
    params_schema: schema_for_params::<EditPageObjectsParams>,
    secret_params: &[],
};

pub const EDIT_APPLY_SPEC: ToolSpec = ToolSpec {
    id: "edit.apply",
    version: "1",
    group: ToolGroup::Edit,
    summary: "Apply an ordered sequence of stateless in-place page edits (replace_text, set_text_style, move_object, delete_object, add_text, add_image, replace_image) with font fallback and optional incremental save.",
    inputs: InputArity::One,
    input_formats: &["pdf"],
    output_format: "pdf",
    output_formats: &[],
    tier: Tier::Pro,
    runtimes: WASM_LOCAL,
    engine: "pdfium",
    params_schema: schema_for_params::<EditApplyParams>,
    secret_params: &[],
};

/// Tool specifications in deterministic registration order.
#[must_use]
pub fn specs() -> Vec<ToolSpec> {
    vec![
        EDIT_OPEN_DOCUMENT_SPEC,
        EDIT_EXPORT_DOCUMENT_SPEC,
        EDIT_PAGE_OBJECTS_SPEC,
        EDIT_APPLY_SPEC,
    ]
}
