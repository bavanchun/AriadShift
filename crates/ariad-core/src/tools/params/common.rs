//! Shared parameter types (`PaperSize`, `StandardPaper`, `CustomPaperSize`, `GridOrder`, `Margins`)
//! used across multiple structural PDF tools.

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};

/// Named ISO and North American standard paper sizes (`docs/tools.md`).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
pub enum StandardPaper {
    /// ISO A0 (`2383.94 × 3370.39 pt`, `841 × 1189 mm`).
    #[serde(alias = "a0")]
    A0,
    /// ISO A1 (`1683.78 × 2383.94 pt`, `594 × 841 mm`).
    #[serde(alias = "a1")]
    A1,
    /// ISO A2 (`1190.55 × 1683.78 pt`, `420 × 594 mm`).
    #[serde(alias = "a2")]
    A2,
    /// ISO A3 (`841.89 × 1190.55 pt`, `297 × 420 mm`).
    #[serde(alias = "a3")]
    A3,
    /// ISO A4 (`595.28 × 841.89 pt`, `210 × 297 mm`).
    #[default]
    #[serde(alias = "a4")]
    A4,
    /// ISO A5 (`419.53 × 595.28 pt`, `148 × 210 mm`).
    #[serde(alias = "a5")]
    A5,
    /// ISO A6 (`297.64 × 419.53 pt`, `105 × 148 mm`).
    #[serde(alias = "a6")]
    A6,
    /// US Letter (`612.0 × 792.0 pt`, `8.5 × 11 in`).
    #[serde(alias = "letter", alias = "LETTER")]
    Letter,
    /// US Legal (`612.0 × 1008.0 pt`, `8.5 × 14 in`).
    #[serde(alias = "legal", alias = "LEGAL")]
    Legal,
    /// US Tabloid (`792.0 × 1224.0 pt`, `11 × 17 in` portrait).
    #[serde(alias = "tabloid", alias = "TABLOID")]
    Tabloid,
    /// US Ledger (`1224.0 × 792.0 pt`, `17 × 11 in` landscape).
    #[serde(alias = "ledger", alias = "LEDGER")]
    Ledger,
}

impl StandardPaper {
    /// Canonical name of this standard paper size.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::A0 => "A0",
            Self::A1 => "A1",
            Self::A2 => "A2",
            Self::A3 => "A3",
            Self::A4 => "A4",
            Self::A5 => "A5",
            Self::A6 => "A6",
            Self::Letter => "Letter",
            Self::Legal => "Legal",
            Self::Tabloid => "Tabloid",
            Self::Ledger => "Ledger",
        }
    }

    /// Width and height in PDF points (`1 pt = 1/72 inch`).
    #[must_use]
    pub const fn dimensions_pt(self) -> (f64, f64) {
        match self {
            Self::A0 => (2383.94, 3370.39),
            Self::A1 => (1683.78, 2383.94),
            Self::A2 => (1190.55, 1683.78),
            Self::A3 => (841.89, 1190.55),
            Self::A4 => (595.28, 841.89),
            Self::A5 => (419.53, 595.28),
            Self::A6 => (297.64, 419.53),
            Self::Letter => (612.0, 792.0),
            Self::Legal => (612.0, 1008.0),
            Self::Tabloid => (792.0, 1224.0),
            Self::Ledger => (1224.0, 792.0),
        }
    }
}

/// Custom paper size specified as `{ w, h }` in PDF points (`1 pt = 1/72 inch`).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CustomPaperSize {
    /// Page width in PDF points (`> 0`).
    #[serde(deserialize_with = "deserialize_positive_pt")]
    #[schemars(extend("exclusiveMinimum" = 0.0, "maximum" = 1_000_000.0))]
    pub w: f64,
    /// Page height in PDF points (`> 0`).
    #[serde(deserialize_with = "deserialize_positive_pt")]
    #[schemars(extend("exclusiveMinimum" = 0.0, "maximum" = 1_000_000.0))]
    pub h: f64,
}

fn deserialize_positive_pt<'de, D>(deserializer: D) -> Result<f64, D::Error>
where
    D: Deserializer<'de>,
{
    let val = f64::deserialize(deserializer)?;
    if !val.is_finite() || val <= 0.0 || val > 1_000_000.0 {
        return Err(serde::de::Error::custom(
            "paper dimension must be a finite number in (0, 1000000] points",
        ));
    }
    Ok(val)
}

/// Paper size specification: either a named standard paper (`"A4"`, `"Letter"`, etc.) or custom
/// `{ w, h }` dimensions in PDF points.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum PaperSize {
    /// Named standard paper size (`"A0"`..`"A6"`, `"Letter"`, `"Legal"`, `"Tabloid"`, `"Ledger"`).
    Named(StandardPaper),
    /// Custom paper dimensions `{ w, h }` in PDF points.
    Custom(CustomPaperSize),
}

impl Default for PaperSize {
    fn default() -> Self {
        Self::Named(StandardPaper::A4)
    }
}

