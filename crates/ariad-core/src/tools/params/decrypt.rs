//! Parameters for PDF decryption.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
/// Parameters for `pdf.decrypt`.
#[derive(Clone, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DecryptParams {
    /// Password; omission means an empty user password.
    #[serde(default)]
    pub password: Option<String>,
}

impl std::fmt::Debug for DecryptParams {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DecryptParams")
            .field("password", &"[REDACTED]")
            .finish()
    }
}
