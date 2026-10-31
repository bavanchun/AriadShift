use std::{
    borrow::Cow,
    collections::{BTreeSet, HashMap},
};

use comrak::{
    Arena, Options,
    nodes::{AstNode, ListType, NodeValue, TableAlignment},
};
use thiserror::Error;

use crate::{
    format::Format,
    ir::{
        Alignment, AssetRef, AssetStore, Block, ColumnSpec, Document, IR_VERSION, Inline, ListItem,
        RawFormat, TableCell,
    },
    limits::{Limits, LimitsError},
    reader::{
        common::{self, nfc, nonempty},
        front_matter::parse_front_matter,
    },
    warning::{SourcePos, Warning, WarningCode},
};

pub use common::ReadOutput;

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum ReadError {
    #[error(transparent)]
    InvalidLimits(#[from] LimitsError),
    #[error("Markdown input exceeds the {limit}-byte limit")]
    InputTooLarge { limit: u64 },
    #[error("Markdown nesting exceeds the configured depth of {limit}")]
    NestingTooDeep { limit: u16 },
    #[error("Markdown contains more than the configured {limit} block limit")]
    TooManyBlocks { limit: u32 },
}

#[derive(Debug)]
enum ConvertedNode {
    Blocks(Vec<Block>),
    Inlines(Vec<Inline>),
    ListItem(ListItem),
    TableRow { header: bool, cells: Vec<TableCell> },
    TableCell(TableCell),
    Skip,
}

#[derive(Clone, Copy)]
enum Visit<'a> {
    Enter(&'a AstNode<'a>, u16),
    Exit(&'a AstNode<'a>),
}

#[derive(Default)]
struct WalkState {
    assets: AssetStore,
    shared_data_urls: HashMap<String, String>,
    shared_image_targets: HashMap<String, Option<AssetRef>>,
    shared_url_bytes_remaining: usize,
    block_nodes: u64,
    warnings: Vec<Warning>,
    footnote_references: BTreeSet<String>,
    footnote_definitions: BTreeSet<String>,
}

/// Reads CommonMark and GFM Markdown into the shared document IR.
/// Nesting limits count both block and inline containers.
pub fn read(markdown: &str, limits: &Limits) -> Result<ReadOutput, ReadError> {
    read_impl(markdown, limits, true)
}

fn read_impl(
    markdown: &str,
    limits: &Limits,
    share_reference_data: bool,
) -> Result<ReadOutput, ReadError> {
    limits.validate()?;
    if let Some(limit) = limits.max_input_bytes
        && markdown.len() as u64 > limit
    {
        return Err(ReadError::InputTooLarge { limit });
    }

    let markdown = markdown
        .strip_prefix('\u{feff}')
        .unwrap_or(markdown)
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    // Only block syntax can be bounded before parsing without treating literal
    // brackets, emphasis markers, code or math as nested inline containers.
    prescan_nesting(&markdown, limits.max_nesting_depth)?;

    let front_matter = front_matter_content(&markdown)
        .map(|yaml| parse_front_matter(yaml, limits.max_front_matter_bytes));
    let (footnote_references, footnote_definitions) = scan_footnotes(&markdown);
    let mut options = Options::default();
    options.extension.table = true;
    options.extension.strikethrough = true;
    options.extension.autolink = true;
    options.extension.tasklist = true;
    options.extension.footnotes = true;
    options.extension.math_dollars = true;
    options.extension.front_matter_delimiter = Some("---".to_owned());
    options.extension.shortcodes = true;
    options.parse.leave_footnote_definitions = true;

    let shared = if share_reference_data {
        intern_reference_data_urls(&markdown, &options)
    } else {
        ReferenceData {
            input: Cow::Borrowed(markdown.as_str()),
            ..ReferenceData::default()
        }
    };
    let arena = Arena::new();
    let root = comrak::parse_document(&arena, &shared.input, &options);
    let mut state = WalkState {
        shared_data_urls: shared.destinations,
        shared_url_bytes_remaining: markdown.len().min(100_000),
        footnote_references,
        footnote_definitions,
        ..WalkState::default()
    };
    let mut converted = HashMap::new();
    let mut stack = vec![Visit::Enter(root, 0)];
    let mut sharing_fallback_warned = false;

    while let Some(task) = stack.pop() {
        match task {
            Visit::Enter(node, depth) => {
                let (is_container, is_block) = {
                    let value = &node.data().value;
                    let is_container = matches!(
                        value,
                        NodeValue::BlockQuote
                            | NodeValue::MultilineBlockQuote(_)
                            | NodeValue::Alert(_)
                            | NodeValue::List(_)
                            | NodeValue::Table(_)
                            | NodeValue::FootnoteDefinition(_)
                            | NodeValue::Emph
                            | NodeValue::Strong
                            | NodeValue::Strikethrough
                            | NodeValue::Superscript
                            | NodeValue::Subscript
                            | NodeValue::Link(_)
                            | NodeValue::Image(_)
                    );
                    let is_block = value.block()
                        && !matches!(value, NodeValue::Document | NodeValue::FrontMatter(_));
                    (is_container, is_block)
                };
                let child_depth = depth + u16::from(is_container);
                if child_depth > limits.max_nesting_depth {
                    return Err(ReadError::NestingTooDeep {
                        limit: limits.max_nesting_depth,
                    });
                }
                if is_block {
                    state.block_nodes += 1;
                    if state.block_nodes > u64::from(limits.max_blocks) {
                        return Err(ReadError::TooManyBlocks {
                            limit: limits.max_blocks,
                        });
                    }
                }

                stack.push(Visit::Exit(node));
                let children: Vec<_> = node.children().collect();
                stack.extend(
                    children
                        .into_iter()
                        .rev()
                        .map(|child| Visit::Enter(child, child_depth)),
                );
            }
            Visit::Exit(node) => {
                if shared.may_have_unresolved_images && !sharing_fallback_warned {
                    let possible_shared_image = match &node.data().value {
                        NodeValue::Image(link) => link.url.starts_with("data:"),
                        NodeValue::Text(text) => text.contains("!["),
                        _ => false,
                    };
                    if possible_shared_image {
                        state.warnings.push(Warning::new(
                            WarningCode::UnsupportedNode,
                            "shared data image references could not be interned; unresolved references may remain text",
                        ));
                        sharing_fallback_warned = true;
                    }
                }
                let children = node
                    .children()
                    .map(|child| {
                        converted
                            .remove(&node_key(child))
                            .unwrap_or(ConvertedNode::Skip)
                    })
                    .collect::<Vec<_>>();
                let output = convert_node(node, children, &mut state, limits);
                converted.insert(node_key(node), output);
            }
        }
    }

    let blocks = match converted.remove(&node_key(root)) {
        Some(ConvertedNode::Blocks(blocks)) => blocks,
        _ => Vec::new(),
    };
    append_footnote_warnings(
        &state.footnote_references,
        &state.footnote_definitions,
        &mut state.warnings,
    );

    let mut metadata = front_matter
        .as_ref()
        .map(|output| output.metadata.clone())
        .unwrap_or_default();
    metadata.source_format = Some(Format::Markdown);
    let mut warnings = front_matter
        .map(|output| output.warnings)
        .unwrap_or_default();
    warnings.append(&mut state.warnings);

    let document = Document {
        version: IR_VERSION.to_owned(),
        meta: metadata,
        body: blocks,
        assets: state.assets,
        ..Document::default()
    };
    Ok(ReadOutput { document, warnings })
}

fn node_key(node: &AstNode<'_>) -> usize {
    std::ptr::from_ref(node).cast::<()>() as usize
}

// Comrak removes standalone definitions from the AST. Only the gaps between
// its block source spans are eligible: code, HTML, titles and other visible
// source stay byte-exact, so they need no token restoration. The small scanner
// below delimits destinations within those parser-consumed spans, including
// definitions with multiline labels, destinations and titles.
//
// Tokens cannot originate in user input: U+E000 is absent, interning is
// disabled for numeric character references, named entities cannot produce
// U+E000, and backslash escapes only unescape ASCII punctuation. Thus an exact token destination is
// sufficient proof of origin, regardless of reference/container context. Never
// reparse a reference use outside that context to decide whether to restore it.
#[derive(Default)]
struct ReferenceData<'a> {
    input: Cow<'a, str>,
    destinations: HashMap<String, String>,
    may_have_unresolved_images: bool,
}