impl PaperSize {
    /// Returns the `(width_pt, height_pt)` dimensions in PDF points.
    #[must_use]
    pub const fn dimensions_pt(self) -> (f64, f64) {
        match self {
            Self::Named(paper) => paper.dimensions_pt(),
            Self::Custom(CustomPaperSize { w, h }) => (w, h),
        }
    }
}

/// Grid cell ordering for multi-cell layout tools (`pdf.nup`, `pdf.divide_pages`).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum GridOrder {
    /// Left-to-right, top-to-bottom row-major order.
    #[default]
    Row,
    /// Top-to-bottom, left-to-right column-major order.
    Column,
}

/// Explicit per-side margins in PDF points (`top`, `right`, `bottom`, `left`).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MarginSides {
    /// Top margin in PDF points (`>= 0`).
    #[serde(deserialize_with = "deserialize_non_negative_pt")]
    #[schemars(range(min = 0.0, max = 100_000.0))]
    pub top: f64,
    /// Right margin in PDF points (`>= 0`).
    #[serde(deserialize_with = "deserialize_non_negative_pt")]
    #[schemars(range(min = 0.0, max = 100_000.0))]
    pub right: f64,
    /// Bottom margin in PDF points (`>= 0`).
    #[serde(deserialize_with = "deserialize_non_negative_pt")]
    #[schemars(range(min = 0.0, max = 100_000.0))]
    pub bottom: f64,
    /// Left margin in PDF points (`>= 0`).
    #[serde(deserialize_with = "deserialize_non_negative_pt")]
    #[schemars(range(min = 0.0, max = 100_000.0))]
    pub left: f64,
}

fn deserialize_non_negative_pt<'de, D>(deserializer: D) -> Result<f64, D::Error>
where
    D: Deserializer<'de>,
{
    let val = f64::deserialize(deserializer)?;
    if !val.is_finite() || !(0.0..=100_000.0).contains(&val) {
        return Err(serde::de::Error::custom(
            "margin must be a finite number in [0, 100000] points",
        ));
    }
    Ok(val)
}

/// Page margin specification in PDF points: either a uniform non-negative number or per-side
/// `{ top, right, bottom, left }`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Margins {
    /// Uniform margin on all four sides in PDF points (`>= 0`).
    Uniform(
        #[serde(deserialize_with = "deserialize_non_negative_pt")]
        #[schemars(range(min = 0.0, max = 100_000.0))]
        f64,
    ),
    /// Per-side margins in PDF points.
    Sides(MarginSides),
}

impl Default for Margins {
    fn default() -> Self {
        Self::Uniform(0.0)
    }
}

impl Margins {
    /// Returns `(top, right, bottom, left)` in PDF points.
    #[must_use]
    pub const fn to_trbl(self) -> (f64, f64, f64, f64) {
        match self {
            Self::Uniform(m) => (m, m, m, m),
            Self::Sides(MarginSides {
                top,
                right,
                bottom,
                left,
            }) => (top, right, bottom, left),
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn paper_size_parses_standard_names_and_custom_dimensions() {
        assert_eq!(PaperSize::default(), PaperSize::Named(StandardPaper::A4));

        let a4: PaperSize = serde_json::from_value(json!("A4")).unwrap();
        assert_eq!(a4.dimensions_pt(), (595.28, 841.89));

        let letter_lower: PaperSize = serde_json::from_value(json!("letter")).unwrap();
        assert_eq!(letter_lower, PaperSize::Named(StandardPaper::Letter));
        assert_eq!(letter_lower.dimensions_pt(), (612.0, 792.0));

        let custom: PaperSize = serde_json::from_value(json!({"w": 400.0, "h": 600.0})).unwrap();
        assert_eq!(custom.dimensions_pt(), (400.0, 600.0));

        assert!(serde_json::from_value::<PaperSize>(json!("UnknownPaper")).is_err());
        assert!(serde_json::from_value::<PaperSize>(json!({"w": 0.0, "h": 600.0})).is_err());
        assert!(
            serde_json::from_value::<PaperSize>(json!({"w": 400.0, "h": 600.0, "extra": 1}))
                .is_err()
        );
    }

    #[test]
    fn margins_parses_uniform_and_per_side_objects() {
        assert_eq!(Margins::default().to_trbl(), (0.0, 0.0, 0.0, 0.0));

        let uniform: Margins = serde_json::from_value(json!(18.0)).unwrap();
        assert_eq!(uniform.to_trbl(), (18.0, 18.0, 18.0, 18.0));

        let sides: Margins = serde_json::from_value(json!({
            "top": 10.0,
            "right": 20.0,
            "bottom": 30.0,
            "left": 40.0
        }))
        .unwrap();
        assert_eq!(sides.to_trbl(), (10.0, 20.0, 30.0, 40.0));

        assert!(serde_json::from_value::<Margins>(json!(-5.0)).is_err());
        assert!(
            serde_json::from_value::<Margins>(json!({
                "top": 10.0,
                "right": 20.0,
                "bottom": 30.0,
                "left": 40.0,
                "unknown": 1.0
            }))
            .is_err()
        );
    }
}
