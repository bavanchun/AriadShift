use std::collections::BTreeSet;

use thiserror::Error;
use unicode_normalization::UnicodeNormalization;

use crate::{
    ir::{
        Alignment as IrAlignment, AssetRef, Block as IrBlock, ColumnSpec, Document,
        Inline as IrInline, ListItem, Metadata, RawFormat, TableCell,
    },
    limits::{Limits, LimitsError},
    links::allowed_link,
    warning::{Warning, WarningCode},
};

use super::ast::{
    Alignment, Block, Caption, Figure, Inline, MathType, MetaValue, Pandoc, QuoteType, Row, Table,
};

#[derive(Clone, Debug, PartialEq)]
pub struct ToIrOutput {
    pub document: Document,
    pub warnings: Vec<Warning>,
}

pub type MapOutput = ToIrOutput;

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum MapError {
    #[error(transparent)]
    InvalidLimits(#[from] LimitsError),
    #[error("unsupported Pandoc API version: {version:?}; expected 1.23.*")]
    UnsupportedApiVersion { version: Vec<u32> },
    #[error("Pandoc document nesting exceeds the configured depth of {limit}")]
    NestingTooDeep { limit: u16 },
    #[error("Pandoc document contains more than the configured {limit} block limit")]
    TooManyBlocks { limit: u32 },
}

/// Maps Pandoc 1.23 AST JSON to AriadShift IR.
pub fn to_ir(pandoc: &Pandoc, limits: &Limits) -> Result<ToIrOutput, MapError> {
    limits.validate().map_err(MapError::InvalidLimits)?;

    // Reject any api version other than 1.23.*
    if !matches!(pandoc.pandoc_api_version.as_slice(), [1, 23, ..]) {
        return Err(MapError::UnsupportedApiVersion {
            version: pandoc.pandoc_api_version.clone(),
        });
    }

    let mut mapper = ToIrMapper::new(limits);
    let mut body = mapper.map_blocks(&pandoc.blocks, 1)?;
    body.extend(mapper.footnotes);

    let meta = map_metadata(&pandoc.meta);

    Ok(ToIrOutput {
        document: Document {
            meta,
            body,
            ..Document::default()
        },
        warnings: mapper.warnings,
    })
}

struct ToIrMapper<'a> {
    limits: &'a Limits,
    block_count: u32,
    footnotes: Vec<IrBlock>,
    warnings: Vec<Warning>,
}

impl<'a> ToIrMapper<'a> {
    fn new(limits: &'a Limits) -> Self {
        Self {
            limits,
            block_count: 0,
            footnotes: Vec::new(),
            warnings: Vec::new(),
        }
    }

    fn check_block_budget(&mut self) -> Result<(), MapError> {
        self.block_count = self.block_count.saturating_add(1);
        if self.block_count > self.limits.max_blocks {
            return Err(MapError::TooManyBlocks {
                limit: self.limits.max_blocks,
            });
        }
        Ok(())
    }

    fn check_depth(&self, depth: u16) -> Result<(), MapError> {
        if depth > self.limits.max_nesting_depth {
            return Err(MapError::NestingTooDeep {
                limit: self.limits.max_nesting_depth,
            });
        }
        Ok(())
    }

    fn map_blocks(&mut self, blocks: &[Block], depth: u16) -> Result<Vec<IrBlock>, MapError> {
        self.check_depth(depth)?;
        let mut result = Vec::new();
        for block in blocks {
            let mapped = self.map_block(block, depth)?;
            result.extend(mapped);
        }
        Ok(result)
    }

