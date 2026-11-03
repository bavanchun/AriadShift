//! Parameters and interchange model for PDF annotations.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::form_model::ActionSummary;
use crate::tools::PageRange;

/// Maximum UTF-8 bytes in an annotation string.
pub const MAX_ANNOTATION_STRING_BYTES: usize = 1024 * 1024;

/// Maximum annotations submitted in one request.
pub const MAX_ANNOTATION_ITEMS: usize = 5_000;
/// Maximum points in one annotation.
pub const MAX_POINTS_PER_ANNOTATION: usize = 10_000;
/// Maximum annotations returned by a listing.
pub const MAX_LISTED_ANNOTATIONS: usize = 20_000;

/// Annotation operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AnnotationsAction {
    List,
    Add,
    Update,
    Delete,
    Flatten,
}

/// Supported annotation types.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AnnotationType {
    Highlight,
    Underline,
    Strikeout,
    Squiggly,
    Note,
    FreeText,
    Ink,
    Line,
    Arrow,
    Rectangle,
    Ellipse,
    Polygon,
    Polyline,
    Stamp,
    Link,
}

/// Standard stamp names from ISO 32000-1.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StampName {
    Approved,
    Experimental,
    NotApproved,
    AsIs,
    Expired,
    NotForPublicRelease,
    Confidential,
    Final,
    Sold,
    Departmental,
    ForComment,
    TopSecret,
    Draft,
    ForPublicRelease,
}

/// Annotation visibility and protection flags.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct AnnotationFlags {
    pub hidden: bool,
    pub print: bool,
    pub no_view: bool,
    pub no_rotate: bool,
    pub locked: bool,
    pub read_only: bool,
}
impl Default for AnnotationFlags {
    fn default() -> Self {
        Self {
            hidden: false,
            print: true,
            no_view: false,
            no_rotate: false,
            locked: false,
            read_only: false,
        }
    }
}

/// An annotation to add or update, using 1-based page numbers.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AnnotationItem {
    #[serde(default)]
    pub id: Option<String>,
    pub page: u32,
    #[serde(rename = "type")]
    pub annotation_type: AnnotationType,
    #[serde(default)]
    pub rect: Option<[f64; 4]>,
    #[serde(default)]
    pub quads: Option<Vec<[f64; 8]>>,
    #[serde(default)]
    pub points: Option<Vec<[f64; 2]>>,
    #[serde(default)]
    pub strokes: Option<Vec<Vec<[f64; 2]>>>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub fill: Option<String>,
    #[serde(default = "one")]
    pub opacity: f64,
    #[serde(default = "one")]
    pub line_width: f64,
    #[serde(default)]
    pub contents: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub subject: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default = "font_size")]
    pub font_size: f64,
    #[serde(default)]
    pub stamp: Option<StampName>,
    #[serde(default)]
    pub uri: Option<String>,
    #[serde(default)]
    pub dest_page: Option<u32>,
    #[serde(default)]
    pub flags: Option<AnnotationFlags>,
}

