//! Parameter types for the `pdf.scale` tool.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::tools::{page_range::PageRange, params::common::PaperSize};

/// Aspect-ratio fitting mode for `pdf.scale`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ScaleFit {
    /// Uniformly scale content to fit inside the target sheet while preserving aspect ratio.
    #[default]
    Contain,
    /// Uniformly scale content to cover the entire target sheet while preserving aspect ratio.
    Cover,
    /// Non-uniformly stretch content horizontally and vertically to fill the target sheet.
    Stretch,
}

/// Parameters for `pdf.scale`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScaleParams {
    /// 1-based pages to scale (`"all"` by default).
    #[serde(default = "PageRange::all")]
    pub pages: PageRange,
    /// Target paper size (`"A4"` by default, or `"Letter"`, `"Legal"`, `"A3"`, `"A5"`, `{w, h}`).
    #[serde(default)]
    pub to: PaperSize,
    /// Fitting mode (`"contain"` by default, `"cover"`, or `"stretch"`).
    #[serde(default)]
    pub fit: ScaleFit,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::tools::params::common::StandardPaper;

    #[test]
    fn scale_params_defaults_unknown_field_and_valid_invalid_json() {
        let defaults: ScaleParams = serde_json::from_value(json!({})).unwrap();
        assert_eq!(defaults, ScaleParams::default());
        assert_eq!(defaults.pages.as_str(), "all");
        assert_eq!(defaults.to, PaperSize::Named(StandardPaper::A4));
        assert_eq!(defaults.fit, ScaleFit::Contain);

        let valid: ScaleParams = serde_json::from_value(json!({
            "pages": "1,3",
            "to": "Letter",
            "fit": "cover"
        }))
        .unwrap();
        assert_eq!(valid.to, PaperSize::Named(StandardPaper::Letter));
        assert_eq!(valid.fit, ScaleFit::Cover);

        assert!(serde_json::from_value::<ScaleParams>(json!({"unknown": 1})).is_err());
        assert!(serde_json::from_value::<ScaleParams>(json!({"fit": "invalid"})).is_err());
    }
}