    fn map_block(&mut self, block: &Block, depth: u16) -> Result<Vec<IrBlock>, MapError> {
        self.check_depth(depth)?;

        match block {
            Block::Plain(inlines) | Block::Para(inlines) => {
                self.check_block_budget()?;
                let content = self.map_inlines(inlines, depth)?;
                Ok(vec![IrBlock::Paragraph { content }])
            }
            Block::LineBlock(lines) => {
                self.check_block_budget()?;
                self.warnings.push(Warning::new(
                    WarningCode::UnsupportedNode,
                    "line block was mapped to a paragraph with line breaks",
                ));
                let mut content = Vec::new();
                for (index, line) in lines.iter().enumerate() {
                    if index > 0 {
                        content.push(IrInline::LineBreak {});
                    }
                    content.extend(self.map_inlines(line, depth)?);
                }
                Ok(vec![IrBlock::Paragraph { content }])
            }
            Block::CodeBlock(attr, text) => {
                self.check_block_budget()?;
                let lang = attr.1.first().cloned();
                Ok(vec![IrBlock::Code {
                    lang,
                    text: text.clone(),
                }])
            }
            Block::RawBlock(format, text) => {
                if format.eq_ignore_ascii_case("html") {
                    self.check_block_budget()?;
                    Ok(vec![IrBlock::Raw {
                        format: RawFormat::Html,
                        text: text.clone(),
                    }])
                } else if format.eq_ignore_ascii_case("tex") || format.eq_ignore_ascii_case("latex")
                {
                    self.check_block_budget()?;
                    Ok(vec![IrBlock::Raw {
                        format: RawFormat::Tex,
                        text: text.clone(),
                    }])
                } else {
                    self.warnings.push(Warning::new(
                        WarningCode::RawDropped,
                        format!("raw {format} block was dropped"),
                    ));
                    Ok(Vec::new())
                }
            }
            Block::BlockQuote(blocks) => {
                self.check_block_budget()?;
                let mapped_blocks = self.map_blocks(blocks, depth.saturating_add(1))?;
                Ok(vec![IrBlock::Quote {
                    blocks: mapped_blocks,
                }])
            }
            Block::OrderedList((start, _style, _delim), items) => {
                self.check_block_budget()?;
                let mut mapped_items = Vec::with_capacity(items.len());
                for item_blocks in items {
                    let blocks = self.map_blocks(item_blocks, depth.saturating_add(1))?;
                    mapped_items.push(ListItem {
                        checked: None,
                        blocks,
                    });
                }
                Ok(vec![IrBlock::List {
                    ordered: true,
                    start: Some(*start),
                    tight: false,
                    items: mapped_items,
                }])
            }
            Block::BulletList(items) => {
                self.check_block_budget()?;
                let mut mapped_items = Vec::with_capacity(items.len());
                for item_blocks in items {
                    let mut blocks = self.map_blocks(item_blocks, depth.saturating_add(1))?;
                    let checked = detect_task_checkbox(&mut blocks);
                    mapped_items.push(ListItem { checked, blocks });
                }
                Ok(vec![IrBlock::List {
                    ordered: false,
                    start: None,
                    tight: false,
                    items: mapped_items,
                }])
            }
            Block::DefinitionList(entries) => {
                self.check_block_budget()?;
                self.warnings.push(Warning::new(
                    WarningCode::UnsupportedNode,
                    "definition list was mapped to a bullet list of strong terms",
                ));
                let mut items = Vec::with_capacity(entries.len());
                for (term, def_blocks_list) in entries {
                    let term_inlines = self.map_inlines(term, depth)?;
                    let term_para = IrBlock::Paragraph {
                        content: vec![IrInline::Strong {
                            content: term_inlines,
                        }],
                    };
                    self.check_block_budget()?;
                    let mut blocks = vec![term_para];
                    for def_blocks in def_blocks_list {
                        let mapped = self.map_blocks(def_blocks, depth.saturating_add(1))?;
                        blocks.extend(mapped);
                    }
                    items.push(ListItem {
                        checked: None,
                        blocks,
                    });
                }
                Ok(vec![IrBlock::List {
                    ordered: false,
                    start: None,
                    tight: false,
                    items,
                }])
            }
            Block::Header(level, _attr, inlines) => {
                self.check_block_budget()?;
                let content = self.map_inlines(inlines, depth)?;
                Ok(vec![IrBlock::Heading {
                    level: *level,
                    content,
                }])
            }
            Block::HorizontalRule => {
                self.check_block_budget()?;
                self.warnings.push(Warning::new(
                    WarningCode::UnsupportedNode,
                    "thematic break was represented as a horizontal rule",
                ));
                Ok(vec![IrBlock::Paragraph {
                    content: vec![IrInline::Text {
                        text: "———".to_owned(),
                    }],
                }])
            }
            Block::Table(table) => {
                self.check_block_budget()?;
                let mapped = self.map_table(table, depth)?;
                Ok(vec![mapped])
            }
            Block::Figure(figure) => {
                self.check_block_budget()?;
                self.map_figure(figure, depth)
            }
            Block::Div(_attr, blocks) => {
                // Div flattens; child blocks are evaluated at depth + 1
                self.map_blocks(blocks, depth.saturating_add(1))
            }
        }
    }