impl AnnotationItem {
    /// Validate geometry, sizes and link destinations.
    pub fn validate(&self) -> Result<(), String> {
        if [
            &self.id,
            &self.color,
            &self.fill,
            &self.contents,
            &self.author,
            &self.subject,
            &self.text,
            &self.uri,
        ]
        .into_iter()
        .flatten()
        .any(|value| value.len() > MAX_ANNOTATION_STRING_BYTES)
        {
            return Err("annotation string exceeds the size limit".into());
        }
        if self.page == 0
            || !self.opacity.is_finite()
            || !(0.0..=1.0).contains(&self.opacity)
            || !self.line_width.is_finite()
            || self.line_width < 0.0
            || !self.font_size.is_finite()
            || self.font_size <= 0.0
        {
            return Err("invalid annotation page, opacity or line or font size".into());
        }
        if self
            .rect
            .is_some_and(|r| r.iter().any(|n| !n.is_finite()) || r[0] >= r[2] || r[1] >= r[3])
        {
            return Err("invalid annotation rectangle".into());
        }
        let quads = self.quads.as_deref().unwrap_or_default();
        let points = self.points.as_deref().unwrap_or_default();
        let strokes = self.strokes.as_deref().unwrap_or_default();
        let count = quads
            .len()
            .saturating_mul(4)
            .saturating_add(points.len())
            .saturating_add(
                strokes
                    .iter()
                    .fold(0usize, |sum, stroke| sum.saturating_add(stroke.len())),
            );
        if count > MAX_POINTS_PER_ANNOTATION
            || quads
                .iter()
                .flatten()
                .chain(points.iter().flatten())
                .chain(strokes.iter().flatten().flatten())
                .any(|n| !n.is_finite())
        {
            return Err("invalid or excessive annotation geometry".into());
        }
        for color in [self.color.as_deref(), self.fill.as_deref()]
            .into_iter()
            .flatten()
        {
            let bytes = color.as_bytes();
            if !matches!(bytes.len(), 4 | 7)
                || bytes.first() != Some(&b'#')
                || !bytes[1..].iter().all(u8::is_ascii_hexdigit)
            {
                return Err("invalid annotation color".into());
            }
        }
        if matches!(
            self.annotation_type,
            AnnotationType::Note
                | AnnotationType::FreeText
                | AnnotationType::Rectangle
                | AnnotationType::Ellipse
                | AnnotationType::Stamp
                | AnnotationType::Link
        ) && self.rect.is_none()
        {
            return Err("annotation requires a rectangle".into());
        }
        if self
            .uri
            .as_deref()
            .is_some_and(|uri| !crate::links::allowed_link(uri))
        {
            return Err("link URI is not allowed".into());
        }
        match self.annotation_type {
            AnnotationType::Highlight
            | AnnotationType::Underline
            | AnnotationType::Strikeout
            | AnnotationType::Squiggly
                if quads.is_empty() =>
            {
                return Err("text markup requires quads".into());
            }
            AnnotationType::Line | AnnotationType::Arrow if points.len() != 2 => {
                return Err("line requires two points".into());
            }
            AnnotationType::Polygon if points.len() < 3 => {
                return Err("polygon requires at least three points".into());
            }
            AnnotationType::Polyline if points.len() < 2 => {
                return Err("polyline requires at least two points".into());
            }
            AnnotationType::Ink if strokes.is_empty() || strokes.iter().any(|s| s.len() < 2) => {
                return Err("ink requires strokes of at least two points".into());
            }
            AnnotationType::Link
                if !self.uri.as_deref().is_some_and(crate::links::allowed_link)
                    && !self.dest_page.is_some_and(|p| p > 0) =>
            {
                return Err("link requires an allowed URI or destination page".into());
            }
            _ => {}
        }
        Ok(())
    }
}

/// Selection and payload for the annotation operation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AnnotationsParams {
    pub action: AnnotationsAction,
    #[serde(default)]
    pub pages: PageRange,
    #[serde(default)]
    pub items: Option<Vec<AnnotationItem>>,
    #[serde(default)]
    pub ids: Option<Vec<String>>,
    #[serde(default)]
    pub types: Option<Vec<AnnotationType>>,
    #[serde(default)]
    pub all: bool,
}
impl AnnotationsParams {
    /// Reject empty or ambiguous write selections.
    pub fn validate(&self) -> Result<(), String> {
        let items = self.items.as_deref().unwrap_or_default();
        if items.len() > MAX_ANNOTATION_ITEMS {
            return Err("too many annotations".into());
        }
        if matches!(
            self.action,
            AnnotationsAction::Add | AnnotationsAction::Update
        ) && items.is_empty()
        {
            return Err("items are required".into());
        }
        if self.action == AnnotationsAction::Update
            && items
                .iter()
                .any(|i| i.id.as_deref().is_none_or(str::is_empty))
        {
            return Err("update requires item ids".into());
        }
        if matches!(
            self.action,
            AnnotationsAction::Delete | AnnotationsAction::Flatten
        ) && !self.all
            && self.ids.as_ref().is_none_or(Vec::is_empty)
            && self.types.as_ref().is_none_or(Vec::is_empty)
        {
            return Err("a selection is required".into());
        }
        items.iter().try_for_each(AnnotationItem::validate)
    }
}

