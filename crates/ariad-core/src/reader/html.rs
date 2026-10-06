//! In-process HTML reader for ariad-core with bounded resource consumption.

use std::{
    cell::Cell,
    collections::{BTreeMap, HashSet},
    rc::Rc,
};

use html5ever::{
    driver::ParseOpts,
    tendril::{StrTendril, TendrilSink},
};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::{
    format::Format,
    ir::{
        Alignment, Asset, AssetRef, AssetStore, Block, ColumnSpec, Document, IR_VERSION, Inline,
        ListItem, Metadata, TableCell,
    },
    limits::{Limits, LimitsError},
    links::allowed_link,
    reader::{
        common::{ReadOutput, nfc, nonempty, sniff_image_type},
        html_sink::{Arena, Handle, HtmlSink, LimitExceeded, NodeData},
    },
    warning::{Warning, WarningCode},
};

/// Maximum allowed DOM open-element nesting depth before chunked feeding stops.
pub const MAX_DOM_DEPTH: usize = 1024;

/// Base DOM node budget allocated to any document regardless of size.
const BASE_DOM_NODES: usize = 8192;

/// Multiplier of input bytes added to the base DOM node budget.
const DOM_NODES_PER_INPUT_BYTE: usize = 1;

/// Multiplier of the configured max_blocks limit added to the base DOM node budget.
const DOM_NODES_PER_MAX_BLOCK: usize = 16;

/// Computes the maximum allowed DOM arena nodes derived from input length and limits.
fn max_dom_nodes(input_len: usize, limits: &Limits) -> usize {
    let byte_bound =
        BASE_DOM_NODES.saturating_add(input_len.saturating_mul(DOM_NODES_PER_INPUT_BYTE));
    let block_bound = BASE_DOM_NODES
        .saturating_add((limits.max_blocks as usize).saturating_mul(DOM_NODES_PER_MAX_BLOCK));
    byte_bound.min(block_bound)
}

/// Maximum size in bytes of each incremental input chunk fed to the parser.
const CHUNK_SIZE: usize = 4096;

/// Specific failure conditions when reading HTML markup.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum ReadError {
    #[error(transparent)]
    InvalidLimits(#[from] LimitsError),
    #[error("HTML input exceeds the {limit}-byte limit")]
    InputTooLarge { limit: u64 },
    #[error("HTML nesting exceeds the configured depth of {limit}")]
    NestingTooDeep { limit: u16 },
    #[error("HTML contains more than the configured {limit} block limit")]
    TooManyBlocks { limit: u32 },
    #[error("HTML DOM node count exceeds the limit of {limit}")]
    TooManyNodes { limit: usize },
}

/// Slices a string slice into chunks of at most `chunk_size` bytes on UTF-8 char boundaries.
fn chunk_str(mut s: &str, chunk_size: usize) -> impl Iterator<Item = &str> {
    std::iter::from_fn(move || {
        if s.is_empty() {
            return None;
        }
        if s.len() <= chunk_size {
            let chunk = s;
            s = "";
            return Some(chunk);
        }
        let mut end = chunk_size;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        let chunk = &s[..end];
        s = &s[end..];
        Some(chunk)
    })
}

/// Parses attribute key/value pairs in a `<meta ...>` tag slice and resolves charset.
fn parse_meta_tag_charset(tag: &[u8]) -> Option<&'static encoding_rs::Encoding> {
    let mut charset_attr: Option<String> = None;
    let mut http_equiv_is_content_type = false;
    let mut content_charset: Option<String> = None;

    let mut i = 0;
    while i < tag.len() {
        while i < tag.len() && (tag[i].is_ascii_whitespace() || tag[i] == b'/') {
            i += 1;
        }
        if i >= tag.len() {
            break;
        }

        let name_start = i;
        while i < tag.len()
            && !tag[i].is_ascii_whitespace()
            && tag[i] != b'='
            && tag[i] != b'/'
            && tag[i] != b'>'
        {
            i += 1;
        }
        let attr_name = String::from_utf8_lossy(&tag[name_start..i]).to_ascii_lowercase();

        while i < tag.len() && tag[i].is_ascii_whitespace() {
            i += 1;
        }

        let attr_val = if i < tag.len() && tag[i] == b'=' {
            i += 1;
            while i < tag.len() && tag[i].is_ascii_whitespace() {
                i += 1;
            }
            if i < tag.len() && (tag[i] == b'"' || tag[i] == b'\'') {
                let quote = tag[i];
                i += 1;
                let val_start = i;
                while i < tag.len() && tag[i] != quote {
                    i += 1;
                }
                let val = String::from_utf8_lossy(&tag[val_start..i]).to_string();
                if i < tag.len() && tag[i] == quote {
                    i += 1;
                }
                val
            } else {
                let val_start = i;
                while i < tag.len()
                    && !tag[i].is_ascii_whitespace()
                    && tag[i] != b'/'
                    && tag[i] != b'>'
                {
                    i += 1;
                }
                String::from_utf8_lossy(&tag[val_start..i]).to_string()
            }
        } else {
            String::new()
        };

        if attr_name == "charset" {
            charset_attr = Some(attr_val);
        } else if attr_name == "http-equiv" && attr_val.eq_ignore_ascii_case("content-type") {
            http_equiv_is_content_type = true;
        } else if attr_name == "content" {
            let lower_val = attr_val.to_ascii_lowercase();
            if let Some(pos) = lower_val.find("charset=") {
                let rest = &attr_val[pos + 8..];
                let trimmed = rest.trim_start();
                let label = trimmed
                    .split([';', ' ', '\t', '\'', '"'])
                    .next()
                    .unwrap_or("")
                    .trim();
                if !label.is_empty() {
                    content_charset = Some(label.to_string());
                }
            }
        }
    }

    let raw_label = charset_attr.or(if http_equiv_is_content_type {
        content_charset
    } else {
        None
    })?;

    let label = raw_label.trim();
    if label.is_empty() {
        return None;
    }

    let lower = label.to_ascii_lowercase();
    // WHATWG Encoding §4.2: If charset is a UTF-16 encoding, change charset to UTF-8.
    if lower == "utf-16" || lower == "utf-16le" || lower == "utf-16be" {
        return Some(encoding_rs::UTF_8);
    }
    // WHATWG Encoding §4.2: If charset is x-user-defined, change charset to windows-1252.
    if lower == "x-user-defined" {
        return Some(encoding_rs::WINDOWS_1252);
    }

    encoding_rs::Encoding::for_label(label.as_bytes()).map(|enc| {
        if enc == encoding_rs::UTF_16LE || enc == encoding_rs::UTF_16BE {
            encoding_rs::UTF_8
        } else {
            enc
        }
    })
}