    fn map_table(&mut self, table: &Table, depth: u16) -> Result<IrBlock, MapError> {
        let (_attr, caption, colspecs, head, body, foot) = table;

        let caption_inlines = self.map_caption(caption, depth)?;

        let columns = colspecs
            .iter()
            .map(|(align, _width)| ColumnSpec {
                align: map_alignment(*align),
            })
            .collect();

        let child_depth = depth.saturating_add(1);
        let head_rows = self.map_table_rows(&head.1, true, child_depth)?;

        let mut body_rows = Vec::new();
        for body_section in body {
            let intermediate_head = self.map_table_rows(&body_section.2, true, child_depth)?;
            body_rows.extend(intermediate_head);
            let regular_body = self.map_table_rows(&body_section.3, false, child_depth)?;
            body_rows.extend(regular_body);
        }

        let foot_rows = self.map_table_rows(&foot.1, false, child_depth)?;
        body_rows.extend(foot_rows);

        Ok(IrBlock::Table {
            caption: caption_inlines,
            columns,
            head: head_rows,
            body: body_rows,
            footnotes: Vec::new(),
        })
    }

    fn map_table_rows(
        &mut self,
        rows: &[Row],
        is_header: bool,
        depth: u16,
    ) -> Result<Vec<Vec<TableCell>>, MapError> {
        let mut mapped_rows = Vec::with_capacity(rows.len());
        for row in rows {
            let mut mapped_cells = Vec::with_capacity(row.1.len());
            for cell in &row.1 {
                let (_attr, _align, rowspan, colspan, blocks) = cell;
                let mapped_blocks = self.map_blocks(blocks, depth.saturating_add(1))?;
                mapped_cells.push(TableCell {
                    rowspan: (*rowspan as u32).max(1),
                    colspan: (*colspan as u32).max(1),
                    header: if is_header { Some(true) } else { None },
                    blocks: mapped_blocks,
                });
            }
            mapped_rows.push(mapped_cells);
        }
        Ok(mapped_rows)
    }

    fn map_caption(
        &mut self,
        caption: &Caption,
        depth: u16,
    ) -> Result<Option<Vec<IrInline>>, MapError> {
        if let Some(short) = &caption.0
            && !short.is_empty()
        {
            return Ok(Some(self.map_inlines(short, depth)?));
        }
        if !caption.1.is_empty() {
            let mut inlines = Vec::new();
            for block in &caption.1 {
                match block {
                    Block::Plain(ins) | Block::Para(ins) => {
                        inlines.extend(self.map_inlines(ins, depth)?);
                    }
                    _ => {}
                }
            }
            if !inlines.is_empty() {
                return Ok(Some(inlines));
            }
        }
        Ok(None)
    }

    fn map_figure(&mut self, figure: &Figure, depth: u16) -> Result<Vec<IrBlock>, MapError> {
        let (_attr, caption, blocks) = figure;
        let caption_inlines = self.map_caption(caption, depth)?.unwrap_or_default();

        // Check if figure body directly wraps an image
        for block in blocks {
            let inlines = match block {
                Block::Plain(inlines) | Block::Para(inlines) => inlines,
                _ => continue,
            };
            for inline in inlines {
                if let Inline::Image(_img_attr, alt, target) = inline {
                    let caption = if caption_inlines.is_empty() {
                        self.map_inlines(alt, depth)?
                    } else {
                        caption_inlines
                    };
                    return Ok(vec![IrBlock::Figure {
                        asset: AssetRef::Url {
                            href: target.0.clone(),
                        },
                        caption,
                    }]);
                }
            }
        }

        // General figure without image: map child blocks
        let mapped_blocks = self.map_blocks(blocks, depth.saturating_add(1))?;
        if !caption_inlines.is_empty() {
            let mut result = mapped_blocks;
            self.check_block_budget()?;
            result.push(IrBlock::Paragraph {
                content: caption_inlines,
            });
            Ok(result)
        } else {
            Ok(mapped_blocks)
        }
    }

