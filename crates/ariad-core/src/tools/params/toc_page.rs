//! Parameters for generating a linked table of contents.
use super::{FontFace, PaperSize};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Use the first page's dimensions or a specified paper size.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum TocPageSize {
    /// Inherit the first page size.
    Same(SamePage),
    /// Use a named or custom paper size.
    Paper(PaperSize),
}
/// Inherit the first page's dimensions.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum SamePage {
    /// Use the first page dimensions.
    #[default]
    SameAsFirst,
}
impl Default for TocPageSize {
    fn default() -> Self {
        Self::Same(SamePage::SameAsFirst)
    }
}
/// Layout of a generated contents page.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TocPageParams {
    /// Contents-page title.
    #[serde(default = "title")]
    pub title: String,
    /// Deepest outline level to include.
    #[serde(default = "depth")]
    #[schemars(range(min = 1, max = 6))]
    pub max_depth: u8,
    /// Page size for inserted contents pages.
    #[serde(default)]
    pub page_size: TocPageSize,
    /// Bundled font face.
    #[serde(default)]
    pub font: FontFace,
    /// Font size in points.
    #[serde(default = "size")]
    pub font_size_pt: f64,
    /// Include dot leaders between labels and page numbers.
    #[serde(default = "yes")]
    pub dot_leaders: bool,
    /// Add clickable links to outline targets.
    #[serde(default = "yes")]
    pub links: bool,
}
fn title() -> String {
    "Contents".into()
}
fn depth() -> u8 {
    3
}
fn size() -> f64 {
    11.0
}
fn yes() -> bool {
    true
}
impl TocPageParams {
    /// Validate text and the bounded layout depth.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.title.trim().is_empty()
            || self.title.chars().count() > 4096
            || !(1..=6).contains(&self.max_depth)
            || !self.font_size_pt.is_finite()
            || self.font_size_pt <= 0.0
            || self.font_size_pt > 10_000.0
        {
            return Err("invalid contents title, depth or font size");
        }
        if let TocPageSize::Paper(size) = self.page_size {
            let (w, h) = size.dimensions_pt();
            if !w.is_finite()
                || !h.is_finite()
                || w <= 0.0
                || h <= 0.0
                || w > 1_000_000.0
                || h > 1_000_000.0
            {
                return Err("invalid contents page size");
            }
        }
        Ok(())
    }
}
