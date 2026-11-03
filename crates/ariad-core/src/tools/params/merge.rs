//! Parameter types for the `pdf.merge` tool.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Outline (bookmark) handling strategy when merging multiple PDFs (`pdf.merge`).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum MergeOutlineMode {
    /// Preserve existing bookmark trees from each input file with updated destination offsets.
    Keep,
    /// Create one top-level bookmark entry per input file titled with the input file stem.
    #[default]
    #[serde(alias = "per_file")]
    PerFile,
    /// Omit outlines from the merged document.
    None,
}

/// Page label handling strategy when merging multiple PDFs (`pdf.merge`).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum MergePageLabelsMode {
    /// Preserve per-document `/PageLabels` number trees at each input file's starting page offset.
    Keep,
    /// Renumber pages sequentially from `1` across the merged document.
    #[default]
    Renumber,
}

/// Parameters for `pdf.merge`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MergeParams {
    /// How bookmarks (`/Outlines`) from the input files are combined (`"per-file"` by default).
    #[serde(default)]
    pub outline: MergeOutlineMode,
    /// How `/PageLabels` are combined across inputs (`"renumber"` by default).
    #[serde(default)]
    pub page_labels: MergePageLabelsMode,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn merge_params_defaults_unknown_field_and_valid_invalid_json() {
        let defaults: MergeParams = serde_json::from_value(json!({})).unwrap();
        assert_eq!(defaults, MergeParams::default());
        assert_eq!(defaults.outline, MergeOutlineMode::PerFile);
        assert_eq!(defaults.page_labels, MergePageLabelsMode::Renumber);

        let valid: MergeParams = serde_json::from_value(json!({
            "outline": "keep",
            "page_labels": "keep"
        }))
        .unwrap();
        assert_eq!(valid.outline, MergeOutlineMode::Keep);
        assert_eq!(valid.page_labels, MergePageLabelsMode::Keep);

        assert!(serde_json::from_value::<MergeParams>(json!({"unknown": true})).is_err());
        assert!(serde_json::from_value::<MergeParams>(json!({"outline": "bogus"})).is_err());
    }
}
