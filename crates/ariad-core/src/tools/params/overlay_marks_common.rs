//! Shared values for page marks.

use super::{Anchor, FontFace, HexColor, Opacity};
use crate::tools::page_range::PageRange;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Place drawing before or after the existing page content.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum MarkLayer {
    /// Draw after the existing page content.
    #[default]
    Over,
    /// Draw before the existing page content.
    Under,
}

pub(crate) fn page_one() -> PageRange {
    PageRange::parse("1").expect("static page range")
}
pub(crate) fn margin() -> f64 {
    36.0
}
pub(crate) fn qr_margin() -> f64 {
    24.0
}
pub(crate) fn twelve() -> f64 {
    12.0
}
pub(crate) fn twenty_four() -> f64 {
    24.0
}
pub(crate) fn seventy_two() -> f64 {
    72.0
}
pub(crate) fn line_height() -> f64 {
    1.2
}
pub(crate) fn bold() -> FontFace {
    FontFace::SerifBold
}
pub(crate) fn top_left() -> Anchor {
    Anchor::TopLeft
}
pub(crate) fn top_right() -> Anchor {
    Anchor::TopRight
}
pub(crate) fn bottom_right() -> Anchor {
    Anchor::BottomRight
}
pub(crate) fn color(value: &str) -> HexColor {
    serde_json::from_value(serde_json::Value::String(value.into())).expect("static colour")
}
pub(crate) fn opacity(value: f64) -> Opacity {
    serde_json::from_value(serde_json::json!(value)).expect("static opacity")
}

pub(crate) fn validate_placement(
    x: Option<f64>,
    y: Option<f64>,
    margin: f64,
    offset_x: f64,
    offset_y: f64,
    rotation: f64,
) -> Result<(), &'static str> {
    if [margin, offset_x, offset_y, rotation]
        .into_iter()
        .chain(x)
        .chain(y)
        .any(|v| !v.is_finite() || v.abs() > 1_000_000.0)
        || margin < 0.0
        || rotation.abs() > 360_000.0
    {
        return Err("invalid mark placement");
    }
    Ok(())
}
