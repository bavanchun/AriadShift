//! Parameters for `scan_split`.

use crate::tools::PageRange;
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};

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
fn default_min_area_ratio() -> f32 {
    0.02
}
fn deserialize_min_area_ratio<'de, D: Deserializer<'de>>(d: D) -> Result<f32, D::Error> {
    let v = f32::deserialize(d)?;
    if !v.is_finite() {
        return Err(serde::de::Error::custom("value must be finite"));
    }
    if !(0.001..=0.9).contains(&v) {
        return Err(serde::de::Error::custom(
            "min_area_ratio outside allowed range",
        ));
    }
    Ok(v)
}
fn default_padding_pt() -> f32 {
    4.0
}
fn deserialize_padding_pt<'de, D: Deserializer<'de>>(d: D) -> Result<f32, D::Error> {
    let v = f32::deserialize(d)?;
    if !v.is_finite() {
        return Err(serde::de::Error::custom("value must be finite"));
    }
    if !(0.0..=72.0).contains(&v) {
        return Err(serde::de::Error::custom("padding_pt outside allowed range"));
    }
    Ok(v)
}
fn default_straighten() -> bool {
    true
}

/// Parameters for the scan_split operation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScanSplitParams {
    /// Selected pages.
    #[serde(default = "PageRange::all")]
    pub pages: PageRange,
    /// dpi.
    #[serde(default = "default_dpi", deserialize_with = "deserialize_dpi")]
    #[schemars(range(min = 72, max = 300))]
    pub dpi: u32,
    /// min area ratio.
    #[serde(
        default = "default_min_area_ratio",
        deserialize_with = "deserialize_min_area_ratio"
    )]
    #[schemars(range(min = 0.001, max = 0.9))]
    pub min_area_ratio: f32,
    /// padding pt.
    #[serde(
        default = "default_padding_pt",
        deserialize_with = "deserialize_padding_pt"
    )]
    #[schemars(range(min = 0.0, max = 72.0))]
    pub padding_pt: f32,
    /// straighten.
    #[serde(default = "default_straighten")]
    pub straighten: bool,
}
