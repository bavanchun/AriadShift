use std::collections::{BTreeMap, BTreeSet};

use base64::{Engine, engine::general_purpose::STANDARD};
use unicode_normalization::UnicodeNormalization;

use crate::{
    ir::{
        Alignment as IrAlignment, AssetRef, Block as IrBlock, Document, Inline as IrInline,
        ListItem, Metadata, RawFormat,
    },
    warning::{Warning, WarningCode},
};

use super::ast::{
    self, Alignment, Block, ColWidth, Inline, ListNumberDelim, ListNumberStyle, MathType,
    MetaValue, Pandoc, empty_attr,
};

const DOCX_PAGE_BREAK: &str = "<w:p><w:r><w:br w:type=\"page\"/></w:r></w:p>";

#[derive(Clone, Debug, PartialEq)]
pub struct MapOutput {
    pub pandoc: Pandoc,
    pub warnings: Vec<Warning>,
}

/// Maps AriadShift IR to the Pandoc 1.23 JSON AST used by the DOCX writer.
#[must_use]
pub fn from_ir(document: &Document) -> MapOutput {
    let mut footnotes = BTreeMap::new();
    collect_footnotes(&document.body, &mut footnotes);

    let mut mapper = Mapper {
        document,
        footnotes,
        used_footnotes: BTreeSet::new(),
        active_footnotes: BTreeSet::new(),
        warned_missing_footnotes: BTreeSet::new(),
        warnings: Vec::new(),
    };
    let blocks = mapper.map_blocks(&document.body);
    for id in mapper.footnotes.keys() {
        if !mapper.used_footnotes.contains(id) {
            mapper.warnings.push(Warning::new(
                WarningCode::FootnoteUnused,
                format!("footnote `{id}` is unused"),
            ));
        }
    }

    MapOutput {
        pandoc: Pandoc {
            meta: map_metadata(&document.meta),
            blocks,
            ..Pandoc::default()
        },
        warnings: mapper.warnings,
    }
}

struct Mapper<'a> {
    document: &'a Document,
    footnotes: BTreeMap<String, &'a [IrBlock]>,
    used_footnotes: BTreeSet<String>,
    active_footnotes: BTreeSet<String>,
    warned_missing_footnotes: BTreeSet<String>,
    warnings: Vec<Warning>,
}

