//! Parameters for PDF encryption.
use super::ParamsError;
use super::permissions::PdfPermissions;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
/// Supported PDF encryption algorithms.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum EncryptionAlgorithm {
    /// AES with a 256-bit key.
    #[default]
    #[serde(rename = "aes-256")]
    Aes256,
    /// AES with a 128-bit key.
    #[serde(rename = "aes-128")]
    Aes128,
}
/// Parameters for `pdf.encrypt`.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EncryptParams {
    /// Optional password needed to open the document.
    #[serde(default)]
    pub user_password: Option<String>,
    /// Required non-empty owner password.
    #[schemars(length(min = 1))]
    pub owner_password: String,
    /// Encryption algorithm (AES-256 by default).
    #[serde(default)]
    pub algorithm: EncryptionAlgorithm,
    /// Permissions granted to the user.
    #[serde(default)]
    pub permissions: PdfPermissions,
}
impl EncryptParams {
    /// Rejects an empty owner password.
    pub fn validate(&self) -> Result<(), ParamsError> {
        if self.owner_password.is_empty() {
            Err(ParamsError::new("owner_password", "must not be empty"))
        } else {
            Ok(())
        }
    }
}

impl std::fmt::Debug for EncryptParams {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EncryptParams")
            .field("user_password", &"[REDACTED]")
            .field("owner_password", &"[REDACTED]")
            .field("algorithm", &self.algorithm)
            .field("permissions", &self.permissions)
            .finish()
    }
}
