//! Pure `serde` + `schemars` parameter structs for every registered document tool.
//!
//! All tool parameter types live in `ariad-core` with no engine or `lopdf` dependencies so
//! [`crate::tools::registry`] can resolve `params_schema` without crate cycles.

pub mod add_image;
pub mod add_text;
pub mod adjust_colors;
pub mod alternate_mix;
pub mod annotations;
pub mod attachments;
pub mod auto_rotate;
pub mod background;
pub mod bates;
pub mod booklet;
pub mod common;
pub mod compare;
pub mod compress;
pub mod crop;
pub mod crop_whitespace;
pub mod decompress;
pub mod decrypt;
pub mod delete_pages;
pub mod deskew;
pub mod detect_scanned;
pub mod divide_pages;
pub mod edit;
pub mod encrypt;
pub mod extract_images;
pub mod extract_pages;
pub mod form_model;
pub mod forms;
pub mod grayscale;
pub mod header_footer;
pub mod image_ocr;
pub mod image_to_pdf;
pub mod info;
pub mod insert_blank;
pub mod invert_colors;
pub mod layers;
pub mod linearize;
pub mod merge;
pub mod metadata;
pub mod nup;
pub mod ocr;
pub mod ocr_internal;
pub mod outline;
pub mod overlay;
pub mod overlay_common;
pub mod overlay_marks_common;
pub mod page_labels;
pub mod page_numbers;
pub mod pdfa;
pub mod permissions;
pub mod portfolio;
pub mod posterize;
pub mod qr;
pub mod redact;
pub mod redact_internal;
pub mod remove_blank;
pub mod render;
pub mod reorder;
pub mod repair;
pub mod replace_color;
pub mod rotate;
pub mod rotate_content;
pub mod sanitize;
pub mod scale;
pub mod scan_split;
pub mod scanner_effect;
pub mod sign;
pub mod single_page;
pub mod split;
pub mod stamp;
pub mod tables_to_csv;
pub mod text_layout;
pub mod thumbnails;
pub mod to_cbz;
pub mod toc_page;
pub mod unlock_forms;
pub mod viewer_prefs;
pub mod watermark;
pub mod watermark_detect;
pub mod watermark_remove;
pub mod web_to_pdf;

use schemars::{JsonSchema, Schema, generate::SchemaSettings};

