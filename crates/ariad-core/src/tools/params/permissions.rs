//! Shared PDF permissions and permission-change parameters.
use super::ParamsError;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
fn enabled() -> bool {
    true
}
/// Permissions granted to a PDF user.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PdfPermissions {
    /// Permit print.
    #[serde(default = "enabled")]
    pub print: bool,
    /// Permit modify.
    #[serde(default = "enabled")]
    pub modify: bool,
    /// Permit copy.
    #[serde(default = "enabled")]
    pub copy: bool,
    /// Permit annotate.
    #[serde(default = "enabled")]
    pub annotate: bool,
    /// Permit fill forms.
    #[serde(default = "enabled")]
    pub fill_forms: bool,
    /// Permit extract accessibility.
    #[serde(default = "enabled")]
    pub extract_accessibility: bool,
    /// Permit assemble.
    #[serde(default = "enabled")]
    pub assemble: bool,
    /// Permit print high.
    #[serde(default = "enabled")]
    pub print_high: bool,
}
impl Default for PdfPermissions {
    fn default() -> Self {
        Self {
            print: true,
            modify: true,
            copy: true,
            annotate: true,
            fill_forms: true,
            extract_accessibility: true,
            assemble: true,
            print_high: true,
        }
    }
}
/// Parameters for `pdf.permissions`.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PermissionsParams {
    /// Required non-empty owner password.
    #[schemars(length(min = 1))]
    pub owner_password: String,
    /// Optional user password for the resulting document.
    #[serde(default)]
    pub user_password: Option<String>,
    /// Permissions granted to the user.
    #[serde(default)]
    pub permissions: PdfPermissions,
}
impl PermissionsParams {
    /// Rejects an empty owner password.
    pub fn validate(&self) -> Result<(), ParamsError> {
        if self.owner_password.is_empty() {
            Err(ParamsError::new("owner_password", "must not be empty"))
        } else {
            Ok(())
        }
    }
}

impl std::fmt::Debug for PermissionsParams {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PermissionsParams")
            .field("owner_password", &"[REDACTED]")
            .field("user_password", &"[REDACTED]")
            .field("permissions", &self.permissions)
            .finish()
    }
}
