//! Frozen ariad-ir/1 wire types. Changes require a new wire version.
#![allow(dead_code)]

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::format::Format;

/// The active version of the document intermediate representation.
pub const IR_VERSION: &str = "ariad-ir/1";

/// Readers retain compatibility with the last draft without accepting future versions.
#[must_use]
pub fn is_supported_version(version: Option<&str>) -> bool {
    matches!(version, Some(IR_VERSION | "ariad-ir/0"))
}

/// A format-independent document.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Document {
    #[serde(default = "default_ir_version")]
    pub version: String,
    pub meta: Metadata,
    pub body: Vec<Block>,
    pub assets: AssetStore,
    pub layout: Option<LayoutIndex>,
    pub provenance: Option<Provenance>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub furniture: Vec<Block>,
}

impl Default for Document {
    fn default() -> Self {
        Self {
            version: IR_VERSION.to_owned(),
            meta: Metadata::default(),
            body: Vec::new(),
            assets: AssetStore::new(),
            layout: None,
            provenance: None,
            furniture: Vec::new(),
        }
    }
}

impl Document {
    /// Recursively counts all blocks in the document.
    #[must_use]
    pub fn block_count(&self) -> usize {
        count_blocks(&self.body) + count_blocks(&self.furniture)
    }
}

/// Iteratively counts all blocks in a slice, including nested blocks.
#[must_use]
pub fn count_blocks(blocks: &[Block]) -> usize {
    let mut count = 0;
    let mut stack: Vec<&Block> = blocks.iter().collect();
    while let Some(block) = stack.pop() {
        count += 1;
        match block {
            Block::Quote { blocks } | Block::Footnote { blocks, .. } => {
                stack.extend(blocks);
            }
            Block::List { items, .. } => {
                for item in items {
                    stack.extend(&item.blocks);
                }
            }
            Block::Table { head, body, .. } => {
                for row in head.iter().chain(body.iter()) {
                    for cell in row {
                        stack.extend(&cell.blocks);
                    }
                }
            }
            _ => {}
        }
    }
    count
}

fn default_ir_version() -> String {
    IR_VERSION.to_owned()
}

/// User-facing document metadata.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Metadata {
    pub title: Option<String>,
    pub authors: Vec<String>,
    pub language: Option<String>,
    pub date: Option<String>,
    pub subject: Option<String>,
    pub keywords: Vec<String>,
    #[serde(
        serialize_with = "serialize_source_format",
        deserialize_with = "deserialize_source_format"
    )]
    #[schemars(schema_with = "source_format_schema")]
    pub source_format: Option<Format>,
}

/// A format supported by the core registry.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize, JsonSchema)]
#[schemars(rename = "Format")]
pub enum IrFormat {
    #[serde(rename = "markdown")]
    Markdown,
    #[serde(rename = "html")]
    Html,
    #[serde(rename = "docx")]
    Docx,
    #[serde(rename = "epub")]
    Epub,
    #[serde(rename = "pdf")]
    Pdf,
    #[serde(rename = "png")]
    Png,
    #[serde(rename = "ariad-ir+json")]
    AriadIrJson,
    #[serde(rename = "pandoc+json")]
    PandocJson,
    #[serde(rename = "docling+json")]
    DoclingJson,
    #[serde(rename = "pptx")]
    Pptx,
    #[serde(rename = "xlsx")]
    Xlsx,
    #[serde(rename = "odt")]
    Odt,
    #[serde(rename = "ods")]
    Ods,
    #[serde(rename = "odp")]
    Odp,
    #[serde(rename = "doc")]
    Doc,
    #[serde(rename = "xls")]
    Xls,
    #[serde(rename = "ppt")]
    Ppt,
    #[serde(rename = "rtf")]
    Rtf,
    #[serde(rename = "jpeg")]
    Jpeg,
    #[serde(rename = "webp")]
    Webp,
    #[serde(rename = "tiff")]
    Tiff,
}

impl IrFormat {
    #[must_use]
    pub const fn from_format(format: Format) -> Option<Self> {
        match format {
            Format::Markdown => Some(Self::Markdown),
            Format::Html => Some(Self::Html),
            Format::Docx => Some(Self::Docx),
            Format::Epub => Some(Self::Epub),
            Format::Pdf => Some(Self::Pdf),
            Format::Png => Some(Self::Png),
            Format::AriadIrJson => Some(Self::AriadIrJson),
            Format::PandocJson => Some(Self::PandocJson),
            Format::DoclingJson => Some(Self::DoclingJson),
            Format::Pptx => Some(Self::Pptx),
            Format::Xlsx => Some(Self::Xlsx),
            Format::Odt => Some(Self::Odt),
            Format::Ods => Some(Self::Ods),
            Format::Odp => Some(Self::Odp),
            Format::Doc => Some(Self::Doc),
            Format::Xls => Some(Self::Xls),
            Format::Ppt => Some(Self::Ppt),
            Format::Rtf => Some(Self::Rtf),
            Format::Jpeg => Some(Self::Jpeg),
            Format::Webp => Some(Self::Webp),
            Format::Tiff => Some(Self::Tiff),
            // Decision D1: IrFormat freezes the 21 format variants supported in ir.v1.
            // Any format variant added after ir.v1 was released (such as Phase 10
            // conversion parity formats or future variants) intentionally maps to
            // None, serializing as null in JSON metadata to preserve ir.v1 schema stability.
            _ => None,
        }
    }