fn intern_reference_data_urls<'a>(markdown: &'a str, options: &Options<'_>) -> ReferenceData<'a> {
    let original = || ReferenceData {
        input: Cow::Borrowed(markdown),
        ..ReferenceData::default()
    };
    if !markdown.contains("data:") || markdown.contains('\u{e000}') || markdown.contains("&#") {
        return original();
    }
    let arena = Arena::new();
    let root = comrak::parse_document(&arena, markdown, options);
    let mut offsets: Vec<_> = std::iter::once(0)
        .chain(markdown.match_indices('\n').map(|(offset, _)| offset + 1))
        .collect();
    if offsets.last() != Some(&markdown.len()) {
        offsets.push(markdown.len());
    }
    let mut visible = vec![false; offsets.len()];
    for node in root.children() {
        let position = node.data().sourcepos;
        visible[position.start.line..=position.end.line].fill(true);
    }
    // Definitions within visible container/paragraph spans stay on the native
    // path. Preserve a diagnostic if that path still leaves image references
    // as text after the final parse; never silently lose an unassociated image.
    let may_have_unresolved_images = root
        .descendants()
        .any(|node| matches!(&node.data().value, NodeValue::Text(text) if text.contains("![")));
    let mut replacements = Vec::new();
    let mut line = 1;
    while line < offsets.len() {
        if visible[line] {
            line += 1;
            continue;
        }
        let start = offsets[line - 1];
        while line < offsets.len() && !visible[line] {
            line += 1;
        }
        let end = offsets[line - 1];
        let mut position = start;
        while position < end {
            position += markdown[position..end]
                .bytes()
                .take_while(u8::is_ascii_whitespace)
                .count();
            if position == end {
                break;
            }
            let Some((destination, consumed)) = reference_definition_span(&markdown[position..end])
            else {
                return ReferenceData {
                    may_have_unresolved_images: true,
                    ..original()
                };
            };
            replacements.push(position + destination.start..position + destination.end);
            position += consumed;
        }
    }
    let mut input = String::with_capacity(markdown.len());
    let mut destinations = HashMap::new();
    let mut tokens = HashMap::<String, String>::new();
    let mut copied = 0;
    for span in replacements {
        let destination = &markdown[span.clone()];
        let Some((raw, _)) = reference_destination(destination) else {
            continue;
        };
        if !raw.starts_with("data:") || raw.len() <= 24 {
            continue;
        }
        let Some(normalized) = normalized_definition_destination(destination, options) else {
            return ReferenceData {
                may_have_unresolved_images: true,
                ..original()
            };
        };
        let next = tokens.len();
        let token = tokens
            .entry(normalized.clone())
            .or_insert_with(|| format!("\u{e000}{next}:"));
        destinations.entry(token.clone()).or_insert(normalized);
        input.push_str(&markdown[copied..span.start]);
        input.push_str(token);
        // Preserve the original input-sized reference budget and source positions.
        input.extend(std::iter::repeat_n(' ', span.len() - token.len()));
        copied = span.end;
    }
    input.push_str(&markdown[copied..]);
    ReferenceData {
        input: if destinations.is_empty() {
            Cow::Borrowed(markdown)
        } else {
            Cow::Owned(input)
        },
        destinations,
        may_have_unresolved_images,
    }
}

fn reference_space(input: &[u8], mut position: usize) -> usize {
    while matches!(input.get(position), Some(b' ' | b'\t')) {
        position += 1;
    }
    if input.get(position) == Some(&b'\n') {
        position += 1;
        while matches!(input.get(position), Some(b' ' | b'\t')) {
            position += 1;
        }
    }
    position
}

fn reference_definition_span(input: &str) -> Option<(std::ops::Range<usize>, usize)> {
    let bytes = input.as_bytes();
    if bytes.first() != Some(&b'[') {
        return None;
    }
    let mut position = 1;
    while position <= 1001 {
        match bytes.get(position)? {
            b'\\'
                if bytes
                    .get(position + 1)
                    .is_some_and(u8::is_ascii_punctuation) =>
            {
                position += 2
            }
            b'[' => return None,
            b']' => break,
            _ => position += 1,
        }
    }
    if bytes.get(position..position + 2) != Some(b"]:") {
        return None;
    }
    position = reference_space(bytes, position + 2);
    let start = position;
    let (_, length) = reference_destination(&input[position..])?;
    position += length;
    let destination = start..position;
    let before_title = position;
    let title_start = reference_space(bytes, position);
    if title_start != position
        && let Some(delimiter @ (b'\'' | b'"' | b'(')) = bytes.get(title_start)
    {
        let close = if *delimiter == b'(' { b')' } else { *delimiter };
        position = title_start + 1;
        loop {
            match bytes.get(position) {
                Some(b'\\')
                    if bytes
                        .get(position + 1)
                        .is_some_and(u8::is_ascii_punctuation) =>
                {
                    position += 2
                }
                Some(byte) if *byte == close => {
                    position += 1;
                    break;
                }
                Some(b'(') if *delimiter == b'(' => return None,
                Some(_) => position += 1,
                None => {
                    position = before_title;
                    break;
                }
            }
        }
    }
    while matches!(bytes.get(position), Some(b' ' | b'\t')) {
        position += 1;
    }
    if bytes.get(position) == Some(&b'\n') {
        position += 1;
    } else if position != bytes.len() {
        return None;
    }
    Some((destination, position))
}

fn reference_destination(destination: &str) -> Option<(&str, usize)> {
    let bytes = destination.as_bytes();
    if !destination.starts_with('<') {
        let mut index = 0;
        let mut parentheses = 0;
        while index < bytes.len() {
            match bytes[index] {
                b'\\' if bytes.get(index + 1).is_some_and(u8::is_ascii_punctuation) => {
                    index += 2;
                }
                b'(' => {
                    parentheses += 1;
                    if parentheses > 32 {
                        return None;
                    }
                    index += 1;
                }
                b')' if parentheses == 0 => break,
                b')' => {
                    parentheses -= 1;
                    index += 1;
                }
                byte if byte == b' ' || (byte.is_ascii_control() && byte != 0) => break,
                _ => index += 1,
            }
        }
        return (index > 0 && parentheses == 0).then_some((&destination[..index], index));
    }
    let mut index = 1;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => index += 2,
            b'>' => return Some((&destination[1..index], index + 1)),
            b'\n' | b'\r' | b'<' => return None,
            _ => index += 1,
        }
    }
    None
}

fn normalized_definition_destination(destination: &str, options: &Options<'_>) -> Option<String> {
    let arena = Arena::new();
    let root = comrak::parse_document(&arena, &format!("[x]({})", destination), options);
    match &root.first_child()?.first_child()?.data().value {
        NodeValue::Link(link) => Some(link.url.clone()),
        _ => None,
    }
}

// Non-image uses and failed decodes still carry their original URLs in the IR.
// Short URLs have bounded cost per reference; retain the expansion ceiling for
// long destinations that cannot be represented by a shared asset identifier.
fn shared_url(token: &str, state: &mut WalkState) -> Option<String> {
    let url = state.shared_data_urls.get(token)?;
    if url.len() <= 128 {
        return Some(url.clone());
    }
    if let Some(remaining) = state.shared_url_bytes_remaining.checked_sub(url.len()) {
        state.shared_url_bytes_remaining = remaining;
        Some(url.clone())
    } else {
        state.warnings.push(Warning::new(
            WarningCode::UnsupportedNode,
            "shared data URL expansion exceeds the Markdown reference budget",
        ));
        None
    }
}

