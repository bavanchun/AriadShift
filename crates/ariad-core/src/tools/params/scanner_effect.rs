//! Parameters for `scanner_effect`.

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
fn default_grayscale() -> bool {
    true
}
fn default_noise() -> f32 {
    0.15
}
fn deserialize_noise<'de, D: Deserializer<'de>>(d: D) -> Result<f32, D::Error> {
    let v = f32::deserialize(d)?;
    if !v.is_finite() {
        return Err(serde::de::Error::custom("value must be finite"));
    }
    if !(0.0..=1.0).contains(&v) {
        return Err(serde::de::Error::custom("noise outside allowed range"));
    }
    Ok(v)
}
fn default_blur() -> f32 {
    0.5
}
fn deserialize_blur<'de, D: Deserializer<'de>>(d: D) -> Result<f32, D::Error> {
    let v = f32::deserialize(d)?;
    if !v.is_finite() {
        return Err(serde::de::Error::custom("value must be finite"));
    }
    if !(0.0..=3.0).contains(&v) {
        return Err(serde::de::Error::custom("blur outside allowed range"));
    }
    Ok(v)
}
fn default_skew_deg() -> f32 {
    0.5
}
fn deserialize_skew_deg<'de, D: Deserializer<'de>>(d: D) -> Result<f32, D::Error> {
    let v = f32::deserialize(d)?;
    if !v.is_finite() {
        return Err(serde::de::Error::custom("value must be finite"));
    }
    if !(0.0..=5.0).contains(&v) {
        return Err(serde::de::Error::custom("skew_deg outside allowed range"));
    }
    Ok(v)
}
fn default_brightness() -> f32 {
    0.0
}
fn deserialize_brightness<'de, D: Deserializer<'de>>(d: D) -> Result<f32, D::Error> {
    let v = f32::deserialize(d)?;
    if !v.is_finite() {
        return Err(serde::de::Error::custom("value must be finite"));
    }
    if !(-1.0..=1.0).contains(&v) {
        return Err(serde::de::Error::custom("brightness outside allowed range"));
    }
    Ok(v)
}
fn default_contrast() -> f32 {
    0.0
}
fn deserialize_contrast<'de, D: Deserializer<'de>>(d: D) -> Result<f32, D::Error> {
    let v = f32::deserialize(d)?;
    if !v.is_finite() {
        return Err(serde::de::Error::custom("value must be finite"));
    }
    if !(-1.0..=1.0).contains(&v) {
        return Err(serde::de::Error::custom("contrast outside allowed range"));
    }
    Ok(v)
}
fn default_quality() -> u8 {
    75
}
fn deserialize_quality<'de, D: Deserializer<'de>>(d: D) -> Result<u8, D::Error> {
    let v = u8::deserialize(d)?;
    if !(10..=100).contains(&v) {
        return Err(serde::de::Error::custom("quality outside allowed range"));
    }
    Ok(v)
}
fn default_seed() -> u64 {
    0
}

/// Parameters for the scanner_effect operation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScannerEffectParams {
    /// Selected pages.
    #[serde(default = "PageRange::all")]
    pub pages: PageRange,
    /// dpi.
    #[serde(default = "default_dpi", deserialize_with = "deserialize_dpi")]
    #[schemars(range(min = 72, max = 300))]
    pub dpi: u32,
    /// grayscale.
    #[serde(default = "default_grayscale")]
    pub grayscale: bool,
    /// noise.
    #[serde(default = "default_noise", deserialize_with = "deserialize_noise")]
    #[schemars(range(min = 0.0, max = 1.0))]
    pub noise: f32,
    /// blur.
    #[serde(default = "default_blur", deserialize_with = "deserialize_blur")]
    #[schemars(range(min = 0.0, max = 3.0))]
    pub blur: f32,
    /// skew deg.
    #[serde(
        default = "default_skew_deg",
        deserialize_with = "deserialize_skew_deg"
    )]
    #[schemars(range(min = 0.0, max = 5.0))]
    pub skew_deg: f32,
    /// brightness.
    #[serde(
        default = "default_brightness",
        deserialize_with = "deserialize_brightness"
    )]
    #[schemars(range(min = -1.0, max = 1.0))]
    pub brightness: f32,
    /// contrast.
    #[serde(
        default = "default_contrast",
        deserialize_with = "deserialize_contrast"
    )]
    #[schemars(range(min = -1.0, max = 1.0))]
    pub contrast: f32,
    /// quality.
    #[serde(default = "default_quality", deserialize_with = "deserialize_quality")]
    #[schemars(range(min = 10, max = 100))]
    pub quality: u8,
    /// seed.
    #[serde(default = "default_seed")]
    pub seed: u64,
}
