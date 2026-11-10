//! Parameter types for `pdf.to_cbz`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::tools::PageRange;

/// Image format for rendering CBZ pages.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ImageFormat {
    /// Lossless PNG (default).
    #[default]
    Png,
    /// JPEG.
    Jpeg,
    /// WebP.
    Webp,
}

impl ImageFormat {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpeg",
            Self::Webp => "webp",
        }
    }
}

/// Parameters for `pdf.to_cbz`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ToCbzParams {
    /// Pages to include (default: all pages).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pages: Option<PageRange>,
    /// Render resolution in dots per inch (default: 150.0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dpi: Option<f32>,
    /// Image format for the rendered pages (default: PNG).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<ImageFormat>,
    /// Encoder quality for lossy formats (1-100).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quality: Option<u8>,
}

impl ToCbzParams {
    pub fn validate(&self) -> Result<(), String> {
        if let Some(dpi) = self.dpi
            && !(36.0..=600.0).contains(&dpi)
        {
            return Err(format!("dpi {dpi} out of range [36, 600]"));
        }
        if let Some(q) = self.quality
            && !(1..=100).contains(&q)
        {
            return Err(format!("quality {q} out of range [1, 100]"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_params() {
        let ok = ToCbzParams {
            pages: None,
            dpi: Some(150.0),
            format: Some(ImageFormat::Png),
            quality: None,
        };
        assert!(ok.validate().is_ok());

        let bad_dpi = ToCbzParams {
            dpi: Some(10.0),
            ..Default::default()
        };
        assert!(bad_dpi.validate().is_err());

        let bad_quality = ToCbzParams {
            quality: Some(101),
            ..Default::default()
        };
        assert!(bad_quality.validate().is_err());
    }
}
