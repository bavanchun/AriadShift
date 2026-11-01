//! Pure `serde` + `schemars` parameter structs for every registered document tool.
//!
//! All tool parameter types live in `ariad-core` with no engine or `lopdf` dependencies so
//! [`crate::tools::registry`] can resolve `params_schema` without crate cycles.

pub mod crop_whitespace;
pub mod detect_scanned;
pub mod extract_images;
pub mod image_to_pdf;
pub mod info;
pub mod remove_blank;
pub mod render;
pub mod text_layout;
pub mod thumbnails;

use schemars::{JsonSchema, Schema, generate::SchemaSettings};

pub use crop_whitespace::CropWhitespaceParams;
pub use detect_scanned::DetectScannedParams;
pub use extract_images::{ExtractFormat, ExtractImagesParams};
pub use image_to_pdf::{ImageToPdfParams, Orientation, PageSize};
pub use info::InfoParams;
pub use remove_blank::RemoveBlankParams;
pub use render::{Background, Combine, RasterFormat, RenderParams, ToImagesParams};
pub use text_layout::TextLayoutParams;
pub use thumbnails::ThumbnailsParams;

/// A parameter value that the JSON schema alone cannot reject, reported without any document
/// content so it is safe to relay as an engine failure message.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("invalid parameter `{field}`: {reason}")]
pub struct ParamsError {
    /// Offending parameter name.
    pub field: &'static str,
    /// Fixed explanation.
    pub reason: &'static str,
}

impl ParamsError {
    /// Builds an error for `field`.
    #[must_use]
    pub const fn new(field: &'static str, reason: &'static str) -> Self {
        Self { field, reason }
    }
}

/// Checks that `value` lies in `min..=max`.
pub(crate) fn check_range<T: PartialOrd>(
    field: &'static str,
    value: T,
    min: T,
    max: T,
) -> Result<(), ParamsError> {
    if value < min || value > max {
        Err(ParamsError::new(
            field,
            "value is outside the allowed range",
        ))
    } else {
        Ok(())
    }
}

/// Generates a Draft 2020-12 JSON Schema for the parameter struct `P`.
#[must_use]
pub fn schema_for_params<P: JsonSchema>() -> Schema {
    SchemaSettings::draft2020_12()
        .into_generator()
        .into_root_schema_for::<P>()
}
