//! Parameters for `pdf.repair`.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
/// Parameters for `pdf.repair`.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RepairParams {}
