//! OCR and scan tool specifications.
use super::pdfium::IMAGE_TO_PDF_INPUT_FORMATS;
use crate::tools::{
    InputArity, Tier, ToolGroup, ToolRuntime, ToolSpec,
    params::{
        auto_rotate::AutoRotateParams,
        deskew::DeskewParams,
        image_ocr::ImageOcrParams,
        ocr::OcrParams,
        ocr_internal::{OcrOrientationParams, OcrRecognizeParams, ScanRenderParams},
        scan_split::ScanSplitParams,
        scanner_effect::ScannerEffectParams,
        schema_for_params,
    },
};
use schemars::{JsonSchema, Schema};
/// Host composite engine identifier.
pub const HOST_COMPOSITE_ENGINE: &str = "ariad-host";
/// OCR artifact formats.
pub const OCR_OUTPUT_FORMATS: &[&str] = &["pdf", "txt", "hocr"];

fn spec<P: JsonSchema>(
    id: &'static str,
    summary: &'static str,
    group: ToolGroup,
    inputs: InputArity,
    input_formats: &'static [&'static str],
    output_formats: &'static [&'static str],
    engine: &'static str,
) -> ToolSpec {
    let schema: fn() -> Schema = schema_for_params::<P>;
    ToolSpec {
        id,
        version: "1",
        group,
        summary,
        inputs,
        input_formats,
        output_format: output_formats[0],
        output_formats,
        tier: Tier::Free,
        runtimes: &[ToolRuntime::Local],
        engine,
        params_schema: schema,
        secret_params: &[],
    }
}
/// Public tool specifications.
#[must_use]
pub fn specs() -> Vec<ToolSpec> {
    vec![
        spec::<OcrParams>(
            "pdf.ocr",
            "Recognize text on PDF pages.",
            ToolGroup::Ocr,
            InputArity::One,
            &["pdf"],
            OCR_OUTPUT_FORMATS,
            HOST_COMPOSITE_ENGINE,
        ),
        spec::<ImageOcrParams>(
            "image.ocr",
            "Recognize text in images.",
            ToolGroup::Ocr,
            InputArity::many(1, 200),
            IMAGE_TO_PDF_INPUT_FORMATS,
            OCR_OUTPUT_FORMATS,
            HOST_COMPOSITE_ENGINE,
        ),
        spec::<AutoRotateParams>(
            "pdf.auto_rotate",
            "Correct page orientation.",
            ToolGroup::Ocr,
            InputArity::One,
            &["pdf"],
            &["pdf"],
            HOST_COMPOSITE_ENGINE,
        ),
        spec::<DeskewParams>(
            "pdf.deskew",
            "Correct page skew.",
            ToolGroup::Ocr,
            InputArity::One,
            &["pdf"],
            &["pdf"],
            "pdfium",
        ),
        spec::<ScanSplitParams>(
            "pdf.scan_split",
            "Split scanned regions into pages.",
            ToolGroup::Ocr,
            InputArity::One,
            &["pdf"],
            &["pdf"],
            "pdfium",
        ),
        spec::<ScannerEffectParams>(
            "pdf.scanner_effect",
            "Apply a scanner effect.",
            ToolGroup::Overlay,
            InputArity::One,
            &["pdf"],
            &["pdf"],
            "pdfium",
        ),
    ]
}
/// Internal engine operations, never registered as public tools.
#[must_use]
pub fn internal_specs() -> Vec<ToolSpec> {
    vec![
        spec::<ScanRenderParams>(
            "scan.render",
            "Render pages for recognition.",
            ToolGroup::Ocr,
            InputArity::One,
            &["pdf"],
            &["png", "json"],
            "pdfium",
        ),
        spec::<OcrRecognizeParams>(
            "ocr.recognize",
            "Recognize image text.",
            ToolGroup::Ocr,
            InputArity::many(1, 200),
            IMAGE_TO_PDF_INPUT_FORMATS,
            &["json"],
            "tesseract",
        ),
        spec::<OcrOrientationParams>(
            "ocr.orientation",
            "Detect image orientation.",
            ToolGroup::Ocr,
            InputArity::many(1, 200),
            IMAGE_TO_PDF_INPUT_FORMATS,
            &["json"],
            "tesseract",
        ),
    ]
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn specifications_are_valid() {
        let public = specs();
        assert_eq!(public.len(), 6);
        crate::tools::validate_specs(&public).unwrap();
        crate::tools::validate_specs(&internal_specs()).unwrap();
        assert!(public[1].inputs.accepts(200));
        assert!(!public[1].inputs.accepts(201));
    }
    #[test]
    fn image_arity_does_not_override_free_quota() {
        use crate::tools::access::{Plan, RunSize, within_run_quota};
        let image = specs()
            .into_iter()
            .find(|spec| spec.id == "image.ocr")
            .unwrap();
        assert!(image.inputs.accepts(25));
        assert!(within_run_quota(Plan::Free, RunSize::new(25, 250_000_000)));
        assert!(image.inputs.accepts(26));
        assert!(!within_run_quota(Plan::Free, RunSize::new(26, 1)));
        assert!(!within_run_quota(Plan::Free, RunSize::new(1, 250_000_001)));
        assert!(image.inputs.accepts(200));
        assert!(!image.inputs.accepts(201));
    }
}
