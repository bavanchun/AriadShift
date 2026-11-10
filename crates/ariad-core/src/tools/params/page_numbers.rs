//! Parameters for page numbering.
use super::{Anchor, FontFace, HexColor, overlay_marks_common as common};
use crate::tools::page_range::PageRange;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Page-number template, sequence and visual placement.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PageNumbersParams {
    /// Template with `{n}`, `{total}`, and `{label}` tokens.
    #[serde(default = "format")]
    pub format: String,
    /// Selected 1-based pages.
    #[serde(default = "PageRange::all")]
    pub pages: PageRange,
    /// First displayed number.
    #[serde(default = "one")]
    pub start: u32,
    /// Number of selected pages to leave unnumbered.
    #[serde(default)]
    pub skip_first: u32,
    /// Visual position.
    #[serde(default = "anchor")]
    pub anchor: Anchor,
    /// Page-edge margin in points.
    #[serde(default = "common::twenty_four")]
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
fn format() -> String {
    "{n}".into()
}
fn one() -> u32 {
    1
}
fn anchor() -> Anchor {
    Anchor::BottomCenter
}
fn size() -> f64 {
    10.0
}
fn black() -> HexColor {
    common::color("#000000")
}
impl PageNumbersParams {
    /// Validate placement and text size.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.format.is_empty()
            || self.format.chars().count() > 4096
            || !self.font_size_pt.is_finite()
            || self.font_size_pt <= 0.0
            || self.font_size_pt > 10_000.0
        {
            return Err("invalid page-number template or font size");
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
