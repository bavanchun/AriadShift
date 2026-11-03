//! Parameters for page headers and footers.
use super::{DateFormat, FontFace, HexColor, overlay_marks_common as common};
use crate::tools::page_range::PageRange;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Three text slots along one page edge.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HeaderFooterSlots {
    /// Left-aligned template.
    #[serde(default)]
    pub left: Option<String>,
    /// Centred template.
    #[serde(default)]
    pub center: Option<String>,
    /// Right-aligned template.
    #[serde(default)]
    pub right: Option<String>,
}
impl HeaderFooterSlots {
    fn values(&self) -> [Option<&str>; 3] {
        [
            self.left.as_deref(),
            self.center.as_deref(),
            self.right.as_deref(),
        ]
    }
}
/// Header and footer templates and appearance.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HeaderFooterParams {
    /// Header slots.
    #[serde(default)]
    pub header: HeaderFooterSlots,
    /// Footer slots.
    #[serde(default)]
    pub footer: HeaderFooterSlots,
    /// Selected 1-based pages.
    #[serde(default = "PageRange::all")]
    pub pages: PageRange,
    /// Number of selected pages to leave unchanged.
    #[serde(default)]
    pub skip_first: u32,
    /// Page-edge margin in points.
    #[serde(default = "common::twenty_four")]
    pub margin_pt: f64,
    /// Bundled font face.
    #[serde(default)]
    pub font: FontFace,
    /// Font size in points.
    #[serde(default = "size")]
    pub font_size_pt: f64,
    /// Text colour.
    #[serde(default = "black")]
    pub color: HexColor,
    /// Optional explicit ISO date.
    #[serde(default)]
    pub date: Option<String>,
    /// Date display format.
    #[serde(default)]
    pub date_format: DateFormat,
}
fn size() -> f64 {
    9.0
}
fn black() -> HexColor {
    common::color("#000000")
}
impl HeaderFooterParams {
    /// Require at least one bounded nonempty template and valid geometry.
    pub fn validate(&self) -> Result<(), &'static str> {
        let slots = self
            .header
            .values()
            .into_iter()
            .chain(self.footer.values())
            .flatten()
            .collect::<Vec<_>>();
        if slots.iter().all(|value| value.trim().is_empty())
            || slots.iter().any(|value| value.chars().count() > 4096)
            || !self.font_size_pt.is_finite()
            || self.font_size_pt <= 0.0
            || self.font_size_pt > 10_000.0
        {
            return Err("at least one valid header or footer template is required");
        }
        common::validate_placement(None, None, self.margin_pt, 0.0, 0.0, 0.0)
    }
}