impl Mapper<'_> {
    fn map_blocks(&mut self, blocks: &[IrBlock]) -> Vec<Block> {
        blocks
            .iter()
            .flat_map(|block| self.map_block(block))
            .collect()
    }

    fn map_block(&mut self, block: &IrBlock) -> Vec<Block> {
        match block {
            IrBlock::Heading { level, content } => {
                vec![Block::Header(
                    *level,
                    empty_attr(),
                    self.map_inlines(content),
                )]
            }
            IrBlock::Paragraph { content } => {
                vec![Block::Para(self.map_inlines(content))]
            }
            IrBlock::Code { lang, text } => {
                let classes = lang
                    .as_deref()
                    .filter(|language| !language.is_empty())
                    .map_or_else(Vec::new, |language| vec![language.to_owned()]);
                vec![Block::CodeBlock(
                    (String::new(), classes, Vec::new()),
                    text.clone(),
                )]
            }
            IrBlock::Math { tex, display } => {
                let math_type = if *display {
                    MathType::DisplayMath
                } else {
                    MathType::InlineMath
                };
                vec![Block::Para(vec![Inline::Math(math_type, tex.clone())])]
            }
            IrBlock::Quote { blocks } => vec![Block::BlockQuote(self.map_blocks(blocks))],
            IrBlock::PageBreak {} => vec![Block::RawBlock(
                "openxml".to_owned(),
                DOCX_PAGE_BREAK.to_owned(),
            )],
            IrBlock::List {
                ordered,
                start,
                tight,
                items,
            } => {
                let items = items
                    .iter()
                    .map(|item| self.map_list_item(item, *tight))
                    .collect();
                if *ordered {
                    vec![Block::OrderedList(
                        (
                            start.unwrap_or(1),
                            ListNumberStyle::Decimal,
                            ListNumberDelim::Period,
                        ),
                        items,
                    )]
                } else {
                    vec![Block::BulletList(items)]
                }
            }
            IrBlock::Table {
                caption,
                columns,
                head,
                body,
            } => {
                let caption = caption
                    .as_deref()
                    .map(|caption| (None, vec![Block::Plain(self.map_inlines(caption))]))
                    .unwrap_or_else(|| (None, Vec::new()));
                let colspecs = columns
                    .iter()
                    .map(|column| (map_alignment(column.align), ColWidth::ColWidthDefault))
                    .collect();
                let head = (
                    empty_attr(),
                    head.iter().map(|row| self.map_table_row(row)).collect(),
                );
                let body_rows = body.iter().map(|row| self.map_table_row(row)).collect();
                let body = vec![(empty_attr(), 0, Vec::new(), body_rows)];
                vec![Block::Table(Box::new((
                    empty_attr(),
                    caption,
                    colspecs,
                    head,
                    body,
                    (empty_attr(), Vec::new()),
                )))]
            }
            IrBlock::Figure { asset, caption } => {
                let mapped_caption = self.map_inlines(caption);
                if let Some(target) = self.embedded_asset_target(asset) {
                    let caption_blocks = if mapped_caption.is_empty() {
                        Vec::new()
                    } else {
                        vec![Block::Plain(mapped_caption)]
                    };
                    let pandoc_figure: ast::Figure = (
                        empty_attr(),
                        (None, caption_blocks),
                        vec![Block::Plain(vec![Inline::Image(
                            empty_attr(),
                            Vec::new(),
                            target,
                        )])],
                    );
                    vec![Block::Figure(pandoc_figure)]
                } else {
                    vec![Block::Para(mapped_caption)]
                }
            }
            IrBlock::Footnote { .. } => Vec::new(),
            IrBlock::Raw { format, text } => {
                self.warn_raw_dropped(*format, "block");
                vec![Block::RawBlock(raw_format(*format), text.clone())]
            }
        }
    }

    fn map_list_item(&mut self, item: &ListItem, tight: bool) -> Vec<Block> {
        let mut blocks = self.map_blocks(&item.blocks);
        if tight {
            for block in &mut blocks {
                if let Block::Para(content) = block {
                    *block = Block::Plain(std::mem::take(content));
                }
            }
        }
        if let Some(checked) = item.checked {
            let mut prefix = vec![
                Inline::Str(if checked { "☒" } else { "☐" }.to_owned()),
                Inline::Space,
            ];
            match blocks.first_mut() {
                Some(Block::Plain(content) | Block::Para(content)) => {
                    prefix.append(content);
                    *content = prefix;
                }
                _ => blocks.insert(0, Block::Plain(prefix)),
            }
        }
        blocks
    }

    fn map_table_row(&mut self, row: &[crate::ir::TableCell]) -> ast::Row {
        (
            empty_attr(),
            row.iter()
                .map(|cell| {
                    (
                        empty_attr(),
                        Alignment::AlignDefault,
                        u64::from(cell.rowspan),
                        u64::from(cell.colspan),
                        self.map_blocks(&cell.blocks),
                    )
                })
                .collect(),
        )
    }

    fn map_inlines(&mut self, inlines: &[IrInline]) -> Vec<Inline> {
        inlines
            .iter()
            .flat_map(|inline| self.map_inline(inline))
            .collect()
    }

    fn map_inline(&mut self, inline: &IrInline) -> Vec<Inline> {
        match inline {
            IrInline::Text { text } => text_inlines(text),
            IrInline::Emph { content } => vec![Inline::Emph(self.map_inlines(content))],
            IrInline::Strong { content } => vec![Inline::Strong(self.map_inlines(content))],
            IrInline::Strikeout { content } => {
                vec![Inline::Strikeout(self.map_inlines(content))]
            }
            IrInline::Superscript { content } => {
                vec![Inline::Superscript(self.map_inlines(content))]
            }
            IrInline::Subscript { content } => {
                vec![Inline::Subscript(self.map_inlines(content))]
            }
            IrInline::Code { text } => vec![Inline::Code(empty_attr(), text.clone())],
            IrInline::Link {
                url,
                title,
                content,
            } => {
                let content = self.map_inlines(content);
                if allowed_link(url) {
                    vec![Inline::Link(
                        empty_attr(),
                        content,
                        (
                            url.clone(),
                            title.as_deref().map(normalize_prose).unwrap_or_default(),
                        ),
                    )]
                } else {
                    self.warnings.push(Warning::new(
                        WarningCode::LinkDropped,
                        format!("link target `{url}` was reduced to its text"),
                    ));
                    content
                }
            }
            IrInline::Image { target, alt, title } => {
                if let Some(target) = self.embedded_asset_target(target) {
                    vec![Inline::Image(
                        empty_attr(),
                        text_inlines(alt),
                        (
                            target.0,
                            title.as_deref().map(normalize_prose).unwrap_or_default(),
                        ),
                    )]
                } else {
                    text_inlines(alt)
                }
            }
            IrInline::SoftBreak {} => vec![Inline::SoftBreak],
            IrInline::LineBreak {} => vec![Inline::LineBreak],
            IrInline::Math { tex, display } => vec![Inline::Math(
                if *display {
                    MathType::DisplayMath
                } else {
                    MathType::InlineMath
                },
                tex.clone(),
            )],
            IrInline::FootnoteRef { id } => self.map_footnote_reference(id),
            IrInline::Raw { format, text } => {
                self.warn_raw_dropped(*format, "inline");
                vec![Inline::RawInline(raw_format(*format), text.clone())]
            }
        }
    }

    fn map_footnote_reference(&mut self, id: &str) -> Vec<Inline> {
        let Some(blocks) = self.footnotes.get(id).copied() else {
            self.warn_missing_footnote(id, "has no definition");
            return text_inlines(&format!("[^{id}]"));
        };
        if !self.active_footnotes.insert(id.to_owned()) {
            self.warn_missing_footnote(id, "contains a recursive reference");
            return text_inlines(&format!("[^{id}]"));
        }
        self.used_footnotes.insert(id.to_owned());
        let mapped = self.map_blocks(blocks);
        self.active_footnotes.remove(id);
        vec![Inline::Note(mapped)]
    }

    fn warn_missing_footnote(&mut self, id: &str, reason: &str) {
        if self.warned_missing_footnotes.insert(id.to_owned()) {
            self.warnings.push(Warning::new(
                WarningCode::FootnoteMissing,
                format!("footnote `{id}` {reason}; its marker was preserved"),
            ));
        }
    }

    fn embedded_asset_target(&mut self, asset: &AssetRef) -> Option<(String, String)> {
        let AssetRef::Asset { id } = asset else {
            self.warnings.push(Warning::new(
                WarningCode::ImageNotEmbedded,
                "image URL was reduced to its alt text because it is not embedded",
            ));
            return None;
        };
        let Some(asset) = self.document.assets.get(id) else {
            self.warnings.push(Warning::new(
                WarningCode::ImageNotEmbedded,
                format!("asset `{id}` was not found; image was reduced to its alt text"),
            ));
            return None;
        };
        Some((
            format!(
                "data:{};base64,{}",
                asset.media_type,
                STANDARD.encode(&asset.bytes)
            ),
            String::new(),
        ))
    }

    fn warn_raw_dropped(&mut self, format: RawFormat, location: &str) {
        let format = raw_format(format);
        self.warnings.push(Warning::new(
            WarningCode::RawDropped,
            format!(
                "raw {format} {location} is represented in the Pandoc AST but the DOCX writer drops it"
            ),
        ));
    }
}

