//! Parameter types for the `pdf.booklet` tool.

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};

use crate::tools::params::common::PaperSize;

/// Binding edge for saddle-stitch booklet imposition (`pdf.booklet`).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum BookletBinding {
    /// Left-to-right binding (`"left"`, default).
    #[default]
    Left,
    /// Right-to-left binding (`"right"`).
    Right,
}

/// Parameters for `pdf.booklet`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BookletParams {
    /// Output sheet paper size (`"A4"` by default).
    #[serde(default)]
    pub sheet: PaperSize,
    /// Spine binding direction (`"left"` by default, or `"right"`).
    #[serde(default)]
    pub binding: BookletBinding,
    /// Per-signature inner margin creep compensation in PDF points (`0.0` by default, `>= 0`).
    #[serde(default, deserialize_with = "deserialize_creep_pt")]
    #[schemars(range(min = 0.0, max = 1000.0))]
    pub creep_pt: f32,
}

fn deserialize_creep_pt<'de, D>(deserializer: D) -> Result<f32, D::Error>
where
    D: Deserializer<'de>,
{
    let val = f32::deserialize(deserializer)?;
    if !val.is_finite() || !(0.0..=1000.0).contains(&val) {
        return Err(serde::de::Error::custom(
            "creep_pt must be a finite number in [0.0, 1000.0]",
        ));
    }
    Ok(val)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::tools::params::common::StandardPaper;

    #[test]
    fn booklet_params_defaults_unknown_field_and_valid_invalid_json() {
        let defaults: BookletParams = serde_json::from_value(json!({})).unwrap();
        assert_eq!(defaults, BookletParams::default());
        assert_eq!(defaults.sheet, PaperSize::Named(StandardPaper::A4));
        assert_eq!(defaults.binding, BookletBinding::Left);
        assert_eq!(defaults.creep_pt, 0.0);

        let valid: BookletParams = serde_json::from_value(json!({
            "sheet": "Tabloid",
            "binding": "right",
            "creep_pt": 0.5
        }))
        .unwrap();
        assert_eq!(valid.sheet, PaperSize::Named(StandardPaper::Tabloid));
        assert_eq!(valid.binding, BookletBinding::Right);
        assert!((valid.creep_pt - 0.5).abs() < 1e-6);

        assert!(serde_json::from_value::<BookletParams>(json!({"creep_pt": -1.0})).is_err());
        assert!(serde_json::from_value::<BookletParams>(json!({"unknown": 1})).is_err());
    }
}
