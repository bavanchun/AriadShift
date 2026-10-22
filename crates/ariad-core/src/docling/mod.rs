//! DoclingDocument JSON (`docling+json`) to IR mapper.
//!
//! The Docling engine writes DoclingDocument JSON; this module turns it into IR inside the
//! host, so IR normalization and the link policy live in one fuzzed Rust place. The reader is
//! bounded like the IR JSON reader: a byte cap, a JSON depth pre-scan, a nesting budget for the
//! item tree and a block budget. Each item is emitted at most once, which also breaks `$ref`
//! cycles.

pub mod model;

use std::collections::{BTreeMap, BTreeSet, HashSet};

use base64::{Engine, engine::general_purpose::STANDARD};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::{
    format::Format,
    ir::{
        Alignment, Asset, AssetRef, Block, BlockPosition, BoundingBox, ColumnSpec, Document,
        Inline, LayoutIndex, ListItem, PageGeometry, TableCell,
    },
    json_depth::{JsonDepthError, json_depth_budget_for_nesting, prescan_json_depth},
    limits::{Limits, LimitsError},
    links::allowed_link,
    reader::common::{ReadOutput, nfc, sniff_image_type},
    warning::{Warning, WarningCode},
};

use model::{
    DoclingDocument, GroupItem, PictureItem, ProvenanceItem, RefItem, SCHEMA_NAME, TableItem,
    TextItem,
};

/// The DoclingDocument schema major version this mapper understands.
pub const SUPPORTED_MAJOR_VERSION: &str = "1";

/// Errors that stop a DoclingDocument from being mapped.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum ReadError {
    #[error(transparent)]
    InvalidLimits(#[from] LimitsError),
    #[error("DoclingDocument JSON exceeds the configured limit of {limit} bytes")]
    InputTooLarge { limit: u64 },
    #[error(transparent)]
    JsonTooDeep(#[from] JsonDepthError),
    #[error("input is not valid DoclingDocument JSON (line {line}, column {column})")]
    InvalidJson { line: usize, column: usize },
    #[error("input is not a DoclingDocument")]
    NotDoclingDocument,
    #[error("unsupported DoclingDocument schema version; expected 1.x")]
    UnsupportedVersion,
    #[error("DoclingDocument nesting exceeds the configured depth of {limit}")]
    NestingTooDeep { limit: u16 },
    #[error("DoclingDocument contains more than the configured {limit} block limit")]
    TooManyBlocks { limit: u32 },
}

/// Reads DoclingDocument JSON bytes and maps them to IR.
pub fn read(bytes: &[u8], limits: &Limits) -> Result<ReadOutput, ReadError> {
    limits.validate()?;
    if bytes.len() as u64 > limits.max_ir_json_bytes {
        return Err(ReadError::InputTooLarge {
            limit: limits.max_ir_json_bytes,
        });
    }
    prescan_json_depth(
        bytes,
        json_depth_budget_for_nesting(limits.max_nesting_depth),
    )?;
    let document: DoclingDocument =
        serde_json::from_slice(bytes).map_err(|error| ReadError::InvalidJson {
            line: error.line(),
            column: error.column(),
        })?;
    to_ir(&document, limits)
}

/// Maps a deserialized DoclingDocument to IR.
pub fn to_ir(document: &DoclingDocument, limits: &Limits) -> Result<ReadOutput, ReadError> {
    limits.validate()?;
    if document.schema_name != SCHEMA_NAME {
        return Err(ReadError::NotDoclingDocument);
    }
    if document.version.split('.').next() != Some(SUPPORTED_MAJOR_VERSION) {
        return Err(ReadError::UnsupportedVersion);
    }

    let mut mapper = Mapper::new(document, limits);
    let body = mapper.map_children(&document.body.children, 1, Layer::Body)?;
    if let Some(furniture) = &document.furniture {
        let blocks = mapper.map_children(&furniture.children, 1, Layer::Furniture)?;
        mapper.furniture.extend(blocks);
    }

    let mut ir = Document::default();
    ir.meta.title = mapper.title.take();
    ir.meta.source_format = document
        .origin
        .as_ref()
        .and_then(|origin| source_format(&origin.mimetype));

    let mut layout = LayoutIndex {
        pages: page_geometry(document),
        blocks: BTreeMap::new(),
    };
    ir.body = place(body, "body", &mut layout.blocks);
    let furniture = std::mem::take(&mut mapper.furniture);
    ir.furniture = place(furniture, "furniture", &mut layout.blocks);
    if !layout.pages.is_empty() || !layout.blocks.is_empty() {
        ir.layout = Some(layout);
    }
    ir.assets = std::mem::take(&mut mapper.assets);

    Ok(ReadOutput {
        document: ir,
        warnings: mapper.warnings,
    })
}

/// Which IR sequence an item belongs to.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Layer {
    Body,
    Furniture,
}

