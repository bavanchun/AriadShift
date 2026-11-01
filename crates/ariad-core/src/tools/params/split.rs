//! Parameter types for the `pdf.split` tool.

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};

use crate::tools::page_range::PageRange;

/// Splitting strategy for `pdf.split`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum SplitMode {
    /// Split into one output PDF per page range in `ranges`.
    #[default]
    Ranges,
    /// Split into chunks of `every` consecutive pages.
    Every,
    /// Split at top-level or `level`-depth bookmark (`/Outlines`) boundaries.
    Bookmarks,
    /// Split so each output chunk stays at or below `max_bytes`.
    Size,
    /// Split on content-empty separator pages (pages with no drawing operators or only whitespace).
    #[serde(alias = "blank_separator")]
    BlankSeparator,
}

/// Parameters for `pdf.split`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SplitParams {
    /// Splitting mode (`"ranges"`, `"every"`, `"bookmarks"`, `"size"`, or `"blank-separator"`).
    #[serde(default)]
    pub mode: SplitMode,
    /// Explicit 1-based page ranges when `mode == "ranges"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ranges: Option<Vec<PageRange>>,
    /// Number of pages per output chunk when `mode == "every"` (`>= 1`).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_optional_nonzero_u32"
    )]
    #[schemars(range(min = 1))]
    pub every: Option<u32>,
    /// Outline hierarchy depth (`>= 1`) when `mode == "bookmarks"`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_optional_nonzero_u8"
    )]
    #[schemars(range(min = 1))]
    pub level: Option<u8>,
    /// Target maximum byte size per output part when `mode == "size"` (`>= 1`).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_optional_nonzero_u64"
    )]
    #[schemars(range(min = 1))]
    pub max_bytes: Option<u64>,
    /// Optional content-emptiness threshold (`0.0..=1.0`) when `mode == "blank-separator"`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_optional_threshold"
    )]
    #[schemars(range(min = 0.0, max = 1.0))]
    pub blank_threshold: Option<f32>,
}

fn deserialize_optional_nonzero_u32<'de, D>(deserializer: D) -> Result<Option<u32>, D::Error>
where
    D: Deserializer<'de>,
{
    let opt = Option::<u32>::deserialize(deserializer)?;
    if matches!(opt, Some(0)) {
        return Err(serde::de::Error::custom("every must be >= 1"));
    }
    Ok(opt)
}

fn deserialize_optional_nonzero_u8<'de, D>(deserializer: D) -> Result<Option<u8>, D::Error>
where
    D: Deserializer<'de>,
{
    let opt = Option::<u8>::deserialize(deserializer)?;
    if matches!(opt, Some(0)) {
        return Err(serde::de::Error::custom("level must be >= 1"));
    }
    Ok(opt)
}

fn deserialize_optional_nonzero_u64<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: Deserializer<'de>,
{
    let opt = Option::<u64>::deserialize(deserializer)?;
    if matches!(opt, Some(0)) {
        return Err(serde::de::Error::custom("max_bytes must be >= 1"));
    }
    Ok(opt)
}

fn deserialize_optional_threshold<'de, D>(deserializer: D) -> Result<Option<f32>, D::Error>
where
    D: Deserializer<'de>,
{
    let opt = Option::<f32>::deserialize(deserializer)?;
    if let Some(val) = opt
        && (!val.is_finite() || !(0.0..=1.0).contains(&val))
    {
        return Err(serde::de::Error::custom(
            "blank_threshold must be a finite number in [0.0, 1.0]",
        ));
    }
    Ok(opt)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn split_params_defaults_unknown_field_and_valid_invalid_json() {
        let defaults: SplitParams = serde_json::from_value(json!({})).unwrap();
        assert_eq!(defaults.mode, SplitMode::Ranges);
        assert!(defaults.ranges.is_none());
        assert!(defaults.every.is_none());

        let valid: SplitParams = serde_json::from_value(json!({
            "mode": "every",
            "every": 5
        }))
        .unwrap();
        assert_eq!(valid.mode, SplitMode::Every);
        assert_eq!(valid.every, Some(5));

        let ranges_valid: SplitParams = serde_json::from_value(json!({
            "mode": "ranges",
            "ranges": ["1-3", "4-last"]
        }))
        .unwrap();
        assert_eq!(ranges_valid.ranges.as_ref().map(Vec::len), Some(2));

        assert!(serde_json::from_value::<SplitParams>(json!({"unknown": 1})).is_err());
        assert!(
            serde_json::from_value::<SplitParams>(json!({"mode": "every", "every": 0})).is_err()
        );
        assert!(serde_json::from_value::<SplitParams>(json!({"blank_threshold": 1.5})).is_err());
    }
}
