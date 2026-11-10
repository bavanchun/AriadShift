//! Parameters for `deskew`.

use crate::tools::PageRange;
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};

fn default_max_angle_deg() -> f32 {
    10.0
}
fn deserialize_max_angle_deg<'de, D: Deserializer<'de>>(d: D) -> Result<f32, D::Error> {
    let v = f32::deserialize(d)?;
    if !v.is_finite() {
        return Err(serde::de::Error::custom("value must be finite"));
    }
    if !(0.5..=45.0).contains(&v) {
        return Err(serde::de::Error::custom(
            "max_angle_deg outside allowed range",
        ));
    }
    Ok(v)
}
fn default_min_angle_deg() -> f32 {
    0.1
}
fn deserialize_min_angle_deg<'de, D: Deserializer<'de>>(d: D) -> Result<f32, D::Error> {
    let v = f32::deserialize(d)?;
    if !v.is_finite() {
        return Err(serde::de::Error::custom("value must be finite"));
    }
    if !(0.0..=5.0).contains(&v) {
        return Err(serde::de::Error::custom(
            "min_angle_deg outside allowed range",
        ));
    }
    Ok(v)
}
fn default_dpi() -> u32 {
    150
}
fn deserialize_dpi<'de, D: Deserializer<'de>>(d: D) -> Result<u32, D::Error> {
    let v = u32::deserialize(d)?;
    if !(72..=300).contains(&v) {
        return Err(serde::de::Error::custom("dpi outside allowed range"));
    }
    Ok(v)
}

/// Parameters for the deskew operation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeskewParams {
    /// Selected pages.
    #[serde(default = "PageRange::all")]
    pub pages: PageRange,
    /// max angle deg.
    #[serde(
        default = "default_max_angle_deg",
        deserialize_with = "deserialize_max_angle_deg"
    )]
    #[schemars(range(min = 0.5, max = 45.0))]
    pub max_angle_deg: f32,
    /// min angle deg.
    #[serde(
        default = "default_min_angle_deg",
        deserialize_with = "deserialize_min_angle_deg"
    )]
    #[schemars(range(min = 0.0, max = 5.0))]
    pub min_angle_deg: f32,
    /// dpi.
    #[serde(default = "default_dpi", deserialize_with = "deserialize_dpi")]
    #[schemars(range(min = 72, max = 300))]
    pub dpi: u32,
}
