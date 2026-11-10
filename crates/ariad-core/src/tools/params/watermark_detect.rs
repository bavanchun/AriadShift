//! Parameters for watermark candidate detection.

use crate::tools::page_range::PageRange;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

fn default_ratio() -> f32 {
    0.6
}

/// Page selection and minimum repetition ratio for watermark detection.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WatermarkDetectParams {
    /// Minimum fraction of selected pages on which repeated content must occur.
    #[serde(default = "default_ratio")]
    #[schemars(range(min = 0.1, max = 1.0))]
    pub min_page_ratio: f32,
    /// Selected 1-based pages.
    #[serde(default = "PageRange::all")]
    pub pages: PageRange,
}
impl Default for WatermarkDetectParams {
    fn default() -> Self {
        Self {
            min_page_ratio: default_ratio(),
            pages: PageRange::all(),
        }
    }
}

impl WatermarkDetectParams {
    /// Reject invalid ratios even for parameters constructed without deserialization.
    pub fn validate(&self) -> Result<(), &'static str> {
        if !self.min_page_ratio.is_finite() || !(0.1..=1.0).contains(&self.min_page_ratio) {
            return Err("min_page_ratio must be between 0.1 and 1");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn defaults_and_ratio_validation() {
        let params: WatermarkDetectParams = serde_json::from_value(json!({})).unwrap();
        assert_eq!(params.min_page_ratio, 0.6);
        assert_eq!(params.pages.as_str(), "all");
        assert!(params.validate().is_ok());
        let invalid: WatermarkDetectParams =
            serde_json::from_value(json!({"min_page_ratio":0.05})).unwrap();
        assert!(invalid.validate().is_err());
        assert!(serde_json::from_value::<WatermarkDetectParams>(json!({"unknown":true})).is_err());
    }
}
