//! Parameter types for the `pdf.posterize` tool.

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};

use crate::tools::{page_range::PageRange, params::common::PaperSize};

/// Explicit tile grid dimensions `{ cols, rows }` for `pdf.posterize`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PosterizeTiles {
    /// Number of horizontal tile columns (`>= 1`).
    #[serde(deserialize_with = "deserialize_tile_dim")]
    #[schemars(range(min = 1, max = 32))]
    pub cols: u8,
    /// Number of vertical tile rows (`>= 1`).
    #[serde(deserialize_with = "deserialize_tile_dim")]
    #[schemars(range(min = 1, max = 32))]
    pub rows: u8,
}

fn deserialize_tile_dim<'de, D>(deserializer: D) -> Result<u8, D::Error>
where
    D: Deserializer<'de>,
{
    let val = u8::deserialize(deserializer)?;
    if !(1..=32).contains(&val) {
        return Err(serde::de::Error::custom("tile dimension must be in 1..=32"));
    }
    Ok(val)
}

const fn default_overlap_mm() -> f32 {
    10.0
}

const fn default_crop_marks() -> bool {
    true
}

/// Parameters for `pdf.posterize`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PosterizeParams {
    /// 1-based pages to posterize (`"all"` by default).
    #[serde(default = "PageRange::all")]
    pub pages: PageRange,
    /// Output tile sheet paper size (`"A4"` by default).
    #[serde(default)]
    pub sheet: PaperSize,
    /// Enlargement scale factor (`> 1.0`, mutually optional with `tiles`).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_optional_scale"
    )]
    #[schemars(extend("exclusiveMinimum" = 1.0, "maximum" = 64.0))]
    pub scale: Option<f32>,
    /// Explicit tile grid `{ cols, rows }` across which each page is enlarged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tiles: Option<PosterizeTiles>,
    /// Tile overlap margin in millimetres (`10.0` by default, `>= 0.0`).
    #[serde(
        default = "default_overlap_mm",
        deserialize_with = "deserialize_overlap_mm"
    )]
    #[schemars(range(min = 0.0, max = 200.0))]
    pub overlap_mm: f32,
    /// Whether to draw corner crop marks on each tile sheet (`true` by default).
    #[serde(default = "default_crop_marks")]
    pub crop_marks: bool,
}

impl Default for PosterizeParams {
    fn default() -> Self {
        Self {
            pages: PageRange::all(),
            sheet: PaperSize::default(),
            scale: None,
            tiles: None,
            overlap_mm: default_overlap_mm(),
            crop_marks: default_crop_marks(),
        }
    }
}

fn deserialize_optional_scale<'de, D>(deserializer: D) -> Result<Option<f32>, D::Error>
where
    D: Deserializer<'de>,
{
    let opt = Option::<f32>::deserialize(deserializer)?;
    if let Some(scale) = opt
        && (!scale.is_finite() || scale <= 1.0 || scale > 64.0)
    {
        return Err(serde::de::Error::custom(
            "scale must be a finite number in (1.0, 64.0]",
        ));
    }
    Ok(opt)
}

fn deserialize_overlap_mm<'de, D>(deserializer: D) -> Result<f32, D::Error>
where
    D: Deserializer<'de>,
{
    let val = f32::deserialize(deserializer)?;
    if !val.is_finite() || !(0.0..=200.0).contains(&val) {
        return Err(serde::de::Error::custom(
            "overlap_mm must be a finite number in [0.0, 200.0]",
        ));
    }
    Ok(val)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn posterize_params_defaults_unknown_field_and_valid_invalid_json() {
        let defaults: PosterizeParams = serde_json::from_value(json!({})).unwrap();
        assert_eq!(defaults, PosterizeParams::default());
        assert_eq!(defaults.pages.as_str(), "all");
        assert!((defaults.overlap_mm - 10.0).abs() < 1e-6);
        assert!(defaults.crop_marks);

        let valid: PosterizeParams = serde_json::from_value(json!({
            "pages": "1",
            "sheet": "A4",
            "tiles": { "cols": 2, "rows": 3 },
            "overlap_mm": 15.0,
            "crop_marks": false
        }))
        .unwrap();
        assert_eq!(valid.tiles, Some(PosterizeTiles { cols: 2, rows: 3 }));
        assert!(!valid.crop_marks);

        assert!(serde_json::from_value::<PosterizeParams>(json!({"scale": 0.5})).is_err());
        assert!(
            serde_json::from_value::<PosterizeParams>(json!({"tiles": {"cols": 0, "rows": 2}}))
                .is_err()
        );
        assert!(serde_json::from_value::<PosterizeParams>(json!({"unknown": 1})).is_err());
    }
}
