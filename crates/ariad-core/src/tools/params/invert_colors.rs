//! Parameters for page colour inversion.
use crate::tools::page_range::PageRange;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
/// Pages receiving a Difference-blend inversion layer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InvertColorsParams {
    /// Selected 1-based pages.
    #[serde(default = "PageRange::all")]
    pub pages: PageRange,
}
