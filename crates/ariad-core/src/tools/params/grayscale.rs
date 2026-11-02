//! Parameters for `pdf.grayscale`.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
/// Parameters for `pdf.grayscale`.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GrayscaleParams {}