/// A mapped block with the page positions of itself and its nested blocks.
///
/// Each position key is a path suffix relative to the block: `""` for the block itself, or
/// `"/items/0/blocks/1"` for a nested block. [`place`] prefixes them with the final path.
struct Mapped {
    block: Block,
    positions: Vec<(String, BlockPosition)>,
}

impl Mapped {
    fn new(block: Block, position: Option<BlockPosition>) -> Self {
        Self {
            block,
            positions: position
                .map(|position| (String::new(), position))
                .into_iter()
                .collect(),
        }
    }
}

/// Places mapped blocks under `prefix` and records their positions.
fn place(
    mapped: Vec<Mapped>,
    prefix: &str,
    positions: &mut BTreeMap<String, BlockPosition>,
) -> Vec<Block> {
    mapped
        .into_iter()
        .enumerate()
        .map(|(index, item)| {
            for (suffix, position) in item.positions {
                positions.insert(format!("{prefix}/{index}{suffix}"), position);
            }
            item.block
        })
        .collect()
}

/// Nests the positions of blocks placed inside a container under `suffix`.
fn nest(
    mapped: Vec<Mapped>,
    suffix: &str,
    positions: &mut Vec<(String, BlockPosition)>,
) -> Vec<Block> {
    mapped
        .into_iter()
        .enumerate()
        .map(|(index, item)| {
            for (inner, position) in item.positions {
                positions.push((format!("{suffix}/{index}{inner}"), position));
            }
            item.block
        })
        .collect()
}

/// A resolved `$ref` target.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
enum NodeRef {
    Group(usize),
    Text(usize),
    Picture(usize),
    Table(usize),
    Unsupported,
    Invalid,
}

fn parse_ref(target: &str) -> NodeRef {
    let Some(rest) = target.strip_prefix("#/") else {
        return NodeRef::Invalid;
    };
    let Some((collection, index)) = rest.split_once('/') else {
        return NodeRef::Invalid;
    };
    let Ok(index) = index.parse::<usize>() else {
        return NodeRef::Invalid;
    };
    match collection {
        "groups" => NodeRef::Group(index),
        "texts" => NodeRef::Text(index),
        "pictures" => NodeRef::Picture(index),
        "tables" => NodeRef::Table(index),
        "key_value_items" | "form_items" | "field_regions" | "field_items" => NodeRef::Unsupported,
        _ => NodeRef::Invalid,
    }
}

struct Mapper<'a> {
    document: &'a DoclingDocument,
    limits: &'a Limits,
    visited: HashSet<NodeRef>,
    consumed: HashSet<NodeRef>,
    block_count: u32,
    title: Option<String>,
    furniture: Vec<Mapped>,
    assets: crate::ir::AssetStore,
    warnings: Vec<Warning>,
    warned: BTreeSet<(String, &'static str)>,
}

impl<'a> Mapper<'a> {
    fn new(document: &'a DoclingDocument, limits: &'a Limits) -> Self {
        // Captions and footnotes belong to their table or picture; never emit them twice.
        let consumed = document
            .tables
            .iter()
            .flat_map(|table| table.captions.iter().chain(&table.footnotes))
            .chain(
                document
                    .pictures
                    .iter()
                    .flat_map(|picture| picture.captions.iter().chain(&picture.footnotes)),
            )
            .map(|reference| parse_ref(&reference.target))
            .collect();
        Self {
            document,
            limits,
            visited: HashSet::new(),
            consumed,
            block_count: 0,
            title: None,
            furniture: Vec::new(),
            assets: crate::ir::AssetStore::new(),
            warnings: Vec::new(),
            warned: BTreeSet::new(),
        }
    }

