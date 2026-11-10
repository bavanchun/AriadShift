//! Shared contracts for document reflow and stateless PDF page editing.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{fidelity::FidelityReport, tools::PageRange};

/// Document export or fidelity-preview target.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EditExportFormat {
    #[default]
    Pdf,
    Docx,
    Html,
    Md,
}

impl EditExportFormat {
    /// Canonical format identifier.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Pdf => "pdf",
            Self::Docx => "docx",
            Self::Html => "html",
            Self::Md => "md",
        }
    }
}

/// Open a document into IR, optionally selecting PDF pages and a preview target.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EditOpenDocumentParams {
    /// Omitted means preview all four export targets.
    pub target_format: Option<EditExportFormat>,
    /// Optional 1-based PDF page selector; omitted means all pages.
    pub pages: Option<String>,
}

impl EditOpenDocumentParams {
    /// Validate the optional page selector without needing a document.
    pub fn validate(&self) -> Result<(), String> {
        validate_pages(self.pages.as_deref())
    }
}

/// Export the complete edited IR document.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EditExportDocumentParams {
    #[serde(default)]
    pub format: EditExportFormat,
}

/// Inspect objects on selected PDF pages.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EditPageObjectsParams {
    pub pages: Option<String>,
}

impl EditPageObjectsParams {
    /// Validate the optional page selector without needing a document.
    pub fn validate(&self) -> Result<(), String> {
        validate_pages(self.pages.as_deref())
    }
}

fn validate_pages(pages: Option<&str>) -> Result<(), String> {
    if let Some(pages) = pages {
        PageRange::parse(pages).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// PDF save strategy after applying the complete ordered operation list.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EditSaveMode {
    #[default]
    Full,
    Incremental,
}

/// One stateless edit request. Object IDs refer to the supplied input document.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EditApplyParams {
    #[schemars(length(min = 1, max = 1000))]
    pub ops: Vec<PageEditOp>,
    #[serde(default)]
    pub save: EditSaveMode,
}

impl EditApplyParams {
    /// Validate operations before any document mutation; errors omit document content.
    pub fn validate(&self) -> Result<(), String> {
        if self.ops.is_empty() || self.ops.len() > 1000 {
            return Err("ops must contain between 1 and 1000 operations".into());
        }
        for (index, op) in self.ops.iter().enumerate() {
            op.validate().map_err(|e| format!("ops[{index}]: {e}"))?;
        }
        Ok(())
    }
}

/// Ordered edits in unrotated PDF page coordinates (points, origin at bottom left).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum PageEditOp {
    ReplaceText {
        #[schemars(range(min = 1))]
        page: u32,
        #[schemars(regex(pattern = "^p[1-9][0-9]*_obj[0-9]+$"))]
        object_id: String,
        #[schemars(length(min = 1))]
        text: String,
    },
    SetTextStyle {
        #[schemars(range(min = 1))]
        page: u32,
        #[schemars(regex(pattern = "^p[1-9][0-9]*_obj[0-9]+$"))]
        object_id: String,
        font_name: Option<String>,
        #[schemars(extend("exclusiveMinimum" = 0.0, "maximum" = 1000.0))]
        font_size: Option<f64>,
        #[schemars(regex(pattern = "^#([0-9A-Fa-f]{6}|[0-9A-Fa-f]{8})$"))]
        color: Option<String>,
    },
    MoveObject {
        #[schemars(range(min = 1))]
        page: u32,
        #[schemars(regex(pattern = "^p[1-9][0-9]*_obj[0-9]+$"))]
        object_id: String,
        dx_pt: f64,
        dy_pt: f64,
    },
    DeleteObject {
        #[schemars(range(min = 1))]
        page: u32,
        #[schemars(regex(pattern = "^p[1-9][0-9]*_obj[0-9]+$"))]
        object_id: String,
    },
    AddText {
        #[schemars(range(min = 1))]
        page: u32,
        #[schemars(length(min = 1))]
        text: String,
        x_pt: f64,
        y_pt: f64,
        font_name: Option<String>,
        #[schemars(extend("exclusiveMinimum" = 0.0, "maximum" = 1000.0))]
        font_size: f64,
        #[schemars(regex(pattern = "^#([0-9A-Fa-f]{6}|[0-9A-Fa-f]{8})$"))]
        color: Option<String>,
    },
    AddImage {
        #[schemars(range(min = 1))]
        page: u32,
        /// Left, bottom, right, top, in PDF points.
        bbox: [f64; 4],
        image_base64: String,
    },
    ReplaceImage {
        #[schemars(range(min = 1))]
        page: u32,
        #[schemars(regex(pattern = "^p[1-9][0-9]*_obj[0-9]+$"))]
        object_id: String,
        image_base64: String,
    },
}