/// Sniffs charset strictly within `<meta>` tags inside the first 1024 bytes.
fn sniff_charset(header: &[u8]) -> Option<&'static encoding_rs::Encoding> {
    let max_len = header.len().min(1024);
    let bytes = &header[..max_len];
    let mut i = 0;

    while i < bytes.len() {
        // Skip HTML comments: <!-- ... -->
        if bytes[i..].starts_with(b"<!--") {
            i += 4;
            if let Some(pos) = bytes[i..].windows(3).position(|w| w == b"-->") {
                i += pos + 3;
            } else {
                break;
            }
            continue;
        }

        // Look for start of <meta tag
        if bytes[i] == b'<' {
            let rest = &bytes[i + 1..];
            if rest.len() >= 4 && rest[..4].eq_ignore_ascii_case(b"meta") {
                let after_meta = &rest[4..];
                if after_meta.is_empty()
                    || after_meta[0].is_ascii_whitespace()
                    || after_meta[0] == b'/'
                    || after_meta[0] == b'>'
                {
                    let meta_end = after_meta.iter().position(|&b| b == b'>');
                    let tag_bytes = match meta_end {
                        Some(end) => &after_meta[..end],
                        None => after_meta,
                    };

                    if let Some(enc) = parse_meta_tag_charset(tag_bytes) {
                        return Some(enc);
                    }

                    if let Some(end) = meta_end {
                        i += 1 + 4 + end + 1;
                        continue;
                    } else {
                        break;
                    }
                }
            }
        }
        i += 1;
    }
    None
}

/// Decodes raw HTML bytes into a UTF-8 String, honouring BOM and meta charset in first 1024 bytes.
fn decode_html(input: &[u8], warnings: &mut Vec<Warning>) -> String {
    // 1. Check BOM
    if let Some((enc, bom_len)) = encoding_rs::Encoding::for_bom(input) {
        let (decoded, had_malformed) = enc.decode_without_bom_handling(&input[bom_len..]);
        if had_malformed {
            warnings.push(Warning::new(
                WarningCode::UnsupportedNode,
                "undecodable bytes were replaced with U+FFFD",
            ));
        }
        return decoded.into_owned();
    }

    // 2. Check meta charset within first 1024 bytes
    let header_len = input.len().min(1024);
    let enc = sniff_charset(&input[..header_len]).unwrap_or(encoding_rs::UTF_8);

    let (decoded, had_malformed) = enc.decode_without_bom_handling(input);
    if had_malformed {
        warnings.push(Warning::new(
            WarningCode::UnsupportedNode,
            "undecodable bytes were replaced with U+FFFD",
        ));
    }
    decoded.into_owned()
}

/// State maintained during mapping from DOM arena to IR Document.
struct TreeMapper<'a> {
    arena: &'a Arena,
    limits: &'a Limits,
    block_count: u64,
    warnings: Vec<Warning>,
    dropped_tags: HashSet<String>,
    unsupported_warnings: HashSet<String>,
    assets: AssetStore,
}

impl<'a> TreeMapper<'a> {
    fn new(arena: &'a Arena, limits: &'a Limits, initial_warnings: Vec<Warning>) -> Self {
        Self {
            arena,
            limits,
            block_count: 0,
            warnings: initial_warnings,
            dropped_tags: HashSet::new(),
            unsupported_warnings: HashSet::new(),
            assets: BTreeMap::new(),
        }
    }

    fn warn_dropped_once(&mut self, tag: &str) {
        if self.dropped_tags.insert(tag.to_owned()) {
            self.warnings.push(Warning::new(
                WarningCode::UnsupportedNode,
                format!("`<{tag}>` element was dropped"),
            ));
        }
    }

    fn warn_unsupported_once(&mut self, message: impl Into<String>) {
        let msg = message.into();
        if self.unsupported_warnings.insert(msg.clone()) {
            self.warnings
                .push(Warning::new(WarningCode::UnsupportedNode, msg));
        }
    }

    fn warn_link_dropped(&mut self, url: &str) {
        self.warnings.push(Warning::new(
            WarningCode::LinkDropped,
            format!("link with disallowed URL `{url}` was reduced to text"),
        ));
    }

    fn check_block_limit(&mut self) -> Result<(), ReadError> {
        self.block_count += 1;
        if self.block_count > u64::from(self.limits.max_blocks) {
            Err(ReadError::TooManyBlocks {
                limit: self.limits.max_blocks,
            })
        } else {
            Ok(())
        }
    }
}

/// Representation of a node after post-order bottom-up conversion.
#[derive(Debug)]
enum ConvertedNode {
    Blocks(Vec<Block>),
    Inlines(Vec<Inline>),
    ListItem(ListItem),
    TableCaption(Vec<Inline>),
    FigCaption(Vec<Inline>),
    TableRows(Vec<TableRowData>),
    TableRow(TableRowData),
    TableCellWithAlign { cell: TableCell, align: Alignment },
    Skip,
}

#[derive(Debug)]
struct TableRowData {
    is_thead: bool,
    cells: Vec<(TableCell, Alignment)>,
}

#[derive(Clone, Copy)]
enum Visit {
    Enter(Handle, u16),
    Exit(Handle),
}

/// Reads HTML markup from bytes into the shared Document IR.
pub fn read(input: &[u8], limits: &Limits) -> Result<ReadOutput, ReadError> {
    limits.validate()?;
    if let Some(limit) = limits.max_input_bytes
        && input.len() as u64 > limit
    {
        return Err(ReadError::InputTooLarge { limit });
    }

    let mut warnings = Vec::new();
    let decoded = decode_html(input, &mut warnings);

    let limit_hit: Rc<Cell<Option<LimitExceeded>>> = Rc::new(Cell::new(None));
    let node_limit = max_dom_nodes(input.len(), limits);
    let sink = HtmlSink::new(Rc::clone(&limit_hit), MAX_DOM_DEPTH, node_limit);
    let mut parser = html5ever::parse_document(sink, ParseOpts::default());

    let map_limit_error = |exceeded: LimitExceeded| match exceeded {
        LimitExceeded::NestingTooDeep { limit } => ReadError::NestingTooDeep { limit },
        LimitExceeded::TooManyNodes { limit } => ReadError::TooManyNodes { limit },
    };

    for chunk in chunk_str(&decoded, CHUNK_SIZE) {
        if let Some(exceeded) = limit_hit.get() {
            return Err(map_limit_error(exceeded));
        }
        let mut tendril = StrTendril::new();
        tendril.push_slice(chunk);
        parser.process(tendril);
    }

    if let Some(exceeded) = limit_hit.get() {
        return Err(map_limit_error(exceeded));
    }

    let (arena, doc_handle) = parser.finish();

    // Extract metadata from <head> and <html>
    let mut metadata = Metadata {
        source_format: Some(Format::Html),
        ..Metadata::default()
    };
    extract_metadata(&arena, doc_handle, &mut metadata);

    // Iteratively convert DOM tree into Document IR
    let mut mapper = TreeMapper::new(&arena, limits, warnings);
    let body_blocks = convert_tree_iteratively(&mut mapper, doc_handle)?;

    Ok(ReadOutput {
        document: Document {
            version: IR_VERSION.to_owned(),
            meta: metadata,
            body: body_blocks,
            assets: mapper.assets,
            layout: None,
            provenance: None,
            furniture: Vec::new(),
        },
        warnings: mapper.warnings,
    })
}

