//! Parameters for `pdf.unlock_forms`.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
/// Parameters for `pdf.unlock_forms`.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UnlockFormsParams {}
