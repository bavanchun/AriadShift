//! Parameters for placing a raster image on PDF pages.

use super::{Anchor, Opacity, overlay_marks_common as common};
use crate::tools::page_range::PageRange;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Image placement and size in visual page coordinates.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AddImageParams {
    /// Selected 1-based pages.
    #[serde(default = "common::page_one")]
    pub pages: PageRange,
    /// Explicit horizontal box position in points.
    #[serde(default)]
    pub x_pt: Option<f64>,
    /// Explicit vertical box position in points.
    #[serde(default)]
    pub y_pt: Option<f64>,
    /// Anchor used for omitted coordinates.
    #[serde(default)]
    pub anchor: Anchor,
    /// Distance from the page edge in points.
    #[serde(default = "common::margin")]
    pub margin_pt: f64,
    /// Horizontal adjustment in points.
    #[serde(default)]
    pub offset_x_pt: f64,
    /// Vertical adjustment in points.
    #[serde(default)]
    pub offset_y_pt: f64,
    /// Display width in points; omitted dimensions preserve image aspect ratio.
    #[serde(default)]
    pub width_pt: Option<f64>,
    /// Display height in points; omitted dimensions preserve image aspect ratio.
    #[serde(default)]
    pub height_pt: Option<f64>,
    /// Image opacity.
    #[serde(default = "opaque")]
    pub opacity: Opacity,
    /// Counterclockwise rotation in degrees.
    #[serde(default)]
    pub rotation_deg: f64,
    /// Draw over or under the page content.
    #[serde(default)]
    pub layer: common::MarkLayer,
}
fn opaque() -> Opacity {
    common::opacity(1.0)
}
impl AddImageParams {
    /// Validate placement and optional dimensions.
    pub fn validate(&self) -> Result<(), &'static str> {
        common::validate_placement(
            self.x_pt,
            self.y_pt,
            self.margin_pt,
            self.offset_x_pt,
            self.offset_y_pt,
            self.rotation_deg,
        )?;
        if [self.width_pt, self.height_pt]
            .into_iter()
            .flatten()
            .any(|v| !v.is_finite() || v <= 0.0 || v > 1_000_000.0)
        {
            return Err("invalid image dimensions");
        }
        Ok(())
    }
}
