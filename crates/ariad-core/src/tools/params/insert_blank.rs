//! Parameter types for the `pdf.insert_blank` tool.

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};

use crate::tools::{page_range::PageRange, params::common::PaperSize};

/// Marker variant for matching the adjacent page's size (`"same"`).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum SamePageSize {
    /// Match the width and height of the reference page.
    #[default]
    Same,
}

/// Blank page size specification for `pdf.insert_blank`: `"same"` or a [`PaperSize`].
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum BlankPageSize {
    /// Match the dimensions of the page after which the blank page is inserted.
    Same(SamePageSize),
    /// Explicit standard or custom paper dimensions.
    Paper(PaperSize),
}

impl Default for BlankPageSize {
    fn default() -> Self {
        Self::Same(SamePageSize::Same)
    }
}

const fn default_blank_count() -> u32 {
    1
}

/// Parameters for `pdf.insert_blank`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InsertBlankParams {
    /// 1-based pages after which to insert blank pages (`"all"` by default).
    #[serde(default = "PageRange::all")]
    pub after: PageRange,
    /// Number of blank pages to insert at each selected position (`1` by default, `>= 1`).
    #[serde(
        default = "default_blank_count",
        deserialize_with = "deserialize_nonzero_count"
    )]
    #[schemars(range(min = 1))]
    pub count: u32,
    /// Blank page dimensions (`"same"` by default, or a named/custom paper size).
    #[serde(default)]
    pub size: BlankPageSize,
}

impl Default for InsertBlankParams {
    fn default() -> Self {
        Self {
            after: PageRange::all(),
            count: default_blank_count(),
            size: BlankPageSize::default(),
        }
    }
}

fn deserialize_nonzero_count<'de, D>(deserializer: D) -> Result<u32, D::Error>
where
    D: Deserializer<'de>,
{
    let count = u32::deserialize(deserializer)?;
    if count == 0 {
        return Err(serde::de::Error::custom("count must be >= 1"));
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::tools::params::common::StandardPaper;

    #[test]
    fn insert_blank_params_defaults_unknown_field_and_valid_invalid_json() {
        let defaults: InsertBlankParams = serde_json::from_value(json!({})).unwrap();
        assert_eq!(defaults, InsertBlankParams::default());
        assert_eq!(defaults.after.as_str(), "all");
        assert_eq!(defaults.count, 1);
        assert_eq!(defaults.size, BlankPageSize::Same(SamePageSize::Same));

        let valid: InsertBlankParams = serde_json::from_value(json!({
            "after": "1,3",
            "count": 2,
            "size": "Letter"
        }))
        .unwrap();
        assert_eq!(valid.after.as_str(), "1,3");
        assert_eq!(valid.count, 2);
        assert_eq!(
            valid.size,
            BlankPageSize::Paper(PaperSize::Named(StandardPaper::Letter))
        );

        assert!(serde_json::from_value::<InsertBlankParams>(json!({"unknown": 1})).is_err());
        assert!(serde_json::from_value::<InsertBlankParams>(json!({"count": 0})).is_err());
    }
}