fn map_metadata(metadata: &Metadata) -> BTreeMap<String, MetaValue> {
    let mut meta = BTreeMap::new();
    if let Some(title) = &metadata.title {
        meta.insert(
            "title".to_owned(),
            MetaValue::MetaInlines(text_inlines(title)),
        );
    }
    if !metadata.authors.is_empty() {
        meta.insert(
            "author".to_owned(),
            MetaValue::MetaList(
                metadata
                    .authors
                    .iter()
                    .map(|author| MetaValue::MetaInlines(text_inlines(author)))
                    .collect(),
            ),
        );
    }
    for (key, value) in [
        ("lang", metadata.language.as_deref()),
        ("date", metadata.date.as_deref()),
        ("subject", metadata.subject.as_deref()),
    ] {
        if let Some(value) = value {
            meta.insert(key.to_owned(), MetaValue::MetaInlines(text_inlines(value)));
        }
    }
    if !metadata.keywords.is_empty() {
        meta.insert(
            "keywords".to_owned(),
            MetaValue::MetaList(
                metadata
                    .keywords
                    .iter()
                    .map(|keyword| MetaValue::MetaInlines(text_inlines(keyword)))
                    .collect(),
            ),
        );
    }
    meta
}

fn text_inlines(text: &str) -> Vec<Inline> {
    let normalized = normalize_prose(text);
    let mut inlines = Vec::new();
    let mut word = String::new();
    for character in normalized.chars() {
        if character == ' ' {
            if !word.is_empty() {
                inlines.push(Inline::Str(std::mem::take(&mut word)));
            }
            inlines.push(Inline::Space);
        } else {
            word.push(character);
        }
    }
    if !word.is_empty() {
        inlines.push(Inline::Str(word));
    }
    inlines
}