fn convert_node(
    node: &AstNode<'_>,
    children: Vec<ConvertedNode>,
    state: &mut WalkState,
    limits: &Limits,
) -> ConvertedNode {
    let data = node.data();
    match &data.value {
        NodeValue::Document => ConvertedNode::Blocks(blocks(children)),
        NodeValue::FrontMatter(_) => ConvertedNode::Skip,
        NodeValue::Paragraph => block(Block::Paragraph {
            content: inlines(children),
        }),
        NodeValue::Heading(heading) => block(Block::Heading {
            level: heading.level,
            content: inlines(children),
        }),
        NodeValue::CodeBlock(code) => {
            let lang = code.info.split_whitespace().next().map(str::to_owned);
            block(Block::Code {
                lang,
                text: code.literal.clone(),
            })
        }
        NodeValue::Math(math) if math.display_math && !parent_is_inline(node) => {
            block(Block::Math {
                tex: math.literal.clone(),
                display: true,
            })
        }
        NodeValue::Math(math) => inline(Inline::Math {
            tex: math.literal.clone(),
            display: math.display_math,
        }),
        NodeValue::BlockQuote | NodeValue::MultilineBlockQuote(_) => block(Block::Quote {
            blocks: blocks(children),
        }),
        NodeValue::Alert(_) => {
            unsupported(
                node,
                state,
                "GitHub alert formatting was reduced to a block quote",
            );
            block(Block::Quote {
                blocks: blocks(children),
            })
        }
        NodeValue::List(list) => {
            let items = children
                .into_iter()
                .filter_map(|child| match child {
                    ConvertedNode::ListItem(item) => Some(item),
                    _ => None,
                })
                .collect();
            block(Block::List {
                ordered: list.list_type == ListType::Ordered,
                start: (list.list_type == ListType::Ordered).then_some(list.start as u64),
                tight: list.tight,
                items,
            })
        }
        NodeValue::Item(_) => ConvertedNode::ListItem(ListItem {
            checked: None,
            blocks: blocks(children),
        }),
        NodeValue::TaskItem(item) => ConvertedNode::ListItem(ListItem {
            checked: Some(item.symbol.is_some()),
            blocks: blocks(children),
        }),
        NodeValue::Table(table) => {
            let mut head = Vec::new();
            let mut body = Vec::new();
            for child in children {
                if let ConvertedNode::TableRow { header, cells } = child {
                    if header {
                        head.push(cells);
                    } else {
                        body.push(cells);
                    }
                }
            }
            let columns = (0..table.num_columns)
                .map(|index| ColumnSpec {
                    align: match table.alignments.get(index) {
                        Some(TableAlignment::Left) => Alignment::Left,
                        Some(TableAlignment::Center) => Alignment::Center,
                        Some(TableAlignment::Right) => Alignment::Right,
                        Some(TableAlignment::None) | None => Alignment::Default,
                    },
                })
                .collect();
            block(Block::Table {
                caption: None,
                columns,
                head,
                body,
                footnotes: Vec::new(),
            })
        }
        NodeValue::TableRow(header) => ConvertedNode::TableRow {
            header: *header,
            cells: children
                .into_iter()
                .filter_map(|child| match child {
                    ConvertedNode::TableCell(cell) => Some(cell),
                    _ => None,
                })
                .collect(),
        },
        NodeValue::TableCell => {
            let content = inlines(children);
            let cell_blocks = if content.is_empty() {
                Vec::new()
            } else {
                vec![Block::Paragraph { content }]
            };
            ConvertedNode::TableCell(TableCell {
                rowspan: 1,
                colspan: 1,
                header: None,
                blocks: cell_blocks,
            })
        }
        NodeValue::FootnoteDefinition(definition) => {
            state
                .footnote_definitions
                .insert(normalize_footnote(&definition.name));
            block(Block::Footnote {
                id: definition.name.clone(),
                blocks: blocks(children),
            })
        }
        NodeValue::ThematicBreak => {
            unsupported(
                node,
                state,
                "thematic break was represented as a horizontal rule",
            );
            block(Block::Paragraph {
                content: vec![Inline::Text {
                    text: "———".to_owned(),
                }],
            })
        }
        NodeValue::HtmlBlock(html) => block(Block::Raw {
            format: RawFormat::Html,
            text: html.literal.clone(),
        }),
        NodeValue::Text(text) => ConvertedNode::Inlines(text_inlines(text.as_ref(), state)),
        NodeValue::Emph => inline(Inline::Emph {
            content: inlines(children),
        }),
        NodeValue::Strong => inline(Inline::Strong {
            content: inlines(children),
        }),
        NodeValue::Strikethrough => inline(Inline::Strikeout {
            content: inlines(children),
        }),
        NodeValue::Superscript => inline(Inline::Superscript {
            content: inlines(children),
        }),
        NodeValue::Subscript => inline(Inline::Subscript {
            content: inlines(children),
        }),
        NodeValue::Code(code) => inline(Inline::Code {
            text: code.literal.clone(),
        }),
        NodeValue::SoftBreak => inline(Inline::SoftBreak {}),
        NodeValue::LineBreak => inline(Inline::LineBreak {}),
        NodeValue::HtmlInline(html) => inline(Inline::Raw {
            format: RawFormat::Html,
            text: html.clone(),
        }),
        NodeValue::Link(link) => {
            let url = if state.shared_data_urls.contains_key(&link.url) {
                let Some(url) = shared_url(&link.url, state) else {
                    return ConvertedNode::Inlines(inlines(children));
                };
                url
            } else {
                link.url.clone()
            };
            inline(Inline::Link {
                url,
                title: nonempty(&link.title),
                content: inlines(children),
            })
        }
        NodeValue::Image(link) => {
            let alt = nfc(&inline_text(&inlines(children)));
            let target = if let Some(url) = state.shared_data_urls.get(&link.url) {
                // Equal normalized destinations already share one token. Cache
                // hits only copy and hash that short key, never the data URL.
                let target = state
                    .shared_image_targets
                    .entry(link.url.clone())
                    .or_insert_with(|| {
                        common::decode_data_uri_image(
                            url,
                            limits,
                            &mut state.assets,
                            &mut state.warnings,
                        )
                    })
                    .clone();
                if let Some(target) = target {
                    target
                } else {
                    let Some(href) = shared_url(&link.url, state) else {
                        return inline(Inline::Text { text: alt });
                    };
                    AssetRef::Url { href }
                }
            } else {
                common::decode_data_uri_image(
                    &link.url,
                    limits,
                    &mut state.assets,
                    &mut state.warnings,
                )
                .unwrap_or_else(|| AssetRef::Url {
                    href: link.url.clone(),
                })
            };
            inline(Inline::Image {
                target,
                alt,
                title: nonempty(&link.title),
            })
        }
        NodeValue::FootnoteReference(reference) => {
            state
                .footnote_references
                .insert(normalize_footnote(&reference.name));
            inline(Inline::FootnoteRef {
                id: reference.name.clone(),
            })
        }
        NodeValue::ShortCode(shortcode) => inline(Inline::Text {
            text: shortcode.emoji.clone(),
        }),
        NodeValue::WikiLink(link) => {
            unsupported(node, state, "wiki link was represented as a regular link");
            inline(Inline::Link {
                url: link.url.clone(),
                title: None,
                content: inlines(children),
            })
        }
        NodeValue::Highlight
        | NodeValue::Insert
        | NodeValue::Underline
        | NodeValue::SpoileredText => {
            unsupported(node, state, "extended emphasis was reduced to emphasis");
            inline(Inline::Emph {
                content: inlines(children),
            })
        }
        NodeValue::EscapedTag(tag) => {
            unsupported(node, state, "escaped tag was kept as text");
            inline(Inline::Text { text: nfc(tag) })
        }
        NodeValue::Raw(text) => {
            unsupported(node, state, "programmatic raw node was kept as text");
            inline(Inline::Text { text: nfc(text) })
        }
        _ => {
            unsupported(node, state, "unsupported Markdown node was flattened");
            if data.value.block() {
                ConvertedNode::Blocks(blocks(children))
            } else {
                ConvertedNode::Inlines(inlines(children))
            }
        }
    }
}

fn block(block: Block) -> ConvertedNode {
    ConvertedNode::Blocks(vec![block])
}

fn inline(inline: Inline) -> ConvertedNode {
    ConvertedNode::Inlines(vec![inline])
}

fn blocks(children: Vec<ConvertedNode>) -> Vec<Block> {
    children
        .into_iter()
        .flat_map(|child| match child {
            ConvertedNode::Blocks(blocks) => blocks,
            _ => Vec::new(),
        })
        .collect()
}

fn inlines(children: Vec<ConvertedNode>) -> Vec<Inline> {
    children
        .into_iter()
        .flat_map(|child| match child {
            ConvertedNode::Inlines(inlines) => inlines,
            _ => Vec::new(),
        })
        .collect()
}

fn inline_text(inlines: &[Inline]) -> String {
    let mut text = String::new();
    let mut stack = inlines.iter().rev().collect::<Vec<_>>();
    while let Some(inline) = stack.pop() {
        match inline {
            Inline::Text { text: value } | Inline::Code { text: value } => text.push_str(value),
            Inline::Math { tex, .. } => text.push_str(tex),
            Inline::Link { content, .. }
            | Inline::Emph { content }
            | Inline::Strong { content }
            | Inline::Strikeout { content }
            | Inline::Superscript { content }
            | Inline::Subscript { content } => stack.extend(content.iter().rev()),
            Inline::Image { alt, .. } => text.push_str(alt),
            Inline::SoftBreak {} | Inline::LineBreak {} => text.push(' '),
            Inline::FootnoteRef { .. } | Inline::Raw { .. } => {}
        }
    }
    text
}

fn parent_is_inline(node: &AstNode<'_>) -> bool {
    node.parent().is_some_and(|parent| {
        let parent = parent.data();
        !parent.value.block() || parent.value.contains_inlines()
    })
}

const MAX_FOOTNOTE_LABEL_BYTES: usize = 999;

fn text_inlines(text: &str, state: &WalkState) -> Vec<Inline> {
    let mut inlines = Vec::new();
    let (mut pending, mut search) = (0, 0);
    while let Some(relative) = text[search..].find("[^") {
        let start = search + relative;
        let label_start = start + 2;
        let Some(relative_end) = text[label_start..].find(['[', ']']) else {
            break;
        };
        let end = label_start + relative_end;
        let label = &text[label_start..end];
        if text.as_bytes()[end] != b']'
            || label.is_empty()
            || label.len() > MAX_FOOTNOTE_LABEL_BYTES
        {
            search = end;
            continue;
        }
        let normalized = normalize_footnote(label);
        if !state.footnote_references.contains(&normalized)
            || state.footnote_definitions.contains(&normalized)
        {
            search = end + 1;
            continue;
        }

        if start > pending {
            inlines.push(Inline::Text {
                text: nfc(&text[pending..start]),
            });
        }
        inlines.push(Inline::FootnoteRef {
            id: label.to_owned(),
        });
        pending = end + 1;
        search = pending;
    }

    if pending < text.len() {
        inlines.push(Inline::Text {
            text: nfc(&text[pending..]),
        });
    }
    inlines
}

fn scan_footnotes(markdown: &str) -> (BTreeSet<String>, BTreeSet<String>) {
    let mut references = BTreeSet::new();
    let mut definitions = BTreeSet::new();
    let mut fence: Option<(char, usize)> = None;
    let mut inline_code_ticks = 0;
    let mut in_front_matter = front_matter_content(markdown).is_some();

    for line in markdown.lines() {
        if in_front_matter {
            if line == "---" {
                in_front_matter = false;
            }
            continue;
        }

        if line.trim().is_empty() {
            inline_code_ticks = 0;
        }
        let trimmed = line.trim_start_matches([' ', '\t']);
        if let Some((fence_char, fence_len)) = fence {
            if is_closing_fence(trimmed, fence_char, fence_len) {
                fence = None;
            }
            continue;
        }
        if let Some(opening) = opening_fence(trimmed) {
            fence = Some(opening);
            inline_code_ticks = 0;
            continue;
        }

        if let Some(label) = footnote_definition_label(line) {
            definitions.insert(normalize_footnote(label));
            continue;
        }
        scan_footnote_references(line, &mut inline_code_ticks, &mut references);
    }

    (references, definitions)
}

fn footnote_definition_label(line: &str) -> Option<&str> {
    let trimmed = line.trim_start_matches(' ');
    if line.len() - trimmed.len() > 3 {
        return None;
    }
    let remainder = trimmed.strip_prefix("[^")?;
    let end = remainder.find(['[', ']'])?;
    let label = &remainder[..end];
    (!label.is_empty()
        && label.len() <= MAX_FOOTNOTE_LABEL_BYTES
        && remainder[end..].starts_with("]:"))
    .then_some(label)
}

