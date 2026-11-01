//! Parameter types for the `pdf.overlay` tool.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::tools::page_range::PageRange;

/// How pages from the second (overlay) PDF are paired with target pages in the base PDF.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum OverlayPagesMode {
    /// Apply page 1 of the overlay PDF to every selected base page (`"first"`, default).
    #[default]
    First,
    /// Cycle through the overlay PDF's pages repeatedly across the selected base pages.
    Cycle,
    /// Pair page `k` of the overlay PDF with page `k` of the base PDF (when present).
    Matching,
}

/// Z-order placement of the overlay relative to the base page content.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum OverlayPosition {
    /// Paint the overlay above the base page content (`"over"`, default).
    #[default]
    Over,
    /// Paint the overlay beneath the base page content (`"under"`).
    Under,
}

/// Parameters for `pdf.overlay`.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OverlayParams {
    /// Overlay page pairing mode (`"first"` by default, `"cycle"`, or `"matching"`).
    #[serde(default)]
    pub overlay_pages: OverlayPagesMode,
    /// Layer ordering relative to the base page (`"over"` by default, or `"under"`).
    #[serde(default)]
    pub position: OverlayPosition,
    /// 1-based base pages to apply the overlay onto (`"all"` by default).
    #[serde(default = "PageRange::all")]
    pub pages: PageRange,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn overlay_params_defaults_unknown_field_and_valid_invalid_json() {
        let defaults: OverlayParams = serde_json::from_value(json!({})).unwrap();
        assert_eq!(defaults, OverlayParams::default());
        assert_eq!(defaults.overlay_pages, OverlayPagesMode::First);
        assert_eq!(defaults.position, OverlayPosition::Over);
        assert_eq!(defaults.pages.as_str(), "all");

        let valid: OverlayParams = serde_json::from_value(json!({
            "overlay_pages": "cycle",
            "position": "under",
            "pages": "1-3"
        }))
        .unwrap();
        assert_eq!(valid.overlay_pages, OverlayPagesMode::Cycle);
        assert_eq!(valid.position, OverlayPosition::Under);
        assert_eq!(valid.pages.as_str(), "1-3");

        assert!(serde_json::from_value::<OverlayParams>(json!({"unknown": 1})).is_err());
        assert!(serde_json::from_value::<OverlayParams>(json!({"position": "side"})).is_err());
    }
}