fn normalize_prose(text: &str) -> String {
    text.nfc().collect()
}

fn map_alignment(alignment: IrAlignment) -> Alignment {
    match alignment {
        IrAlignment::Default => Alignment::AlignDefault,
        IrAlignment::Left => Alignment::AlignLeft,
        IrAlignment::Center => Alignment::AlignCenter,
        IrAlignment::Right => Alignment::AlignRight,
    }
}

fn raw_format(format: RawFormat) -> String {
    match format {
        RawFormat::Html => "html",
        RawFormat::Tex => "tex",
    }
    .to_owned()
}

fn allowed_link(url: &str) -> bool {
    if url.starts_with('#') {
        return true;
    }
    if url.trim() != url {
        return false;
    }
    let Some((scheme, _)) = url.split_once(':') else {
        return false;
    };
    let mut chars = scheme.chars();
    if !chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic())
        || !chars.all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '+' | '-' | '.')
        })
    {
        return false;
    }
    scheme.eq_ignore_ascii_case("http")
        || scheme.eq_ignore_ascii_case("https")
        || scheme.eq_ignore_ascii_case("mailto")
}

fn collect_footnotes<'a>(blocks: &'a [IrBlock], footnotes: &mut BTreeMap<String, &'a [IrBlock]>) {
    let mut pending: Vec<&'a IrBlock> = blocks.iter().rev().collect();
    while let Some(block) = pending.pop() {
        match block {
            IrBlock::Quote { blocks } => pending.extend(blocks.iter().rev()),
            IrBlock::List { items, .. } => {
                pending.extend(items.iter().flat_map(|item| item.blocks.iter().rev()));
            }
            IrBlock::Table { head, body, .. } => {
                pending.extend(
                    head.iter()
                        .chain(body)
                        .flatten()
                        .flat_map(|cell| cell.blocks.iter().rev()),
                );
            }
            IrBlock::Footnote { id, blocks } => {
                footnotes.entry(id.clone()).or_insert(blocks);
                pending.extend(blocks.iter().rev());
            }
            IrBlock::Heading { .. }
            | IrBlock::Paragraph { .. }
            | IrBlock::Code { .. }
            | IrBlock::Math { .. }
            | IrBlock::PageBreak {}
            | IrBlock::Figure { .. }
            | IrBlock::Raw { .. } => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::from_ir;
    use crate::{
        ir::{
            Alignment as IrAlignment, Asset, AssetRef, Block as IrBlock, Document,
            Inline as IrInline, ListItem, Metadata, RawFormat, TableCell,
        },
        pandoc::ast::{
            Alignment, Block, Inline, ListNumberDelim, ListNumberStyle, MathType, MetaValue,
        },
        warning::WarningCode,
    };

    #[test]
    fn maps_all_metadata_and_normalizes_its_prose() {
        let document = Document {
            meta: Metadata {
                title: Some("Cafe\u{301} title".to_owned()),
                authors: vec!["Author One".to_owned(), "Author Two".to_owned()],
                language: Some("en".to_owned()),
                date: Some("2026-10-06".to_owned()),
                subject: Some("A subject".to_owned()),
                keywords: vec!["one".to_owned(), "two".to_owned()],
                source_format: None,
            },
            ..Document::default()
        };

        let output = from_ir(&document);
        assert_eq!(
            output.pandoc.meta["title"],
            MetaValue::MetaInlines(vec![
                Inline::Str("Café".to_owned()),
                Inline::Space,
                Inline::Str("title".to_owned()),
            ])
        );
        assert!(
            matches!(&output.pandoc.meta["author"], MetaValue::MetaList(authors) if authors.len() == 2)
        );
        for key in ["lang", "date", "subject", "keywords"] {
            assert!(output.pandoc.meta.contains_key(key));
        }
        assert!(output.warnings.is_empty());
    }

    #[test]
    fn maps_block_and_inline_shapes() {
        let document = Document {
            body: vec![
                IrBlock::Heading {
                    level: 2,
                    content: vec![IrInline::Text {
                        text: "A title".to_owned(),
                    }],
                },
                IrBlock::Paragraph {
                    content: vec![
                        IrInline::Emph {
                            content: vec![IrInline::Text {
                                text: "em".to_owned(),
                            }],
                        },
                        IrInline::Strong {
                            content: vec![IrInline::Text {
                                text: "strong".to_owned(),
                            }],
                        },
                        IrInline::Strikeout {
                            content: vec![IrInline::Text {
                                text: "strike".to_owned(),
                            }],
                        },
                        IrInline::Superscript {
                            content: vec![IrInline::Text {
                                text: "up".to_owned(),
                            }],
                        },
                        IrInline::Subscript {
                            content: vec![IrInline::Text {
                                text: "down".to_owned(),
                            }],
                        },
                        IrInline::Code {
                            text: "Cafe\u{301}".to_owned(),
                        },
                        IrInline::SoftBreak {},
                        IrInline::LineBreak {},
                        IrInline::Math {
                            tex: "x^2".to_owned(),
                            display: false,
                        },
                        IrInline::Raw {
                            format: RawFormat::Html,
                            text: "<i>raw</i>".to_owned(),
                        },
                    ],
                },
                IrBlock::Code {
                    lang: Some("rust".to_owned()),
                    text: "fn main() {}".to_owned(),
                },
                IrBlock::Math {
                    tex: "x = 2".to_owned(),
                    display: true,
                },
                IrBlock::Quote {
                    blocks: vec![IrBlock::Paragraph {
                        content: vec![IrInline::Text {
                            text: "quoted".to_owned(),
                        }],
                    }],
                },
                IrBlock::PageBreak {},
                IrBlock::Raw {
                    format: RawFormat::Tex,
                    text: "\\emph{x}".to_owned(),
                },
            ],
            ..Document::default()
        };

        let output = from_ir(&document);
        assert!(matches!(output.pandoc.blocks[0], Block::Header(2, _, _)));
        assert!(matches!(output.pandoc.blocks[1], Block::Para(_)));
        assert!(
            matches!(&output.pandoc.blocks[2], Block::CodeBlock((_, classes, _), _) if classes == &["rust"])
        );
        assert!(
            matches!(&output.pandoc.blocks[3], Block::Para(content) if matches!(content.as_slice(), [Inline::Math(MathType::DisplayMath, _)]))
        );
        assert!(matches!(output.pandoc.blocks[4], Block::BlockQuote(_)));
        assert!(
            matches!(&output.pandoc.blocks[5], Block::RawBlock(format, text) if format == "openxml" && text.contains("w:type=\"page\""))
        );
        assert!(matches!(&output.pandoc.blocks[6], Block::RawBlock(format, _) if format == "tex"));
        assert!(
            output
                .warnings
                .iter()
                .filter(|warning| warning.code == WarningCode::RawDropped)
                .count()
                == 2
        );
        let serialized = serde_json::to_string(&output.pandoc).unwrap();
        assert!(serialized.contains(r#""t":"CodeBlock""#));
        assert!(serialized.contains(r#""t":"Math""#));
        assert!(serialized.contains(r#""t":"RawInline","c":["html","<i>raw</i>"]"#));
        assert!(serialized.contains("openxml"));
    }

    #[test]
    fn maps_tight_and_ordered_lists_with_task_prefixes() {
        let document = Document {
            body: vec![
                IrBlock::List {
                    ordered: false,
                    start: None,
                    tight: true,
                    items: vec![ListItem {
                        checked: Some(true),
                        blocks: vec![IrBlock::Paragraph {
                            content: vec![IrInline::Text {
                                text: "done".to_owned(),
                            }],
                        }],
                    }],
                },
                IrBlock::List {
                    ordered: true,
                    start: Some(3),
                    tight: false,
                    items: vec![ListItem {
                        checked: None,
                        blocks: vec![IrBlock::Paragraph {
                            content: vec![IrInline::Text {
                                text: "item".to_owned(),
                            }],
                        }],
                    }],
                },
            ],
            ..Document::default()
        };

        let output = from_ir(&document);
        assert!(
            matches!(&output.pandoc.blocks[0], Block::BulletList(items) if matches!(&items[0][0], Block::Plain(content) if content.starts_with(&[Inline::Str("☒".to_owned()), Inline::Space])))
        );
        assert!(matches!(
            &output.pandoc.blocks[1],
            Block::OrderedList((3, ListNumberStyle::Decimal, ListNumberDelim::Period), _)
        ));
    }

    #[test]
    fn maps_table_alignment_spans_caption_and_figures() {
        let mut document = Document {
            body: vec![
                IrBlock::Table {
                    caption: Some(vec![IrInline::Text {
                        text: "Caption".to_owned(),
                    }]),
                    columns: vec![crate::ir::ColumnSpec {
                        align: IrAlignment::Center,
                    }],
                    head: vec![vec![TableCell {
                        rowspan: 1,
                        colspan: 2,
                        blocks: vec![IrBlock::Paragraph {
                            content: vec![IrInline::Text {
                                text: "head".to_owned(),
                            }],
                        }],
                    }]],
                    body: vec![vec![TableCell {
                        rowspan: 2,
                        colspan: 1,
                        blocks: vec![IrBlock::Paragraph {
                            content: vec![IrInline::Text {
                                text: "body".to_owned(),
                            }],
                        }],
                    }]],
                },
                IrBlock::Figure {
                    asset: AssetRef::Asset {
                        id: "figure".to_owned(),
                    },
                    caption: vec![IrInline::Text {
                        text: "Figure caption".to_owned(),
                    }],
                },
            ],
            ..Document::default()
        };
        document.assets.insert(
            "figure".to_owned(),
            Asset {
                media_type: "image/png".to_owned(),
                bytes: vec![1, 2, 3],
            },
        );

        let output = from_ir(&document);
        assert!(
            matches!(&output.pandoc.blocks[0], Block::Table(table) if table.2 == [(Alignment::AlignCenter, crate::pandoc::ast::ColWidth::ColWidthDefault)] && table.3.1[0].1[0].3 == 2 && table.4[0].3[0].1[0].2 == 2)
        );
        assert!(
            matches!(&output.pandoc.blocks[1], Block::Figure((_, (_, caption), blocks)) if caption.len() == 1 && matches!(blocks.as_slice(), [Block::Plain(inlines)] if matches!(inlines.as_slice(), [Inline::Image(_, _, (url, _))] if url == "data:image/png;base64,AQID")))
        );
        assert!(output.warnings.is_empty());
    }

    #[test]
    fn embeds_assets_and_reduces_url_images_to_alt_text() {
        let mut document = Document {
            body: vec![IrBlock::Paragraph {
                content: vec![
                    IrInline::Image {
                        target: AssetRef::Asset {
                            id: "asset".to_owned(),
                        },
                        alt: "embedded".to_owned(),
                        title: Some("title".to_owned()),
                    },
                    IrInline::Image {
                        target: AssetRef::Url {
                            href: "https://example.test/image.png".to_owned(),
                        },
                        alt: "remote alt".to_owned(),
                        title: None,
                    },
                ],
            }],
            ..Document::default()
        };
        document.assets.insert(
            "asset".to_owned(),
            Asset {
                media_type: "image/png".to_owned(),
                bytes: vec![0, 1, 2, 255],
            },
        );

        let output = from_ir(&document);
        assert!(
            matches!(&output.pandoc.blocks[0], Block::Para(content) if content.len() == 4
                && matches!(&content[0], Inline::Image(_, alt, (url, title)) if matches!(alt.as_slice(), [Inline::Str(text)] if text == "embedded") && url == "data:image/png;base64,AAEC/w==" && title == "title")
                && matches!(&content[1], Inline::Str(text) if text == "remote")
                && matches!(&content[2], Inline::Space)
                && matches!(&content[3], Inline::Str(text) if text == "alt"))
        );
        assert!(
            output
                .warnings
                .iter()
                .any(|warning| warning.code == WarningCode::ImageNotEmbedded)
        );
    }

    #[test]
    fn enforces_the_link_scheme_allow_list() {
        let targets = [
            "http://example.test",
            "https://example.test",
            "mailto:user@example.test",
            "#section",
        ];
        let mut content = targets
            .iter()
            .map(|target| IrInline::Link {
                url: (*target).to_owned(),
                title: None,
                content: vec![IrInline::Text {
                    text: "safe".to_owned(),
                }],
            })
            .collect::<Vec<_>>();
        for target in ["file:///etc/passwd", "javascript:alert(1)", r"\\host\share"] {
            content.push(IrInline::Link {
                url: target.to_owned(),
                title: None,
                content: vec![IrInline::Text {
                    text: "shown".to_owned(),
                }],
            });
        }
        let output = from_ir(&Document {
            body: vec![IrBlock::Paragraph { content }],
            ..Document::default()
        });

        assert!(
            matches!(&output.pandoc.blocks[0], Block::Para(content) if content.iter().filter(|inline| matches!(inline, Inline::Link(..))).count() == 4 && content.iter().any(|inline| matches!(inline, Inline::Str(text) if text == "shown")))
        );
        assert_eq!(
            output
                .warnings
                .iter()
                .filter(|warning| warning.code == WarningCode::LinkDropped)
                .count(),
            3
        );
        let serialized = serde_json::to_string(&output.pandoc).unwrap();
        assert!(!serialized.contains("file:///etc/passwd"));
        assert!(!serialized.contains("javascript:alert"));
        assert!(!serialized.contains(r"\\host\share"));
    }

    #[test]
    fn maps_footnotes_and_preserves_missing_markers() {
        let document = Document {
            body: vec![
                IrBlock::Paragraph {
                    content: vec![
                        IrInline::Text {
                            text: "note".to_owned(),
                        },
                        IrInline::FootnoteRef {
                            id: "used".to_owned(),
                        },
                        IrInline::FootnoteRef {
                            id: "missing".to_owned(),
                        },
                    ],
                },
                IrBlock::Footnote {
                    id: "used".to_owned(),
                    blocks: vec![IrBlock::Paragraph {
                        content: vec![IrInline::Text {
                            text: "defined".to_owned(),
                        }],
                    }],
                },
                IrBlock::Footnote {
                    id: "unused".to_owned(),
                    blocks: Vec::new(),
                },
            ],
            ..Document::default()
        };

        let output = from_ir(&document);
        assert!(
            matches!(&output.pandoc.blocks[0], Block::Para(content) if content.iter().any(|inline| matches!(inline, Inline::Note(_))) && content.iter().any(|inline| matches!(inline, Inline::Str(text) if text == "[^missing]")))
        );
        assert!(
            output
                .warnings
                .iter()
                .any(|warning| warning.code == WarningCode::FootnoteMissing)
        );
        assert!(
            output
                .warnings
                .iter()
                .any(|warning| warning.code == WarningCode::FootnoteUnused)
        );
        assert!(
            output.pandoc.blocks.len() == 1,
            "footnote definitions are not emitted outside their references"
        );
    }

    #[test]
    fn emits_exact_json_for_a_paragraph_and_reduces_unsafe_links() {
        let output = from_ir(&Document {
            body: vec![IrBlock::Paragraph {
                content: vec![
                    IrInline::Text {
                        text: "Hello world".to_owned(),
                    },
                    IrInline::Link {
                        url: "https://example.test".to_owned(),
                        title: None,
                        content: vec![IrInline::Text {
                            text: "site".to_owned(),
                        }],
                    },
                ],
            }],
            ..Document::default()
        });
        assert_eq!(
            serde_json::to_string(&output.pandoc).unwrap(),
            r#"{"pandoc-api-version":[1,23],"meta":{},"blocks":[{"t":"Para","c":[{"t":"Str","c":"Hello"},{"t":"Space"},{"t":"Str","c":"world"},{"t":"Link","c":[["",[],[]],[{"t":"Str","c":"site"}],["https://example.test",""]]}]}]}"#
        );
    }
}
