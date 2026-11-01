//! Pure `serde` + `schemars` parameter structs for every registered document tool.
//!
//! All tool parameter types live in `ariad-core` with no engine or `lopdf` dependencies so
//! [`crate::tools::registry`] can resolve `params_schema` without crate cycles.

pub mod alternate_mix;
pub mod attachments;
pub mod booklet;
pub mod common;
pub mod crop;
pub mod crop_whitespace;
pub mod decompress;
pub mod delete_pages;
pub mod detect_scanned;
pub mod divide_pages;
pub mod extract_images;
pub mod extract_pages;
pub mod image_to_pdf;
pub mod info;
pub mod insert_blank;
pub mod layers;
pub mod merge;
pub mod metadata;
pub mod nup;
pub mod outline;
pub mod overlay;
pub mod page_labels;
pub mod portfolio;
pub mod posterize;
pub mod remove_blank;
pub mod render;
pub mod reorder;
pub mod rotate;
pub mod rotate_content;
pub mod scale;
pub mod single_page;
pub mod split;
pub mod text_layout;
pub mod thumbnails;
pub mod viewer_prefs;

use schemars::{JsonSchema, Schema, generate::SchemaSettings};

pub use alternate_mix::AlternateMixParams;
pub use attachments::{AttachmentsAction, AttachmentsParams};
pub use booklet::{BookletBinding, BookletParams};
pub use common::{CustomPaperSize, GridOrder, MarginSides, Margins, PaperSize, StandardPaper};
pub use crop::{CropAutoMode, CropBoxSpec, CropParams, CropRect, CropTarget};
pub use crop_whitespace::CropWhitespaceParams;
pub use decompress::{DecompressParams, ObjectStreamsMode};
pub use delete_pages::DeletePagesParams;
pub use detect_scanned::DetectScannedParams;
pub use divide_pages::DividePagesParams;
pub use extract_images::{ExtractFormat, ExtractImagesParams};
pub use extract_pages::ExtractPagesParams;
pub use image_to_pdf::{ImageToPdfParams, Orientation, PageSize};
pub use info::InfoParams;
pub use insert_blank::{BlankPageSize, InsertBlankParams, SamePageSize};
pub use layers::{LayersAction, LayersParams};
pub use merge::{MergeOutlineMode, MergePageLabelsMode, MergeParams};
pub use metadata::{MetadataFields, MetadataParams};
pub use nup::NupParams;
pub use outline::{OutlineAction, OutlineNode, OutlineParams};
pub use overlay::{OverlayPagesMode, OverlayParams, OverlayPosition};
pub use page_labels::{PageLabelRange, PageLabelStyle, PageLabelsParams};
pub use portfolio::{PortfolioAction, PortfolioParams, PortfolioView};
pub use posterize::{PosterizeParams, PosterizeTiles};
pub use remove_blank::RemoveBlankParams;
pub use render::{Background, Combine, RasterFormat, RenderParams, ToImagesParams};
pub use reorder::{ReorderParams, ReorderPreset, ReorderSpec};
pub use rotate::RotateParams;
pub use rotate_content::RotateContentParams;
pub use scale::{ScaleFit, ScaleParams};
pub use single_page::SinglePageParams;
pub use split::{SplitMode, SplitParams};
pub use text_layout::TextLayoutParams;
pub use thumbnails::ThumbnailsParams;
pub use viewer_prefs::{ViewerPageLayout, ViewerPageMode, ViewerPrefsParams};

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
