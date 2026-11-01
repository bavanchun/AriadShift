//! Tool specifications for the tools that render pages, read geometry or analyse pixels
//! (`pdf.render`, `pdf.to_images`, `pdf.thumbnails`, `image.to_pdf`, `pdf.extract_images`,
//! `pdf.text_layout`, `pdf.remove_blank`, `pdf.crop_whitespace`, `pdf.detect_scanned`).
//!
//! Every tool except `image.to_pdf` runs in the `pdfium` engine pack over `ariad-engine/2`.
//! `image.to_pdf` needs no rendering and runs in `ariad-pdf`.
//!
//! [`ToolSpec::output_format`](crate::tools::ToolSpec) holds one format today, so each spec
//! carries its primary format and the full allowed list is declared next to it as a `pub const`
//! (`*_OUTPUT_FORMATS`). The host's artifact-extension check accepts the whole list once
//! `ToolSpec` carries it.

use crate::tools::{
    InputArity, Tier, ToolGroup, ToolRuntime, ToolSpec,
    params::{
        CropWhitespaceParams, DetectScannedParams, ExtractImagesParams, ImageToPdfParams,
        RemoveBlankParams, RenderParams, TextLayoutParams, ThumbnailsParams, ToImagesParams,
        schema_for_params,
    },
};

/// Artifact formats of `pdf.render`: one image per page, or one `zip` when more than 50 pages
/// are rendered.
pub const RENDER_OUTPUT_FORMATS: &[&str] = &["png", "jpeg", "webp", "bmp", "tiff", "zip"];

/// Artifact formats of `pdf.to_images`; identical to [`RENDER_OUTPUT_FORMATS`].
pub const TO_IMAGES_OUTPUT_FORMATS: &[&str] = RENDER_OUTPUT_FORMATS;

/// Artifact formats of `pdf.thumbnails`: PNG thumbnails and a `json` index.
pub const THUMBNAILS_OUTPUT_FORMATS: &[&str] = &["png", "json"];

/// Artifact formats of `image.to_pdf`.
pub const IMAGE_TO_PDF_OUTPUT_FORMATS: &[&str] = &["pdf"];

/// Artifact formats of `pdf.extract_images`: images in their stored encoding where it is a
/// standalone format (`jpeg`), otherwise `png`, or one `zip` when many images are found.
pub const EXTRACT_IMAGES_OUTPUT_FORMATS: &[&str] = &["png", "jpeg", "zip"];

/// Artifact formats of `pdf.text_layout`.
pub const TEXT_LAYOUT_OUTPUT_FORMATS: &[&str] = &["json"];

/// Artifact formats of `pdf.remove_blank`.
pub const REMOVE_BLANK_OUTPUT_FORMATS: &[&str] = &["pdf"];

/// Artifact formats of `pdf.crop_whitespace`.
pub const CROP_WHITESPACE_OUTPUT_FORMATS: &[&str] = &["pdf"];

/// Artifact formats of `pdf.detect_scanned`.
pub const DETECT_SCANNED_OUTPUT_FORMATS: &[&str] = &["json"];

/// Input formats of `image.to_pdf`. HEIC is excluded because no permissive decoder is verified.
pub const IMAGE_TO_PDF_INPUT_FORMATS: &[&str] = &["png", "jpeg", "webp", "tiff", "bmp"];

/// Largest number of images `image.to_pdf` accepts in one run.
pub const IMAGE_TO_PDF_MAX_INPUTS: usize = 500;

/// Engine identifier of the PDFium engine pack.
pub const PDFIUM_ENGINE: &str = "pdfium";

const PDFIUM_RUNTIMES: &[ToolRuntime] = &[ToolRuntime::Local];