    fn warn(&mut self, code: WarningCode, message: &'static str) {
        let key = (format!("{code:?}"), message);
        if self.warned.insert(key) {
            self.warnings.push(Warning::new(code, message));
        }
    }

    fn count_block(&mut self) -> Result<(), ReadError> {
        self.block_count = self.block_count.saturating_add(1);
        if self.block_count > self.limits.max_blocks {
            return Err(ReadError::TooManyBlocks {
                limit: self.limits.max_blocks,
            });
        }
        Ok(())
    }

    fn check_depth(&self, depth: u16) -> Result<(), ReadError> {
        if depth > self.limits.max_nesting_depth {
            return Err(ReadError::NestingTooDeep {
                limit: self.limits.max_nesting_depth,
            });
        }
        Ok(())
    }

    /// Marks a node as emitted; returns `false` when it was already emitted or is consumed.
    fn claim(&mut self, node: NodeRef) -> bool {
        !self.consumed.contains(&node) && self.visited.insert(node)
    }

    fn map_children(
        &mut self,
        children: &[RefItem],
        depth: u16,
        layer: Layer,
    ) -> Result<Vec<Mapped>, ReadError> {
        self.check_depth(depth)?;
        let mut out = Vec::new();
        let mut pending_items: Vec<(usize, bool)> = Vec::new();
        for child in children {
            let node = parse_ref(&child.target);
            // Docling writers before list groups existed left list items directly in their
            // parent; consecutive ones with the same numbering form one list.
            if let NodeRef::Text(index) = node
                && let Some(text) = self.document.texts.get(index)
                && text.label == "list_item"
                && item_layer(text.content_layer.as_deref(), &text.label)
                    .map(|item| merge_layer(layer, item))
                    == Some(layer)
            {
                let ordered = text.enumerated.unwrap_or(false);
                if pending_items
                    .first()
                    .is_some_and(|&(_, first_ordered)| first_ordered != ordered)
                {
                    self.flush_list(&mut pending_items, depth, layer, &mut out)?;
                }
                if self.claim(node) {
                    pending_items.push((index, ordered));
                }
                continue;
            }
            self.flush_list(&mut pending_items, depth, layer, &mut out)?;
            self.map_node(node, depth, layer, &mut out)?;
        }
        self.flush_list(&mut pending_items, depth, layer, &mut out)?;
        Ok(out)
    }

    fn flush_list(
        &mut self,
        pending: &mut Vec<(usize, bool)>,
        depth: u16,
        layer: Layer,
        out: &mut Vec<Mapped>,
    ) -> Result<(), ReadError> {
        if pending.is_empty() {
            return Ok(());
        }
        let indices: Vec<usize> = pending.drain(..).map(|(index, _)| index).collect();
        if let Some(list) = self.list_from_items(&indices, None, depth, layer)? {
            out.push(list);
        }
        Ok(())
    }

    fn map_node(
        &mut self,
        node: NodeRef,
        depth: u16,
        context: Layer,
        out: &mut Vec<Mapped>,
    ) -> Result<(), ReadError> {
        self.check_depth(depth)?;
        let (content_layer, label) = match node {
            NodeRef::Group(index) => match self.document.groups.get(index) {
                Some(group) => (group.content_layer.as_deref(), ""),
                None => return self.dangling(),
            },
            NodeRef::Text(index) => match self.document.texts.get(index) {
                Some(text) => (text.content_layer.as_deref(), text.label.as_str()),
                None => return self.dangling(),
            },
            NodeRef::Table(index) => match self.document.tables.get(index) {
                Some(table) => (table.content_layer.as_deref(), ""),
                None => return self.dangling(),
            },
            NodeRef::Picture(index) => match self.document.pictures.get(index) {
                Some(picture) => (picture.content_layer.as_deref(), ""),
                None => return self.dangling(),
            },
            NodeRef::Unsupported => {
                self.warn(
                    WarningCode::UnsupportedNode,
                    "form and key-value regions were omitted",
                );
                return Ok(());
            }
            NodeRef::Invalid => return self.dangling(),
        };
        let Some(item) = item_layer(content_layer, label) else {
            if content_layer == Some("notes") {
                self.warn(WarningCode::UnsupportedNode, "speaker notes were omitted");
            }
            return Ok(());
        };
        if !self.claim(node) {
            return Ok(());
        }
        let layer = merge_layer(context, item);
        let mut local = Vec::new();
        match node {
            NodeRef::Group(index) => {
                let group = &self.document.groups[index];
                self.map_group(group, depth, layer, &mut local)?;
            }
            NodeRef::Text(index) => {
                let text = &self.document.texts[index];
                local.extend(self.map_text(index, text, depth)?);
                // Headings in Word documents own the content of their section.
                local.extend(self.map_children(&text.children, depth + 1, layer)?);
            }
            NodeRef::Table(index) => {
                let table = &self.document.tables[index];
                local.extend(self.map_table(table, depth)?);
            }
            NodeRef::Picture(index) => {
                let picture = &self.document.pictures[index];
                local.extend(self.map_picture(picture)?);
            }
            NodeRef::Unsupported | NodeRef::Invalid => {}
        }
        // Running headers and footers leave the body; everything else stays where it was.
        if context == Layer::Body && layer == Layer::Furniture {
            self.furniture.extend(local);
        } else {
            out.extend(local);
        }
        Ok(())
    }

