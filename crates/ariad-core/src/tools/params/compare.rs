//! Parameters for the `pdf.compare` tool.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::tools::PageRange;

/// Comparison mode for `pdf.compare`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CompareMode {
    /// Semantic text diff over reading-order text per page (WASM-capable).
    #[default]
    Text,
    /// Rasterized pixel difference with connected bounding box grouping (Local only).
    Visual,
    /// Run both text and visual comparisons.
    Both,
}

impl CompareMode {
    /// Returns the string identifier of the comparison mode.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Visual => "visual",
            Self::Both => "both",
        }
    }
}

/// Parameters for `pdf.compare`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct CompareParams {
    /// Comparison mode: "text", "visual", or "both" (default: "text").
    pub mode: CompareMode,
    /// Optional page selection to compare (e.g. "1-5", "odd", "1,3").
    /// When omitted, all pages are compared.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pages: Option<PageRange>,
}

impl CompareParams {
    /// Validates comparison parameters.
    pub fn validate(&self) -> Result<(), String> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn defaults_to_text_mode_and_all_pages() {
        let params: CompareParams = serde_json::from_value(json!({})).expect("valid");
        assert_eq!(params.mode, CompareMode::Text);
        assert_eq!(params.pages, None);
    }

    #[test]
    fn parses_modes_and_page_ranges() {
        let text: CompareParams = serde_json::from_value(json!({"mode": "text"})).expect("valid");
        assert_eq!(text.mode, CompareMode::Text);

        let visual: CompareParams =
            serde_json::from_value(json!({"mode": "visual", "pages": "1-3"})).expect("valid");
        assert_eq!(visual.mode, CompareMode::Visual);
        assert_eq!(visual.pages.as_ref().map(PageRange::as_str), Some("1-3"));

        let both: CompareParams = serde_json::from_value(json!({"mode": "both"})).expect("valid");
        assert_eq!(both.mode, CompareMode::Both);
    }

    #[test]
    fn rejects_unknown_fields_and_invalid_modes() {
        assert!(serde_json::from_value::<CompareParams>(json!({"unknown": true})).is_err());
        assert!(serde_json::from_value::<CompareParams>(json!({"mode": "invalid"})).is_err());
    }
}