/// Returns the tool specifications registered by this module.
#[must_use]
pub fn specs() -> Vec<ToolSpec> {
    vec![
        ToolSpec {
            id: "pdf.render",
            version: "1",
            group: ToolGroup::Convert,
            summary: "Render PDF pages to PNG, JPEG, WebP, BMP or TIFF images at a chosen resolution.",
            inputs: InputArity::One,
            input_formats: &["pdf"],
            output_format: "png",
            tier: Tier::Free,
            runtimes: PDFIUM_RUNTIMES,
            engine: PDFIUM_ENGINE,
            params_schema: schema_for_params::<RenderParams>,
            secret_params: &[],
        },
        ToolSpec {
            id: "pdf.to_images",
            version: "1",
            group: ToolGroup::Convert,
            summary: "Convert PDF pages to images, optionally stacked into one tall image (alias of pdf.render with 300 dpi default).",
            inputs: InputArity::One,
            input_formats: &["pdf"],
            output_format: "png",
            tier: Tier::Free,
            runtimes: PDFIUM_RUNTIMES,
            engine: PDFIUM_ENGINE,
            params_schema: schema_for_params::<ToImagesParams>,
            secret_params: &[],
        },
        ToolSpec {
            id: "pdf.thumbnails",
            version: "1",
            group: ToolGroup::Convert,
            summary: "Render small page thumbnails and a JSON index of them.",
            inputs: InputArity::One,
            input_formats: &["pdf"],
            output_format: "png",
            tier: Tier::Free,
            runtimes: PDFIUM_RUNTIMES,
            engine: PDFIUM_ENGINE,
            params_schema: schema_for_params::<ThumbnailsParams>,
            secret_params: &[],
        },
        ToolSpec {
            id: "image.to_pdf",
            version: "1",
            group: ToolGroup::Convert,
            summary: "Combine PNG, JPEG, WebP, TIFF or BMP images into one PDF, one image per page.",
            inputs: InputArity::many(1, IMAGE_TO_PDF_MAX_INPUTS),
            input_formats: IMAGE_TO_PDF_INPUT_FORMATS,
            output_format: "pdf",
            tier: Tier::Free,
            runtimes: &[ToolRuntime::Wasm, ToolRuntime::Local],
            engine: "ariad-pdf",
            params_schema: schema_for_params::<ImageToPdfParams>,
            secret_params: &[],
        },
        ToolSpec {
            id: "pdf.extract_images",
            version: "1",
            group: ToolGroup::Convert,
            summary: "Extract the embedded images of a PDF, keeping their stored encoding where possible.",
            inputs: InputArity::One,
            input_formats: &["pdf"],
            output_format: "png",
            tier: Tier::Free,
            runtimes: PDFIUM_RUNTIMES,
            engine: PDFIUM_ENGINE,
            params_schema: schema_for_params::<ExtractImagesParams>,
            secret_params: &[],
        },
        ToolSpec {
            id: "pdf.text_layout",
            version: "1",
            group: ToolGroup::Organize,
            summary: "Report characters, words and lines with bounding boxes, font name, size and colour as JSON.",
            inputs: InputArity::One,
            input_formats: &["pdf"],
            output_format: "json",
            tier: Tier::Free,
            runtimes: PDFIUM_RUNTIMES,
            engine: PDFIUM_ENGINE,
            params_schema: schema_for_params::<TextLayoutParams>,
            secret_params: &[],
        },
        ToolSpec {
            id: "pdf.remove_blank",
            version: "1",
            group: ToolGroup::Organize,
            summary: "Remove blank pages detected from low-resolution renders.",
            inputs: InputArity::One,
            input_formats: &["pdf"],
            output_format: "pdf",
            tier: Tier::Free,
            runtimes: PDFIUM_RUNTIMES,
            engine: PDFIUM_ENGINE,
            params_schema: schema_for_params::<RemoveBlankParams>,
            secret_params: &[],
        },
        ToolSpec {
            id: "pdf.crop_whitespace",
            version: "1",
            group: ToolGroup::Organize,
            summary: "Set each page's crop box to its content bounding box plus optional padding.",
            inputs: InputArity::One,
            input_formats: &["pdf"],
            output_format: "pdf",
            tier: Tier::Free,
            runtimes: PDFIUM_RUNTIMES,
            engine: PDFIUM_ENGINE,
            params_schema: schema_for_params::<CropWhitespaceParams>,
            secret_params: &[],
        },
        ToolSpec {
            id: "pdf.detect_scanned",
            version: "1",
            group: ToolGroup::Organize,
            summary: "Report per page whether a text layer exists and how much of the page an image covers.",
            inputs: InputArity::One,
            input_formats: &["pdf"],
            output_format: "json",
            tier: Tier::Free,
            runtimes: PDFIUM_RUNTIMES,
            engine: PDFIUM_ENGINE,
            params_schema: schema_for_params::<DetectScannedParams>,
            secret_params: &[],
        },
    ]
}

