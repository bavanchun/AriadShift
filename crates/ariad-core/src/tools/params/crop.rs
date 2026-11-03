//! Parameter types for the `pdf.crop` tool.

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};

use crate::tools::page_range::PageRange;

/// Explicit crop rectangle coordinates `{ left, bottom, right, top }` in PDF points.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CropRect {
    /// Left coordinate (`llx`) in PDF points.
    #[serde(deserialize_with = "deserialize_finite_pt")]
    pub left: f64,
    /// Bottom coordinate (`lly`) in PDF points.
    #[serde(deserialize_with = "deserialize_finite_pt")]
    pub bottom: f64,
    /// Right coordinate (`urx`) in PDF points.
    #[serde(deserialize_with = "deserialize_finite_pt")]
    pub right: f64,
    /// Top coordinate (`ury`) in PDF points.
    #[serde(deserialize_with = "deserialize_finite_pt")]
    pub top: f64,
}

fn deserialize_finite_pt<'de, D>(deserializer: D) -> Result<f64, D::Error>
where
    D: Deserializer<'de>,
{
    let val = f64::deserialize(deserializer)?;
    if !val.is_finite() || val.abs() > 1_000_000.0 {
        return Err(serde::de::Error::custom(
            "crop coordinate must be a finite number in [-1000000, 1000000] points",
        ));
    }
    Ok(val)
}

/// Deferred whitespace auto-crop preset (`"trim-whitespace-later"`).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum CropAutoMode {
    /// Placeholder marker for rendering-based whitespace auto-crop.
    #[serde(alias = "trim_whitespace_later")]
    TrimWhitespaceLater,
}

/// Crop box specification for `pdf.crop`: explicit `{ left, bottom, right, top }` or
/// `"trim-whitespace-later"`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum CropBoxSpec {
    /// Explicit rectangle `{ left, bottom, right, top }` in PDF points.
    Box(CropRect),
    /// Rendering-dependent auto-crop preset (`"trim-whitespace-later"`).
    Auto(CropAutoMode),
}

/// Target PDF page box updated by `pdf.crop`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum CropTarget {
    /// Update `/CropBox` (`"crop"`, default).
    #[default]
    Crop,
    /// Update `/MediaBox` (and clamp `/CropBox` to match).
    Media,
}

/// Parameters for `pdf.crop`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CropParams {
    /// 1-based pages to crop (`"all"` by default).
    #[serde(default = "PageRange::all")]
    pub pages: PageRange,
    /// Crop box coordinates in PDF points or `"trim-whitespace-later"`.
    pub r#box: CropBoxSpec,
    /// Target page box to set (`"crop"` by default, or `"media"`).
    #[serde(default)]
    pub target: CropTarget,
}

impl CropParams {
    /// Borrows the crop box specification.
    #[must_use]
    pub const fn crop_box(&self) -> &CropBoxSpec {
        &self.r#box
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn crop_params_defaults_unknown_field_and_valid_invalid_json() {
        let valid: CropParams = serde_json::from_value(json!({
            "box": { "left": 36.0, "bottom": 36.0, "right": 576.0, "top": 756.0 }
        }))
        .unwrap();
        assert_eq!(valid.pages.as_str(), "all");
        assert_eq!(valid.target, CropTarget::Crop);
        assert_eq!(
            valid.r#box,
            CropBoxSpec::Box(CropRect {
                left: 36.0,
                bottom: 36.0,
                right: 576.0,
                top: 756.0,
            })
        );

        let auto_mode: CropParams = serde_json::from_value(json!({
            "pages": "1-2",
            "box": "trim-whitespace-later",
            "target": "media"
        }))
        .unwrap();
        assert_eq!(
            auto_mode.r#box,
            CropBoxSpec::Auto(CropAutoMode::TrimWhitespaceLater)
        );
        assert_eq!(auto_mode.target, CropTarget::Media);

        assert!(serde_json::from_value::<CropParams>(json!({})).is_err());
        assert!(
            serde_json::from_value::<CropParams>(json!({
                "box": "trim-whitespace-later",
                "unknown": true
            }))
            .is_err()
        );
        assert!(serde_json::from_value::<CropParams>(json!({"box": "invalid"})).is_err());
    }
}