    fn dangling(&mut self) -> Result<(), ReadError> {
        self.warn(
            WarningCode::UnsupportedNode,
            "a dangling reference was skipped",
        );
        Ok(())
    }

    fn map_group(
        &mut self,
        group: &GroupItem,
        depth: u16,
        layer: Layer,
        out: &mut Vec<Mapped>,
    ) -> Result<(), ReadError> {
        match group.label.as_str() {
            "list" | "ordered_list" => {
                let ordered = group.label == "ordered_list";
                out.extend(self.map_list_group(group, ordered, depth, layer)?);
            }
            "inline" => {
                let content = self.inline_group(group, depth)?;
                if !content.is_empty() {
                    self.count_block()?;
                    out.push(Mapped::new(Block::Paragraph { content }, None));
                }
            }
            _ => out.extend(self.map_children(&group.children, depth + 1, layer)?),
        }
        Ok(())
    }

    fn map_list_group(
        &mut self,
        group: &GroupItem,
        ordered_label: bool,
        depth: u16,
        layer: Layer,
    ) -> Result<Option<Mapped>, ReadError> {
        let mut indices = Vec::new();
        let mut extra = Vec::new();
        for child in &group.children {
            let node = parse_ref(&child.target);
            match node {
                NodeRef::Text(index)
                    if self
                        .document
                        .texts
                        .get(index)
                        .is_some_and(|text| text.label == "list_item") =>
                {
                    if self.claim(node) {
                        indices.push(index);
                    }
                }
                _ => extra.push(child.clone()),
            }
        }
        let ordered = ordered_label
            || indices
                .first()
                .and_then(|&index| self.document.texts.get(index))
                .is_some_and(|text| text.enumerated.unwrap_or(false));
        let mut list = self.list_from_items(&indices, Some(ordered), depth, layer)?;
        if !extra.is_empty() {
            // Anything else inside a list becomes one more item.
            let blocks = self.map_children(&extra, depth + 1, layer)?;
            if !blocks.is_empty() {
                let list = list.get_or_insert_with(|| {
                    Mapped::new(
                        Block::List {
                            ordered,
                            start: ordered.then_some(1),
                            tight: true,
                            items: Vec::new(),
                        },
                        None,
                    )
                });
                if let Block::List { items, .. } = &mut list.block {
                    let suffix = format!("/items/{}/blocks", items.len());
                    let blocks = nest(blocks, &suffix, &mut list.positions);
                    items.push(ListItem {
                        checked: None,
                        blocks,
                    });
                }
            }
        }
        Ok(list)
    }

