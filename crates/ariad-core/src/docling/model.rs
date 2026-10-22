//! The subset of the DoclingDocument JSON schema (version 1.x) that the mapper reads.
//!
//! Every field the mapper does not need is ignored, and every field it reads has a default,
//! so documents from older or newer 1.x writers deserialize as long as their shape matches.

use std::collections::BTreeMap;

use serde::Deserialize;

/// The schema name every DoclingDocument carries in `schema_name`.
pub const SCHEMA_NAME: &str = "DoclingDocument";

/// A DoclingDocument as written by `DoclingDocument.save_as_json`.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct DoclingDocument {
    pub schema_name: String,
    pub version: String,
    pub origin: Option<Origin>,
    pub furniture: Option<GroupItem>,
    pub body: GroupItem,
    pub groups: Vec<GroupItem>,
    pub texts: Vec<TextItem>,
    pub pictures: Vec<PictureItem>,
    pub tables: Vec<TableItem>,
    pub pages: BTreeMap<String, PageItem>,
}

/// Where the converted document came from.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Origin {
    pub mimetype: String,
}

/// A JSON pointer to another item, such as `#/texts/3`.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct RefItem {
    #[serde(rename = "$ref")]
    pub target: String,
}

/// A container node: the body, a list, an inline run, a slide or a section.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct GroupItem {
    pub children: Vec<RefItem>,
    pub content_layer: Option<String>,
    pub label: String,
}

/// A text-bearing node: paragraphs, headings, list items, captions, code and formulas.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct TextItem {
    pub children: Vec<RefItem>,
    pub content_layer: Option<String>,
    pub label: String,
    pub prov: Vec<ProvenanceItem>,
    pub text: String,
    pub formatting: Option<Formatting>,
    pub hyperlink: Option<String>,
    /// Section header level, starting at 1.
    pub level: Option<i64>,
    /// Whether a list item belongs to a numbered list.
    pub enumerated: Option<bool>,
    /// The bullet or number that prefixed a list item.
    pub marker: Option<String>,
    pub code_language: Option<String>,
}

/// A table with its cells, captions and footnotes.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct TableItem {
    pub content_layer: Option<String>,
    pub prov: Vec<ProvenanceItem>,
    pub captions: Vec<RefItem>,
    pub footnotes: Vec<RefItem>,
    pub data: TableData,
}

/// The cells of a table; `grid` duplicates them and is not read.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct TableData {
    pub table_cells: Vec<TableCell>,
}

/// One table cell. A rich cell points at a node that holds formatted content.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct TableCell {
    pub row_span: i64,
    pub col_span: i64,
    pub start_row_offset_idx: i64,
    pub start_col_offset_idx: i64,
    pub text: String,
    pub column_header: bool,
    pub row_header: bool,
    pub row_section: bool,
    #[serde(rename = "ref")]
    pub rich: Option<RefItem>,
}

/// A picture or chart, with its rendered image when the writer embedded it.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct PictureItem {
    pub content_layer: Option<String>,
    pub prov: Vec<ProvenanceItem>,
    pub captions: Vec<RefItem>,
    pub footnotes: Vec<RefItem>,
    pub image: Option<ImageRef>,
}

/// An image reference; embedded images use a `data:` URI.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct ImageRef {
    pub uri: String,
}

/// Inline formatting of a text item.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Formatting {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
    pub script: String,
}

/// Where an item sits on a page.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct ProvenanceItem {
    pub page_no: i64,
    pub bbox: Option<BoundingBox>,
}

/// A box in page coordinates; `coord_origin` is `TOPLEFT` or `BOTTOMLEFT`.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct BoundingBox {
    pub l: f64,
    pub t: f64,
    pub r: f64,
    pub b: f64,
    pub coord_origin: Option<String>,
}

/// Page geometry keyed by page number.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct PageItem {
    pub size: Size,
    pub page_no: i64,
}

/// Width and height in points.
#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Size {
    pub width: f64,
    pub height: f64,
}
