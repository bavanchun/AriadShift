//! Parameters for `auto_rotate`.

use crate::tools::PageRange;
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};

fn default_min_confidence() -> f32 {
    2.0
}
fn deserialize_min_confidence<'de, D: Deserializer<'de>>(d: D) -> Result<f32, D::Error> {
    let v = f32::deserialize(d)?;
    if !v.is_finite() {
        return Err(serde::de::Error::custom("value must be finite"));
    }
    if !(0.0..=100.0).contains(&v) {
        return Err(serde::de::Error::custom(
            "min_confidence outside allowed range",
        ));
    }
    Ok(v)
}
fn default_dpi() -> u32 {
    300
}
fn deserialize_dpi<'de, D: Deserializer<'de>>(d: D) -> Result<u32, D::Error> {
    let v = u32::deserialize(d)?;
    if !(150..=600).contains(&v) {
        return Err(serde::de::Error::custom("dpi outside allowed range"));
    }
    Ok(v)
}

/// Parameters for the auto_rotate operation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AutoRotateParams {
    /// Selected pages.
    #[serde(default = "PageRange::all")]
    pub pages: PageRange,
    /// min confidence.
    #[serde(
        default = "default_min_confidence",
        deserialize_with = "deserialize_min_confidence"
    )]
    #[schemars(range(min = 0.0, max = 100.0))]
    pub min_confidence: f32,
    /// dpi.
    #[serde(default = "default_dpi", deserialize_with = "deserialize_dpi")]
    #[schemars(range(min = 150, max = 600))]
    pub dpi: u32,
}
