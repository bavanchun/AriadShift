//! Specifications for adding marks to PDF pages.

use crate::tools::{
    InputArity, Tier, ToolGroup, ToolRuntime, ToolSpec,
    params::schema_for_params,
    params::{AddImageParams, AddTextParams, QrParams, StampParams, WatermarkParams},
};

/// Text, image, stamp and QR tools run in the pure-Rust PDF engine.
#[must_use]
pub fn specs() -> Vec<ToolSpec> {
    macro_rules! spec {
        ($id:literal,$summary:literal,$arity:expr,$formats:expr,$params:ty) => {
            ToolSpec {
                id: $id,
                version: "1",
                group: ToolGroup::Overlay,
                summary: $summary,
                inputs: $arity,
                input_formats: $formats,
                output_format: "pdf",
                output_formats: &[],
                params_schema: schema_for_params::<$params>,
                tier: Tier::Free,
                runtimes: &[ToolRuntime::Wasm, ToolRuntime::Local, ToolRuntime::Cloud],
                engine: "ariad-pdf",
                secret_params: &[],
            }
        };
    }
    vec![
        spec!(
            "pdf.watermark",
            "Add a removable text or image watermark.",
            InputArity::Many { min: 1, max: 2 },
            &["pdf", "png", "jpeg", "webp", "tiff", "bmp"],
            WatermarkParams
        ),
        spec!(
            "pdf.stamp",
            "Add a removable text stamp.",
            InputArity::One,
            &["pdf"],
            StampParams
        ),
        spec!(
            "pdf.add_image",
            "Place an image on selected pages.",
            InputArity::Many { min: 2, max: 2 },
            &["pdf", "png", "jpeg", "webp", "tiff", "bmp"],
            AddImageParams
        ),
        spec!(
            "pdf.add_text",
            "Place text on selected pages.",
            InputArity::One,
            &["pdf"],
            AddTextParams
        ),
        spec!(
            "pdf.qr",
            "Place a vector QR code on selected pages.",
            InputArity::One,
            &["pdf"],
            QrParams
        ),
    ]
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn marks_have_expected_contracts() {
        let specs = specs();
        assert_eq!(specs.len(), 5);
        for spec in &specs {
            assert_eq!(spec.group, ToolGroup::Overlay);
            assert_eq!(spec.tier, Tier::Free);
            assert_eq!(spec.engine, "ariad-pdf");
            assert_eq!(spec.output_format, "pdf");
            assert!(spec.runtimes.contains(&ToolRuntime::Wasm));
            assert!(spec.runtimes.contains(&ToolRuntime::Local));
        }
        assert_eq!(specs[0].inputs, InputArity::Many { min: 1, max: 2 });
        assert_eq!(specs[2].inputs, InputArity::Many { min: 2, max: 2 });
    }
}

#[cfg(test)]
mod parameter_tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn mark_defaults_and_validation() {
        let watermark: WatermarkParams = serde_json::from_value(json!({"text":"DRAFT"})).unwrap();
        assert!(watermark.validate().is_ok());
        assert_eq!(watermark.pages.as_str(), "all");
        assert_eq!(watermark.opacity.value(), 0.3);
        assert_eq!(watermark.rotation_deg, 45.0);
        assert!(
            serde_json::from_value::<WatermarkParams>(json!({"text":"x","font_size_pt":"invalid"}))
                .is_err()
        );
        let stamp: StampParams = serde_json::from_value(json!({})).unwrap();
        assert!(stamp.validate().is_ok());
        assert_eq!(stamp.pages.as_str(), "1");
        let image: AddImageParams = serde_json::from_value(json!({})).unwrap();
        assert!(image.validate().is_ok());
        assert_eq!(image.pages.as_str(), "1");
        let text: AddTextParams = serde_json::from_value(json!({"text":"hello"})).unwrap();
        assert!(text.validate().is_ok());
        assert_eq!(text.pages.as_str(), "1");
        let qr: QrParams = serde_json::from_value(json!({"content":"hello"})).unwrap();
        assert!(qr.validate().is_ok());
        assert_eq!(qr.quiet_zone_modules, 4);
    }
}