fn convert_tree_iteratively(
    mapper: &mut TreeMapper<'_>,
    doc_handle: Handle,
) -> Result<Vec<Block>, ReadError> {
    let num_nodes = mapper.arena.nodes.len();
    let mut converted: Vec<Option<ConvertedNode>> = (0..num_nodes).map(|_| None).collect();
    let mut stack = vec![Visit::Enter(doc_handle, 0)];

    while let Some(task) = stack.pop() {
        match task {
            Visit::Enter(handle, ir_depth) => {
                let node = &mapper.arena.nodes[handle];
                let is_container = match &node.data {
                    NodeData::Element { name, attrs, .. } => {
                        let local = &*name.local;
                        match local {
                            "blockquote" | "ul" | "ol" | "table" | "em" | "i" | "strong" | "b"
                            | "s" | "del" | "strike" | "sup" | "sub" | "a" => true,
                            _ => attrs.iter().any(|a| {
                                (&*a.name.local == "role" && &*a.value == "doc-footnote")
                                    || (&*a.name.local == "epub:type" && &*a.value == "footnote")
                            }),
                        }
                    }
                    _ => false,
                };

                let next_ir_depth = ir_depth + u16::from(is_container);
                if next_ir_depth > mapper.limits.max_nesting_depth {
                    return Err(ReadError::NestingTooDeep {
                        limit: mapper.limits.max_nesting_depth,
                    });
                }

                stack.push(Visit::Exit(handle));

                // Check if element is a dropped element or preformatted text that handles its own children
                let skip_children = match &node.data {
                    NodeData::Element { name, attrs, .. } => {
                        let local = &*name.local;
                        if is_math_container_with_tex(mapper.arena, handle, attrs) {
                            true
                        } else {
                            matches!(
                                local,
                                "script"
                                    | "style"
                                    | "template"
                                    | "noscript"
                                    | "iframe"
                                    | "object"
                                    | "embed"
                                    | "svg"
                                    | "canvas"
                                    | "button"
                                    | "select"
                                    | "textarea"
                                    | "pre"
                                    | "math"
                            )
                        }
                    }
                    _ => false,
                };

                if !skip_children {
                    for &child_handle in node.children.iter().rev() {
                        stack.push(Visit::Enter(child_handle, next_ir_depth));
                    }
                }
            }
            Visit::Exit(handle) => {
                let node = &mapper.arena.nodes[handle];
                let children: Vec<ConvertedNode> = node
                    .children
                    .iter()
                    .filter_map(|&ch| converted[ch].take())
                    .collect();
                let output = convert_node(mapper, handle, children)?;
                converted[handle] = Some(output);
            }
        }
    }

    match converted[doc_handle].take() {
        Some(ConvertedNode::Blocks(blks)) => Ok(blks),
        _ => Ok(Vec::new()),
    }
}