    fn list_from_items(
        &mut self,
        indices: &[usize],
        ordered: Option<bool>,
        depth: u16,
        layer: Layer,
    ) -> Result<Option<Mapped>, ReadError> {
        if indices.is_empty() {
            return Ok(None);
        }
        self.check_depth(depth + 1)?;
        self.count_block()?;
        let first = &self.document.texts[indices[0]];
        let ordered = ordered.unwrap_or(first.enumerated.unwrap_or(false));
        let start = ordered.then(|| list_start(first.marker.as_deref()));
        let mut positions = Vec::new();
        let mut items = Vec::with_capacity(indices.len());
        for (item_index, &index) in indices.iter().enumerate() {
            let text = &self.document.texts[index];
            let mut mapped = Vec::new();
            let content = self.inlines(text)?;
            if !content.is_empty() {
                self.count_block()?;
                mapped.push(Mapped::new(
                    Block::Paragraph { content },
                    self.position(&text.prov),
                ));
            }
            mapped.extend(self.map_children(&text.children, depth + 2, layer)?);
            let suffix = format!("/items/{item_index}/blocks");
            let blocks = nest(mapped, &suffix, &mut positions);
            items.push(ListItem {
                checked: None,
                blocks,
            });
        }
        Ok(Some(Mapped {
            block: Block::List {
                ordered,
                start,
                tight: true,
                items,
            },
            positions,
        }))
    }

    fn inline_group(&mut self, group: &GroupItem, depth: u16) -> Result<Vec<Inline>, ReadError> {
        self.check_depth(depth + 1)?;
        let mut content: Vec<Inline> = Vec::new();
        for child in &group.children {
            let node = parse_ref(&child.target);
            let parts = match node {
                NodeRef::Text(index) => {
                    let Some(text) = self.document.texts.get(index) else {
                        continue;
                    };
                    if !self.claim(node) {
                        continue;
                    }
                    match text.label.as_str() {
                        "formula" if !text.text.is_empty() => vec![Inline::Math {
                            tex: text.text.clone(),
                            display: false,
                        }],
                        "code" if !text.text.is_empty() => vec![Inline::Code {
                            text: text.text.clone(),
                        }],
                        _ => self.inlines(text)?,
                    }
                }
                NodeRef::Group(index) => {
                    let Some(nested) = self.document.groups.get(index) else {
                        continue;
                    };
                    if !self.claim(node) {
                        continue;
                    }
                    self.inline_group(nested, depth + 1)?
                }
                _ => {
                    self.warn(
                        WarningCode::UnsupportedNode,
                        "an unsupported node inside running text was omitted",
                    );
                    Vec::new()
                }
            };
            if parts.is_empty() {
                continue;
            }
            if !content.is_empty() {
                content.push(Inline::Text {
                    text: " ".to_owned(),
                });
            }
            content.extend(parts);
        }
        Ok(content)
    }

    fn map_text(
        &mut self,
        index: usize,
        text: &TextItem,
        depth: u16,
    ) -> Result<Option<Mapped>, ReadError> {
        let position = self.position(&text.prov);
        let block = match text.label.as_str() {
            "title" => {
                let content = self.inlines(text)?;
                if content.is_empty() {
                    return Ok(None);
                }
                if self.title.is_none() {
                    self.title = Some(nfc(text.text.trim()));
                }
                Block::Heading { level: 1, content }
            }
            "section_header" => {
                let content = self.inlines(text)?;
                if content.is_empty() {
                    return Ok(None);
                }
                let level = text.level.unwrap_or(1).clamp(1, 5) + 1;
                Block::Heading {
                    level: u8::try_from(level).unwrap_or(6),
                    content,
                }
            }
            "code" => {
                if text.text.is_empty() {
                    return Ok(None);
                }
                Block::Code {
                    lang: code_language(text.code_language.as_deref()),
                    text: text.text.clone(),
                }
            }
            "formula" => {
                if text.text.trim().is_empty() {
                    self.warn(
                        WarningCode::UnsupportedNode,
                        "a formula without LaTeX text was omitted",
                    );
                    return Ok(None);
                }
                Block::Math {
                    tex: text.text.clone(),
                    display: true,
                }
            }
            "list_item" => {
                // A list item found outside any list becomes a one-item list.
                return self.list_from_items(&[index], None, depth, Layer::Body);
            }
            "checkbox_selected" | "checkbox_unselected" => {
                let content = self.inlines(text)?;
                self.check_depth(depth + 1)?;
                self.count_block()?;
                let paragraph = if content.is_empty() {
                    Vec::new()
                } else {
                    self.count_block()?;
                    vec![Block::Paragraph { content }]
                };
                Block::List {
                    ordered: false,
                    start: None,
                    tight: true,
                    items: vec![ListItem {
                        checked: Some(text.label == "checkbox_selected"),
                        blocks: paragraph,
                    }],
                }
            }
            _ => {
                let content = self.inlines(text)?;
                if content.is_empty() {
                    return Ok(None);
                }
                Block::Paragraph { content }
            }
        };
        if !matches!(block, Block::List { .. }) {
            self.count_block()?;
        }
        Ok(Some(Mapped::new(block, position)))
    }