fn scan_footnote_references(
    line: &str,
    inline_code_ticks: &mut usize,
    references: &mut BTreeSet<String>,
) {
    let bytes = line.as_bytes();
    let last_close = line.rfind(']');
    let mut cursor = 0;
    while cursor < bytes.len() {
        if bytes[cursor] == b'\\' {
            cursor += 1;
            if cursor < bytes.len() {
                cursor += line[cursor..].chars().next().map_or(0, char::len_utf8);
            }
            continue;
        }
        if bytes[cursor] == b'`' {
            let length = bytes[cursor..]
                .iter()
                .take_while(|byte| **byte == b'`')
                .count();
            if *inline_code_ticks == 0 {
                *inline_code_ticks = length;
            } else if *inline_code_ticks == length {
                *inline_code_ticks = 0;
            }
            cursor += length;
            continue;
        }
        // Once no closing bracket remains, only track escapes and backticks.
        if *inline_code_ticks == 0
            && line[cursor..].starts_with("[^")
            && last_close.is_some_and(|close| close > cursor + 1)
        {
            let label_start = cursor + 2;
            let Some(relative_end) = line[label_start..].find(['[', ']']) else {
                break;
            };
            let end = label_start + relative_end;
            let label = &line[label_start..end];
            if bytes[end] == b']' && !label.is_empty() && label.len() <= MAX_FOOTNOTE_LABEL_BYTES {
                references.insert(normalize_footnote(label));
            }
            // A '[' can begin the next candidate; labels never contain brackets.
            cursor = end + usize::from(bytes[end] == b']');
            continue;
        }
        cursor += line[cursor..].chars().next().map_or(1, char::len_utf8);
    }
}