/// Returns the full list of artifact formats a tool of this module may emit.
#[must_use]
pub fn output_formats(tool_id: &str) -> Option<&'static [&'static str]> {
    Some(match tool_id {
        "pdf.render" => RENDER_OUTPUT_FORMATS,
        "pdf.to_images" => TO_IMAGES_OUTPUT_FORMATS,
        "pdf.thumbnails" => THUMBNAILS_OUTPUT_FORMATS,
        "image.to_pdf" => IMAGE_TO_PDF_OUTPUT_FORMATS,
        "pdf.extract_images" => EXTRACT_IMAGES_OUTPUT_FORMATS,
        "pdf.text_layout" => TEXT_LAYOUT_OUTPUT_FORMATS,
        "pdf.remove_blank" => REMOVE_BLANK_OUTPUT_FORMATS,
        "pdf.crop_whitespace" => CROP_WHITESPACE_OUTPUT_FORMATS,
        "pdf.detect_scanned" => DETECT_SCANNED_OUTPUT_FORMATS,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::validate_specs;

    fn spec(id: &str) -> ToolSpec {
        specs()
            .into_iter()
            .find(|spec| spec.id == id)
            .unwrap_or_else(|| panic!("missing spec {id}"))
    }

    #[test]
    fn specs_pass_registry_validation() {
        validate_specs(&specs()).expect("pdfium specs are valid");
    }

    #[test]
    fn lists_the_nine_tools_in_a_stable_order() {
        let ids: Vec<_> = specs().iter().map(|spec| spec.id).collect();
        assert_eq!(
            ids,
            [
                "pdf.render",
                "pdf.to_images",
                "pdf.thumbnails",
                "image.to_pdf",
                "pdf.extract_images",
                "pdf.text_layout",
                "pdf.remove_blank",
                "pdf.crop_whitespace",
                "pdf.detect_scanned",
            ]
        );
    }

    #[test]
    fn groups_follow_the_parity_matrix() {
        for id in [
            "pdf.render",
            "pdf.to_images",
            "pdf.thumbnails",
            "image.to_pdf",
            "pdf.extract_images",
        ] {
            assert_eq!(spec(id).group, ToolGroup::Convert, "{id}");
        }
        for id in [
            "pdf.text_layout",
            "pdf.remove_blank",
            "pdf.crop_whitespace",
            "pdf.detect_scanned",
        ] {
            assert_eq!(spec(id).group, ToolGroup::Organize, "{id}");
        }
    }

    #[test]
    fn every_tool_is_free() {
        assert!(specs().iter().all(|spec| spec.tier == Tier::Free));
    }

    #[test]
    fn pdfium_tools_run_locally_in_the_pdfium_engine() {
        for spec in specs().iter().filter(|spec| spec.id != "image.to_pdf") {
            assert_eq!(spec.engine, "pdfium", "{}", spec.id);
            assert_eq!(spec.runtimes, [ToolRuntime::Local], "{}", spec.id);
            assert_eq!(spec.inputs, InputArity::One, "{}", spec.id);
            assert_eq!(spec.input_formats, ["pdf"], "{}", spec.id);
        }
    }

    #[test]
    fn image_to_pdf_runs_in_ariad_pdf_on_wasm_and_local() {
        let spec = spec("image.to_pdf");
        assert_eq!(spec.engine, "ariad-pdf");
        assert_eq!(spec.runtimes, [ToolRuntime::Wasm, ToolRuntime::Local]);
        assert_eq!(spec.inputs, InputArity::many(1, IMAGE_TO_PDF_MAX_INPUTS));
        assert_eq!(spec.input_formats, ["png", "jpeg", "webp", "tiff", "bmp"]);
        assert!(!spec.accepts_format("heic"));
    }

    #[test]
    fn primary_format_leads_the_declared_output_list() {
        for spec in specs() {
            let formats = output_formats(spec.id).expect("output list declared");
            assert!(
                formats.contains(&spec.output_format),
                "{} primary format missing from its list",
                spec.id
            );
            assert!(
                formats
                    .iter()
                    .all(|format| !format.is_empty()
                        && format.bytes().all(|b| b.is_ascii_lowercase())),
                "{}",
                spec.id
            );
        }
        assert_eq!(output_formats("pdf.merge"), None);
    }

    #[test]
    fn render_lists_every_image_format_and_zip() {
        for format in ["png", "jpeg", "webp", "bmp", "tiff", "zip"] {
            assert!(RENDER_OUTPUT_FORMATS.contains(&format), "{format}");
        }
        assert!(THUMBNAILS_OUTPUT_FORMATS.contains(&"json"));
    }

    #[test]
    fn params_schemas_reject_unknown_fields() {
        for spec in specs() {
            let schema = serde_json::to_value((spec.params_schema)()).expect("schema serializes");
            assert_eq!(schema["additionalProperties"], false, "{}", spec.id);
        }
    }
}
