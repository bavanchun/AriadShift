use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub type Attr = (String, Vec<String>, Vec<(String, String)>);
pub type Target = (String, String);
pub type ListAttributes = (u64, ListNumberStyle, ListNumberDelim);
pub type Caption = (Option<Vec<Inline>>, Vec<Block>);
pub type ColSpec = (Alignment, ColWidth);
pub type Row = (Attr, Vec<Cell>);
pub type Cell = (Attr, Alignment, u64, u64, Vec<Block>);
pub type TableHead = (Attr, Vec<Row>);
pub type TableBody = (Attr, u64, Vec<Row>, Vec<Row>);
pub type TableFoot = (Attr, Vec<Row>);
pub type Table = (
    Attr,
    Caption,
    Vec<ColSpec>,
    TableHead,
    Vec<TableBody>,
    TableFoot,
);
pub type Figure = (Attr, Caption, Vec<Block>);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pandoc {
    #[serde(rename = "pandoc-api-version")]
    pub pandoc_api_version: Vec<u32>,
    pub meta: BTreeMap<String, MetaValue>,
    pub blocks: Vec<Block>,
}

impl Default for Pandoc {
    fn default() -> Self {
        Self {
            pandoc_api_version: vec![1, 23],
            meta: BTreeMap::new(),
            blocks: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", content = "c")]
pub enum Block {
    Plain(Vec<Inline>),
    Para(Vec<Inline>),
    LineBlock(Vec<Vec<Inline>>),
    CodeBlock(Attr, String),
    RawBlock(String, String),
    BlockQuote(Vec<Block>),
    OrderedList(ListAttributes, Vec<Vec<Block>>),
    BulletList(Vec<Vec<Block>>),
    DefinitionList(Vec<(Vec<Inline>, Vec<Vec<Block>>)>),
    Header(u8, Attr, Vec<Inline>),
    HorizontalRule,
    Table(Box<Table>),
    Figure(Figure),
    Div(Attr, Vec<Block>),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", content = "c")]
pub enum Inline {
    Str(String),
    Emph(Vec<Inline>),
    Underline(Vec<Inline>),
    Strong(Vec<Inline>),
    Strikeout(Vec<Inline>),
    Superscript(Vec<Inline>),
    Subscript(Vec<Inline>),
    SmallCaps(Vec<Inline>),
    Quoted(QuoteType, Vec<Inline>),
    Cite(Vec<Citation>, Vec<Inline>),
    Code(Attr, String),
    Space,
    SoftBreak,
    LineBreak,
    Math(MathType, String),
    RawInline(String, String),
    Link(Attr, Vec<Inline>, Target),
    Image(Attr, Vec<Inline>, Target),
    Note(Vec<Block>),
    Span(Attr, Vec<Inline>),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Citation {
    #[serde(rename = "citationId")]
    pub citation_id: String,
    #[serde(rename = "citationPrefix")]
    pub citation_prefix: Vec<Inline>,
    #[serde(rename = "citationSuffix")]
    pub citation_suffix: Vec<Inline>,
    #[serde(rename = "citationMode")]
    pub citation_mode: CitationMode,
    #[serde(rename = "citationNoteNum")]
    pub citation_note_num: u64,
    #[serde(rename = "citationHash")]
    pub citation_hash: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t")]
pub enum CitationMode {
    AuthorInText,
    SuppressAuthor,
    NormalCitation,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", content = "c")]
pub enum MetaValue {
    MetaMap(BTreeMap<String, MetaValue>),
    MetaList(Vec<MetaValue>),
    MetaBool(bool),
    MetaString(String),
    MetaInlines(Vec<Inline>),
    MetaBlocks(Vec<Block>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", content = "c")]
pub enum Alignment {
    AlignDefault,
    AlignLeft,
    AlignCenter,
    AlignRight,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", content = "c")]
pub enum ColWidth {
    ColWidthDefault,
    ColWidth(f64),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", content = "c")]
pub enum ListNumberStyle {
    DefaultStyle,
    Example,
    Decimal,
    LowerRoman,
    UpperRoman,
    LowerAlpha,
    UpperAlpha,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", content = "c")]
pub enum ListNumberDelim {
    DefaultDelim,
    Period,
    OneParen,
    TwoParens,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", content = "c")]
pub enum MathType {
    InlineMath,
    DisplayMath,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", content = "c")]
pub enum QuoteType {
    SingleQuote,
    DoubleQuote,
}

#[must_use]
pub fn empty_attr() -> Attr {
    (String::new(), Vec::new(), Vec::new())
}

#[cfg(test)]
mod tests {
    use super::{Alignment, Block, Inline, Pandoc, Table, empty_attr};

    #[test]
    fn serializes_a_small_pandoc_document_exactly() {
        let document = Pandoc {
            blocks: vec![Block::Para(vec![
                super::Inline::Str("Hello".to_owned()),
                super::Inline::Space,
                super::Inline::Strong(vec![super::Inline::Str("world".to_owned())]),
            ])],
            ..Pandoc::default()
        };

        assert_eq!(
            serde_json::to_string(&document).unwrap(),
            r#"{"pandoc-api-version":[1,23],"meta":{},"blocks":[{"t":"Para","c":[{"t":"Str","c":"Hello"},{"t":"Space"},{"t":"Strong","c":[{"t":"Str","c":"world"}]}]}]}"#
        );
    }

    #[test]
    fn serializes_the_api_1_23_table_tuple_shape() {
        let table: Table = (
            empty_attr(),
            (None, Vec::new()),
            vec![(Alignment::AlignLeft, super::ColWidth::ColWidthDefault)],
            (empty_attr(), Vec::new()),
            vec![(empty_attr(), 0, Vec::new(), Vec::new())],
            (empty_attr(), Vec::new()),
        );
        let document = Pandoc {
            blocks: vec![Block::Table(Box::new(table))],
            ..Pandoc::default()
        };

        assert_eq!(
            serde_json::to_string(&document).unwrap(),
            r#"{"pandoc-api-version":[1,23],"meta":{},"blocks":[{"t":"Table","c":[["",[],[]],[null,[]],[[{"t":"AlignLeft"},{"t":"ColWidthDefault"}]],[["",[],[]],[]],[[["",[],[]],0,[],[]]],[["",[],[]],[]]]}]}"#
        );
    }

    #[test]
    fn deserializes_a_small_pandoc_document_with_four_part_version() {
        let json = r#"{"pandoc-api-version":[1,23,1,2],"meta":{},"blocks":[{"t":"Para","c":[{"t":"Str","c":"Hello"}]}]}"#;
        let doc: Pandoc = serde_json::from_str(json).unwrap();
        assert_eq!(doc.pandoc_api_version, vec![1, 23, 1, 2]);
        assert_eq!(
            doc.blocks,
            vec![Block::Para(vec![Inline::Str("Hello".to_owned())])]
        );
    }

    #[test]
    fn deserializes_constructors_emitted_by_readers() {
        let json = r#"{
            "pandoc-api-version":[1,23,1,2],
            "meta":{},
            "blocks":[
                {"t":"HorizontalRule"},
                {"t":"Div","c":[["my-div",["cls"],[["k","v"]]],[{"t":"Para","c":[{"t":"Str","c":"in div"}]}]]},
                {"t":"LineBlock","c":[[{"t":"Str","c":"line 1"}],[{"t":"Str","c":"line 2"}]]},
                {"t":"DefinitionList","c":[[[{"t":"Str","c":"term"}],[[{"t":"Plain","c":[{"t":"Str","c":"def"}]}]]]]},
                {"t":"Para","c":[
                    {"t":"Span","c":[["span-id",[],[]],[{"t":"Str","c":"span text"}]]},
                    {"t":"Underline","c":[{"t":"Str","c":"underlined"}]},
                    {"t":"SmallCaps","c":[{"t":"Str","c":"caps"}]},
                    {"t":"Quoted","c":[{"t":"DoubleQuote"},[{"t":"Str","c":"quoted"}]]},
                    {"t":"Cite","c":[
                        [{"citationId":"ref1","citationPrefix":[],"citationSuffix":[],"citationMode":{"t":"NormalCitation"},"citationNoteNum":1,"citationHash":42}],
                        [{"t":"Str","c":"[@ref1]"}]
                    ]},
                    {"t":"RawInline","c":["html","<span>raw</span>"]},
                    {"t":"Note","c":[{"t":"Para","c":[{"t":"Str","c":"footnote body"}]}]}
                ]},
                {"t":"RawBlock","c":["html","<div>raw block</div>"]}
            ]
        }"#;

        let doc: Pandoc = serde_json::from_str(json).unwrap();
        assert_eq!(doc.blocks.len(), 6);
        assert!(matches!(doc.blocks[0], Block::HorizontalRule));
        assert!(matches!(doc.blocks[1], Block::Div(..)));
        assert!(matches!(doc.blocks[2], Block::LineBlock(..)));
        assert!(matches!(doc.blocks[3], Block::DefinitionList(..)));
        assert!(matches!(doc.blocks[4], Block::Para(ref inlines) if inlines.len() == 7));
        assert!(matches!(doc.blocks[5], Block::RawBlock(..)));
    }
}