fn convert_node(
    mapper: &mut TreeMapper<'_>,
    handle: Handle,
    children: Vec<ConvertedNode>,
) -> Result<ConvertedNode, ReadError> {
    let node = &mapper.arena.nodes[handle];
    match &node.data {
        NodeData::Document => Ok(ConvertedNode::Blocks(blocks(mapper, children)?)),
        NodeData::Doctype { .. }
        | NodeData::Comment { .. }
        | NodeData::ProcessingInstruction { .. } => Ok(ConvertedNode::Skip),
        NodeData::Text { text } => {
            let collapsed = collapse_whitespace(text);
            if collapsed.is_empty() {
                Ok(ConvertedNode::Skip)
            } else {
                Ok(ConvertedNode::Inlines(vec![Inline::Text {
                    text: nfc(&collapsed),
                }]))
            }
        }
        NodeData::Element { name, attrs, .. } => {
            let local = &*name.local;
            match local {
                "head" => Ok(ConvertedNode::Skip),
                "html" | "body" => Ok(ConvertedNode::Blocks(blocks(mapper, children)?)),
                "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                    mapper.check_block_limit()?;
                    let level = local[1..].parse::<u8>().unwrap_or(1);
                    let content = trim_inlines(inlines(children));
                    Ok(ConvertedNode::Blocks(vec![Block::Heading {
                        level,
                        content,
                    }]))
                }
                "p" => {
                    let content = trim_inlines(inlines(children));
                    if content.is_empty() {
                        Ok(ConvertedNode::Skip)
                    } else {
                        mapper.check_block_limit()?;
                        Ok(ConvertedNode::Blocks(vec![Block::Paragraph { content }]))
                    }
                }
                "blockquote" => {
                    let blks = blocks(mapper, children)?;
                    if blks.is_empty() {
                        Ok(ConvertedNode::Skip)
                    } else {
                        mapper.check_block_limit()?;
                        Ok(ConvertedNode::Blocks(vec![Block::Quote { blocks: blks }]))
                    }
                }
                "ul" => {
                    let items = list_items(children);
                    mapper.check_block_limit()?;
                    Ok(ConvertedNode::Blocks(vec![Block::List {
                        ordered: false,
                        start: None,
                        tight: false,
                        items,
                    }]))
                }
                "ol" => {
                    let mut start = None;
                    for attr in attrs {
                        if &*attr.name.local == "start" {
                            if let Ok(val) = attr.value.parse::<u64>() {
                                start = Some(val);
                                mapper.warn_unsupported_once("ol start attribute was applied");
                            }
                        } else if &*attr.name.local == "reversed" {
                            mapper.warn_unsupported_once("ol reversed attribute is unsupported");
                        }
                    }
                    let items = list_items(children);
                    mapper.check_block_limit()?;
                    Ok(ConvertedNode::Blocks(vec![Block::List {
                        ordered: true,
                        start,
                        tight: false,
                        items,
                    }]))
                }
                "li" => {
                    let checked = check_task_item(mapper.arena, handle);
                    let item_blocks = blocks(mapper, children)?;
                    Ok(ConvertedNode::ListItem(ListItem {
                        checked,
                        blocks: item_blocks,
                    }))
                }
                "table" => {
                    mapper.check_block_limit()?;
                    let table = map_table(children);
                    Ok(ConvertedNode::Blocks(vec![table]))
                }
                "caption" => Ok(ConvertedNode::TableCaption(trim_inlines(inlines(children)))),
                "figcaption" => Ok(ConvertedNode::FigCaption(trim_inlines(inlines(children)))),
                "thead" => {
                    let mut rows = Vec::new();
                    for c in children {
                        match c {
                            ConvertedNode::TableRow(mut r) => {
                                r.is_thead = true;
                                rows.push(r);
                            }
                            ConvertedNode::TableRows(rs) => {
                                for mut r in rs {
                                    r.is_thead = true;
                                    rows.push(r);
                                }
                            }
                            _ => {}
                        }
                    }
                    Ok(ConvertedNode::TableRows(rows))
                }
                "tbody" | "tfoot" => {
                    let mut rows = Vec::new();
                    for c in children {
                        match c {
                            ConvertedNode::TableRow(mut r) => {
                                r.is_thead = false;
                                rows.push(r);
                            }
                            ConvertedNode::TableRows(rs) => {
                                for mut r in rs {
                                    r.is_thead = false;
                                    rows.push(r);
                                }
                            }
                            _ => {}
                        }
                    }
                    Ok(ConvertedNode::TableRows(rows))
                }
                "tr" => {
                    let mut cells = Vec::new();
                    for child in children {
                        if let ConvertedNode::TableCellWithAlign { cell, align } = child {
                            cells.push((cell, align));
                        }
                    }
                    Ok(ConvertedNode::TableRow(TableRowData {
                        is_thead: false,
                        cells,
                    }))
                }
                "th" | "td" => {
                    let is_header = local == "th";
                    let mut rowspan = 1;
                    let mut colspan = 1;
                    for attr in attrs {
                        if &*attr.name.local == "rowspan" {
                            rowspan = attr.value.parse::<u32>().unwrap_or(1).clamp(1, 1024);
                        } else if &*attr.name.local == "colspan" {
                            colspan = attr.value.parse::<u32>().unwrap_or(1).clamp(1, 1024);
                        }
                    }
                    let align = parse_align(attrs);
                    let cell_blocks = blocks(mapper, children)?;
                    Ok(ConvertedNode::TableCellWithAlign {
                        cell: TableCell {
                            rowspan,
                            colspan,
                            header: if is_header { Some(true) } else { None },
                            blocks: cell_blocks,
                        },
                        align,
                    })
                }
                "pre" => {
                    let (lang, text) = collect_pre_content(mapper.arena, handle);
                    mapper.check_block_limit()?;
                    Ok(ConvertedNode::Blocks(vec![Block::Code { lang, text }]))
                }
                "figure" => {
                    let mut img_asset = None;
                    let mut img_count = 0;
                    let mut figcaption = Vec::new();
                    let mut other_children = Vec::new();

                    for child in children {
                        match child {
                            ConvertedNode::Inlines(ref ins) => {
                                for inline in ins {
                                    if let Inline::Image { target, .. } = inline {
                                        img_count += 1;
                                        img_asset = Some(target.clone());
                                    }
                                }
                                other_children.push(child);
                            }
                            ConvertedNode::FigCaption(cap) => {
                                figcaption = cap;
                            }
                            _ => {
                                other_children.push(child);
                            }
                        }
                    }

                    if img_count == 1
                        && let Some(asset) = img_asset
                    {
                        mapper.check_block_limit()?;
                        Ok(ConvertedNode::Blocks(vec![Block::Figure {
                            asset,
                            caption: figcaption,
                        }]))
                    } else {
                        let mut blks = blocks(mapper, other_children)?;
                        if !figcaption.is_empty() {
                            mapper.check_block_limit()?;
                            blks.push(Block::Paragraph {
                                content: figcaption,
                            });
                        }
                        Ok(ConvertedNode::Blocks(blks))
                    }
                }
                "img" => {
                    let (src, alt, title) = parse_img_attrs(attrs);
                    let target = if src.starts_with("data:") {
                        match decode_data_uri_image(mapper, &src)? {
                            Some(target) => target,
                            None => AssetRef::Url { href: src },
                        }
                    } else {
                        AssetRef::Url { href: src }
                    };
                    Ok(ConvertedNode::Inlines(vec![Inline::Image {
                        target,
                        alt: nfc(&alt),
                        title: title.as_deref().and_then(nonempty),
                    }]))
                }
                "a" => {
                    let is_footnote_ref = attrs.iter().any(|a| {
                        (&*a.name.local == "role" && &*a.value == "doc-noteref")
                            || (&*a.name.local == "epub:type" && &*a.value == "noteref")
                    });
                    let href = attrs
                        .iter()
                        .find(|a| &*a.name.local == "href")
                        .map(|a| a.value.to_string())
                        .unwrap_or_default();

                    if is_footnote_ref {
                        let id = href.strip_prefix('#').unwrap_or(&href).to_owned();
                        Ok(ConvertedNode::Inlines(vec![Inline::FootnoteRef { id }]))
                    } else {
                        let title = attrs
                            .iter()
                            .find(|a| &*a.name.local == "title")
                            .map(|a| nfc(a.value.trim()));
                        let content = inlines(children);
                        if href.is_empty() {
                            Ok(ConvertedNode::Inlines(content))
                        } else if allowed_link(&href) {
                            Ok(ConvertedNode::Inlines(vec![Inline::Link {
                                url: href,
                                title,
                                content,
                            }]))
                        } else {
                            mapper.warn_link_dropped(&href);
                            Ok(ConvertedNode::Inlines(content))
                        }
                    }
                }
                "em" | "i" => Ok(ConvertedNode::Inlines(vec![Inline::Emph {
                    content: inlines(children),
                }])),
                "strong" | "b" => Ok(ConvertedNode::Inlines(vec![Inline::Strong {
                    content: inlines(children),
                }])),
                "s" | "del" | "strike" => Ok(ConvertedNode::Inlines(vec![Inline::Strikeout {
                    content: inlines(children),
                }])),
                "sup" => Ok(ConvertedNode::Inlines(vec![Inline::Superscript {
                    content: inlines(children),
                }])),
                "sub" => Ok(ConvertedNode::Inlines(vec![Inline::Subscript {
                    content: inlines(children),
                }])),
                "code" | "kbd" | "samp" => {
                    let text = collect_text(mapper.arena, handle);
                    Ok(ConvertedNode::Inlines(vec![Inline::Code { text }]))
                }
                "br" => Ok(ConvertedNode::Inlines(vec![Inline::LineBreak {}])),
                _ if is_math_container_with_tex(mapper.arena, handle, attrs) => {
                    let tex = find_math_tex_annotation(mapper.arena, handle)
                        .expect("math tex presence checked");
                    let is_display = attrs.iter().any(|a| {
                        &*a.name.local == "class"
                            && a.value
                                .split_ascii_whitespace()
                                .any(|c| c == "display" || c == "katex-display")
                    });
                    Ok(ConvertedNode::Inlines(vec![Inline::Math {
                        tex,
                        display: is_display,
                    }]))
                }
                _ if is_rendered_math_sibling(attrs) => {
                    if is_paired_with_math_tex(mapper.arena, handle) {
                        Ok(ConvertedNode::Skip)
                    } else {
                        mapper.warn_unsupported_once(
                            "standalone math helper element without paired TeX annotation was preserved as text",
                        );
                        match local {
                            "span" | "u" | "mark" | "small" | "abbr" | "cite" | "q" | "time"
                            | "font" | "label" | "ins" | "var" | "dfn" | "bdi" | "bdo" | "data"
                            | "ruby" | "rt" | "rp" | "wbr" | "tt" | "big" | "acronym" | "nobr" => {
                                Ok(ConvertedNode::Inlines(inlines(children)))
                            }
                            "p" => {
                                let content = trim_inlines(inlines(children));
                                if content.is_empty() {
                                    Ok(ConvertedNode::Skip)
                                } else {
                                    mapper.check_block_limit()?;
                                    Ok(ConvertedNode::Blocks(vec![Block::Paragraph { content }]))
                                }
                            }
                            _ => Ok(ConvertedNode::Blocks(blocks(mapper, children)?)),
                        }
                    }
                }
                "span" | "u" | "mark" | "small" | "abbr" | "cite" | "q" | "time" | "font"
                | "label" | "ins" | "var" | "dfn" | "bdi" | "bdo" | "data" | "ruby" | "rt"
                | "rp" | "wbr" | "tt" | "big" | "acronym" | "nobr" => {
                    Ok(ConvertedNode::Inlines(inlines(children)))
                }
                "hr" => {
                    mapper.warn_dropped_once("hr");
                    Ok(ConvertedNode::Skip)
                }
                "math" => {
                    if let Some(tex) = find_math_tex_annotation(mapper.arena, handle) {
                        let is_display = attrs.iter().any(|a| {
                            (&*a.name.local == "display" && &*a.value == "block")
                                || (&*a.name.local == "mode" && &*a.value == "display")
                        });
                        Ok(ConvertedNode::Inlines(vec![Inline::Math {
                            tex,
                            display: is_display,
                        }]))
                    } else {
                        mapper.warn_unsupported_once(
                            "math element without application/x-tex annotation was reduced to text",
                        );
                        let text = collect_text(mapper.arena, handle);
                        Ok(ConvertedNode::Inlines(vec![Inline::Text {
                            text: nfc(&text),
                        }]))
                    }
                }
                "script" | "style" | "template" | "noscript" | "iframe" | "object" | "embed"
                | "svg" | "canvas" | "button" | "select" | "textarea" => {
                    mapper.warn_dropped_once(local);
                    Ok(ConvertedNode::Skip)
                }
                "input" => {
                    // Handled inside task item li; bare input is dropped
                    if !is_task_checkbox(mapper.arena, handle) {
                        mapper.warn_dropped_once("input");
                    }
                    Ok(ConvertedNode::Skip)
                }
                _ => {
                    // Check footnote container
                    let is_footnote = attrs.iter().any(|a| {
                        (&*a.name.local == "role" && &*a.value == "doc-footnote")
                            || (&*a.name.local == "epub:type" && &*a.value == "footnote")
                    });
                    if is_footnote {
                        let id = attrs
                            .iter()
                            .find(|a| &*a.name.local == "id")
                            .map(|a| a.value.to_string())
                            .unwrap_or_default();
                        mapper.check_block_limit()?;
                        Ok(ConvertedNode::Blocks(vec![Block::Footnote {
                            id,
                            blocks: blocks(mapper, children)?,
                        }]))
                    } else {
                        // Non-semantic container (div, section, article, main, header, footer, nav, aside, etc.)
                        Ok(ConvertedNode::Blocks(blocks(mapper, children)?))
                    }
                }
            }
        }
    }
}