impl PageEditOp {
    fn validate(&self) -> Result<(), String> {
        let (page, object_id) = match self {
            Self::ReplaceText {
                page, object_id, ..
            }
            | Self::SetTextStyle {
                page, object_id, ..
            }
            | Self::MoveObject {
                page, object_id, ..
            }
            | Self::DeleteObject { page, object_id }
            | Self::ReplaceImage {
                page, object_id, ..
            } => (*page, Some(object_id)),
            Self::AddText { page, .. } | Self::AddImage { page, .. } => (*page, None),
        };
        if page == 0 {
            return Err("page must be at least 1".into());
        }
        if let Some(id) = object_id {
            let valid = id.strip_prefix('p').and_then(|id| id.split_once("_obj"));
            if !valid.is_some_and(|(p, index)| {
                p == page.to_string()
                    && !index.is_empty()
                    && index.bytes().all(|b| b.is_ascii_digit())
            }) {
                return Err("object_id must match p{page}_obj{index} on the selected page".into());
            }
        }
        match self {
            Self::ReplaceText { text, .. } => validate_text(text)?,
            Self::SetTextStyle {
                font_size, color, ..
            } => {
                if let Some(size) = font_size {
                    validate_font_size(*size)?;
                }
                validate_color(color.as_deref())?;
            }
            Self::MoveObject { dx_pt, dy_pt, .. } => validate_coordinates(&[*dx_pt, *dy_pt])?,
            Self::AddText {
                text,
                x_pt,
                y_pt,
                font_size,
                color,
                ..
            } => {
                validate_text(text)?;
                validate_coordinates(&[*x_pt, *y_pt])?;
                validate_font_size(*font_size)?;
                validate_color(color.as_deref())?;
            }
            Self::AddImage { bbox, .. } => {
                validate_coordinates(bbox)?;
                if bbox[0] >= bbox[2] || bbox[1] >= bbox[3] {
                    return Err("bbox must have left < right and bottom < top".into());
                }
            }
            Self::DeleteObject { .. } | Self::ReplaceImage { .. } => {}
        }
        Ok(())
    }
}

fn validate_text(text: &str) -> Result<(), String> {
    if text.is_empty() {
        return Err("text must not be empty".into());
    }
    Ok(())
}

fn validate_font_size(size: f64) -> Result<(), String> {
    if !size.is_finite() || size <= 0.0 || size > 1000.0 {
        return Err("font_size must be finite and in (0, 1000]".into());
    }
    Ok(())
}

fn validate_coordinates(values: &[f64]) -> Result<(), String> {
    if values.iter().any(|v| !v.is_finite()) {
        return Err("coordinates must be finite".into());
    }
    Ok(())
}

fn validate_color(color: Option<&str>) -> Result<(), String> {
    if let Some(color) = color {
        let bytes = color.as_bytes();
        if !matches!(bytes.len(), 7 | 9)
            || bytes[0] != b'#'
            || !bytes[1..].iter().all(u8::is_ascii_hexdigit)
        {
            return Err("color must be #RRGGBB or #RRGGBBAA".into());
        }
    }
    Ok(())
}

/// Versioned output of `edit.page_objects`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PageObjectsDocument {
    /// Must equal [`Self::SCHEMA`].
    #[schemars(regex(pattern = "^ariad-pdf-page-objects/1$"))]
    pub schema: String,
    /// Total number of pages in the input, including unselected pages.
    pub page_count: usize,
    pub pages: Vec<PageObjectsPage>,
}

impl PageObjectsDocument {
    pub const SCHEMA: &'static str = "ariad-pdf-page-objects/1";

    /// Construct a report for all or selected pages of the input document.
    #[must_use]
    pub fn new(page_count: usize, pages: Vec<PageObjectsPage>) -> Self {
        Self {
            schema: Self::SCHEMA.into(),
            page_count,
            pages,
        }
    }
}

/// One inspected page and its objects in original object-index order.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PageObjectsPage {
    pub page: u32,
    pub width_pt: f64,
    pub height_pt: f64,
    pub rotation_deg: u16,
    pub objects: Vec<PageObjectEntry>,
}

/// PDF object information shared by engines and surfaces.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PageObjectEntry {
    pub id: String,
    pub page: u32,
    pub index: usize,
    pub kind: PageObjectKind,
    /// Left, bottom, right, top, in unrotated PDF page coordinates.
    pub bbox: [f64; 4],
    pub text: Option<String>,
    pub font_name: Option<String>,
    pub font_size: Option<f64>,
    pub weight: Option<u16>,
    pub italic: Option<bool>,
    pub fill_color: Option<String>,
    pub stroke_color: Option<String>,
    pub width_px: Option<u32>,
    pub height_px: Option<u32>,
    /// PDF affine matrix [a, b, c, d, e, f].
    pub matrix: [f64; 6],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PageObjectKind {
    Text,
    Image,
    Path,
}

/// Export-preview measurement and explicit losses for one target format.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FidelityPreview {
    pub format: EditExportFormat,
    pub report: FidelityReport,
    pub losses: Vec<String>,
}

/// IR block paths changed relative to the source document.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BlockChangeList {
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub modified: Vec<String>,
}
