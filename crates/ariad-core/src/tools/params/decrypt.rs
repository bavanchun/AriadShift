//! Parameters for PDF decryption.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
/// Parameters for `pdf.decrypt`.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DecryptParams {
    /// Password; omission means an empty user password.
    #[serde(default)]
    pub password: Option<String>,
}
