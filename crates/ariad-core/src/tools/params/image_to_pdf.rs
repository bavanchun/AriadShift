//! Parameter type for the `image.to_pdf` tool.
//!
//! Accepted inputs are PNG, JPEG, WebP, TIFF and BMP. HEIC is excluded because no permissive
//! decoder has been verified.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::ParamsError;

/// Largest accepted page margin in points (two inches).
pub const MAX_MARGIN_PT: f64 = 144.0;

/// Page size of the generated PDF.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum PageSize {
    /// Every page takes the size of its image.
    #[default]
    FitImage,
    /// ISO A3.
    A3,
    /// ISO A4.
    A4,
    /// ISO A5.
    A5,
    /// US Letter.
    Letter,
    /// US Legal.
    Legal,
}

impl PageSize {
    /// Portrait page size in points for a paper size, or `None` for `fit-image`.
    #[must_use]
    pub const fn portrait_pt(self) -> Option<(f64, f64)> {
        match self {
            Self::FitImage => None,
            Self::A3 => Some((841.89, 1190.55)),
            Self::A4 => Some((595.28, 841.89)),
            Self::A5 => Some((419.53, 595.28)),
            Self::Letter => Some((612.0, 792.0)),
            Self::Legal => Some((612.0, 1008.0)),
        }
    }
}

/// Page orientation for paper sizes; ignored by `fit-image`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Orientation {
    /// Landscape when the image is wider than tall, portrait otherwise.
    #[default]
    Auto,
    /// Always portrait.
    Portrait,
    /// Always landscape.
    Landscape,
}

/// Parameters for `image.to_pdf`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ImageToPdfParams {
    /// Page size: the image size (`fit-image`) or a paper size.
    #[serde(default)]
    pub page: PageSize,
    /// Margin around the image in points.
    #[serde(default)]
    #[schemars(range(min = 0.0, max = 144.0))]
    pub margin_pt: f64,
    /// Page orientation for paper sizes.
    #[serde(default)]
    pub orientation: Orientation,
}

impl ImageToPdfParams {
    /// Checks the range the JSON schema alone cannot enforce.
    pub fn validate(&self) -> Result<(), ParamsError> {
        if !(self.margin_pt.is_finite() && (0.0..=MAX_MARGIN_PT).contains(&self.margin_pt)) {
            return Err(ParamsError::new(
                "margin_pt",
                "margin_pt must be between 0 and 144",
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
    fn defaults_fit_the_image_with_no_margin() {
        let params: ImageToPdfParams = serde_json::from_value(json!({})).expect("valid");
        assert_eq!(params, ImageToPdfParams::default());
        assert_eq!(params.page, PageSize::FitImage);
        assert_eq!(params.margin_pt, 0.0);
        assert_eq!(params.orientation, Orientation::Auto);
        params.validate().expect("defaults validate");
    }

    #[test]
    fn accepts_a_valid_example() {
        let params: ImageToPdfParams = serde_json::from_value(
            json!({"page": "a4", "margin_pt": 36.5, "orientation": "landscape"}),
        )
        .expect("valid");
        assert_eq!(params.page, PageSize::A4);
        params.validate().expect("valid");
        assert_eq!(PageSize::A4.portrait_pt(), Some((595.28, 841.89)));
        assert_eq!(PageSize::FitImage.portrait_pt(), None);
    }

    #[test]
    fn rejects_invalid_examples() {
        for value in [
            json!({"page": "a0"}),
            json!({"orientation": "sideways"}),
            json!({"margin_pt": "wide"}),
            json!({"heic": true}),
        ] {
            assert!(serde_json::from_value::<ImageToPdfParams>(value).is_err());
        }
        for margin_pt in [-1.0, 144.5, f64::NAN, f64::INFINITY] {
            let params = ImageToPdfParams {
                margin_pt,
                ..ImageToPdfParams::default()
            };
            assert!(params.validate().is_err(), "{margin_pt}");
        }
    }
}