    /// Maps a text item's text, formatting and hyperlink to inline content.
    fn inlines(&mut self, text: &TextItem) -> Result<Vec<Inline>, ReadError> {
        let value = nfc(&text.text);
        if value.trim().is_empty() {
            return Ok(Vec::new());
        }
        let mut inline = Inline::Text { text: value };
        if let Some(formatting) = &text.formatting {
            if formatting.underline && !(formatting.bold || formatting.italic) {
                self.warn(
                    WarningCode::UnsupportedNode,
                    "underline is not supported and was mapped to emphasis",
                );
                inline = Inline::Emph {
                    content: vec![inline],
                };
            }
            if formatting.bold {
                inline = Inline::Strong {
                    content: vec![inline],
                };
            }
            if formatting.italic {
                inline = Inline::Emph {
                    content: vec![inline],
                };
            }
            if formatting.strikethrough {
                inline = Inline::Strikeout {
                    content: vec![inline],
                };
            }
            match formatting.script.as_str() {
                "sub" => {
                    inline = Inline::Subscript {
                        content: vec![inline],
                    };
                }
                "super" => {
                    inline = Inline::Superscript {
                        content: vec![inline],
                    };
                }
                _ => {}
            }
        }
        if let Some(url) = text.hyperlink.as_deref().filter(|url| !url.is_empty()) {
            if allowed_link(url) {
                inline = Inline::Link {
                    url: url.to_owned(),
                    title: None,
                    content: vec![inline],
                };
            } else {
                self.warn(
                    WarningCode::LinkDropped,
                    "a link with an unsupported scheme became plain text",
                );
            }
        }
        Ok(vec![inline])
    }

    fn text_of(&mut self, references: &[RefItem]) -> Result<Vec<Inline>, ReadError> {
        let mut content = Vec::new();
        for reference in references {
            if let NodeRef::Text(index) = parse_ref(&reference.target)
                && let Some(text) = self.document.texts.get(index)
            {
                let parts = self.inlines(text)?;
                if parts.is_empty() {
                    continue;
                }
                if !content.is_empty() {
                    content.push(Inline::SoftBreak {});
                }
                content.extend(parts);
            }
        }
        Ok(content)
    }

    fn map_table(&mut self, table: &TableItem, depth: u16) -> Result<Option<Mapped>, ReadError> {
        let cells: Vec<&model::TableCell> = table
            .data
            .table_cells
            .iter()
            .filter(|cell| cell.start_row_offset_idx >= 0 && cell.start_col_offset_idx >= 0)
            .collect();
        if cells.is_empty() {
            return Ok(None);
        }
        self.check_depth(depth + 1)?;
        self.count_block()?;

        // Rows and columns are compressed to those where a cell starts, so the IR stays
        // proportional to the cells even when the declared grid is huge or sparse.
        let rows: Vec<i64> = cells
            .iter()
            .map(|cell| cell.start_row_offset_idx)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let columns: Vec<i64> = cells
            .iter()
            .map(|cell| cell.start_col_offset_idx)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();

        let mut grid: Vec<Vec<(i64, &model::TableCell)>> = vec![Vec::new(); rows.len()];
        let mut seen = HashSet::new();
        for cell in &cells {
            if !seen.insert((cell.start_row_offset_idx, cell.start_col_offset_idx)) {
                continue;
            }
            let row = rows.partition_point(|&row| row < cell.start_row_offset_idx);
            grid[row].push((cell.start_col_offset_idx, cell));
        }

        let mut head = Vec::new();
        let mut body = Vec::new();
        let mut positions = Vec::new();
        for mut row_cells in grid {
            row_cells.sort_by_key(|(column, _)| *column);
            let is_head = body.is_empty() && row_cells.iter().any(|(_, cell)| cell.column_header);
            let section = if is_head { "head" } else { "body" };
            let target_row = if is_head { head.len() } else { body.len() };
            let mut ir_cells = Vec::with_capacity(row_cells.len());
            for (cell_index, (_, cell)) in row_cells.iter().enumerate() {
                let rowspan = span(&rows, cell.start_row_offset_idx, cell.row_span);
                let colspan = span(&columns, cell.start_col_offset_idx, cell.col_span);
                let mapped = self.cell_blocks(cell, depth + 2)?;
                let suffix = format!("/{section}/{target_row}/cells/{cell_index}/blocks");
                let blocks = nest(mapped, &suffix, &mut positions);
                ir_cells.push(TableCell {
                    rowspan,
                    colspan,
                    header: (!is_head && (cell.row_header || cell.row_section)).then_some(true),
                    blocks,
                });
            }
            if is_head {
                head.push(ir_cells);
            } else {
                body.push(ir_cells);
            }
        }

        let caption = self.text_of(&table.captions)?;
        let footnotes = self.text_of(&table.footnotes)?;
        let block = Block::Table {
            caption: (!caption.is_empty()).then_some(caption),
            columns: vec![
                ColumnSpec {
                    align: Alignment::Default
                };
                columns.len()
            ],
            head,
            body,
            footnotes,
        };
        let mut mapped = Mapped::new(block, self.position(&table.prov));
        mapped.positions.extend(positions);
        Ok(Some(mapped))
    }

