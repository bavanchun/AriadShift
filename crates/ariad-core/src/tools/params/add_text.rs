//! Parameters for adding text to existing pages.

use super::{Anchor, FontFace, HexColor, Opacity, overlay_marks_common as common};
use crate::tools::page_range::PageRange;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Horizontal alignment inside the text layout box.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum TextAlignment {
    /// Align the text to the left.
    #[default]
    Left,
    /// Centre the text.
    Center,
    /// Align the text to the right.
    Right,
}

/// Text and its placement in visual page coordinates.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AddTextParams {
    /// Text to draw (1 to 4096 Unicode characters).
    #[schemars(length(min = 1, max = 4096))]
    pub text: String,
    /// Selected 1-based pages.
    #[serde(default = "common::page_one")]
    pub pages: PageRange,
    /// Optional explicit horizontal box position in points.
    #[serde(default)]
    pub x_pt: Option<f64>,
    /// Optional explicit vertical box position in points.
    #[serde(default)]
    pub y_pt: Option<f64>,
    /// Default placement when a coordinate is omitted.
    #[serde(default = "common::top_left")]
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
    /// Bundled font face.
    #[serde(default)]
    pub font: FontFace,
    /// Font size in points.
    #[serde(default = "common::twelve")]
    pub font_size_pt: f64,
    /// Text colour in RGB hexadecimal form.
    #[serde(default = "black")]
    pub color: HexColor,
    /// Text opacity.
    #[serde(default = "opaque")]
    pub opacity: Opacity,
    /// Counterclockwise rotation in degrees.
    #[serde(default)]
    pub rotation_deg: f64,
    /// Maximum text layout width in points.
    #[serde(default)]
    pub max_width_pt: Option<f64>,
    /// Horizontal alignment within the layout.
    #[serde(default)]
    pub align: TextAlignment,
    /// Baseline spacing as a multiple of font size.
    #[serde(default = "common::line_height")]
    pub line_height: f64,
    /// Draw over or under the page content.
    #[serde(default)]
    pub layer: common::MarkLayer,
}
fn black() -> HexColor {
    common::color("#000000")
}
fn opaque() -> Opacity {
    common::opacity(1.0)
}
impl AddTextParams {
    /// Validate values supplied by either deserialization or direct construction.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.text.is_empty() || self.text.chars().count() > 4096 {
            return Err("text must contain 1 to 4096 characters");
        }
        if !self.font_size_pt.is_finite()
            || !(0.0..=10_000.0).contains(&self.font_size_pt)
            || self.font_size_pt == 0.0
            || !self.line_height.is_finite()
            || !(0.1..=10.0).contains(&self.line_height)
        {
            return Err("invalid font size or line height");
        }
        common::validate_placement(
            self.x_pt,
            self.y_pt,
            self.margin_pt,
            self.offset_x_pt,
            self.offset_y_pt,
            self.rotation_deg,
        )?;
        if self
            .max_width_pt
            .is_some_and(|v| !v.is_finite() || v <= 0.0 || v > 1_000_000.0)
        {
            return Err("invalid maximum text width");
        }
        Ok(())
    }
}
