//! Parameters for removable text stamps.

use super::{Anchor, DateFormat, FontFace, HexColor, Opacity, overlay_marks_common as common};
use crate::tools::page_range::PageRange;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Built-in English stamp label or custom text.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum StampPreset {
    /// Approved.
    #[default]
    Approved,
    /// Draft.
    Draft,
    /// Confidential.
    Confidential,
    /// Caller-supplied text.
    Custom,
}
/// Stamp text, colour, date and position.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StampParams {
    /// Preset or custom label.
    #[serde(default)]
    pub preset: StampPreset,
    /// Text template required for a custom stamp.
    #[serde(default)]
    pub text: Option<String>,
    /// Append a date to the label.
    #[serde(default)]
    pub show_date: bool,
    /// Explicit date source, taking precedence over the environment and caller clock.
    #[serde(default)]
    pub date: Option<String>,
    /// Date formatting convention.
    #[serde(default)]
    pub date_format: DateFormat,
    /// Override the preset colour.
    #[serde(default)]
    pub color: Option<HexColor>,
    /// Draw a border around the label.
    #[serde(default = "yes")]
    pub border: bool,
    /// Bundled font face.
    #[serde(default = "common::bold")]
    pub font: FontFace,
    /// Font size in points.
    #[serde(default = "common::twenty_four")]
    pub font_size_pt: f64,
    /// Explicit horizontal box position in points.
    #[serde(default)]
    pub x_pt: Option<f64>,
    /// Explicit vertical box position in points.
    #[serde(default)]
    pub y_pt: Option<f64>,
    /// Anchor used for omitted coordinates.
    #[serde(default = "common::top_right")]
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
    /// Counterclockwise rotation in degrees.
    #[serde(default)]
    pub rotation_deg: f64,
    /// Stamp opacity.
    #[serde(default = "stamp_opacity")]
    pub opacity: Opacity,
    /// Selected 1-based pages.
    #[serde(default = "common::page_one")]
    pub pages: PageRange,
}
fn yes() -> bool {
    true
}
fn stamp_opacity() -> Opacity {
    common::opacity(0.9)
}
impl StampParams {
    /// Validate custom text and geometry.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.preset == StampPreset::Custom
            && self
                .text
                .as_ref()
                .is_none_or(|s| s.trim().is_empty() || s.chars().count() > 4096)
        {
            return Err("custom stamp requires 1 to 4096 characters");
        }
        if !self.font_size_pt.is_finite()
            || self.font_size_pt <= 0.0
            || self.font_size_pt > 10_000.0
        {
            return Err("invalid stamp font size");
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
