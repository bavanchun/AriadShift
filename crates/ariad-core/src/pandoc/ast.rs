use std::collections::BTreeMap;

use serde::Serialize;

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

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Pandoc {
    #[serde(rename = "pandoc-api-version")]
    pub pandoc_api_version: [u8; 2],
    pub meta: BTreeMap<String, MetaValue>,
    pub blocks: Vec<Block>,
}

impl Default for Pandoc {
    fn default() -> Self {
        Self {
            pandoc_api_version: [1, 23],
            meta: BTreeMap::new(),
            blocks: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "t", content = "c")]
pub enum Block {
    Plain(Vec<Inline>),
    Para(Vec<Inline>),
    CodeBlock(Attr, String),
    RawBlock(String, String),
    BlockQuote(Vec<Block>),
    OrderedList(ListAttributes, Vec<Vec<Block>>),
    BulletList(Vec<Vec<Block>>),
    Header(u8, Attr, Vec<Inline>),
    HorizontalRule,
    Table(Box<Table>),
    Figure(Figure),
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "t", content = "c")]
pub enum Inline {
    Str(String),
    Emph(Vec<Inline>),
    Strong(Vec<Inline>),
    Strikeout(Vec<Inline>),
    Superscript(Vec<Inline>),
    Subscript(Vec<Inline>),
    SmallCaps(Vec<Inline>),
    Quoted(QuoteType, Vec<Inline>),
    Code(Attr, String),
    Space,
    SoftBreak,
    LineBreak,
    Math(MathType, String),
    RawInline(String, String),
    Link(Attr, Vec<Inline>, Target),
    Image(Attr, Vec<Inline>, Target),
    Note(Vec<Block>),
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "t", content = "c")]
pub enum MetaValue {
    MetaMap(BTreeMap<String, MetaValue>),
    MetaList(Vec<MetaValue>),
    MetaBool(bool),
    MetaString(String),
    MetaInlines(Vec<Inline>),
    MetaBlocks(Vec<Block>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "t", content = "c")]
pub enum Alignment {
    AlignDefault,
    AlignLeft,
    AlignCenter,
    AlignRight,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(tag = "t", content = "c")]
pub enum ColWidth {
    ColWidthDefault,
    ColWidth(f64),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "t", content = "c")]
pub enum ListNumberDelim {
    DefaultDelim,
    Period,
    OneParen,
    TwoParens,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "t", content = "c")]
pub enum MathType {
    InlineMath,
    DisplayMath,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
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
    use super::{Alignment, Block, Pandoc, Table, empty_attr};

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
}
