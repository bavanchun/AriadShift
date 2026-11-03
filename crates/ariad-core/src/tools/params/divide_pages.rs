//! Parameter types for the `pdf.divide_pages` tool.

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};

use crate::tools::{page_range::PageRange, params::common::GridOrder};

const fn default_grid_dim() -> u8 {
    2
}

/// Parameters for `pdf.divide_pages`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DividePagesParams {
    /// 1-based pages to subdivide (`"all"` by default).
    #[serde(default = "PageRange::all")]
    pub pages: PageRange,
    /// Number of horizontal columns to divide each page into (`1..=8`, `2` by default).
    #[serde(
        default = "default_grid_dim",
        deserialize_with = "deserialize_grid_dim"
    )]
    #[schemars(range(min = 1, max = 8))]
    pub cols: u8,
    /// Number of vertical rows to divide each page into (`1..=8`, `2` by default).
    #[serde(
        default = "default_grid_dim",
        deserialize_with = "deserialize_grid_dim"
    )]
    #[schemars(range(min = 1, max = 8))]
    pub rows: u8,
    /// Emission order of subdivided page tiles (`"row"` by default, or `"column"`).
    #[serde(default)]
    pub order: GridOrder,
}

impl Default for DividePagesParams {
    fn default() -> Self {
        Self {
            pages: PageRange::all(),
            cols: default_grid_dim(),
            rows: default_grid_dim(),
            order: GridOrder::default(),
        }
    }
}

fn deserialize_grid_dim<'de, D>(deserializer: D) -> Result<u8, D::Error>
where
    D: Deserializer<'de>,
{
    let dim = u8::deserialize(deserializer)?;
    if !(1..=8).contains(&dim) {
        return Err(serde::de::Error::custom(format!(
            "grid dimension {dim} out of bounds: expected 1..=8"
        )));
    }
    Ok(dim)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn divide_pages_params_defaults_unknown_field_and_valid_invalid_json() {
        let defaults: DividePagesParams = serde_json::from_value(json!({})).unwrap();
        assert_eq!(defaults, DividePagesParams::default());
        assert_eq!(defaults.pages.as_str(), "all");
        assert_eq!(defaults.cols, 2);
        assert_eq!(defaults.rows, 2);
        assert_eq!(defaults.order, GridOrder::Row);

        let valid: DividePagesParams = serde_json::from_value(json!({
            "pages": "1-2",
            "cols": 3,
            "rows": 4,
            "order": "column"
        }))
        .unwrap();
        assert_eq!(valid.cols, 3);
        assert_eq!(valid.rows, 4);
        assert_eq!(valid.order, GridOrder::Column);

        assert!(serde_json::from_value::<DividePagesParams>(json!({"cols": 0})).is_err());
        assert!(serde_json::from_value::<DividePagesParams>(json!({"rows": 9})).is_err());
        assert!(serde_json::from_value::<DividePagesParams>(json!({"unknown": 1})).is_err());
    }
}
