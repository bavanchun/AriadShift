//! Parameters for removable text and image watermarks.

use super::{Anchor, FontFace, HexColor, Opacity, overlay_marks_common as common};
use crate::tools::page_range::PageRange;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Content of a watermark.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum WatermarkKind {
    /// Draw supplied text.
    #[default]
    Text,
    /// Draw a second image input.
    Image,
}
/// Watermark repetition and orientation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum WatermarkLayout {
    /// One mark at its anchor.
    Single,
    /// Repeat marks across the page.
    Tiled,
    /// One mark following the page diagonal.
    #[default]
    Diagonal,
}
/// A font size in points or the automatic sizing rule.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum WatermarkFontSize {
    /// Size in points.
    Points(f64),
    /// Automatic sizing, represented by the string `auto`.
    Auto(AutoSize),
}
/// Automatic font sizing.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum AutoSize {
    /// Fit to the requested layout.
    #[default]
    Auto,
}
impl Default for WatermarkFontSize {
    fn default() -> Self {
        Self::Auto(AutoSize::Auto)
    }
}
/// Text or image watermark appearance and placement.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WatermarkParams {
    /// Text or image watermark.
    #[serde(default)]
    pub kind: WatermarkKind,
    /// Required nonempty text for a text watermark.
    #[serde(default)]
    pub text: Option<String>,
    /// Bundled font face.
    #[serde(default = "common::bold")]
    pub font: FontFace,
    /// Point size or `auto`.
    #[serde(default)]
    pub font_size_pt: WatermarkFontSize,
    /// Text colour in RGB hexadecimal form.
    #[serde(default = "grey")]
    pub color: HexColor,
    /// Watermark opacity.
    #[serde(default = "watermark_opacity")]
    pub opacity: Opacity,
    /// Counterclockwise rotation, except diagonal layout follows the page diagonal.
    #[serde(default = "diagonal")]
    pub rotation_deg: f64,
    /// Single, tiled or diagonal placement.
    #[serde(default)]
    pub layout: WatermarkLayout,
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
    /// Spacing between tiles in points.
    #[serde(default = "common::seventy_two")]
    pub tile_gap_pt: f64,
    /// Display width in points for image watermarks.
    #[serde(default)]
    pub image_width_pt: Option<f64>,
    /// Draw over or under the page content.
    #[serde(default)]
    pub layer: common::MarkLayer,
    /// Selected 1-based pages.
    #[serde(default = "PageRange::all")]
    pub pages: PageRange,
}
fn grey() -> HexColor {
    common::color("#808080")
}
fn watermark_opacity() -> Opacity {
    common::opacity(0.3)
}
fn diagonal() -> f64 {
    45.0
}
impl WatermarkParams {
    /// Validate source-specific fields and geometry.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.kind == WatermarkKind::Text
            && self
                .text
                .as_ref()
                .is_none_or(|s| s.trim().is_empty() || s.chars().count() > 4096)
        {
            return Err("text watermark requires 1 to 4096 characters");
        }
        if let WatermarkFontSize::Points(size) = self.font_size_pt
            && (!size.is_finite() || size <= 0.0 || size > 10_000.0)
        {
            return Err("invalid watermark font size");
        }
        if !self.tile_gap_pt.is_finite()
            || self.tile_gap_pt < 0.0
            || self.tile_gap_pt > 1_000_000.0
            || self
                .image_width_pt
                .is_some_and(|v| !v.is_finite() || v <= 0.0 || v > 1_000_000.0)
        {
            return Err("invalid watermark dimensions");
        }
        common::validate_placement(
            self.x_pt,
            self.y_pt,
            self.margin_pt,
            self.offset_x_pt,
            self.offset_y_pt,
            self.rotation_deg,
        )
    }
}
