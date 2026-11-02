//! Parameters for `pdf.linearize`.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
/// Parameters for `pdf.linearize`.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LinearizeParams {}
