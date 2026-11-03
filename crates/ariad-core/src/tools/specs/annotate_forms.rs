//! Tool specifications for annotations and AcroForms.

use crate::tools::{
    InputArity, Tier, ToolGroup, ToolRuntime, ToolSpec,
    params::{
        annotations::{AnnotationsExportParams, AnnotationsImportParams, AnnotationsParams},
        forms::{
            FormsCreateParams, FormsDetectParams, FormsExportParams, FormsFillParams,
            FormsFlattenParams, FormsImportParams, FormsListParams,
        },
        schema_for_params,
    },
};

/// Formats emitted by the annotations tool, depending on the action.
pub const ANNOTATIONS_OUTPUT_FORMATS: &[&str] = &["pdf", "json"];
/// Supported annotation interchange formats.
pub const ANNOTATIONS_EXPORT_OUTPUT_FORMATS: &[&str] = &["xfdf", "json"];
/// Supported form-data export formats.
pub const FORMS_EXPORT_OUTPUT_FORMATS: &[&str] = &["json", "fdf", "xfdf", "csv"];
const LOCAL: &[ToolRuntime] = &[ToolRuntime::Local];
const WASM_LOCAL: &[ToolRuntime] = &[ToolRuntime::Wasm, ToolRuntime::Local];

macro_rules! spec {
    ($constant:ident, $id:literal, $group:ident, $summary:literal, $arity:expr, $output:literal, $outputs:expr, $runtime:expr, $engine:literal, $params:ty) => {
        #[doc = concat!("Specification for `", $id, "`.")]
        pub const $constant: ToolSpec = ToolSpec {
            id: $id,
            version: "1",
            group: ToolGroup::$group,
            summary: $summary,
            inputs: $arity,
            input_formats: &["pdf"],
            output_format: $output,
            output_formats: $outputs,
            tier: Tier::Free,
            runtimes: $runtime,
            engine: $engine,
            params_schema: schema_for_params::<$params>,
            secret_params: &[],
        };
    };
}
spec!(
    PDF_ANNOTATIONS_SPEC,
    "pdf.annotations",
    Annotate,
    "List or edit PDF annotations.",
    InputArity::One,
    "pdf",
    ANNOTATIONS_OUTPUT_FORMATS,
    WASM_LOCAL,
    "ariad-pdf",
    AnnotationsParams
);
spec!(
    PDF_ANNOTATIONS_IMPORT_SPEC,
    "pdf.annotations_import",
    Annotate,
    "Import PDF annotations from XFDF or JSON.",
    InputArity::Many { min: 2, max: 2 },
    "pdf",
    &[],
    WASM_LOCAL,
    "ariad-pdf",
    AnnotationsImportParams
);
spec!(
    PDF_ANNOTATIONS_EXPORT_SPEC,
    "pdf.annotations_export",
    Annotate,
    "Export PDF annotations as XFDF or JSON.",
    InputArity::One,
    "xfdf",
    ANNOTATIONS_EXPORT_OUTPUT_FORMATS,
    WASM_LOCAL,
    "ariad-pdf",
    AnnotationsExportParams
);
spec!(
    PDF_FORMS_LIST_SPEC,
    "pdf.forms_list",
    Forms,
    "List PDF form fields and their properties.",
    InputArity::One,
    "json",
    &[],
    WASM_LOCAL,
    "ariad-pdf",
    FormsListParams
);
spec!(
    PDF_FORMS_FILL_SPEC,
    "pdf.forms_fill",
    Forms,
    "Fill PDF form fields.",
    InputArity::One,
    "pdf",
    &[],
    WASM_LOCAL,
    "ariad-pdf",
    FormsFillParams
);
spec!(
    PDF_FORMS_CREATE_SPEC,
    "pdf.forms_create",
    Forms,
    "Create fillable PDF form fields.",
    InputArity::One,
    "pdf",
    &[],
    WASM_LOCAL,
    "ariad-pdf",
    FormsCreateParams
);
spec!(
    PDF_FORMS_FLATTEN_SPEC,
    "pdf.forms_flatten",
    Forms,
    "Flatten PDF form fields into page content.",
    InputArity::One,
    "pdf",
    &[],
    WASM_LOCAL,
    "ariad-pdf",
    FormsFlattenParams
);
spec!(
    PDF_FORMS_DETECT_SPEC,
    "pdf.forms_detect",
    Forms,
    "Propose form fields from page content.",
    InputArity::One,
    "json",
    &[],
    LOCAL,
    "pdfium",
    FormsDetectParams
);
spec!(
    PDF_FORMS_IMPORT_SPEC,
    "pdf.forms_import",
    Forms,
    "Import PDF form values from data files.",
    InputArity::Many { min: 2, max: 2 },
    "pdf",
    &[],
    WASM_LOCAL,
    "ariad-pdf",
    FormsImportParams
);
spec!(
    PDF_FORMS_EXPORT_SPEC,
    "pdf.forms_export",
    Forms,
    "Export PDF form values.",
    InputArity::One,
    "json",
    FORMS_EXPORT_OUTPUT_FORMATS,
    WASM_LOCAL,
    "ariad-pdf",
    FormsExportParams
);

/// Returns all annotation and form specifications.
#[must_use]
pub fn specs() -> Vec<ToolSpec> {
    vec![
        PDF_ANNOTATIONS_SPEC,
        PDF_ANNOTATIONS_IMPORT_SPEC,
        PDF_ANNOTATIONS_EXPORT_SPEC,
        PDF_FORMS_LIST_SPEC,
        PDF_FORMS_FILL_SPEC,
        PDF_FORMS_CREATE_SPEC,
        PDF_FORMS_FLATTEN_SPEC,
        PDF_FORMS_DETECT_SPEC,
        PDF_FORMS_IMPORT_SPEC,
        PDF_FORMS_EXPORT_SPEC,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn specifications_are_valid() {
        let specs = specs();
        assert_eq!(specs.len(), 10);
        crate::tools::validate_specs(&specs).unwrap();
    }
}