fn blocks(
    mapper: &mut TreeMapper<'_>,
    children: Vec<ConvertedNode>,
) -> Result<Vec<Block>, ReadError> {
    let mut blocks = Vec::new();
    let mut pending_inlines = Vec::new();

    for child in children {
        match child {
            ConvertedNode::Inlines(inlines) => {
                pending_inlines.extend(inlines);
            }
            ConvertedNode::Blocks(inner_blocks) => {
                flush_inlines(mapper, &mut pending_inlines, &mut blocks)?;
                blocks.extend(inner_blocks);
            }
            ConvertedNode::ListItem(item) => {
                flush_inlines(mapper, &mut pending_inlines, &mut blocks)?;
                mapper.check_block_limit()?;
                blocks.push(Block::List {
                    ordered: false,
                    start: None,
                    tight: false,
                    items: vec![item],
                });
            }
            ConvertedNode::TableRows(rows) => {
                flush_inlines(mapper, &mut pending_inlines, &mut blocks)?;
                for row in rows {
                    for (cell, _) in row.cells {
                        blocks.extend(cell.blocks);
                    }
                }
            }
            ConvertedNode::TableRow(row) => {
                flush_inlines(mapper, &mut pending_inlines, &mut blocks)?;
                for (cell, _) in row.cells {
                    blocks.extend(cell.blocks);
                }
            }
            ConvertedNode::TableCellWithAlign { cell, .. } => {
                flush_inlines(mapper, &mut pending_inlines, &mut blocks)?;
                blocks.extend(cell.blocks);
            }
            ConvertedNode::TableCaption(cap) | ConvertedNode::FigCaption(cap) => {
                flush_inlines(mapper, &mut pending_inlines, &mut blocks)?;
                if !cap.is_empty() {
                    mapper.check_block_limit()?;
                    blocks.push(Block::Paragraph { content: cap });
                }
            }
            ConvertedNode::Skip => {}
        }
    }
    flush_inlines(mapper, &mut pending_inlines, &mut blocks)?;
    Ok(blocks)
}

fn flush_inlines(
    mapper: &mut TreeMapper<'_>,
    pending_inlines: &mut Vec<Inline>,
    blocks: &mut Vec<Block>,
) -> Result<(), ReadError> {
    if !pending_inlines.is_empty() {
        let inlines = std::mem::take(pending_inlines);
        let trimmed = trim_inlines(inlines);
        if !trimmed.is_empty() {
            mapper.check_block_limit()?;
            blocks.push(Block::Paragraph { content: trimmed });
        }
    }
    Ok(())
}

fn inlines(children: Vec<ConvertedNode>) -> Vec<Inline> {
    let mut inlines = Vec::new();
    for child in children {
        match child {
            ConvertedNode::Inlines(ins) => inlines.extend(ins),
            ConvertedNode::Blocks(blks) => {
                for blk in blks {
                    inlines.extend(extract_inlines_from_block(blk));
                }
            }
            _ => {}
        }
    }
    inlines
}

