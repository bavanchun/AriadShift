//! Parameter type for the `pdf.remove_blank` tool.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::render::{MAX_DPI, MIN_DPI};
use super::{ParamsError, check_range};

/// Default share of white pixels at or above which a page counts as blank.
pub const DEFAULT_THRESHOLD: f64 = 0.99;
/// Default analysis resolution in dots per inch.
pub const DEFAULT_DPI: u32 = 50;

fn default_threshold() -> f64 {
    DEFAULT_THRESHOLD
}

fn default_dpi() -> u32 {
    DEFAULT_DPI
}

/// Parameters for `pdf.remove_blank`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RemoveBlankParams {
    /// Share of white pixels (0 to 1) at or above which a page counts as blank.
    #[serde(default = "default_threshold")]
    #[schemars(range(min = 0.0, max = 1.0))]
    pub threshold: f64,
    /// Resolution in dots per inch at which pages are rendered for the analysis.
    #[serde(default = "default_dpi")]
    #[schemars(range(min = 36, max = 600))]
    pub dpi: u32,
}

impl Default for RemoveBlankParams {
    fn default() -> Self {
        Self {
            threshold: DEFAULT_THRESHOLD,
            dpi: DEFAULT_DPI,
        }
    }
}

impl RemoveBlankParams {
    /// Checks the ranges the JSON schema alone cannot enforce.
    pub fn validate(&self) -> Result<(), ParamsError> {
        if !(self.threshold.is_finite() && (0.0..=1.0).contains(&self.threshold)) {
            return Err(ParamsError::new(
                "threshold",
                "threshold must be between 0 and 1",
            ));
        }
        check_range("dpi", self.dpi, MIN_DPI, MAX_DPI)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn defaults_follow_the_tool_table() {
        let params: RemoveBlankParams = serde_json::from_value(json!({})).expect("valid");
        assert_eq!(params, RemoveBlankParams::default());
        assert_eq!(params.threshold, 0.99);
        assert_eq!(params.dpi, 50);
        params.validate().expect("defaults validate");
    }

    #[test]
    fn accepts_a_valid_example() {
        let params: RemoveBlankParams =
            serde_json::from_value(json!({"threshold": 0.95, "dpi": 72})).expect("valid");
        params.validate().expect("valid");
    }

    #[test]
    fn rejects_invalid_examples() {
        for value in [
            json!({"threshold": "high"}),
            json!({"pages": "all"}),
            json!({"dpi": 1.5}),
        ] {
            assert!(serde_json::from_value::<RemoveBlankParams>(value).is_err());
        }
        for (threshold, dpi) in [(-0.1, 50), (1.1, 50), (f64::NAN, 50), (0.5, 35), (0.5, 601)] {
            let params = RemoveBlankParams { threshold, dpi };
            assert!(params.validate().is_err(), "{threshold} {dpi}");
        }
    }
}
