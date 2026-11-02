//! Parameter type for the `pdf.crop_whitespace` tool.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::ParamsError;
use crate::tools::PageRange;

/// Largest accepted padding in points (two inches).
pub const MAX_PADDING_PT: f64 = 144.0;

/// Parameters for `pdf.crop_whitespace`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CropWhitespaceParams {
    /// Pages to crop (1-based selector).
    #[serde(default)]
    pub pages: PageRange,
    /// Padding kept around the detected content in points.
    #[serde(default)]
    #[schemars(range(min = 0.0, max = 144.0))]
    pub padding_pt: f64,
}

impl CropWhitespaceParams {
    /// Checks the range the JSON schema alone cannot enforce.
    pub fn validate(&self) -> Result<(), ParamsError> {
        if !(self.padding_pt.is_finite() && (0.0..=MAX_PADDING_PT).contains(&self.padding_pt)) {
            return Err(ParamsError::new(
                "padding_pt",
                "padding_pt must be between 0 and 144",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn defaults_crop_every_page_without_padding() {
        let params: CropWhitespaceParams = serde_json::from_value(json!({})).expect("valid");
        assert_eq!(params.pages, PageRange::all());
        assert_eq!(params.padding_pt, 0.0);
        params.validate().expect("defaults validate");
    }

    #[test]
    fn accepts_a_valid_example() {
        let params: CropWhitespaceParams =
            serde_json::from_value(json!({"pages": "2-", "padding_pt": 12})).expect("valid");
        params.validate().expect("valid");
    }

    #[test]
    fn rejects_invalid_examples() {
        for value in [
            json!({"padding_pt": "tight"}),
            json!({"margin": 1}),
            json!({"pages": "last-0x"}),
        ] {
            assert!(serde_json::from_value::<CropWhitespaceParams>(value).is_err());
        }
        for padding_pt in [-0.5, 145.0, f64::NAN] {
            let params = CropWhitespaceParams {
                padding_pt,
                ..CropWhitespaceParams::default()
            };
            assert!(params.validate().is_err(), "{padding_pt}");
        }
    }
}