/// Annotation output with raw subtype and action metadata.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AnnotationInfo {
    pub id: String,
    pub page: u32,
    pub subtype: String,
    #[serde(rename = "type")]
    pub annotation_type: Option<AnnotationType>,
    pub rect: Option<[f64; 4]>,
    pub quads: Option<Vec<[f64; 8]>>,
    pub points: Option<Vec<[f64; 2]>>,
    pub strokes: Option<Vec<Vec<[f64; 2]>>>,
    pub color: Option<String>,
    pub fill: Option<String>,
    pub opacity: f64,
    pub line_width: f64,
    pub contents: Option<String>,
    pub author: Option<String>,
    pub subject: Option<String>,
    pub text: Option<String>,
    pub font_size: f64,
    pub stamp: Option<StampName>,
    pub uri: Option<String>,
    pub dest_page: Option<u32>,
    pub flags: AnnotationFlags,
    pub has_appearance: bool,
    pub popup_of: Option<String>,
    pub reply_to: Option<String>,
    pub actions: ActionSummary,
}
impl AnnotationInfo {
    /// Convert a supported annotation into an importable item.
    pub fn to_item(&self) -> Option<AnnotationItem> {
        Some(AnnotationItem {
            id: Some(self.id.clone()),
            page: self.page,
            annotation_type: self.annotation_type?,
            rect: self.rect,
            quads: self.quads.clone(),
            points: self.points.clone(),
            strokes: self.strokes.clone(),
            color: self.color.clone(),
            fill: self.fill.clone(),
            opacity: self.opacity,
            line_width: self.line_width,
            contents: self.contents.clone(),
            author: self.author.clone(),
            subject: self.subject.clone(),
            text: self.text.clone(),
            font_size: self.font_size,
            stamp: self.stamp,
            uri: self.uri.clone(),
            dest_page: self.dest_page,
            flags: Some(self.flags.clone()),
        })
    }
}

/// Controls whether an import replaces existing annotations.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct AnnotationsImportParams {
    pub replace: bool,
}

/// Annotation data output format.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AnnotationsExportFormat {
    #[default]
    Xfdf,
    Json,
}

/// Selects annotations and their export format.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AnnotationsExportParams {
    #[serde(default)]
    pub format: AnnotationsExportFormat,
    #[serde(default)]
    pub pages: PageRange,
}

fn one() -> f64 {
    1.0
}

fn font_size() -> f64 {
    12.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn bounds_annotation_strings() {
        let mut item: AnnotationItem =
            serde_json::from_value(json!({"page":1,"type":"note","rect":[0,0,10,10]})).unwrap();
        item.contents = Some("x".repeat(MAX_ANNOTATION_STRING_BYTES));
        assert!(item.validate().is_ok());
        item.contents = Some("x".repeat(MAX_ANNOTATION_STRING_BYTES + 1));
        assert!(item.validate().is_err());
    }

    #[test]
    fn validates_geometry_for_each_annotation_type() {
        let cases = [
            ("highlight", json!({"quads":[[0,1,1,1,0,0,1,0]]})),
            ("underline", json!({"quads":[[0,1,1,1,0,0,1,0]]})),
            ("strikeout", json!({"quads":[[0,1,1,1,0,0,1,0]]})),
            ("squiggly", json!({"quads":[[0,1,1,1,0,0,1,0]]})),
            ("ink", json!({"strokes":[[[0,0],[1,1]]]})),
            ("line", json!({"points":[[0,0],[1,1]]})),
            ("arrow", json!({"points":[[0,0],[1,1]]})),
            ("polygon", json!({"points":[[0,0],[1,0],[1,1]]})),
            ("polyline", json!({"points":[[0,0],[1,1]]})),
            ("note", json!({"rect":[0,0,10,10]})),
            ("free_text", json!({"rect":[0,0,10,10]})),
            ("rectangle", json!({"rect":[0,0,10,10]})),
            ("ellipse", json!({"rect":[0,0,10,10]})),
            ("stamp", json!({"rect":[0,0,10,10]})),
            ("link", json!({"rect":[0,0,10,10],"dest_page":1})),
        ];
        for (kind, extra) in cases {
            let mut input = json!({"type":kind,"page":1});
            input
                .as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            let mut item: AnnotationItem = serde_json::from_value(input).unwrap();
            assert!(item.validate().is_ok(), "{kind}");
            item.page = 0;
            assert!(item.validate().is_err(), "{kind}");
        }
    }

    #[test]
    fn selection_and_link_validation() {
        let mut params: AnnotationsParams =
            serde_json::from_value(json!({"action":"delete"})).unwrap();
        assert!(params.validate().is_err());
        params.all = true;
        assert!(params.validate().is_ok());
        let mut item: AnnotationItem = serde_json::from_value(
            json!({"page":1,"type":"link","rect":[0,0,10,10],"uri":"https://example.com"}),
        )
        .unwrap();
        assert!(item.validate().is_ok());
        item.uri = Some("javascript:alert(1)".into());
        assert!(item.validate().is_err());
    }
}
