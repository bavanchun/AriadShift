use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::format::Format;

/// The active version of the document intermediate representation.
pub const IR_VERSION: &str = "ariad-ir/0";

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
        }
    }
}

impl Document {
    /// Recursively counts all blocks in the document.
    #[must_use]
    pub fn block_count(&self) -> usize {
        count_blocks(&self.body)
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
    pub source_format: Option<Format>,
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

/// Optional layout hints, keyed by a stable extension identifier.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct LayoutIndex(pub BTreeMap<String, serde_json::Value>);

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

#[cfg(test)]
mod tests {
    use super::{Asset, AssetRef, Block, Document, IR_VERSION, Inline, RawFormat};

    #[test]
    fn enum_variants_use_internally_tagged_struct_forms() {
        let block = Block::Paragraph {
            content: vec![Inline::Text {
                text: "Hello".to_owned(),
            }],
        };
        assert_eq!(
            serde_json::to_string(&block).unwrap(),
            r#"{"type":"paragraph","content":[{"type":"text","text":"Hello"}]}"#
        );

        let reference = AssetRef::Url {
            href: "https://example.test/image.png".to_owned(),
        };
        assert_eq!(
            serde_json::to_string(&reference).unwrap(),
            r#"{"type":"url","href":"https://example.test/image.png"}"#
        );
    }

    #[test]
    fn document_roundtrips_and_asset_bytes_are_base64() {
        let mut document = Document::default();
        document.body.push(Block::Raw {
            format: RawFormat::Html,
            text: "<b>text</b>".to_owned(),
        });
        document.assets.insert(
            "abc".to_owned(),
            Asset {
                media_type: "image/png".to_owned(),
                bytes: vec![0, 1, 2, 255],
            },
        );

        let json = serde_json::to_string(&document).unwrap();
        assert!(json.contains(r#""version":"ariad-ir/0""#));
        assert!(json.contains(r#""bytes":"AAEC/w==""#));
        let roundtripped: Document = serde_json::from_str(&json).unwrap();
        assert_eq!(roundtripped, document);
        assert_eq!(Document::default().version, IR_VERSION);
    }

    #[test]
    fn raw_format_is_closed_to_html_and_tex() {
        assert!(serde_json::from_str::<RawFormat>("\"openxml\"").is_err());
        assert_eq!(serde_json::to_string(&RawFormat::Tex).unwrap(), "\"tex\"");
    }
}