pub use add_image::AddImageParams;
pub use add_text::{AddTextParams, TextAlignment};
pub use adjust_colors::AdjustColorsParams;
pub use alternate_mix::AlternateMixParams;
pub use attachments::{AttachmentsAction, AttachmentsParams};
pub use auto_rotate::AutoRotateParams;
pub use background::BackgroundParams;
pub use bates::BatesParams;
pub use booklet::{BookletBinding, BookletParams};
pub use common::{CustomPaperSize, GridOrder, MarginSides, Margins, PaperSize, StandardPaper};
pub use compare::{CompareMode, CompareParams};
pub use compress::{CompressParams, CompressPreset};
pub use crop::{CropAutoMode, CropBoxSpec, CropParams, CropRect, CropTarget};
pub use crop_whitespace::CropWhitespaceParams;
pub use decompress::{DecompressParams, ObjectStreamsMode};
pub use decrypt::DecryptParams;
pub use delete_pages::DeletePagesParams;
pub use deskew::DeskewParams;
pub use detect_scanned::DetectScannedParams;
pub use divide_pages::DividePagesParams;
pub use edit::{
    BlockChangeList, EditApplyParams, EditExportDocumentParams, EditExportFormat,
    EditOpenDocumentParams, EditPageObjectsParams, EditSaveMode, FidelityPreview, PageEditOp,
    PageObjectEntry, PageObjectKind, PageObjectsDocument, PageObjectsPage,
};
pub use encrypt::{EncryptParams, EncryptionAlgorithm};
pub use extract_images::{ExtractFormat, ExtractImagesParams};
pub use extract_pages::ExtractPagesParams;
pub use grayscale::GrayscaleParams;
pub use header_footer::{HeaderFooterParams, HeaderFooterSlots};
pub use image_ocr::{ImageOcrOutput, ImageOcrParams};
pub use image_to_pdf::{ImageToPdfParams, Orientation, PageSize};
pub use info::InfoParams;
pub use insert_blank::{BlankPageSize, InsertBlankParams, SamePageSize};
pub use invert_colors::InvertColorsParams;
pub use layers::{LayersAction, LayersParams};
pub use linearize::LinearizeParams;
pub use merge::{MergeOutlineMode, MergePageLabelsMode, MergeParams};
pub use metadata::{MetadataFields, MetadataParams};
pub use nup::NupParams;
pub use ocr::{OcrMode, OcrOutput, OcrParams};
pub use ocr_internal::{OcrOrientationParams, OcrRecognizeParams, ScanRenderParams};
pub use outline::{OutlineAction, OutlineNode, OutlineParams};
pub use overlay::{OverlayPagesMode, OverlayParams, OverlayPosition};
pub use overlay_common::{Anchor, DateFormat, FontFace, HexColor, Opacity};
pub use page_labels::{PageLabelRange, PageLabelStyle, PageLabelsParams};
pub use page_numbers::PageNumbersParams;
pub use pdfa::{
    AccessibilityReportParams, PdfaLevel, ToPdfUaParams, ToPdfaParams, ValidatePdfUaParams,
    ValidatePdfaParams,
};
pub use permissions::{PdfPermissions, PermissionsParams};
pub use portfolio::{PortfolioAction, PortfolioParams, PortfolioView};
pub use posterize::{PosterizeParams, PosterizeTiles};
pub use qr::{QrBackground, QrLevel, QrParams};
pub use redact::{
    RedactParams, RedactVerifyParams, RedactionCarrierEvidence, RedactionPreset, RedactionScope,
    RedactionTarget, RedactionVerificationReport,
};
pub use redact_internal::{
    RedactGeometry, RedactGeometryChar, RedactGeometryPage, RedactInspectParams,
    RedactionCheckStatus, RedactionChecks, RedactionVerificationStatus,
};
pub use remove_blank::RemoveBlankParams;
pub use render::{Background, Combine, RasterFormat, RenderParams, ToImagesParams};
pub use reorder::{ReorderParams, ReorderPreset, ReorderSpec};
pub use repair::RepairParams;
pub use replace_color::{ColorTarget, ReplaceColorParams};
pub use rotate::RotateParams;
pub use rotate_content::RotateContentParams;
pub use sanitize::{SanitizeForms, SanitizeParams};
pub use scale::{ScaleFit, ScaleParams};
pub use scan_split::ScanSplitParams;
pub use scanner_effect::ScannerEffectParams;
pub use sign::{
    KeychainParams, OfflineEvidence, SignLevel, SignParams, SignRemoveParams, SignTokenParams,
    SignVerifyParams, SignVisualMode, SignVisualParams, TimestampParams, TokenParams,
    VisibleSignature,
};
pub use single_page::SinglePageParams;
pub use split::{SplitDetectMode, SplitMode, SplitParams};
pub use stamp::{StampParams, StampPreset};
pub use tables_to_csv::TablesToCsvParams;
pub use text_layout::TextLayoutParams;
pub use thumbnails::ThumbnailsParams;
pub use to_cbz::{ImageFormat, ToCbzParams};
pub use toc_page::{SamePage, TocPageParams, TocPageSize};
pub use unlock_forms::UnlockFormsParams;
pub use viewer_prefs::{ViewerPageLayout, ViewerPageMode, ViewerPrefsParams};
pub use watermark::{AutoSize, WatermarkFontSize, WatermarkKind, WatermarkLayout, WatermarkParams};
pub use watermark_detect::WatermarkDetectParams;
pub use watermark_remove::WatermarkRemoveParams;
pub use web_to_pdf::WebToPdfParams;

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