fn extract_inlines_from_block(block: Block) -> Vec<Inline> {
    match block {
        Block::Paragraph { content } | Block::Heading { content, .. } => content,
        Block::Code { text, .. } => vec![Inline::Code { text }],
        Block::Quote { blocks } => blocks
            .into_iter()
            .flat_map(extract_inlines_from_block)
            .collect(),
        _ => Vec::new(),
    }
}

fn list_items(children: Vec<ConvertedNode>) -> Vec<ListItem> {
    let mut items = Vec::new();
    let mut pending_inlines = Vec::new();

    for child in children {
        match child {
            ConvertedNode::ListItem(item) => {
                if !pending_inlines.is_empty() {
                    let inlines = std::mem::take(&mut pending_inlines);
                    let trimmed = trim_inlines(inlines);
                    if !trimmed.is_empty() {
                        items.push(ListItem {
                            checked: None,
                            blocks: vec![Block::Paragraph { content: trimmed }],
                        });
                    }
                }
                items.push(item);
            }
            ConvertedNode::Inlines(ins) => {
                pending_inlines.extend(ins);
            }
            ConvertedNode::Blocks(blks) => {
                if !pending_inlines.is_empty() {
                    let inlines = std::mem::take(&mut pending_inlines);
                    let trimmed = trim_inlines(inlines);
                    if !trimmed.is_empty() {
                        items.push(ListItem {
                            checked: None,
                            blocks: vec![Block::Paragraph { content: trimmed }],
                        });
                    }
                }
                if !blks.is_empty() {
                    items.push(ListItem {
                        checked: None,
                        blocks: blks,
                    });
                }
            }
            _ => {}
        }
    }
    if !pending_inlines.is_empty() {
        let inlines = std::mem::take(&mut pending_inlines);
        let trimmed = trim_inlines(inlines);
        if !trimmed.is_empty() {
            items.push(ListItem {
                checked: None,
                blocks: vec![Block::Paragraph { content: trimmed }],
            });
        }
    }
    items
}

fn collapse_whitespace(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut in_whitespace = false;
    for ch in text.chars() {
        if ch.is_ascii_whitespace() {
            if !in_whitespace {
                result.push(' ');
                in_whitespace = true;
            }
        } else {
            result.push(ch);
            in_whitespace = false;
        }
    }
    result
}

fn trim_inlines(mut inlines: Vec<Inline>) -> Vec<Inline> {
    let mut prev_ends_with_space = true;
    collapse_inlines_whitespace(&mut inlines, &mut prev_ends_with_space);
    remove_empty_inlines(&mut inlines);
    trim_inlines_trailing(&mut inlines);
    inlines
}

fn collapse_inlines_whitespace(inlines: &mut [Inline], prev_ends_with_space: &mut bool) {
    for inline in inlines {
        match inline {
            Inline::Text { text } => {
                if *prev_ends_with_space && text.starts_with(' ') {
                    let trimmed = text.trim_start_matches(' ');
                    *text = trimmed.to_owned();
                }
                if !text.is_empty() {
                    *prev_ends_with_space = text.ends_with(' ');
                }
            }
            Inline::Emph { content }
            | Inline::Strong { content }
            | Inline::Strikeout { content }
            | Inline::Superscript { content }
            | Inline::Subscript { content }
            | Inline::Link { content, .. } => {
                collapse_inlines_whitespace(content, prev_ends_with_space);
            }
            Inline::LineBreak {} | Inline::SoftBreak {} => {
                *prev_ends_with_space = true;
            }
            Inline::Code { .. }
            | Inline::Math { .. }
            | Inline::Image { .. }
            | Inline::FootnoteRef { .. }
            | Inline::Raw { .. } => {
                *prev_ends_with_space = false;
            }
        }
    }
}

fn trim_inlines_trailing(inlines: &mut Vec<Inline>) {
    while let Some(last) = inlines.last_mut() {
        match last {
            Inline::Text { text } => {
                let trimmed = text.trim_end_matches(' ');
                if trimmed.is_empty() {
                    inlines.pop();
                    continue;
                } else if trimmed.len() != text.len() {
                    *text = trimmed.to_owned();
                }
                break;
            }
            Inline::Emph { content }
            | Inline::Strong { content }
            | Inline::Strikeout { content }
            | Inline::Superscript { content }
            | Inline::Subscript { content }
            | Inline::Link { content, .. } => {
                trim_inlines_trailing(content);
                if content.is_empty() {
                    inlines.pop();
                    continue;
                }
                break;
            }
            _ => break,
        }
    }
}

fn remove_empty_inlines(inlines: &mut Vec<Inline>) {
    inlines.retain_mut(|inline| match inline {
        Inline::Text { text } => !text.is_empty(),
        Inline::Emph { content }
        | Inline::Strong { content }
        | Inline::Strikeout { content }
        | Inline::Superscript { content }
        | Inline::Subscript { content }
        | Inline::Link { content, .. } => {
            remove_empty_inlines(content);
            !content.is_empty()
        }
        _ => true,
    });
}

fn check_task_item(arena: &Arena, li_handle: Handle) -> Option<bool> {
    for &child in &arena.nodes[li_handle].children {
        let node = &arena.nodes[child];
        if let NodeData::Element { name, attrs, .. } = &node.data {
            if &*name.local == "input" {
                if let Some(checked) = get_checkbox_checked(attrs) {
                    return Some(checked);
                }
            } else if &*name.local == "p" {
                for &p_child in &node.children {
                    let p_node = &arena.nodes[p_child];
                    if let NodeData::Element {
                        name: p_name,
                        attrs: p_attrs,
                        ..
                    } = &p_node.data
                        && &*p_name.local == "input"
                        && let Some(checked) = get_checkbox_checked(p_attrs)
                    {
                        return Some(checked);
                    }
                }
            }
        }
    }
    None
}

fn get_checkbox_checked(attrs: &[html5ever::Attribute]) -> Option<bool> {
    let is_checkbox = attrs
        .iter()
        .any(|a| &*a.name.local == "type" && a.value.eq_ignore_ascii_case("checkbox"));
    if is_checkbox {
        Some(attrs.iter().any(|a| &*a.name.local == "checked"))
    } else {
        None
    }
}

fn is_task_checkbox(arena: &Arena, handle: Handle) -> bool {
    let node = &arena.nodes[handle];
    if let NodeData::Element { ref attrs, .. } = node.data {
        let is_checkbox = attrs
            .iter()
            .any(|a| &*a.name.local == "type" && a.value.eq_ignore_ascii_case("checkbox"));
        if !is_checkbox {
            return false;
        }
        if let Some(parent) = node.parent {
            let parent_node = &arena.nodes[parent];
            if let NodeData::Element { ref name, .. } = parent_node.data {
                if &*name.local == "li" {
                    return true;
                }
                if &*name.local == "p"
                    && let Some(grandparent) = parent_node.parent
                    && let NodeData::Element {
                        name: ref gp_name, ..
                    } = arena.nodes[grandparent].data
                {
                    return &*gp_name.local == "li";
                }
            }
        }
    }
    false
}