    fn cell_blocks(
        &mut self,
        cell: &model::TableCell,
        depth: u16,
    ) -> Result<Vec<Mapped>, ReadError> {
        if let Some(rich) = &cell.rich {
            let node = parse_ref(&rich.target);
            let mut out = Vec::new();
            self.map_node(node, depth, Layer::Body, &mut out)?;
            if !out.is_empty() {
                return Ok(out);
            }
        }
        let value = nfc(&cell.text);
        if value.trim().is_empty() {
            return Ok(Vec::new());
        }
        self.count_block()?;
        Ok(vec![Mapped::new(
            Block::Paragraph {
                content: vec![Inline::Text { text: value }],
            },
            None,
        )])
    }

    fn map_picture(&mut self, picture: &PictureItem) -> Result<Option<Mapped>, ReadError> {
        let caption = self.text_of(&picture.captions)?;
        let position = self.position(&picture.prov);
        let asset = picture
            .image
            .as_ref()
            .and_then(|image| self.embedded_image(&image.uri));
        match asset {
            Some(id) => {
                self.count_block()?;
                Ok(Some(Mapped::new(
                    Block::Figure {
                        asset: AssetRef::Asset { id },
                        caption,
                    },
                    position,
                )))
            }
            None => {
                self.warn(
                    WarningCode::ImageNotEmbedded,
                    "a picture without embedded image data was omitted",
                );
                if caption.is_empty() {
                    return Ok(None);
                }
                self.count_block()?;
                Ok(Some(Mapped::new(
                    Block::Paragraph { content: caption },
                    position,
                )))
            }
        }
    }

    /// Decodes a base64 `data:` URI into the asset store and returns its id.
    fn embedded_image(&mut self, uri: &str) -> Option<String> {
        let rest = uri.strip_prefix("data:")?;
        let (header, encoded) = rest.split_once(',')?;
        if !header
            .rsplit(';')
            .next()
            .is_some_and(|part| part.eq_ignore_ascii_case("base64"))
        {
            return None;
        }
        let estimated = (encoded.len() as u64).saturating_mul(3) / 4;
        if self
            .limits
            .max_asset_bytes
            .is_some_and(|limit| estimated > limit.saturating_add(3))
        {
            self.warn(
                WarningCode::ImageNotEmbedded,
                "a picture larger than the asset limit was omitted",
            );
            return None;
        }
        let bytes = STANDARD.decode(encoded.trim()).ok()?;
        if self
            .limits
            .max_asset_bytes
            .is_some_and(|limit| bytes.len() as u64 > limit)
        {
            self.warn(
                WarningCode::ImageNotEmbedded,
                "a picture larger than the asset limit was omitted",
            );
            return None;
        }
        let media_type = sniff_image_type(&bytes)?;
        let id = hex::encode(Sha256::digest(&bytes));
        self.assets.entry(id.clone()).or_insert_with(|| Asset {
            media_type: media_type.to_owned(),
            bytes,
        });
        Some(id)
    }

