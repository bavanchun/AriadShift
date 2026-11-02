//! Parameters for PDF sanitization.
use super::ParamsError;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
fn enabled() -> bool {
    true
}
/// Form handling when sanitizing a document.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum SanitizeForms {
    /// Preserve forms.
    #[default]
    Keep,
    /// Flatten forms into page content.
    Flatten,
    /// Remove forms.
    Remove,
}
/// Parameters for `pdf.sanitize`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SanitizeParams {
    /// Remove javascript.
    #[serde(default = "enabled")]
    pub javascript: bool,
    /// Remove embedded files.
    #[serde(default = "enabled")]
    pub embedded_files: bool,
    /// Remove thumbnails.
    #[serde(default = "enabled")]
    pub thumbnails: bool,
    /// Remove links.
    #[serde(default)]
    pub links: bool,
    /// Remove metadata.
    #[serde(default)]
    pub metadata: bool,
    /// Remove xmp.
    #[serde(default)]
    pub xmp: bool,
    /// Annotation subtype names, without a slash and containing ASCII letters only.
    #[serde(default)]
    pub annotations_of_type: Vec<String>,
    /// Form handling (keep by default).
    #[serde(default)]
    pub forms: SanitizeForms,
}
impl Default for SanitizeParams {
    fn default() -> Self {
        Self {
            javascript: true,
            embedded_files: true,
            thumbnails: true,
            links: false,
            metadata: false,
            xmp: false,
            annotations_of_type: vec![],
            forms: SanitizeForms::Keep,
        }
    }
}
impl SanitizeParams {
    /// Checks annotation subtype names.
    pub fn validate(&self) -> Result<(), ParamsError> {
        if self
            .annotations_of_type
            .iter()
            .any(|s| s.is_empty() || !s.bytes().all(|b| b.is_ascii_alphabetic()))
        {
            Err(ParamsError::new(
                "annotations_of_type",
                "names must contain ASCII letters only",
            ))
        } else {
            Ok(())
        }
    }
}