    fn map_inlines(&mut self, inlines: &[Inline], depth: u16) -> Result<Vec<IrInline>, MapError> {
        let mut result = Vec::new();
        for inline in inlines {
            let mapped = self.map_inline(inline, depth)?;
            result.extend(mapped);
        }
        Ok(result)
    }

    fn map_inline(&mut self, inline: &Inline, depth: u16) -> Result<Vec<IrInline>, MapError> {
        match inline {
            Inline::Str(text) => {
                let normalized: String = text.nfc().collect();
                Ok(vec![IrInline::Text { text: normalized }])
            }
            Inline::Emph(inlines) => {
                let content = self.map_inlines(inlines, depth)?;
                Ok(vec![IrInline::Emph { content }])
            }
            Inline::Underline(inlines) => {
                self.warnings.push(Warning::new(
                    WarningCode::UnsupportedNode,
                    "underline is not supported in IR and was mapped to emphasis",
                ));
                let content = self.map_inlines(inlines, depth)?;
                Ok(vec![IrInline::Emph { content }])
            }
            Inline::Strong(inlines) => {
                let content = self.map_inlines(inlines, depth)?;
                Ok(vec![IrInline::Strong { content }])
            }
            Inline::Strikeout(inlines) => {
                let content = self.map_inlines(inlines, depth)?;
                Ok(vec![IrInline::Strikeout { content }])
            }
            Inline::Superscript(inlines) => {
                let content = self.map_inlines(inlines, depth)?;
                Ok(vec![IrInline::Superscript { content }])
            }
            Inline::Subscript(inlines) => {
                let content = self.map_inlines(inlines, depth)?;
                Ok(vec![IrInline::Subscript { content }])
            }
            Inline::SmallCaps(inlines) => {
                self.warnings.push(Warning::new(
                    WarningCode::UnsupportedNode,
                    "small caps formatting is not supported in IR and was degraded to text",
                ));
                self.map_inlines(inlines, depth)
            }
            Inline::Quoted(quote_type, inlines) => {
                let inner = self.map_inlines(inlines, depth)?;
                let (open, close) = match quote_type {
                    QuoteType::SingleQuote => ("'", "'"),
                    QuoteType::DoubleQuote => ("\"", "\""),
                };
                let mut result = vec![IrInline::Text {
                    text: open.to_owned(),
                }];
                result.extend(inner);
                result.push(IrInline::Text {
                    text: close.to_owned(),
                });
                Ok(result)
            }
            Inline::Cite(_citations, inlines) => {
                self.warnings.push(Warning::new(
                    WarningCode::UnsupportedNode,
                    "citation is not supported in IR and was degraded to inline text",
                ));
                self.map_inlines(inlines, depth)
            }
            Inline::Code(_attr, text) => Ok(vec![IrInline::Code { text: text.clone() }]),
            Inline::Space => Ok(vec![IrInline::Text {
                text: " ".to_owned(),
            }]),
            Inline::SoftBreak => Ok(vec![IrInline::SoftBreak {}]),
            Inline::LineBreak => Ok(vec![IrInline::LineBreak {}]),
            Inline::Math(math_type, tex) => {
                let display = matches!(math_type, MathType::DisplayMath);
                Ok(vec![IrInline::Math {
                    tex: tex.clone(),
                    display,
                }])
            }
            Inline::RawInline(format, text) => {
                if format.eq_ignore_ascii_case("html") {
                    Ok(vec![IrInline::Raw {
                        format: RawFormat::Html,
                        text: text.clone(),
                    }])
                } else if format.eq_ignore_ascii_case("tex") || format.eq_ignore_ascii_case("latex")
                {
                    Ok(vec![IrInline::Raw {
                        format: RawFormat::Tex,
                        text: text.clone(),
                    }])
                } else {
                    self.warnings.push(Warning::new(
                        WarningCode::RawDropped,
                        format!("raw {format} inline was dropped"),
                    ));
                    Ok(Vec::new())
                }
            }
            Inline::Link(_attr, inlines, target) => {
                let url = &target.0;
                if allowed_link(url) {
                    let title = if target.1.is_empty() {
                        None
                    } else {
                        Some(target.1.nfc().collect())
                    };
                    let content = self.map_inlines(inlines, depth)?;
                    Ok(vec![IrInline::Link {
                        url: url.clone(),
                        title,
                        content,
                    }])
                } else {
                    self.warnings.push(Warning::new(
                        WarningCode::LinkDropped,
                        format!("link `{url}` dropped because its scheme is not allowed"),
                    ));
                    self.map_inlines(inlines, depth)
                }
            }
            Inline::Image(_attr, inlines, target) => {
                let alt: String = inlines_to_text(inlines).nfc().collect();
                let title = if target.1.is_empty() {
                    None
                } else {
                    Some(target.1.nfc().collect())
                };
                Ok(vec![IrInline::Image {
                    target: AssetRef::Url {
                        href: target.0.clone(),
                    },
                    alt,
                    title,
                }])
            }
            Inline::Note(blocks) => {
                let id = format!("fn{}", self.footnotes.len() + 1);
                self.check_block_budget()?;
                let mapped_blocks = self.map_blocks(blocks, depth.saturating_add(1))?;
                self.footnotes.push(IrBlock::Footnote {
                    id: id.clone(),
                    blocks: mapped_blocks,
                });
                Ok(vec![IrInline::FootnoteRef { id }])
            }
            Inline::Span(_attr, inlines) => {
                // Span flattens
                self.map_inlines(inlines, depth)
            }
        }
    }
}