    #[must_use]
    pub const fn to_format(self) -> Format {
        match self {
            Self::Markdown => Format::Markdown,
            Self::Html => Format::Html,
            Self::Docx => Format::Docx,
            Self::Epub => Format::Epub,
            Self::Pdf => Format::Pdf,
            Self::Png => Format::Png,
            Self::AriadIrJson => Format::AriadIrJson,
            Self::PandocJson => Format::PandocJson,
            Self::DoclingJson => Format::DoclingJson,
            Self::Pptx => Format::Pptx,
            Self::Xlsx => Format::Xlsx,
            Self::Odt => Format::Odt,
            Self::Ods => Format::Ods,
            Self::Odp => Format::Odp,
            Self::Doc => Format::Doc,
            Self::Xls => Format::Xls,
            Self::Ppt => Format::Ppt,
            Self::Rtf => Format::Rtf,
            Self::Jpeg => Format::Jpeg,
            Self::Webp => Format::Webp,
            Self::Tiff => Format::Tiff,
        }
    }
}

fn serialize_source_format<S>(value: &Option<Format>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    match value {
        Some(format) => IrFormat::from_format(*format).serialize(serializer),
        None => serializer.serialize_none(),
    }
}

fn deserialize_source_format<'de, D>(deserializer: D) -> Result<Option<Format>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let frozen: Option<IrFormat> = Option::deserialize(deserializer)?;
    Ok(frozen.map(IrFormat::to_format))
}

fn source_format_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
    generator.subschema_for::<Option<IrFormat>>()
}

/// The document's semantic content.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Block {
    Heading {
        level: u8,
        content: Vec<Inline>,
    },
    Paragraph {
        content: Vec<Inline>,
    },
    Code {
        lang: Option<String>,
        text: String,
    },
    Math {
        tex: String,
        display: bool,
    },
    Quote {
        blocks: Vec<Block>,
    },
    PageBreak {},
    List {
        ordered: bool,
        start: Option<u64>,
        tight: bool,
        items: Vec<ListItem>,
    },
    Table {
        caption: Option<Vec<Inline>>,
        columns: Vec<ColumnSpec>,
        head: Vec<Vec<TableCell>>,
        body: Vec<Vec<TableCell>>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        footnotes: Vec<Inline>,
    },
    Figure {
        asset: AssetRef,
        caption: Vec<Inline>,
    },
    Footnote {
        id: String,
        blocks: Vec<Block>,
    },
    Raw {
        format: RawFormat,
        text: String,
    },
}

/// A span of inline content.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Inline {
    Text {
        text: String,
    },
    Emph {
        content: Vec<Inline>,
    },
    Strong {
        content: Vec<Inline>,
    },
    Strikeout {
        content: Vec<Inline>,
    },
    Superscript {
        content: Vec<Inline>,
    },
    Subscript {
        content: Vec<Inline>,
    },
    Code {
        text: String,
    },
    Link {
        url: String,
        title: Option<String>,
        content: Vec<Inline>,
    },
    Image {
        target: AssetRef,
        alt: String,
        title: Option<String>,
    },
    SoftBreak {},
    LineBreak {},
    Math {
        tex: String,
        display: bool,
    },
    FootnoteRef {
        id: String,
    },
    Raw {
        format: RawFormat,
        text: String,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ListItem {
    pub checked: Option<bool>,
    pub blocks: Vec<Block>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Alignment {
    Default,
    Left,
    Center,
    Right,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ColumnSpec {
    pub align: Alignment,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TableCell {
    pub rowspan: u32,
    pub colspan: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub header: Option<bool>,
    pub blocks: Vec<Block>,
}

/// An asset or external URL referenced by an image.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AssetRef {
    Asset { id: String },
    Url { href: String },
}

/// Formats permitted for raw passthrough blocks and inlines.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum RawFormat {
    Html,
    Tex,
}

pub type AssetStore = BTreeMap<String, Asset>;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Asset {
    pub media_type: String,
    #[serde(with = "base64_bytes")]
    #[schemars(with = "String")]
    pub bytes: Vec<u8>,
}

/// Bounding box coordinates with a top-left origin: `[left, top, right, bottom]`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BoundingBox {
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
}

/// Page dimensions and geometry with a top-left origin.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PageGeometry {
    pub page_no: u32,
    pub width: f64,
    pub height: f64,
}

/// Position and page location for a block in the layout index.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BlockPosition {
    pub page_no: u32,
    pub bbox: BoundingBox,
}

/// Page geometry and per-block bounding boxes keyed by stable block path.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LayoutIndex {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pages: Vec<PageGeometry>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub blocks: BTreeMap<String, BlockPosition>,
}

/// Optional source and processing metadata, keyed by a stable extension identifier.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct Provenance(pub BTreeMap<String, serde_json::Value>);

mod base64_bytes {
    use base64::{Engine, engine::general_purpose::STANDARD};
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&STANDARD.encode(bytes))
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let encoded = String::deserialize(deserializer)?;
        STANDARD.decode(encoded).map_err(serde::de::Error::custom)
    }
}