fn collect_pre_content(arena: &Arena, pre_handle: Handle) -> (Option<String>, String) {
    let mut text = String::new();
    let mut lang = None;

    let pre_node = &arena.nodes[pre_handle];
    if let NodeData::Element { attrs, .. } = &pre_node.data {
        for attr in attrs {
            if &*attr.name.local == "class" {
                lang = extract_lang_from_class(&attr.value);
            }
        }
    }

    let mut stack = vec![pre_handle];
    while let Some(h) = stack.pop() {
        let node = &arena.nodes[h];
        if let NodeData::Text { text: t } = &node.data {
            text.push_str(t);
        } else if let NodeData::Element { name, attrs, .. } = &node.data
            && &*name.local == "code"
            && lang.is_none()
        {
            for attr in attrs {
                if &*attr.name.local == "class" {
                    lang = extract_lang_from_class(&attr.value);
                }
            }
        }
        stack.extend(node.children.iter().copied().rev());
    }

    (lang, text)
}

fn extract_lang_from_class(class_val: &str) -> Option<String> {
    for token in class_val.split_whitespace() {
        if let Some(rest) = token.strip_prefix("language-")
            && !rest.is_empty()
        {
            return Some(rest.to_owned());
        }
    }
    None
}

fn is_math_container(attrs: &[html5ever::Attribute]) -> bool {
    attrs.iter().any(|a| {
        &*a.name.local == "class"
            && a.value
                .split_ascii_whitespace()
                .any(|c| c == "katex" || c == "katex-display" || c == "MathJax" || c == "math")
    })
}

fn is_math_container_with_tex(
    arena: &Arena,
    handle: Handle,
    attrs: &[html5ever::Attribute],
) -> bool {
    is_math_container(attrs) && find_math_tex_annotation(arena, handle).is_some()
}

fn is_rendered_math_sibling(attrs: &[html5ever::Attribute]) -> bool {
    attrs.iter().any(|a| {
        &*a.name.local == "class"
            && a.value.split_ascii_whitespace().any(|c| {
                c == "katex-html"
                    || c == "MJX_Assistive_MathML"
                    || c.eq_ignore_ascii_case("mjx_assistive_mathml")
                    || c == "MathJax_Preview"
            })
    })
}

fn is_paired_with_math_tex(arena: &Arena, handle: Handle) -> bool {
    let mut curr = arena.nodes[handle].parent;
    for _ in 0..3 {
        let Some(p) = curr else { break };
        if find_math_tex_annotation(arena, p).is_some() {
            return true;
        }
        if arena.nodes[p]
            .children
            .iter()
            .any(|&child| child != handle && find_math_tex_annotation(arena, child).is_some())
        {
            return true;
        }
        curr = arena.nodes[p].parent;
    }
    false
}

fn parse_img_attrs(attrs: &[html5ever::Attribute]) -> (String, String, Option<String>) {
    let mut src = String::new();
    let mut alt = String::new();
    let mut title = None;

    for attr in attrs {
        let local = &*attr.name.local;
        match local {
            "src" => src = attr.value.to_string(),
            "alt" => alt = attr.value.to_string(),
            "title" => title = Some(attr.value.to_string()),
            _ => {}
        }
    }
    (src, alt, title)
}

fn decode_data_uri_image(
    mapper: &mut TreeMapper<'_>,
    uri: &str,
) -> Result<Option<AssetRef>, ReadError> {
    let Some(data_part) = uri.strip_prefix("data:") else {
        return Ok(None);
    };
    let Some((meta, encoded)) = data_part.split_once(',') else {
        return Ok(None);
    };
    let mut parts = meta.split(';');
    let _ = parts.next();
    let is_base64 = parts.any(|p| p.eq_ignore_ascii_case("base64"));
    if !is_base64 {
        return Ok(None);
    }
    let encoded = encoded.trim();

    // Pre-check estimated size before decoding (base64 is ~4/3 of binary size)
    let estimated_len = (encoded.len() as u64).saturating_mul(3) / 4;
    if let Some(max_asset) = mapper.limits.max_asset_bytes
        && estimated_len > max_asset
    {
        mapper.warnings.push(Warning::new(
            WarningCode::ImageNotEmbedded,
            format!("data URI image estimated size exceeds limit of {max_asset} bytes"),
        ));
        return Ok(None);
    }

    use base64::Engine;
    let decoded = match base64::engine::general_purpose::STANDARD.decode(encoded) {
        Ok(bytes) => bytes,
        Err(_) => return Ok(None),
    };

    if let Some(max_asset) = mapper.limits.max_asset_bytes
        && decoded.len() as u64 > max_asset
    {
        mapper.warnings.push(Warning::new(
            WarningCode::ImageNotEmbedded,
            format!("data URI image size exceeds limit of {max_asset} bytes"),
        ));
        return Ok(None);
    }

    let sniffed_type = sniff_image_type(&decoded);
    let media_type = match sniffed_type {
        Some(t) => t.to_owned(),
        None => {
            mapper.warnings.push(Warning::new(
                WarningCode::ImageNotEmbedded,
                "data URI content is not a supported image format (expected PNG, JPEG, GIF, or WEBP)",
            ));
            return Ok(None);
        }
    };

    let asset_id = hex::encode(Sha256::digest(&decoded));
    mapper.assets.insert(
        asset_id.clone(),
        Asset {
            media_type,
            bytes: decoded,
        },
    );
    Ok(Some(AssetRef::Asset { id: asset_id }))
}

fn find_math_tex_annotation(arena: &Arena, math_handle: Handle) -> Option<String> {
    let mut stack = vec![math_handle];
    while let Some(h) = stack.pop() {
        let node = &arena.nodes[h];
        if let NodeData::Element { name, attrs, .. } = &node.data
            && &*name.local == "annotation"
        {
            let is_tex = attrs
                .iter()
                .any(|a| &*a.name.local == "encoding" && &*a.value == "application/x-tex");
            if is_tex {
                return Some(collect_text(arena, h));
            }
        }
        stack.extend(node.children.iter().copied().rev());
    }
    None
}

fn parse_align(attrs: &[html5ever::Attribute]) -> Alignment {
    for attr in attrs {
        let local = &*attr.name.local;
        if local == "align" {
            let val = attr.value.trim().to_ascii_lowercase();
            return match val.as_str() {
                "left" => Alignment::Left,
                "center" => Alignment::Center,
                "right" => Alignment::Right,
                _ => Alignment::Default,
            };
        } else if local == "style" {
            let val = attr.value.to_ascii_lowercase();
            if let Some(pos) = val.find("text-align") {
                let after = &val[pos + 10..];
                if let Some(rest) = after.trim_start().strip_prefix(':') {
                    let token = rest.trim_start().split(';').next().unwrap_or("").trim();
                    return match token {
                        "left" => Alignment::Left,
                        "center" => Alignment::Center,
                        "right" => Alignment::Right,
                        _ => Alignment::Default,
                    };
                }
            }
        }
    }
    Alignment::Default
}