fn detect_task_checkbox(blocks: &mut [IrBlock]) -> Option<bool> {
    let first_para = match blocks.first_mut() {
        Some(IrBlock::Paragraph { content }) => content,
        _ => return None,
    };
    if first_para.is_empty() {
        return None;
    }

    let (checked, to_remove) = match &first_para[0] {
        IrInline::Text { text } if text.starts_with("☒ ") => (true, 1),
        IrInline::Text { text } if text.starts_with("☐ ") => (false, 1),
        IrInline::Text { text } if text.starts_with("[x] ") || text.starts_with("[X] ") => {
            (true, 1)
        }
        IrInline::Text { text } if text.starts_with("[ ] ") => (false, 1),
        IrInline::Text { text } if text == "☒" => {
            let next_is_space = first_para.get(1).is_some_and(
                |inl| matches!(inl, IrInline::Text { text } if text == " " || text.is_empty()),
            );
            (true, if next_is_space { 2 } else { 1 })
        }
        IrInline::Text { text } if text == "☐" => {
            let next_is_space = first_para.get(1).is_some_and(
                |inl| matches!(inl, IrInline::Text { text } if text == " " || text.is_empty()),
            );
            (false, if next_is_space { 2 } else { 1 })
        }
        _ => return None,
    };

    if to_remove == 1 {
        let IrInline::Text { text } = &mut first_para[0] else {
            return None;
        };
        if text == "☒" || text == "☐" {
            first_para.remove(0);
        } else if text.starts_with("☒ ") || text.starts_with("☐ ") {
            let split_idx = text
                .char_indices()
                .nth(2)
                .map(|(i, _)| i)
                .unwrap_or(text.len());
            text.drain(..split_idx);
        } else if text.starts_with("[x] ") || text.starts_with("[X] ") || text.starts_with("[ ] ") {
            text.drain(..4);
        }
        if let Some(IrInline::Text { text }) = first_para.first()
            && text.is_empty()
        {
            first_para.remove(0);
        }
    } else if to_remove == 2 {
        first_para.remove(0);
        first_para.remove(0);
    }

    Some(checked)
}

fn map_alignment(align: Alignment) -> IrAlignment {
    match align {
        Alignment::AlignLeft => IrAlignment::Left,
        Alignment::AlignCenter => IrAlignment::Center,
        Alignment::AlignRight => IrAlignment::Right,
        Alignment::AlignDefault => IrAlignment::Default,
    }
}

