//! Parameters for PDF compression.
use super::{ParamsError, check_range};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
/// Compression profile.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum CompressPreset {
    /// Small files for screens.
    Screen,
    /// Balanced compression.
    Ebook,
    /// Printing quality.
    Print,
    /// Lossless compression.
    Lossless,
}
/// Parameters for `pdf.compress`.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CompressParams {
    /// Compression level from 1 through 9.
    #[serde(default)]
    #[schemars(range(min = 1, max = 9))]
    pub level: Option<u8>,
    /// Optional named compression profile.
    #[serde(default)]
    pub preset: Option<CompressPreset>,
    /// Desired maximum output size in bytes, at least 1024.
    #[serde(default)]
    #[schemars(range(min = 1024))]
    pub target_bytes: Option<u64>,
    /// Convert images to grayscale.
    #[serde(default)]
    pub grayscale: bool,
    /// Maximum image resolution from 36 through 1200 dpi.
    #[serde(default)]
    #[schemars(range(min = 36, max = 1200))]
    pub max_dpi: Option<u32>,
    /// JPEG quality from 10 through 100.
    #[serde(default)]
    #[schemars(range(min = 10, max = 100))]
    pub jpeg_quality: Option<u8>,
}
impl CompressParams {
    /// Checks ranges and mutually exclusive selectors.
    pub fn validate(&self) -> Result<(), ParamsError> {
        if self.level.is_some() && self.preset.is_some() {
            return Err(ParamsError::new("level", "cannot be combined with preset"));
        }
        if let Some(v) = self.level {
            check_range("level", v, 1, 9)?;
        }
        if let Some(v) = self.target_bytes {
            check_range("target_bytes", v, 1024, u64::MAX)?;
        }
        if let Some(v) = self.max_dpi {
            check_range("max_dpi", v, 36, 1200)?;
        }
        if let Some(v) = self.jpeg_quality {
            check_range("jpeg_quality", v, 10, 100)?;
        }
        Ok(())
    }
    /// Returns the selected profile, or ebook when neither selector is set.
    #[must_use]
    pub fn effective_preset(&self) -> Option<CompressPreset> {
        self.preset.or(if self.level.is_none() {
            Some(CompressPreset::Ebook)
        } else {
            None
        })
    }
}
