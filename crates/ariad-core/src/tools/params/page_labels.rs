//! Parameter types for the `pdf.page_labels` tool.

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};

/// Numbering style for a `/PageLabels` range (`pdf.page_labels`).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum PageLabelStyle {
    /// Decimal Arabic numerals (`1, 2, 3, ...`; PDF `/S /D`).
    #[default]
    Decimal,
    /// Uppercase Roman numerals (`I, II, III, ...`; PDF `/S /R`).
    #[serde(alias = "roman_upper")]
    RomanUpper,
    /// Lowercase Roman numerals (`i, ii, iii, ...`; PDF `/S /r`).
    #[serde(alias = "roman_lower")]
    RomanLower,
    /// Uppercase letters (`A, B, C, ...`; PDF `/S /A`).
    #[serde(alias = "alpha_upper")]
    AlphaUpper,
    /// Lowercase letters (`a, b, c, ...`; PDF `/S /a`).
    #[serde(alias = "alpha_lower")]
    AlphaLower,
    /// Prefix only with no numeric portion (PDF `/S` omitted).
    None,
}

/// A single page-label numbering range starting at 1-based page `from`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PageLabelRange {
    /// 1-based first page index where this numbering range begins (`>= 1`).
    #[serde(deserialize_with = "deserialize_nonzero_u32")]
    #[schemars(range(min = 1))]
    pub from: u32,
    /// Numbering style (`"decimal"`, `"roman-upper"`, `"roman-lower"`, `"alpha-upper"`, `"alpha-lower"`, `"none"`).
    #[serde(default)]
    pub style: PageLabelStyle,
    /// Optional label prefix prepended before the numeric portion (PDF `/P`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
    /// Starting numeric value for this range (`1` by default, `>= 1`; PDF `/St`).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_optional_nonzero_u32"
    )]
    #[schemars(range(min = 1))]
    pub start: Option<u32>,
}

fn deserialize_nonzero_u32<'de, D>(deserializer: D) -> Result<u32, D::Error>
where
    D: Deserializer<'de>,
{
    let val = u32::deserialize(deserializer)?;
    if val == 0 {
        return Err(serde::de::Error::custom("from page index must be >= 1"));
    }
    Ok(val)
}

fn deserialize_optional_nonzero_u32<'de, D>(deserializer: D) -> Result<Option<u32>, D::Error>
where
    D: Deserializer<'de>,
{
    let opt = Option::<u32>::deserialize(deserializer)?;
    if matches!(opt, Some(0)) {
        return Err(serde::de::Error::custom("start value must be >= 1"));
    }
    Ok(opt)
}

/// Parameters for `pdf.page_labels`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PageLabelsParams {
    /// Ordered list of page-label ranges (`>= 1` entry).
    #[serde(deserialize_with = "deserialize_nonempty_ranges")]
    #[schemars(length(min = 1))]
    pub ranges: Vec<PageLabelRange>,
}

fn deserialize_nonempty_ranges<'de, D>(deserializer: D) -> Result<Vec<PageLabelRange>, D::Error>
where
    D: Deserializer<'de>,
{
    let ranges = Vec::<PageLabelRange>::deserialize(deserializer)?;
    if ranges.is_empty() {
        return Err(serde::de::Error::custom(
            "ranges must contain at least one page label range",
        ));
    }
    Ok(ranges)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn page_labels_params_validates_ranges_and_rejects_unknown_fields() {
        let valid: PageLabelsParams = serde_json::from_value(json!({
            "ranges": [
                { "from": 1, "style": "roman-lower" },
                { "from": 5, "style": "decimal", "prefix": "p-", "start": 1 }
            ]
        }))
        .unwrap();
        assert_eq!(valid.ranges.len(), 2);
        assert_eq!(valid.ranges[0].style, PageLabelStyle::RomanLower);
        assert_eq!(valid.ranges[1].prefix.as_deref(), Some("p-"));

        assert!(serde_json::from_value::<PageLabelsParams>(json!({})).is_err());
        assert!(serde_json::from_value::<PageLabelsParams>(json!({"ranges": []})).is_err());
        assert!(
            serde_json::from_value::<PageLabelsParams>(json!({
                "ranges": [{ "from": 0, "style": "decimal" }]
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<PageLabelsParams>(json!({
                "ranges": [{ "from": 1 }],
                "unknown": true
            }))
            .is_err()
        );
    }
}