fn map_table(children: Vec<ConvertedNode>) -> Block {
    let mut caption = None;
    let mut raw_rows: Vec<TableRowData> = Vec::new();

    for child in children {
        match child {
            ConvertedNode::TableCaption(cap) => {
                if caption.is_none() && !cap.is_empty() {
                    caption = Some(cap);
                }
            }
            ConvertedNode::TableRow(row) => {
                raw_rows.push(row);
            }
            ConvertedNode::TableRows(rows) => {
                raw_rows.extend(rows);
            }
            _ => {}
        }
    }

    let mut head = Vec::new();
    let mut body = Vec::new();
    let mut alignments: Vec<Alignment> = Vec::new();

    let has_thead = raw_rows.iter().any(|r| r.is_thead);

    if has_thead {
        for row in raw_rows {
            let mut row_cells = Vec::new();
            let mut grid_col = 0;
            for (cell, align) in row.cells {
                let span = cell.colspan as usize;
                if grid_col + span > alignments.len() {
                    alignments.resize((grid_col + span).min(1024), Alignment::Default);
                }
                if align != Alignment::Default
                    && grid_col < alignments.len()
                    && alignments[grid_col] == Alignment::Default
                {
                    alignments[grid_col] = align;
                }
                grid_col = grid_col.saturating_add(span);
                row_cells.push(cell);
            }
            if row.is_thead {
                head.push(row_cells);
            } else {
                body.push(row_cells);
            }
        }
    } else {
        // No thead: leading run of rows where all cells are th become head rows.
        let mut in_leading_head = true;
        for row in raw_rows {
            let all_th =
                !row.cells.is_empty() && row.cells.iter().all(|(c, _)| c.header == Some(true));
            let is_head_row = in_leading_head && all_th;
            if !is_head_row {
                in_leading_head = false;
            }

            let mut row_cells = Vec::new();
            let mut grid_col = 0;
            for (cell, align) in row.cells {
                let span = cell.colspan as usize;
                if grid_col + span > alignments.len() {
                    alignments.resize((grid_col + span).min(1024), Alignment::Default);
                }
                if align != Alignment::Default
                    && grid_col < alignments.len()
                    && alignments[grid_col] == Alignment::Default
                {
                    alignments[grid_col] = align;
                }
                grid_col = grid_col.saturating_add(span);
                row_cells.push(cell);
            }

            if is_head_row {
                head.push(row_cells);
            } else {
                body.push(row_cells);
            }
        }
    }

    let mut col_count = alignments.len();
    for row in head.iter().chain(body.iter()) {
        let row_cols: usize = row.iter().map(|c| c.colspan as usize).sum();
        col_count = col_count.max(row_cols);
    }
    col_count = col_count.min(1024);
    alignments.resize(col_count, Alignment::Default);
    let columns = alignments
        .into_iter()
        .map(|align| ColumnSpec { align })
        .collect();

    Block::Table {
        caption,
        columns,
        head,
        body,
        footnotes: Vec::new(),
    }
}

fn extract_metadata(arena: &Arena, doc_handle: Handle, metadata: &mut Metadata) {
    // 1. Find <html> element
    let doc_node = &arena.nodes[doc_handle];
    let html_handle = doc_node.children.iter().copied().find(|&h| {
        let n = &arena.nodes[h];
        if let NodeData::Element { ref name, .. } = n.data {
            name.ns == html5ever::ns!(html) && &*name.local == "html"
        } else {
            false
        }
    });

    let Some(html_h) = html_handle else {
        return;
    };

    let html_node = &arena.nodes[html_h];
    if let NodeData::Element { ref attrs, .. } = html_node.data {
        for attr in attrs {
            if &*attr.name.local == "lang" {
                metadata.language = nonempty(&nfc(attr.value.trim()));
            }
        }
    }

    // 2. Find <head> element under <html>
    let head_handle = html_node.children.iter().copied().find(|&h| {
        let n = &arena.nodes[h];
        if let NodeData::Element { ref name, .. } = n.data {
            name.ns == html5ever::ns!(html) && &*name.local == "head"
        } else {
            false
        }
    });

    let Some(head_h) = head_handle else {
        return;
    };

    // 3. Walk only inside <head>, HTML namespace only
    let mut stack = vec![head_h];
    while let Some(handle) = stack.pop() {
        let node = &arena.nodes[handle];
        if let NodeData::Element {
            ref name,
            ref attrs,
            ..
        } = node.data
            && name.ns == html5ever::ns!(html)
        {
            if &*name.local == "title" && metadata.title.is_none() {
                let text = collect_text(arena, handle);
                metadata.title = nonempty(&nfc(text.trim()));
            } else if &*name.local == "meta" {
                let mut meta_name: Option<String> = None;
                let mut content: Option<String> = None;
                for attr in attrs {
                    if &*attr.name.local == "name" {
                        meta_name = Some(attr.value.to_string());
                    } else if &*attr.name.local == "content" {
                        content = Some(attr.value.to_string());
                    }
                }
                if let (Some(m_name), Some(m_content)) = (meta_name, content) {
                    let m_name_lower = m_name.to_ascii_lowercase();
                    match m_name_lower.as_str() {
                        "author" => {
                            let author_clean = nfc(m_content.trim());
                            if !author_clean.is_empty() {
                                metadata.authors.push(author_clean);
                            }
                        }
                        "description" | "subject" => {
                            if metadata.subject.is_none() {
                                metadata.subject = nonempty(&nfc(m_content.trim()));
                            }
                        }
                        "date" => {
                            if metadata.date.is_none() {
                                metadata.date = nonempty(&nfc(m_content.trim()));
                            }
                        }
                        "keywords" if metadata.keywords.is_empty() => {
                            metadata.keywords = m_content
                                .split(',')
                                .map(|kw| nfc(kw.trim()))
                                .filter(|kw| !kw.is_empty())
                                .collect();
                        }
                        _ => {}
                    }
                }
            }
        }
        stack.extend(node.children.iter().copied().rev());
    }
}

fn collect_text(arena: &Arena, handle: Handle) -> String {
    let mut text_acc = String::new();
    let mut stack = vec![handle];
    while let Some(h) = stack.pop() {
        let node = &arena.nodes[h];
        if let NodeData::Text { ref text } = node.data {
            text_acc.push_str(text);
        }
        stack.extend(node.children.iter().copied().rev());
    }
    text_acc
}