    /// Converts the first provenance entry to a top-left page position in points.
    fn position(&self, provenance: &[ProvenanceItem]) -> Option<BlockPosition> {
        let first = provenance.first()?;
        let page_no = u32::try_from(first.page_no).ok().filter(|&page| page > 0)?;
        let bbox = first.bbox.as_ref()?;
        if ![bbox.l, bbox.t, bbox.r, bbox.b]
            .iter()
            .all(|value| value.is_finite())
        {
            return None;
        }
        let (top, bottom) = if bbox.coord_origin.as_deref() == Some("BOTTOMLEFT") {
            let height = self
                .document
                .pages
                .get(&page_no.to_string())
                .map(|page| page.size.height)
                .filter(|height| height.is_finite() && *height > 0.0)?;
            (height - bbox.t, height - bbox.b)
        } else {
            (bbox.t, bbox.b)
        };
        Some(BlockPosition {
            page_no,
            bbox: BoundingBox {
                left: bbox.l.min(bbox.r),
                top: top.min(bottom),
                right: bbox.l.max(bbox.r),
                bottom: top.max(bottom),
            },
        })
    }
}

/// Returns which layer an item belongs to, or `None` when the item is not part of the reading
/// content (background, invisible text, speaker notes).
fn item_layer(content_layer: Option<&str>, label: &str) -> Option<Layer> {
    if matches!(label, "page_header" | "page_footer") {
        return Some(Layer::Furniture);
    }
    match content_layer.unwrap_or("body") {
        "body" => Some(Layer::Body),
        "furniture" => Some(Layer::Furniture),
        _ => None,
    }
}

/// Furniture is sticky: everything under a furniture node stays furniture.
fn merge_layer(parent: Layer, item: Layer) -> Layer {
    if parent == Layer::Furniture {
        Layer::Furniture
    } else {
        item
    }
}

/// Counts how many compressed rows or columns a span covers, at least one.
fn span(starts: &[i64], start: i64, length: i64) -> u32 {
    let end = start.saturating_add(length.max(1));
    let first = starts.partition_point(|&value| value < start);
    let last = starts.partition_point(|&value| value < end);
    u32::try_from(last.saturating_sub(first).max(1)).unwrap_or(u32::MAX)
}

/// Reads the number from an ordered list marker such as `3.` or `(12)`.
fn list_start(marker: Option<&str>) -> u64 {
    marker
        .map(|marker| {
            marker
                .chars()
                .skip_while(|character| !character.is_ascii_digit())
                .take_while(char::is_ascii_digit)
                .collect::<String>()
        })
        .and_then(|digits| digits.parse().ok())
        .unwrap_or(1)
}

fn code_language(language: Option<&str>) -> Option<String> {
    let language = language?.trim();
    if language.is_empty() || language.eq_ignore_ascii_case("unknown") {
        return None;
    }
    Some(language.to_ascii_lowercase())
}

fn source_format(mimetype: &str) -> Option<Format> {
    [
        Format::Pdf,
        Format::Docx,
        Format::Pptx,
        Format::Xlsx,
        Format::Png,
        Format::Jpeg,
        Format::Tiff,
        Format::Webp,
        Format::Html,
        Format::Markdown,
    ]
    .into_iter()
    .find(|format| format.media_type().eq_ignore_ascii_case(mimetype))
}

fn page_geometry(document: &DoclingDocument) -> Vec<PageGeometry> {
    let mut pages: Vec<PageGeometry> = document
        .pages
        .iter()
        .filter_map(|(key, page)| {
            let page_no = u32::try_from(page.page_no)
                .ok()
                .filter(|&page| page > 0)
                .or_else(|| key.parse().ok())?;
            let size = page.size;
            (size.width.is_finite()
                && size.height.is_finite()
                && size.width > 0.0
                && size.height > 0.0)
                .then_some(PageGeometry {
                    page_no,
                    width: size.width,
                    height: size.height,
                })
        })
        .collect();
    pages.sort_by_key(|page| page.page_no);
    pages.dedup_by_key(|page| page.page_no);
    pages
}
