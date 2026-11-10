//! Parameters for explicit watermark removal.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The exact candidate identifiers selected by the user.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WatermarkRemoveParams {
    /// Nonempty set of detector-issued identifiers; wildcards are never accepted.
    #[schemars(length(min = 1))]
    pub candidates: Vec<String>,
}

impl WatermarkRemoveParams {
    /// Refuse wildcards, empty identifiers, and duplicate selections.
    pub fn validate(&self) -> Result<(), &'static str> {
        let mut seen = std::collections::HashSet::new();
        if self.candidates.is_empty()
            || self.candidates.iter().any(|id| {
                id.trim().is_empty() || matches!(id.as_str(), "all" | "*") || !seen.insert(id)
            })
        {
            return Err("candidates must be unique explicit identifiers");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn selection_must_be_explicit_and_unique() {
        assert!(serde_json::from_value::<WatermarkRemoveParams>(json!({})).is_err());
        for candidates in [vec![], vec!["all"], vec!["*"], vec!["x", "x"]] {
            let params: WatermarkRemoveParams =
                serde_json::from_value(json!({"candidates":candidates})).unwrap();
            assert!(params.validate().is_err());
        }
        let params: WatermarkRemoveParams =
            serde_json::from_value(json!({"candidates":["x"]})).unwrap();
        assert!(params.validate().is_ok());
    }
}

/// Inpainting method used by the isolated raster watermark engine.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum InpaintMethod {
    /// Fast marching inpainting.
    #[default]
    Telea,
    /// Navier-Stokes inpainting.
    Ns,
}

/// Automatic mask selection.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AutoMask {
    /// Detect a candidate raster watermark mask.
    #[default]
    Auto,
}

/// A rectangle in raster coordinates, optionally restricted to one PDF page.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RasterMaskRect {
    /// 1-based page, or every selected page when omitted.
    #[serde(default)]
    #[schemars(range(min = 1))]
    pub page: Option<u32>,
    /// Left coordinate.
    #[schemars(range(min = 0))]
    pub x: f64,
    /// Top coordinate.
    #[schemars(range(min = 0))]
    pub y: f64,
    /// Positive width.
    #[schemars(extend("exclusiveMinimum" = 0))]
    pub width: f64,
    /// Positive height.
    #[schemars(extend("exclusiveMinimum" = 0))]
    pub height: f64,
}

/// Automatic detection or an explicit nonempty rectangle list.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum RasterWatermarkMask {
    /// The string `"auto"`.
    Auto(AutoMask),
    /// Explicit raster coordinates.
    Rects(#[schemars(length(min = 1))] Vec<RasterMaskRect>),
}
impl Default for RasterWatermarkMask {
    fn default() -> Self {
        Self::Auto(AutoMask::Auto)
    }
}
fn default_true() -> bool {
    true
}

/// Parameters for `pdf.watermark_remove_raster` (preview is enabled by default).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WatermarkRemoveRasterParams {
    /// Selected PDF pages, or all pages when omitted.
    #[serde(default)]
    pub pages: Option<crate::tools::PageRange>,
    /// Mask (automatic detection when omitted).
    #[serde(default)]
    pub mask: Option<RasterWatermarkMask>,
    /// Inpainting method (Telea by default).
    #[serde(default)]
    pub method: InpaintMethod,
    /// Inpainting radius (3 pixels when omitted).
    #[serde(default)]
    #[schemars(range(min = 1.0, max = 25.0))]
    pub radius: Option<f64>,
    /// Return a preview for review before applying the result.
    #[serde(default = "default_true")]
    pub preview: bool,
}
impl Default for WatermarkRemoveRasterParams {
    fn default() -> Self {
        Self {
            pages: None,
            mask: None,
            method: InpaintMethod::Telea,
            radius: None,
            preview: true,
        }
    }
}
impl WatermarkRemoveRasterParams {
    /// Checks finite coordinates, positive dimensions and bounded radius.
    pub fn validate(&self) -> Result<(), super::ParamsError> {
        if let Some(radius) = self.radius {
            if !radius.is_finite() {
                return Err(super::ParamsError::new("radius", "must be finite"));
            }
            super::check_range("radius", radius, 1.0, 25.0)?;
        }
        if let Some(RasterWatermarkMask::Rects(rects)) = &self.mask
            && (rects.is_empty()
                || rects.iter().any(|r| {
                    r.page == Some(0)
                        || ![r.x, r.y, r.width, r.height].iter().all(|v| v.is_finite())
                        || r.x < 0.0
                        || r.y < 0.0
                        || r.width <= 0.0
                        || r.height <= 0.0
                }))
        {
            return Err(super::ParamsError::new(
                "mask",
                "requires nonempty valid rectangles",
            ));
        }
        Ok(())
    }
}
