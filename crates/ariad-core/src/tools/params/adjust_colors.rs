//! Parameters for `pdf.adjust_colors`.

use crate::tools::page_range::PageRange;
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};

fn bounded<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f32, D::Error> {
    let value = f32::deserialize(deserializer)?;
    if !value.is_finite() || !(-1.0..=1.0).contains(&value) {
        return Err(serde::de::Error::custom(
            "adjustment must be finite and between -1 and 1",
        ));
    }
    Ok(value)
}
fn gamma<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f32, D::Error> {
    let value = f32::deserialize(deserializer)?;
    if !value.is_finite() || !(0.1..=5.0).contains(&value) {
        return Err(serde::de::Error::custom(
            "gamma must be finite and between 0.1 and 5",
        ));
    }
    Ok(value)
}
const fn one() -> f32 {
    1.0
}
const fn yes() -> bool {
    true
}

/// Adjust colour components on selected PDF pages.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AdjustColorsParams {
    /// Brightness adjustment in [-1, 1].
    #[serde(default, deserialize_with = "bounded")]
    #[schemars(range(min = -1.0, max = 1.0))]
    pub brightness: f32,
    /// Contrast adjustment in [-1, 1].
    #[serde(default, deserialize_with = "bounded")]
    #[schemars(range(min = -1.0, max = 1.0))]
    pub contrast: f32,
    /// Saturation adjustment in [-1, 1].
    #[serde(default, deserialize_with = "bounded")]
    #[schemars(range(min = -1.0, max = 1.0))]
    pub saturation: f32,
    /// Gamma exponent in [0.1, 5].
    #[serde(default = "one", deserialize_with = "gamma")]
    #[schemars(range(min = 0.1, max = 5.0))]
    pub gamma: f32,
    /// Whether supported image pixels are adjusted.
    #[serde(default = "yes")]
    pub images: bool,
    /// Selected 1-based pages.
    #[serde(default = "PageRange::all")]
    pub pages: PageRange,
}

impl AdjustColorsParams {
    /// Validate directly constructed parameters as well as deserialized values.
    pub fn validate(&self) -> Result<(), &'static str> {
        if [self.brightness, self.contrast, self.saturation]
            .into_iter()
            .any(|v| !v.is_finite() || !(-1.0..=1.0).contains(&v))
        {
            return Err("adjustments must be finite and between -1 and 1");
        }
        if !self.gamma.is_finite() || !(0.1..=5.0).contains(&self.gamma) {
            return Err("gamma must be finite and between 0.1 and 5");
        }
        if self.is_neutral() {
            return Err("at least one colour adjustment is required");
        }
        Ok(())
    }
    /// Whether the parameters leave all colours unchanged.
    #[must_use]
    pub fn is_neutral(&self) -> bool {
        self.brightness == 0.0
            && self.contrast == 0.0
            && self.saturation == 0.0
            && self.gamma == 1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn defaults_and_validation() {
        let params: AdjustColorsParams = serde_json::from_value(json!({})).unwrap();
        assert!(params.is_neutral());
        assert!(params.images);
        assert_eq!(params.pages.as_str(), "all");
        assert!(
            !serde_json::from_value::<AdjustColorsParams>(json!({"brightness": 0.1}))
                .unwrap()
                .is_neutral()
        );
        assert!(serde_json::from_value::<AdjustColorsParams>(json!({"contrast": 1.1})).is_err());
        assert!(serde_json::from_value::<AdjustColorsParams>(json!({"gamma": 0.0})).is_err());
        assert!(serde_json::from_value::<AdjustColorsParams>(json!({"unknown": true})).is_err());
    }
}