fn normalize_footnote(label: &str) -> String {
    label
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn unsupported(node: &AstNode<'_>, state: &mut WalkState, message: &str) {
    let source = node.data().sourcepos.start;
    state.warnings.push(
        Warning::new(WarningCode::UnsupportedNode, message).at(SourcePos {
            line: u32::try_from(source.line).unwrap_or(u32::MAX),
            column: u32::try_from(source.column).unwrap_or(u32::MAX),
        }),
    );
}

fn append_footnote_warnings(
    references: &BTreeSet<String>,
    definitions: &BTreeSet<String>,
    warnings: &mut Vec<Warning>,
) {
    for name in references.difference(definitions) {
        warnings.push(Warning::new(
            WarningCode::FootnoteMissing,
            format!("footnote reference `{name}` has no definition"),
        ));
    }
    for name in definitions.difference(references) {
        warnings.push(Warning::new(
            WarningCode::FootnoteUnused,
            format!("footnote definition `{name}` is unused"),
        ));
    }
}

fn front_matter_content(markdown: &str) -> Option<&str> {
    let rest = markdown.strip_prefix("---\n")?;
    let (index, _) = rest.match_indices("\n---").find(|(index, _)| {
        let after = index + "\n---".len();
        after == rest.len() || rest[after..].starts_with('\n')
    })?;
    Some(&rest[..index])
}

fn prescan_nesting(markdown: &str, limit: u16) -> Result<(), ReadError> {
    let mut fence: Option<(char, usize)> = None;

    for line in markdown.lines() {
        let trimmed = line.trim_start_matches([' ', '\t']);
        if let Some((fence_char, fence_len)) = fence {
            if is_closing_fence(trimmed, fence_char, fence_len) {
                fence = None;
            }
            continue;
        }
        if let Some(opening) = opening_fence(trimmed) {
            fence = Some(opening);
            continue;
        }

        if trimmed
            .chars()
            .all(|ch| matches!(ch, '*' | '_' | ' ' | '\t'))
        {
            continue;
        }
        check_container_depth(line, limit)?;
    }
    Ok(())
}

fn opening_fence(line: &str) -> Option<(char, usize)> {
    let marker = line.chars().next()?;
    if marker != '`' && marker != '~' {
        return None;
    }
    let length = line.chars().take_while(|value| *value == marker).count();
    (length >= 3).then_some((marker, length))
}

fn is_closing_fence(line: &str, marker: char, minimum_length: usize) -> bool {
    let length = line.chars().take_while(|value| *value == marker).count();
    length >= minimum_length && line[length..].trim().is_empty()
}

fn check_container_depth(line: &str, limit: u16) -> Result<(), ReadError> {
    let bytes = line.as_bytes();
    let mut cursor = 0;
    let mut indentation = 0_usize;
    while matches!(bytes.get(cursor), Some(b' ' | b'\t')) {
        indentation += if bytes[cursor] == b'\t' { 2 } else { 1 };
        cursor += 1;
    }

    let mut quotes = 0_usize;
    loop {
        if bytes.get(cursor) != Some(&b'>') {
            break;
        }
        quotes += 1;
        cursor += 1;
        if matches!(bytes.get(cursor), Some(b' ' | b'\t')) {
            cursor += 1;
        }
    }

    let mut list_markers = 0_usize;
    let mut list_cursor = cursor;
    while let Some(next) = after_list_marker(bytes, list_cursor) {
        list_markers += 1;
        list_cursor = next;
        while matches!(bytes.get(list_cursor), Some(b' ' | b'\t')) {
            list_cursor += 1;
        }
    }

    let indent_depth = if list_markers == 0 {
        0
    } else {
        indentation.div_ceil(2) + 1
    };
    let depth = quotes + list_markers.max(indent_depth);
    if depth > usize::from(limit) {
        return Err(ReadError::NestingTooDeep { limit });
    }
    Ok(())
}

fn after_list_marker(bytes: &[u8], start: usize) -> Option<usize> {
    let first = *bytes.get(start)?;
    if matches!(first, b'-' | b'+' | b'*') {
        return matches!(bytes.get(start + 1), Some(b' ' | b'\t')).then_some(start + 1);
    }

    if !first.is_ascii_digit() {
        return None;
    }
    let mut cursor = start;
    while bytes.get(cursor).is_some_and(u8::is_ascii_digit) {
        cursor += 1;
    }
    if !matches!(bytes.get(cursor), Some(b'.' | b')'))
        || !matches!(bytes.get(cursor + 1), Some(b' ' | b'\t'))
    {
        return None;
    }
    Some(cursor + 1)
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use crate::{
        ir::{Alignment, AssetRef, Block, Inline},
        limits::Limits,
        reader::markdown::{ReadError, read},
        warning::WarningCode,
    };

    fn paragraph_text(block: &Block) -> String {
        match block {
            Block::Paragraph { content } | Block::Heading { content, .. } => content
                .iter()
                .filter_map(|inline| match inline {
                    Inline::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect(),
            _ => String::new(),
        }
    }

    #[test]
    fn empty_quotes_in_a_list_obey_the_nesting_limit() {
        for limit in [1, 2, 64] {
            let mut limits = Limits::local();
            limits.max_nesting_depth = limit;
            let at_limit = format!("- {}\n", "> ".repeat(usize::from(limit - 1)));
            assert!(read(&at_limit, &limits).is_ok());
            let too_deep = format!("- {}\n", "> ".repeat(usize::from(limit)));
            assert_eq!(
                read(&too_deep, &limits),
                Err(ReadError::NestingTooDeep { limit })
            );
        }
    }

    #[test]
    fn tables_inside_quotes_obey_the_nesting_limit() {
        for limit in [1, 2, 64] {
            let mut limits = Limits::local();
            limits.max_nesting_depth = limit;
            let table = |quotes| {
                let prefix = "> ".repeat(quotes);
                format!("{prefix}| a |\n{prefix}| --- |\n{prefix}| b |\n")
            };
            assert!(read(&table(usize::from(limit - 1)), &limits).is_ok());
            assert_eq!(
                read(&table(usize::from(limit)), &limits),
                Err(ReadError::NestingTooDeep { limit })
            );
        }
    }

    #[test]
    fn reads_gfm_features_shortcodes_and_raw_html() {
        let output = read(
            "# Title\n\n~~gone~~ https://example.com :smile:\n\n| A | B |\n|---|:---:|\n| 1 | 2 |\n\n- [x] done\n- [ ] later\n\n$e=mc^2$\n\n<div>x</div>\n",
            &Limits::local(),
        )
        .unwrap();

        assert!(matches!(
            output.document.body[0],
            Block::Heading { level: 1, .. }
        ));
        assert!(output.document.body.iter().any(|block| matches!(block, Block::Table { columns, body, .. } if columns[1].align == Alignment::Center && body.len() == 1)));
        assert!(output.document.body.iter().any(|block| matches!(block, Block::List { items, .. } if items.len() == 2 && items[0].checked == Some(true) && items[1].checked == Some(false))));
        assert!(output.document.body.iter().any(|block| matches!(block, Block::Paragraph { content } if content.iter().any(|inline| matches!(inline, Inline::Link { url, .. } if url == "https://example.com")))));
        let serialized = serde_json::to_string(&output.document).unwrap();
        assert!(serialized.contains("strikeout"));
        assert!(serialized.contains("https://example.com"));
        assert!(serialized.contains("😄"));
        assert!(serialized.contains("math"));
        assert!(serialized.contains("<div>x</div>"));
    }

    #[test]
    fn handles_front_matter_bom_and_line_endings_and_normalizes_prose_only() {
        let input = "\u{feff}---\r\ntitle: Cafe\u{301}\r\n---\r\n\r\nCafe\u{301}\r\n\r\n```\r\nCafe\u{301}\r\n```\r\n";
        let output = read(input, &Limits::local()).unwrap();
        assert_eq!(output.document.meta.title.as_deref(), Some("Café"));
        assert_eq!(paragraph_text(&output.document.body[0]), "Café");
        assert!(
            matches!(&output.document.body[1], Block::Code { text, .. } if text.contains("Cafe\u{301}"))
        );

        let crlf = read("hello\r\nworld\r\n", &Limits::local()).unwrap();
        let lf = read("hello\nworld\n", &Limits::local()).unwrap();
        assert_eq!(crlf.document, lf.document);
    }

    #[test]
    fn keeps_missing_footnote_markers_and_warns_on_missing_or_unused_notes() {
        let output = read("Missing[^x].\n\n[^unused]: Unused.\n", &Limits::local()).unwrap();
        assert!(matches!(output.document.body[0], Block::Paragraph { .. }));
        assert!(
            output
                .document
                .body
                .iter()
                .any(|block| matches!(block, Block::Footnote { id, .. } if id == "unused"))
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
            serde_json::to_string(&output.document)
                .unwrap()
                .contains("footnote_ref")
        );
    }

    #[test]
    fn places_defined_footnotes_in_the_ir_without_warnings() {
        let output = read("A[^note].\n\n[^note]: Definition.\n", &Limits::local()).unwrap();
        assert!(
            output
                .document
                .body
                .iter()
                .any(|block| matches!(block, Block::Footnote { id, .. } if id == "note"))
        );
        assert!(
            matches!(&output.document.body[0], Block::Paragraph { content } if content.iter().any(|inline| matches!(inline, Inline::FootnoteRef { id } if id == "note")))
        );
        assert!(output.warnings.is_empty());
    }

    #[test]
    fn warns_when_a_markdown_node_is_reduced() {
        let output = read("Text.\n\n---\n", &Limits::local()).unwrap();
        assert!(
            output
                .warnings
                .iter()
                .any(|warning| warning.code == WarningCode::UnsupportedNode)
        );
    }

    #[test]
    fn shared_references_keep_multiline_container_context() {
        for (first, next) in [("> ", "> "), ("- ", "  "), ("> > ", "> > ")] {
            for image in ["!", ""] {
                let markdown = format!(
                    "{first}{image}[first\n{next}second][p]\n\n[p]: data:image/png;base64,iVBORw0KGgo=\n"
                );
                assert_eq!(
                    read(&markdown, &Limits::local()),
                    super::read_impl(&markdown, &Limits::local(), false),
                    "{markdown}"
                );
            }
        }
    }

    #[test]
    fn shared_png_definitions_accept_multiline_destinations_and_titles() {
        use base64::Engine as _;
        let png = include_bytes!("../../../../fixtures/md/assets/vietnamese-route.png");
        let encoded = base64::engine::general_purpose::STANDARD.encode(png);
        for definition in [
            format!("[pixel]:\n  data:image/png;base64,{encoded}\n"),
            format!("[pixel]:\n  <data:image/png;base64,{encoded}>\n  \"first\nsecond\"\n"),
            format!("[pixel]: data:image/png;base64,{encoded}\n  \"first\nsecond\"\n"),
        ] {
            for crlf in [false, true] {
                let markdown = format!("{}\n{definition}", "![pixel][pixel]\n\n".repeat(3));
                let markdown = if crlf {
                    markdown.replace('\n', "\r\n")
                } else {
                    markdown
                };
                let output = read(&markdown, &Limits::local()).unwrap();
                assert_eq!(output.document.assets.len(), 1);
                assert_eq!(output.document.assets.values().next().unwrap().bytes, png);
                let images = output.document.body.iter().filter(|block| {
                    matches!(block, Block::Paragraph { content } if matches!(content.as_slice(), [Inline::Image { target: AssetRef::Asset { .. }, .. }]))
                }).count();
                assert_eq!(images, 3, "{markdown}");
                assert!(output.warnings.is_empty(), "{:?}", output.warnings);
                if definition.contains("first") {
                    for block in &output.document.body {
                        if let Block::Paragraph { content } = block {
                            assert!(
                                matches!(content.as_slice(), [Inline::Image { title: Some(title), .. }] if title == "first\nsecond")
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn private_reference_tokens_disable_interning_for_unsafe_input() {
        for marker in ["&#65;", "&#xE000;", "&#57344;", "\u{e000}"] {
            let markdown = format!(
                "{marker}\n\n> ![first\n> second][p]\n\n[p]: data:image/png;base64,iVBORw0KGgo=\n"
            );
            assert!(
                super::intern_reference_data_urls(&markdown, &comrak::Options::default())
                    .destinations
                    .is_empty()
            );
            assert_eq!(
                read(&markdown, &Limits::local()),
                super::read_impl(&markdown, &Limits::local(), false),
                "{markdown}"
            );
        }
    }

    #[test]
    fn shared_reference_preprocessing_keeps_literal_definitions_exact() {
        let definition = "[p]: data:image/png;base64,iVBORw0KGgo=";
        for literal in [
            format!("```md\n{definition}\n```\n"),
            format!("<div>\n{definition}\n</div>\n"),
            format!("`{definition}`\n"),
        ] {
            let markdown = format!("{literal}\n> ![first\n> second][p]\n\n{definition}\n");
            let output = read(&markdown, &Limits::local()).unwrap();
            let baseline = super::read_impl(&markdown, &Limits::local(), false).unwrap();
            assert_eq!(output.document, baseline.document, "{markdown}");
            match &output.document.body[0] {
                Block::Code { text, .. } => assert_eq!(text, &format!("{definition}\n")),
                Block::Raw { text, .. } => assert_eq!(text, &literal),
                Block::Paragraph { content } => assert!(
                    matches!(content.as_slice(), [Inline::Code { text }] if text == definition)
                ),
                other => panic!("unexpected literal: {other:?}"),
            }
        }
    }

    #[test]
    fn shared_definition_spans_preserve_eof_duplicates_and_title_syntax() {
        let uri = "data:image/png;base64,iVBORw0KGgo=";
        for definitions in [
            format!("[p]: {uri}"),
            format!("[p]:\n  <{uri}>"),
            format!("[p]: {uri} \"a title\""),
            format!("[p]: {uri} 'a title'"),
            format!("[p]: {uri} (a title)"),
            format!("[p]: {uri} \"a [q]: literal\"\n[q]: <>"),
            format!("[p]: {uri}\n  \"first\nsecond\"\n[q]: https://example.test/"),
            format!("[p]: {uri}\n[P]: https://example.test/"),
            format!("[P]: https://example.test/\n[p]: {uri}"),
            format!("[p]: <{uri}>\n[q]: {uri}"),
        ] {
            let markdown = format!("![p][p] [q][q]\n\n{definitions}");
            assert_eq!(
                read(&markdown, &Limits::local()).unwrap().document,
                super::read_impl(&markdown, &Limits::local(), false)
                    .unwrap()
                    .document,
                "{markdown}"
            );
        }
    }

    #[test]
    fn unassociated_shared_image_definitions_keep_a_budget_diagnostic() {
        use base64::Engine as _;
        let png = include_bytes!("../../../../fixtures/md/assets/vietnamese-route.png");
        let encoded = base64::engine::general_purpose::STANDARD.encode(png);
        for definition in [
            format!("[pixel]:\n  data:image/png;base64,{encoded}\nParagraph.\n"),
            format!("> [pixel]: data:image/png;base64,{encoded}\n"),
        ] {
            let markdown = format!("{}\n{definition}", "![pixel][pixel]\n\n".repeat(3));
            let output = read(&markdown, &Limits::local()).unwrap();
            let images = output.document.body.iter().filter(|block| {
                matches!(block, Block::Paragraph { content } if matches!(content.as_slice(), [Inline::Image { target: AssetRef::Asset { .. }, .. }]))
            }).count();
            assert!(images == 3 || output.warnings.iter().any(|warning| warning.message == "shared data image references could not be interned; unresolved references may remain text"), "{markdown}");
        }
    }

    #[test]
    fn shared_reference_reader_matches_a_markdown_corpus() {
        let mut samples = vec![
            include_str!("../../../../fixtures/md/en-crlf.md").to_owned(),
            include_str!("../../../../fixtures/md/en-deep-list.md").to_owned(),
            include_str!("../../../../fixtures/md/en-emoji-shortcodes.md").to_owned(),
            include_str!("../../../../fixtures/md/en-emphasis-strike.md").to_owned(),
            include_str!("../../../../fixtures/md/en-fenced-code.md").to_owned(),
            include_str!("../../../../fixtures/md/en-footnotes.md").to_owned(),
            include_str!("../../../../fixtures/md/en-hard-breaks.md").to_owned(),
            include_str!("../../../../fixtures/md/en-kitchen-sink.md").to_owned(),
            include_str!("../../../../fixtures/md/en-lf.md").to_owned(),
            include_str!("../../../../fixtures/md/en-task-list.md").to_owned(),
            include_str!("../../../../fixtures/md/mixed-kitchen-sink.md").to_owned(),
            include_str!("../../../../fixtures/md/mixed-math.md").to_owned(),
            include_str!("../../../../fixtures/md/vi-aligned-table.md").to_owned(),
            include_str!("../../../../fixtures/md/vi-blockquote.md").to_owned(),
            include_str!("../../../../fixtures/md/vi-front-matter.md").to_owned(),
            include_str!("../../../../fixtures/md/vi-kitchen-sink.md").to_owned(),
            include_str!("../../../../fixtures/md/vi-links-autolinks.md").to_owned(),
            include_str!("../../../../fixtures/md/vi-local-image.md").to_owned(),
            include_str!("../../../../fixtures/md/vi-long-headings.md").to_owned(),
            include_str!("../../../../fixtures/md/vi-nfc.md").to_owned(),
            include_str!("../../../../fixtures/md/vi-nfd.md").to_owned(),
            include_str!("../../../../fixtures/md/vi-ordered-start.md").to_owned(),
            include_str!("../../../../fixtures/md/vi-raw-html.md").to_owned(),
            include_str!("../../../../fixtures/md/vi-tone-dense-table.md").to_owned(),
        ];
        let definition = "[p]: data:image/png;base64,iVBORw0KGgo=";
        for sample in &mut samples {
            sample.push_str("\n\n![shared probe][shared-probe]\n\n[shared-probe]: data:image/png;base64,iVBORw0KGgo=\n");
        }
        for body in [
            "> ![first\n> second][p]",
            "> [first\n> second][p]",
            "- ![first\n  second][p]",
            "- [first\n  second][p]",
            "> > ![first\n> > second][p]",
            "> > [first\n> > second][p]",
            "![p][p]",
            "[p][p]",
            "`[example]: literal`\n\n![p][p]",
            "```md\n[p]: data:image/png;base64,iVBORw0KGgo=\n```\n\n![p][p]",
            "<div>\n[p]: data:image/png;base64,iVBORw0KGgo=\n</div>\n\n![p][p]",
        ] {
            samples.push(format!("{body}\n\n{definition}\n"));
        }
        samples.extend([
            "![p][p]\n\n[p]:\n  data:image/png;base64,iVBORw0KGgo=\n".into(),
            format!("![p][p]\n\n{definition}\n  \"first\nsecond\"\n"),
            format!("> ![first\r\n> second][p]\r\n\r\n{definition}\r\n"),
            format!("&#65;\n\n> ![first\n> second][p]\n\n{definition}\n"),
            format!("\u{e000}\n\n> ![first\n> second][p]\n\n{definition}\n"),
        ]);
        assert_eq!(samples.len(), 40);
        for (index, markdown) in samples.iter().enumerate() {
            let baseline = super::read_impl(markdown, &Limits::local(), false).unwrap();
            let output = read(markdown, &Limits::local()).unwrap();
            assert_eq!(
                output.document, baseline.document,
                "sample {index}: {markdown}"
            );
        }
    }

    #[test]
    fn literal_reference_roundtrip_keeps_fenced_code_and_all_shared_images() {
        let output = shared_png_roundtrip("```md\n[example]: literal\n```\n");
        assert!(
            matches!(&output.document.body[0], Block::Code { text, .. } if text == "[example]: literal\n")
        );
    }

    #[test]
    fn literal_reference_roundtrip_keeps_code_spans_and_all_shared_images() {
        let output = shared_png_roundtrip("`[example]: literal`\n");
        assert!(
            matches!(&output.document.body[0], Block::Paragraph { content }
            if matches!(content.as_slice(), [Inline::Code { text }] if text == "[example]: literal"))
        );
    }

    #[test]
    fn literal_reference_roundtrip_keeps_html_and_all_shared_images() {
        let literal = "<div>\n[example]: literal\n</div>\n";
        let output = shared_png_roundtrip(literal);
        assert!(matches!(&output.document.body[0], Block::Raw { text, .. } if text == literal));
    }

    #[test]
    fn literal_reference_roundtrip_keeps_paragraphs_and_all_shared_images() {
        let literal = "Paragraph with [example]: literal text.";
        let output = shared_png_roundtrip(literal);
        assert_eq!(paragraph_text(&output.document.body[0]), literal);
    }

    fn shared_png_roundtrip(literal: &str) -> super::ReadOutput {
        use crate::ir::{Asset, Document};
        let png = include_bytes!("../../../../fixtures/md/assets/vietnamese-route.png");
        let mut document = Document::default();
        document.assets.insert(
            "pixel".into(),
            Asset {
                media_type: "image/png".into(),
                bytes: png.to_vec(),
            },
        );
        for _ in 0..3 {
            document.body.push(Block::Paragraph {
                content: vec![Inline::Image {
                    target: AssetRef::Asset { id: "pixel".into() },
                    alt: "pixel".into(),
                    title: None,
                }],
            });
        }
        let written = crate::writer::markdown::write(&document);
        assert_eq!(written.content.matches("data:image/png;base64,").count(), 1);
        let output = read(
            &format!("{literal}\n\n{}", written.content),
            &Limits::local(),
        )
        .unwrap();
        assert_eq!(output.document.assets.len(), 1);
        assert_eq!(output.document.assets.values().next().unwrap().bytes, png);
        let images = output.document.body.iter().filter(|block| {
            matches!(block, Block::Paragraph { content }
                if matches!(content.as_slice(), [Inline::Image { target: AssetRef::Asset { .. }, .. }]))
        }).count();
        assert_eq!(images, 3);
        assert!(output.warnings.is_empty());
        output
    }

    #[test]
    fn escaped_angle_reference_destinations_match_the_original_reader() {
        for url in [
            r"data:text/plain,12345678901234567890\>suffix",
            r"data:text/plain,12345678901234567890\>one\>two",
            r"data:text/plain,12345678901234567890\\\>suffix",
        ] {
            for image in ["", "!"] {
                let markdown = format!("{image}[p][p]\n\n[p]: <{url}>\n");
                assert_eq!(
                    read(&markdown, &Limits::local()),
                    super::read_impl(&markdown, &Limits::local(), false),
                    "{markdown}"
                );
            }
        }
    }

    #[test]
    fn shared_image_fallback_never_silently_loses_references() {
        use base64::Engine as _;
        let png = include_bytes!("../../../../fixtures/md/assets/vietnamese-route.png");
        let encoded = base64::engine::general_purpose::STANDARD.encode(png);
        let markdown = format!(
            "[other]: https://example.test/\nParagraph.\n\n{}\n[pixel]: data:image/png;base64,{encoded}\n",
            "![pixel][pixel]\n\n".repeat(3)
        );
        let output = read(&markdown, &Limits::local()).unwrap();
        assert_eq!(output.document.assets.values().next().unwrap().bytes, png);
        let images = output.document.body.iter().filter(|block| {
            matches!(block, Block::Paragraph { content }
                if matches!(content.as_slice(), [Inline::Image { target: AssetRef::Asset { .. }, .. }]))
        }).count();
        assert!(images == 3 || output.warnings.iter().any(|warning| warning.message == "shared data image references could not be interned; unresolved references may remain text"));
    }

    #[test]
    fn escaped_internal_urls_match_the_original_reader() {
        for url in [
            "a&#58;0:0",
            r"a\:0:0",
            "&#97;&#58;0&#58;0",
            "a:&#48;:0",
            "a:0:0",
        ] {
            for image in ["", "!"] {
                for body in [
                    format!("{image}[link]({url})"),
                    format!("{image}[link][other]\n\n[other]: {url}"),
                    format!("{image}[other][]\n\n[other]: {url}"),
                    format!("{image}[other]\n\n[other]: {url}"),
                ] {
                    let markdown =
                        format!("{body}\n\n[pixel]: data:image/png;base64,iVBORw0KGgo=\n");
                    assert_eq!(
                        read(&markdown, &Limits::local()),
                        super::read_impl(&markdown, &Limits::local(), false),
                        "{markdown}"
                    );
                }
            }
        }
    }

    #[test]
    fn normalized_reference_destinations_match_the_original_reader() {
        for url in [
            "data:image/png;base64,iVBORw0KGgo&#61;",
            r"data:image/png;base64,iVBORw0KGgo\=",
            r"data:image/png\;base64\,iVBORw0KGgo\=",
            "data:text/plain;base64,YQ&#61;&#61;",
            r"data:application/foo\;bar\,baz\=qux",
            r"https://example.test/a\(b\)?x&#61;1",
            "data:text/plain,1234567890\u{a0}1234567890",
            "data:text/plain,1234567890\u{2003}1234567890",
            "data:text/plain,1234567890(1234567890)",
            r"data:text/plain,1234567890\(1234567890\)",
        ] {
            for image in ["", "!"] {
                let markdown = format!("{image}[p][p]\n\n[p]: {url}\n");
                assert_eq!(
                    read(&markdown, &Limits::local()),
                    super::read_impl(&markdown, &Limits::local(), false),
                    "{markdown}"
                );
            }
        }
    }

    #[test]
    fn literal_reference_definitions_match_the_original_reader() {
        let definition = "[pixel]: data:image/png;base64,iVBORw0KGgo=";
        for literal in [
            format!("`{definition}`"),
            format!("```md\n{definition}\n```"),
            format!("    {definition}"),
            format!("> `{definition}`"),
        ] {
            let markdown = format!(
                "{literal}\n\n[link](a&#58;0:0)\n\n[unused]: data:image/png;base64,iVBORw0KGgo=\n"
            );
            assert_eq!(
                read(&markdown, &Limits::local()),
                super::read_impl(&markdown, &Limits::local(), false),
                "{markdown}"
            );
        }
        for prefix in [
            "[pixel]: a&#58;0:0\nParagraph",
            "> [pixel]: a&#58;0:0",
            "[^note]: [pixel]: a&#58;0:0",
            r"[^note\]: literal",
            "[pi\n xel]: a&#58;0:0",
            "[pixel]: a&#58;0:0",
        ] {
            let markdown = format!(
                "{prefix}\n\n[link][pixel]\n\n[pixel]: data:image/png;base64,iVBORw0KGgo=\n"
            );
            assert_eq!(
                read(&markdown, &Limits::local()),
                super::read_impl(&markdown, &Limits::local(), false),
                "{markdown}"
            );
        }
        for tail in ["", "\nParagraph."] {
            let markdown = format!(
                "[pi\n xel]: a&#58;0:0{tail}\n\n[link][pi xel]\n\n[pi xel]: data:image/png;base64,iVBORw0KGgo=\n"
            );
            assert_eq!(
                read(&markdown, &Limits::local()),
                super::read_impl(&markdown, &Limits::local(), false),
                "{markdown}"
            );
        }
    }

    #[test]
    fn large_shared_images_keep_reader_work_linear_in_reference_count() {
        use base64::Engine as _;
        use std::time::Duration;

        let fixture = include_bytes!("../../../../fixtures/md/assets/vietnamese-route.png");
        let iend = &fixture[fixture.len() - 12..];
        assert_eq!(&iend[4..8], b"IEND");
        let mut png = fixture[..fixture.len() - 12].to_vec();
        // Add a valid 1,400,001-byte tEXt chunk before IEND. Its fixed payload
        // is the keyword "padding", a NUL separator and repeated ASCII 'a'.
        let mut metadata = b"tEXtpadding\0".to_vec();
        metadata.resize(4 + 1_400_001, b'a');
        png.extend_from_slice(&1_400_001_u32.to_be_bytes());
        png.extend_from_slice(&metadata);
        png.extend_from_slice(&0x920e534d_u32.to_be_bytes());
        png.extend_from_slice(iend);
        let encoded = base64::engine::general_purpose::STANDARD.encode(&png);
        assert!((1024 * 1024..=2 * 1024 * 1024).contains(&encoded.len()));

        let measure = |references: usize| {
            let markdown = format!(
                "{}\n[p]: data:image/png;base64,{encoded}\n",
                "![p]\n\n".repeat(references)
            );
            let mut limits = Limits::local();
            limits.max_asset_bytes = Some(png.len() as u64);
            let start = Instant::now();
            let output = read(&markdown, &limits).unwrap();
            let elapsed = start.elapsed();
            assert!(output.warnings.is_empty());
            assert_eq!(output.document.assets.len(), 1);
            let (id, asset) = output.document.assets.first_key_value().unwrap();
            assert!(asset.bytes == png);
            assert_eq!(asset.media_type, "image/png");
            assert_eq!(output.document.body.len(), references);
            for block in &output.document.body {
                assert!(matches!(block, Block::Paragraph { content }
                    if matches!(content.as_slice(), [Inline::Image {
                        target: AssetRef::Asset { id: target }, ..
                    }] if target == id)));
            }
            let json_bytes = serde_json::to_vec(&output.document).unwrap().len();
            assert!(json_bytes <= encoded.len() + references * 256 + 2048);
            eprintln!(
                "reader work: references={references}, definition_bytes={}, input_bytes={}, elapsed={elapsed:?}, json_bytes={json_bytes}",
                encoded.len(),
                markdown.len()
            );
            (elapsed, json_bytes)
        };
        let (small, small_bytes) = measure(2_000);
        let (large, large_bytes) = measure(20_000);
        eprintln!(
            "reader work ratio: {:.3}",
            large.as_secs_f64() / small.as_secs_f64()
        );
        // Fixed definition parsing can dominate the small case. Allow a linear
        // upper bound plus scheduling noise; the absolute bound also rejects
        // repeatedly copying/hashing the large asset at every reference.
        assert!(large <= small.mul_f64(15.0) + Duration::from_millis(100));
        assert!(large < Duration::from_secs(10), "reader work: {large:?}");
        assert!(large_bytes <= small_bytes + 18_000 * 256);
    }

    #[test]
    fn shared_data_image_definition_is_decoded_once_for_all_references() {
        use base64::Engine as _;

        let png = include_bytes!("../../../../fixtures/md/assets/vietnamese-route.png");
        let encoded = base64::engine::general_purpose::STANDARD.encode(png);
        let markdown = format!(
            "{}\n[pixel]: data:image/png;base64,{encoded} \"Shared title\"\n",
            "![pixel][pixel]\n\n".repeat(1_000)
        );
        let mut limits = Limits::local();
        limits.max_asset_bytes = Some(png.len() as u64);
        let output = read(&markdown, &limits).unwrap();
        assert_eq!(output.document.assets.len(), 1);
        assert_eq!(output.document.assets.values().next().unwrap().bytes, png);
        assert_eq!(output.document.body.len(), 1_000);
        for block in &output.document.body {
            assert!(matches!(block, Block::Paragraph { content }
                if matches!(content.as_slice(), [Inline::Image {
                    target: AssetRef::Asset { .. }, title: Some(title), ..
                }] if title == "Shared title")));
        }
        assert!(output.warnings.is_empty());
    }

    #[test]
    fn data_image_definition_interning_preserves_literal_blocks_and_urls() {
        let uri = "data:image/png;base64,iVBORw0KGgo=";
        let definition = format!("[pixel]: <{uri}> \"Pixel\"");
        let markdown = format!(
            "```md\n{definition}\n```\n\n<div>\n{definition}\n</div>\n\na:0:0\n\n![one][real]\n\n![two][real]\n\n[link][real]\n\n[real]: <{uri}> \"Pixel\"\n"
        );
        let output = read(&markdown, &Limits::local()).unwrap();
        assert!(matches!(&output.document.body[0], Block::Code { text, .. }
            if text == &format!("{definition}\n")));
        assert!(matches!(&output.document.body[1], Block::Raw { text, .. }
            if text.contains(&definition)));
        assert_eq!(paragraph_text(&output.document.body[2]), "a:0:0");
        assert_eq!(output.document.assets.len(), 1);
        assert!(
            matches!(&output.document.body[5], Block::Paragraph { content }
            if matches!(&content[0], Inline::Link { url, title: Some(title), .. }
                if url == uri && title == "Pixel"))
        );

        let mut limits = Limits::local();
        limits.max_asset_bytes = Some(1);
        let limited = read(&markdown, &limits).unwrap();
        assert!(limited.document.assets.is_empty());
        assert!(
            limited
                .warnings
                .iter()
                .any(|warning| warning.code == WarningCode::ImageNotEmbedded)
        );
        assert!(
            !serde_json::to_string(&limited.document)
                .unwrap()
                .contains("a:1:")
        );
    }

    #[test]
    fn shared_data_hyperlinks_keep_the_reference_expansion_ceiling() {
        let uri = format!("data:image/png;base64,{}", "iVBORw0KGgo=".repeat(1_000));
        let markdown = format!("{}\n[pixel]: {uri}\n", "[pixel][pixel]\n\n".repeat(100));
        let output = read(&markdown, &Limits::local()).unwrap();
        let url_bytes: usize = output
            .document
            .body
            .iter()
            .filter_map(|block| {
                if let Block::Paragraph { content } = block
                    && let Some(Inline::Link { url, .. }) = content.first()
                {
                    return Some(url.len());
                }
                None
            })
            .sum();
        assert!(url_bytes <= markdown.len().min(100_000));
        assert!(output.document.assets.is_empty());
        assert!(
            output
                .warnings
                .iter()
                .any(|warning| warning.message.contains("reference budget"))
        );
    }

    #[test]
    fn embeds_bounded_data_images_without_loading_url_images() {
        let markdown = "![pixel](data:image/png;base64,iVBORw0KGgo=)\n\n![remote](https://example.test/image.png)\n";
        let output = read(markdown, &Limits::local()).unwrap();
        assert_eq!(output.document.assets.len(), 1);
        let (id, asset) = output.document.assets.first_key_value().unwrap();
        assert_eq!(asset.media_type, "image/png");
        assert_eq!(asset.bytes, b"\x89PNG\r\n\x1a\n");
        assert!(
            matches!(&output.document.body[0], Block::Paragraph { content }
            if matches!(&content[0], Inline::Image { target: AssetRef::Asset { id: embedded }, .. } if embedded == id))
        );
        assert!(
            matches!(&output.document.body[1], Block::Paragraph { content }
            if matches!(&content[0], Inline::Image { target: AssetRef::Url { href }, .. } if href == "https://example.test/image.png"))
        );

        let mut limits = Limits::local();
        limits.max_asset_bytes = Some(1);
        let limited = read(markdown, &limits).unwrap();
        assert!(limited.document.assets.is_empty());
        assert!(
            limited
                .warnings
                .iter()
                .any(|warning| warning.code == WarningCode::ImageNotEmbedded)
        );
        for uri in [
            "data:image/png;base64,invalid!",
            "data:image/png;base64,aGVsbG8=",
        ] {
            let output = read(&format!("![image]({uri})"), &Limits::local()).unwrap();
            assert!(output.document.assets.is_empty());
        }
    }

    #[test]
    fn maps_images_to_urls_and_enforces_input_and_block_limits() {
        let output = read("![alt](assets/a.png)\n", &Limits::local()).unwrap();
        assert!(
            matches!(&output.document.body[0], Block::Paragraph { content } if matches!(&content[0], Inline::Image { target: AssetRef::Url { href }, alt, .. } if href == "assets/a.png" && alt == "alt"))
        );

        let mut input_limited = Limits::local();
        input_limited.max_input_bytes = Some(1);
        assert!(matches!(
            read("hello", &input_limited),
            Err(ReadError::InputTooLarge { .. })
        ));

        let mut block_limited = Limits::local();
        block_limited.max_blocks = 1;
        assert!(matches!(
            read("one\n\ntwo\n", &block_limited),
            Err(ReadError::TooManyBlocks { .. })
        ));

        let mut front_matter_limited = Limits::local();
        front_matter_limited.max_front_matter_bytes = 5;
        let output = read("---\ntitle: long\n---\nText\n", &front_matter_limited).unwrap();
        assert!(output.document.meta.title.is_none());
        assert!(
            output
                .warnings
                .iter()
                .any(|warning| warning.code == WarningCode::FrontMatterInvalid)
        );
    }

    #[test]
    fn accepts_sixty_four_nested_lists_and_rejects_sixty_five() {
        let mut input = String::new();
        for depth in 0..64 {
            input.push_str(&"  ".repeat(depth));
            input.push_str("- item\n");
        }
        assert!(read(&input, &Limits::local()).is_ok());

        input.push_str(&"  ".repeat(64));
        input.push_str("- too deep\n");
        assert!(matches!(
            read(&input, &Limits::local()),
            Err(ReadError::NestingTooDeep { limit: 64 })
        ));
    }

    #[test]
    fn counts_empty_quotes_and_tables_at_the_nesting_boundary() {
        for depth in [1, 2, 64] {
            let mut limits = Limits::local();
            limits.max_nesting_depth = depth;
            let at_limit = format!("- {}\n", ">".repeat(usize::from(depth - 1)));
            assert!(read(&at_limit, &limits).is_ok());
            let above_limit = format!("- {}\n", ">".repeat(usize::from(depth)));
            assert!(matches!(
                read(&above_limit, &limits),
                Err(ReadError::NestingTooDeep { .. })
            ));
            for (quotes, accepted) in [(depth - 1, true), (depth, false)] {
                let prefix = ">".repeat(usize::from(quotes));
                let table = format!("{prefix} | A |\n{prefix} | - |\n");
                assert_eq!(
                    read(&table, &limits).is_ok(),
                    accepted,
                    "quotes: {quotes}, limit: {depth}"
                );
            }
        }
    }

    #[test]
    fn rejects_hostile_block_nesting_before_calling_the_parser() {
        let cases = [
            format!("{} text", ">".repeat(100_000)),
            format!("{}item", "- ".repeat(100_000)),
        ];
        for input in cases {
            let start = Instant::now();
            assert!(matches!(
                read(&input, &Limits::local()),
                Err(ReadError::NestingTooDeep { .. })
            ));
            assert!(start.elapsed().as_secs_f64() < 1.0);
        }
    }

    #[test]
    fn bounds_wide_table_rows_with_the_block_limit() {
        let mut input = String::new();
        for column in 0..10_000 {
            if column > 0 {
                input.push('|');
            }
            input.push('x');
        }
        input.push('\n');
        for column in 0..10_000 {
            if column > 0 {
                input.push('|');
            }
            input.push('-');
        }
        input.push('\n');
        let mut limits = Limits::local();
        limits.max_blocks = 128;

        let start = Instant::now();
        assert!(matches!(
            read(&input, &limits),
            Err(ReadError::TooManyBlocks { limit: 128 })
        ));
        assert!(start.elapsed().as_secs_f64() < 1.0);
    }
    /// Deepest chain of emphasis-like inlines in a paragraph.
    fn inline_depth(block: &Block) -> usize {
        let Block::Paragraph { content } = block else {
            return 0;
        };
        let mut deepest = 0;
        let mut stack: Vec<(&Inline, usize)> = content.iter().map(|inline| (inline, 1)).collect();
        while let Some((inline, depth)) = stack.pop() {
            if let Inline::Emph { content }
            | Inline::Strong { content }
            | Inline::Strikeout { content } = inline
            {
                deepest = deepest.max(depth);
                stack.extend(content.iter().map(|inline| (inline, depth + 1)));
            }
        }
        deepest
    }

    /// `depth` openers that follow `(` and closers that precede `)`. Each inner opener is only
    /// left-flanking and each closer only right-flanking, so comrak nests them all.
    /// Punctuation between markers must not hide their actual AST depth.
    fn punctuated_nesting(marker: &str, depth: usize) -> String {
        let open = format!("{marker}a(");
        let close = format!("a{marker})");
        format!("{}x{}\n", open.repeat(depth), close.repeat(depth))
    }

    #[test]
    fn inline_nesting_counts_toward_the_limit_when_the_prescan_misses_it() {
        let limit = Limits::local().max_nesting_depth;
        let depth = usize::from(limit);
        for marker in ["*", "**"] {
            let at_limit = read(&punctuated_nesting(marker, depth), &Limits::local())
                .expect("nesting at the limit is accepted");
            assert_eq!(
                inline_depth(&at_limit.document.body[0]),
                depth,
                "marker: {marker}"
            );
            assert_eq!(
                read(&punctuated_nesting(marker, depth + 1), &Limits::local()),
                Err(ReadError::NestingTooDeep { limit }),
                "marker: {marker}"
            );
        }
        // If comrak also nests `~~`, it must be bounded the same way.
        match read(&punctuated_nesting("~~", depth + 1), &Limits::local()) {
            Ok(output) => assert!(inline_depth(&output.document.body[0]) <= depth),
            Err(error) => assert_eq!(error, ReadError::NestingTooDeep { limit }),
        }
    }

    #[test]
    fn deep_inline_nesting_is_rejected_on_a_wasm_sized_stack() {
        // The browser module runs with a 1 MiB stack; dropping, printing or serialising
        // 50,000 nested inlines recurses far past it.
        std::thread::Builder::new()
            .stack_size(1024 * 1024)
            .spawn(|| {
                let input = punctuated_nesting("*", 50_000);
                let start = Instant::now();
                let rejected = matches!(
                    read(&input, &Limits::local()),
                    Err(ReadError::NestingTooDeep { limit: 64 })
                );
                assert!(rejected, "50,000 nested inlines must be rejected");
                eprintln!("debug elapsed: {:?}", start.elapsed());
                assert!(start.elapsed().as_secs_f64() < 10.0);
            })
            .unwrap()
            .join()
            .unwrap();
    }

    #[test]
    fn rejected_footnote_labels_are_scanned_in_linear_time() {
        // `\[^` is an escaped bracket, so no candidate is a known reference, and comrak
        // coalesces the run into one Text node that `text_inlines` rescanned from every `[^`.
        let input = format!("{}x\\]\n", "\\[^".repeat(100_000));
        let start = Instant::now();
        let output = read(&input, &Limits::local()).unwrap();
        eprintln!("debug elapsed: {:?}", start.elapsed());
        assert!(start.elapsed().as_secs_f64() < 5.0);
        assert_eq!(
            paragraph_text(&output.document.body[0]).chars().count(),
            200_002
        );
    }

    #[test]
    fn unterminated_footnote_candidates_are_scanned_in_linear_time() {
        // A blank line ends the open code span. The next backtick must not expose
        // a million unclosed candidates to repeated scans of the same suffix.
        // Comrak reads that indented line as a code block.
        let input = format!("`\n\n    `{}\n", "[^".repeat(1_000_000));
        let start = Instant::now();
        let result = read(&input, &Limits::local());
        assert!(
            matches!(result, Ok(_) | Err(ReadError::NestingTooDeep { .. })),
            "{:?}",
            result.as_ref().err()
        );
        eprintln!("debug elapsed: {:?}", start.elapsed());
        assert!(start.elapsed().as_secs_f64() < 2.0);
    }

    #[test]
    fn rejected_footnote_labels_keep_all_pending_text() {
        for text in ["a [^] b", "a [^outer [^inner] b", "a [^bad] b [^ok]"] {
            let output = read(text, &Limits::local()).unwrap();
            let Block::Paragraph { content } = &output.document.body[0] else {
                panic!("expected paragraph");
            };
            let reconstructed: String = content
                .iter()
                .map(|inline| match inline {
                    Inline::Text { text } => text.clone(),
                    Inline::FootnoteRef { id } => format!("[^{id}]"),
                    other => panic!("unexpected inline: {other:?}"),
                })
                .collect();
            assert_eq!(reconstructed, text);
        }
    }

    #[test]
    fn flat_markdown_does_not_accumulate_inline_depth() {
        for input in [
            "*".repeat(70),
            "_".repeat(70),
            "* ".repeat(70),
            "_ ".repeat(70),
            "*args, **kwargs\n\n".repeat(65),
            format!("Name: {}", "_".repeat(70)),
            "[".repeat(50_000),
            "*".repeat(50_000),
            "[0, 1) ".repeat(65),
            "[ordinary paragraph\n\n".repeat(65),
            format!("`{}`", "[".repeat(100)),
            format!("${}$", "[".repeat(100)),
        ] {
            assert!(read(&input, &Limits::local()).is_ok(), "input: {input}");
        }
    }

    #[test]
    fn inline_containers_share_the_block_nesting_budget() {
        for limit in [1, 2, 64] {
            let mut limits = Limits::local();
            limits.max_nesting_depth = limit;
            for depth in [limit - 1, limit] {
                let nested = punctuated_nesting("*", usize::from(depth));
                let nested = nested.trim_end();
                for input in [
                    format!("> {nested}\n"),
                    format!("[{nested}](https://example.test)\n"),
                    format!("![{nested}](image.png)\n"),
                    format!("~~{nested}~~\n"),
                ] {
                    assert_eq!(read(&input, &limits).is_ok(), depth < limit, "{input}");
                }
            }
        }
    }

    #[test]
    fn footnote_label_byte_limit_preserves_unicode_text() {
        for (label, accepted) in [
            (format!("{}a", "é".repeat(499)), true),
            ("é".repeat(500), false),
        ] {
            let input = format!("before [^{label}] after");
            let output = read(&input, &Limits::local()).unwrap();
            assert_eq!(
                output
                    .warnings
                    .iter()
                    .any(|warning| warning.code == WarningCode::FootnoteMissing),
                accepted
            );
            if !accepted {
                assert_eq!(paragraph_text(&output.document.body[0]), input);
            }
        }
    }

    #[test]
    fn footnote_code_spans_end_at_paragraph_boundaries() {
        let same_paragraph = read("`line\n[^inside]` outside[^outside]", &Limits::local()).unwrap();
        assert_eq!(same_paragraph.warnings.len(), 1);
        assert!(same_paragraph.warnings[0].message.contains("outside"));
        let unclosed_label_before_code =
            read("[^unclosed `code\n[^inside]`", &Limits::local()).unwrap();
        assert!(unclosed_label_before_code.warnings.is_empty());
        let separate_paragraphs = read("`\n\n[^visible]", &Limits::local()).unwrap();
        assert_eq!(separate_paragraphs.warnings.len(), 1);
        assert!(separate_paragraphs.warnings[0].message.contains("visible"));
    }

    #[test]
    fn footnote_reference_scanner_stops_after_an_unclosed_label() {
        let input = "[^".repeat(1_000_000);
        let mut references = std::collections::BTreeSet::new();
        let start = Instant::now();
        super::scan_footnote_references(&input, &mut 0, &mut references);
        eprintln!("debug elapsed: {:?}", start.elapsed());
        assert!(references.is_empty());
        assert!(start.elapsed().as_secs_f64() < 2.0);
    }
}
