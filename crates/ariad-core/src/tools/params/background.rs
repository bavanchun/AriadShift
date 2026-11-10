//! Parameters for a page background.
use super::{HexColor, Opacity, overlay_marks_common as common};
use crate::tools::page_range::PageRange;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
/// Background colour, opacity and selected pages.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BackgroundParams {
    /// RGB colour.
    #[serde(default = "white")]
    pub color: HexColor,
    /// Fill opacity.
    #[serde(default = "opaque")]
    pub opacity: Opacity,
    /// Selected 1-based pages.
    #[serde(default = "PageRange::all")]
    pub pages: PageRange,
}
fn white() -> HexColor {
    common::color("#FFFFFF")
}
fn opaque() -> Opacity {
    common::opacity(1.0)
}
