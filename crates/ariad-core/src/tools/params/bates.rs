//! Parameters for single-document Bates numbering.
use super::{Anchor, FontFace, HexColor, overlay_marks_common as common};
use crate::tools::page_range::PageRange;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Bates sequence and visual placement.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BatesParams {
    /// Text before each number.
    #[serde(default)]
    pub prefix: String,
    /// Text after each number.
    #[serde(default)]
    pub suffix: String,
    /// Number of decimal digits, between 1 and 12.
    #[serde(default = "digits")]
    #[schemars(range(min = 1, max = 12))]
    pub digits: u8,
    /// First value in the sequence.
    #[serde(default = "one")]
    pub start: u64,
    /// Selected 1-based pages.
    #[serde(default = "PageRange::all")]
    pub pages: PageRange,
    /// Visual position.
    #[serde(default = "common::bottom_right")]
    pub anchor: Anchor,
    /// Page-edge margin in points.
    #[serde(default = "margin")]
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
    #[serde(default = "size")]
    pub font_size_pt: f64,
    /// Text colour.
    #[serde(default = "black")]
    pub color: HexColor,
}
fn digits() -> u8 {
    6
}
fn one() -> u64 {
    1
}
fn margin() -> f64 {
    18.0
}
fn size() -> f64 {
    9.0
}
fn black() -> HexColor {
    common::color("#000000")
}
impl BatesParams {
    /// Validate sequence width and placement.
    pub fn validate(&self) -> Result<(), &'static str> {
        if !(1..=12).contains(&self.digits)
            || self.prefix.chars().count() + self.suffix.chars().count() > 4084
            || !self.font_size_pt.is_finite()
            || self.font_size_pt <= 0.0
            || self.font_size_pt > 10_000.0
        {
            return Err("invalid Bates width, text or font size");
        }
        common::validate_placement(
            None,
            None,
            self.margin_pt,
            self.offset_x_pt,
            self.offset_y_pt,
            0.0,
        )
    }
}
