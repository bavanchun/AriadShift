//! Parameter types for the `pdf.nup` tool.

use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Deserializer, Serialize};

use crate::tools::params::common::{GridOrder, Margins, PaperSize};

const fn default_per_sheet() -> u8 {
    2
}

/// Parameters for `pdf.nup`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NupParams {
    /// Number of input pages placed on each output sheet (`2` by default; `2 | 4 | 6 | 8 | 9 | 16`).
    #[serde(
        default = "default_per_sheet",
        deserialize_with = "deserialize_per_sheet"
    )]
    #[schemars(schema_with = "per_sheet_schema")]
    pub per_sheet: u8,
    /// Target output sheet size (`"A4"` by default).
    #[serde(default)]
    pub sheet: PaperSize,
    /// Grid cell filling order (`"row"` by default, or `"column"`).
    #[serde(default)]
    pub order: GridOrder,
    /// Outer and cell margins in PDF points (`0.0` by default).
    #[serde(default)]
    pub margins_pt: Margins,
    /// Whether to draw thin border hairlines around each placed page cell (`false` by default).
    #[serde(default)]
    pub borders: bool,
}

impl Default for NupParams {
    fn default() -> Self {
        Self {
            per_sheet: default_per_sheet(),
            sheet: PaperSize::default(),
            order: GridOrder::default(),
            margins_pt: Margins::default(),
            borders: false,
        }
    }
}

fn deserialize_per_sheet<'de, D>(deserializer: D) -> Result<u8, D::Error>
where
    D: Deserializer<'de>,
{
    let val = u8::deserialize(deserializer)?;
    match val {
        2 | 4 | 6 | 8 | 9 | 16 => Ok(val),
        other => Err(serde::de::Error::custom(format!(
            "invalid per_sheet {other}: expected 2, 4, 6, 8, 9, or 16"
        ))),
    }
}

fn per_sheet_schema(_gen: &mut SchemaGenerator) -> Schema {
    schemars::json_schema!({
        "description": "Number of input pages placed on each output sheet (`2`, `4`, `6`, `8`, `9`, or `16`).",
        "type": "integer",
        "default": 2,
        "enum": [2, 4, 6, 8, 9, 16],
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::tools::params::common::StandardPaper;

    #[test]
    fn nup_params_defaults_unknown_field_and_valid_invalid_json() {
        let defaults: NupParams = serde_json::from_value(json!({})).unwrap();
        assert_eq!(defaults, NupParams::default());
        assert_eq!(defaults.per_sheet, 2);
        assert_eq!(defaults.sheet, PaperSize::Named(StandardPaper::A4));
        assert_eq!(defaults.order, GridOrder::Row);
        assert_eq!(defaults.margins_pt, Margins::Uniform(0.0));
        assert!(!defaults.borders);

        let valid: NupParams = serde_json::from_value(json!({
            "per_sheet": 4,
            "sheet": "Letter",
            "order": "column",
            "margins_pt": 18.0,
            "borders": true
        }))
        .unwrap();
        assert_eq!(valid.per_sheet, 4);
        assert_eq!(valid.order, GridOrder::Column);
        assert!(valid.borders);

        assert!(serde_json::from_value::<NupParams>(json!({"per_sheet": 3})).is_err());
        assert!(serde_json::from_value::<NupParams>(json!({"unknown": 1})).is_err());
    }
}
