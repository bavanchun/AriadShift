//! Parameters for `pdf.replace_color`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::tools::{page_range::PageRange, params::HexColor};

/// Which painted elements are recoloured.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ColorTarget {
    /// Text only.
    Text,
    /// Vector graphics only.
    Vector,
    /// Text and vector graphics.
    #[default]
    All,
}

/// Replace matching text and vector colours on selected PDF pages.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReplaceColorParams {
    /// Source RGB colour; when absent, every supported colour matches.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<HexColor>,
    /// Replacement RGB colour.
    pub to: HexColor,
    /// Maximum per-channel difference in 8-bit RGB.
    #[serde(default)]
    pub tolerance: u8,
    /// Target paint operations.
    #[serde(default)]
    pub target: ColorTarget,
    /// Selected 1-based pages.
    #[serde(default = "PageRange::all")]
    pub pages: PageRange,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn defaults_and_validation() {
        let params: ReplaceColorParams = serde_json::from_value(json!({"to": "#B00020"})).unwrap();
        assert_eq!(params.from, None);
        assert_eq!(params.tolerance, 0);
        assert_eq!(params.target, ColorTarget::All);
        assert_eq!(params.pages.as_str(), "all");
        assert!(serde_json::from_value::<ReplaceColorParams>(json!({})).is_err());
        assert!(serde_json::from_value::<ReplaceColorParams>(json!({"to":"red"})).is_err());
        assert!(
            serde_json::from_value::<ReplaceColorParams>(json!({"to":"#000000", "tolerance":256}))
                .is_err()
        );
        assert!(
            serde_json::from_value::<ReplaceColorParams>(json!({"to":"#000000", "target":"image"}))
                .is_err()
        );
    }
}
