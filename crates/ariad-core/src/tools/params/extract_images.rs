//! Parameter type for the `pdf.extract_images` tool.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{ParamsError, check_range};
use crate::tools::PageRange;

/// Largest accepted `min_px` filter.
pub const MAX_MIN_PX: u32 = 16_384;

/// Encoding of the extracted images.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExtractFormat {
    /// Keep the encoding stored in the PDF where it is a standalone image format, otherwise PNG.
    #[default]
    Original,
    /// Always re-encode as PNG.
    Png,
}

/// Parameters for `pdf.extract_images`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExtractImagesParams {
    /// Pages to scan (1-based selector).
    #[serde(default)]
    pub pages: PageRange,
    /// Output encoding.
    #[serde(default)]
    pub format: ExtractFormat,
    /// Skip images whose width or height is below this many pixels (0 keeps every image).
    #[serde(default)]
    #[schemars(range(min = 0, max = 16384))]
    pub min_px: u32,
}

impl Default for ExtractImagesParams {
    fn default() -> Self {
        Self {
            pages: PageRange::all(),
            format: ExtractFormat::Original,
            min_px: 0,
        }
    }
}

impl ExtractImagesParams {
    /// Checks the range the JSON schema alone cannot enforce.
    pub fn validate(&self) -> Result<(), ParamsError> {
        check_range("min_px", self.min_px, 0, MAX_MIN_PX)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn defaults_keep_original_encoding_and_every_image() {
        let params: ExtractImagesParams = serde_json::from_value(json!({})).expect("valid");
        assert_eq!(params, ExtractImagesParams::default());
        assert_eq!(params.format, ExtractFormat::Original);
        assert_eq!(params.min_px, 0);
        params.validate().expect("defaults validate");
    }

    #[test]
    fn accepts_a_valid_example() {
        let params: ExtractImagesParams =
            serde_json::from_value(json!({"pages": "odd", "format": "png", "min_px": 64}))
                .expect("valid");
        params.validate().expect("valid");
    }

    #[test]
    fn rejects_invalid_examples() {
        for value in [
            json!({"format": "jpeg"}),
            json!({"min_px": -1}),
            json!({"extra": true}),
        ] {
            assert!(serde_json::from_value::<ExtractImagesParams>(value).is_err());
        }
        let params = ExtractImagesParams {
            min_px: MAX_MIN_PX + 1,
            ..ExtractImagesParams::default()
        };
        assert!(params.validate().is_err());
    }
}
