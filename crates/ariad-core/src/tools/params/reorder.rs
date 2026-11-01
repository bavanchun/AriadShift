//! Parameter types for the `pdf.reorder` tool.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::tools::page_range::PageRange;

/// Named page-reordering preset for `pdf.reorder`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ReorderPreset {
    /// Reverse the document page order (`last..=1`).
    Reverse,
    /// Interleave front and back duplex scan stacks (`1, N, 2, N-1, ...`).
    #[serde(alias = "duplex_interleave")]
    DuplexInterleave,
    /// Place all odd-numbered pages first followed by all even-numbered pages.
    #[serde(alias = "odd_then_even")]
    OddThenEven,
}

/// Reorder specification: either a named preset (`"reverse"`, `"duplex-interleave"`,
/// `"odd-then-even"`) or an explicit 1-based [`PageRange`] sequence.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum ReorderSpec {
    /// Named reordering preset.
    Preset(ReorderPreset),
    /// Explicit 1-based page range or sequence (duplicates allowed and kept in order).
    Pages(PageRange),
}

/// Parameters for `pdf.reorder`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReorderParams {
    /// Target page order specification.
    pub order: ReorderSpec,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn reorder_params_parses_presets_and_page_ranges_and_rejects_invalid() {
        let preset: ReorderParams =
            serde_json::from_value(json!({"order": "duplex-interleave"})).unwrap();
        assert_eq!(
            preset.order,
            ReorderSpec::Preset(ReorderPreset::DuplexInterleave)
        );

        let range: ReorderParams =
            serde_json::from_value(json!({"order": "3,1,2,4-last"})).unwrap();
        assert!(matches!(range.order, ReorderSpec::Pages(_)));

        assert!(serde_json::from_value::<ReorderParams>(json!({})).is_err());
        assert!(
            serde_json::from_value::<ReorderParams>(json!({"order": "reverse", "extra": 1}))
                .is_err()
        );
        assert!(serde_json::from_value::<ReorderParams>(json!({"order": "not-a-range"})).is_err());
    }
}
