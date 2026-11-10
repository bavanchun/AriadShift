//! Parameters for a vector QR code.

use super::{Anchor, HexColor, overlay_marks_common as common};
use crate::tools::page_range::PageRange;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// QR error correction level.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub enum QrLevel {
    /// Low correction.
    L,
    /// Medium correction.
    #[default]
    M,
    /// Quartile correction.
    Q,
    /// High correction.
    H,
}
/// Background behind the QR modules.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum QrBackground {
    /// White background.
    #[default]
    White,
    /// Transparent background.
    None,
}
/// QR content, appearance, and placement.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QrParams {
    /// UTF-8 content, at most 2953 bytes.
    pub content: String,
    /// Code size in points, including its quiet zone.
    #[serde(default = "common::seventy_two")]
    #[schemars(range(min = 18.0, max = 1000.0))]
    pub size_pt: f64,
    /// Error-correction level.
    #[serde(default)]
    pub ec_level: QrLevel,
    /// Width of the quiet zone in modules.
    #[serde(default = "quiet")]
    #[schemars(range(max = 10))]
    pub quiet_zone_modules: u8,
    /// Module colour in RGB hexadecimal form.
    #[serde(default = "black")]
    pub color: HexColor,
    /// Whether to draw a white background.
    #[serde(default)]
    pub background: QrBackground,
    /// Explicit horizontal box position in points.
    #[serde(default)]
    pub x_pt: Option<f64>,
    /// Explicit vertical box position in points.
    #[serde(default)]
    pub y_pt: Option<f64>,
    /// Anchor used for omitted coordinates.
    #[serde(default = "common::bottom_right")]
    pub anchor: Anchor,
    /// Distance from the page edge in points.
    #[serde(default = "common::qr_margin")]
    pub margin_pt: f64,
    /// Horizontal adjustment in points.
    #[serde(default)]
    pub offset_x_pt: f64,
    /// Vertical adjustment in points.
    #[serde(default)]
    pub offset_y_pt: f64,
    /// Selected 1-based pages.
    #[serde(default = "common::page_one")]
    pub pages: PageRange,
}
fn quiet() -> u8 {
    4
}
fn black() -> HexColor {
    common::color("#000000")
}
impl QrParams {
    /// Validate the content size and geometry.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.content.is_empty() || self.content.len() > 2953 {
            return Err("QR content must contain 1 to 2953 UTF-8 bytes");
        }
        if !self.size_pt.is_finite()
            || !(18.0..=1000.0).contains(&self.size_pt)
            || self.quiet_zone_modules > 10
        {
            return Err("invalid QR dimensions or quiet zone");
        }
        common::validate_placement(
            self.x_pt,
            self.y_pt,
            self.margin_pt,
            self.offset_x_pt,
            self.offset_y_pt,
            0.0,
        )
    }
}