fn inlines_to_text(inlines: &[Inline]) -> String {
    let mut out = String::new();
    for inline in inlines {
        match inline {
            Inline::Str(s) => out.push_str(s),
            Inline::Space => out.push(' '),
            Inline::Code(_attr, s) => out.push_str(s),
            Inline::Emph(ins)
            | Inline::Underline(ins)
            | Inline::Strong(ins)
            | Inline::Strikeout(ins)
            | Inline::Superscript(ins)
            | Inline::Subscript(ins)
            | Inline::SmallCaps(ins)
            | Inline::Quoted(_, ins)
            | Inline::Cite(_, ins)
            | Inline::Span(_, ins)
            | Inline::Link(_, ins, _) => out.push_str(&inlines_to_text(ins)),
            Inline::SoftBreak | Inline::LineBreak => out.push(' '),
            Inline::Math(_, s) | Inline::RawInline(_, s) => out.push_str(s),
            Inline::Image(_, alt, _) => out.push_str(&inlines_to_text(alt)),
            Inline::Note(_) => {}
        }
    }
    out
}

fn blocks_to_text(blocks: &[Block]) -> String {
    let mut out = String::new();
    for block in blocks {
        match block {
            Block::Plain(ins) | Block::Para(ins) | Block::Header(_, _, ins) => {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(&inlines_to_text(ins));
            }
            Block::CodeBlock(_, s) | Block::RawBlock(_, s) => {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(s);
            }
            Block::BlockQuote(bls) | Block::Div(_, bls) => {
                let inner = blocks_to_text(bls);
                if !inner.is_empty() {
                    if !out.is_empty() {
                        out.push('\n');
                    }
                    out.push_str(&inner);
                }
            }
            _ => {}
        }
    }
    out
}

fn extract_meta_string(val: &MetaValue) -> Option<String> {
    match val {
        MetaValue::MetaString(s) => Some(s.clone()),
        MetaValue::MetaInlines(inlines) => Some(inlines_to_text(inlines)),
        MetaValue::MetaBlocks(blocks) => Some(blocks_to_text(blocks)),
        MetaValue::MetaBool(b) => Some(b.to_string()),
        MetaValue::MetaList(list) => {
            let parts: Vec<String> = list.iter().filter_map(extract_meta_string).collect();
            if parts.is_empty() {
                None
            } else {
                Some(parts.join(", "))
            }
        }
        MetaValue::MetaMap(_) => None,
    }
}

fn extract_meta_strings(val: &MetaValue) -> Vec<String> {
    match val {
        MetaValue::MetaList(list) => list.iter().filter_map(extract_meta_string).collect(),
        MetaValue::MetaString(s) => vec![s.clone()],
        MetaValue::MetaInlines(inlines) => vec![inlines_to_text(inlines)],
        MetaValue::MetaBlocks(blocks) => vec![blocks_to_text(blocks)],
        _ => Vec::new(),
    }
}

fn map_metadata(meta: &std::collections::BTreeMap<String, MetaValue>) -> Metadata {
    let mut title = None;
    let mut authors = Vec::new();
    let mut language = None;
    let mut date = None;
    let mut subject = None;
    let mut keywords = Vec::new();

    // Drop presentation metadata such as HTML generator and viewport
    let ignored_presentation_keys: BTreeSet<&str> = ["generator", "viewport"].into_iter().collect();

    for (key, val) in meta {
        if ignored_presentation_keys.contains(key.as_str()) {
            continue;
        }

        match key.to_ascii_lowercase().as_str() {
            "title" => {
                if let Some(s) = extract_meta_string(val) {
                    title = Some(s.nfc().collect());
                }
            }
            "author" | "authors" => {
                for author in extract_meta_strings(val) {
                    authors.push(author.nfc().collect());
                }
            }
            "lang" | "language" => {
                if let Some(s) = extract_meta_string(val) {
                    language = Some(s.nfc().collect());
                }
            }
            "date" => {
                if let Some(s) = extract_meta_string(val) {
                    date = Some(s.nfc().collect());
                }
            }
            "subject" => {
                if let Some(s) = extract_meta_string(val) {
                    subject = Some(s.nfc().collect());
                }
            }
            "keywords" => {
                for kw in extract_meta_strings(val) {
                    keywords.push(kw.nfc().collect());
                }
            }
            _ => {}
        }
    }

    Metadata {
        title,
        authors,
        language,
        date,
        subject,
        keywords,
        source_format: None,
    }
}
