use std::collections::{BTreeSet, HashMap};

use comrak::{
    Arena, Options,
    nodes::{AstNode, ListType, NodeValue, TableAlignment},
};
use thiserror::Error;
use unicode_normalization::UnicodeNormalization;

use crate::{
    format::Format,
    ir::{
        Alignment, AssetRef, Block, ColumnSpec, Document, IR_VERSION, Inline, ListItem, RawFormat,
        TableCell,
    },
    limits::{Limits, LimitsError},
    reader::front_matter::parse_front_matter,
    warning::{SourcePos, Warning, WarningCode},
};

#[derive(Clone, Debug, PartialEq)]
pub struct ReadOutput {
    pub document: Document,
    pub warnings: Vec<Warning>,
}

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
    block_nodes: u64,
    warnings: Vec<Warning>,
    footnote_references: BTreeSet<String>,
    footnote_definitions: BTreeSet<String>,
}

/// Reads CommonMark and GFM Markdown into the shared document IR.
pub fn read(markdown: &str, limits: &Limits) -> Result<ReadOutput, ReadError> {
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

    let arena = Arena::new();
    let root = comrak::parse_document(&arena, &markdown, &options);
    let mut state = WalkState {
        footnote_references,
        footnote_definitions,
        ..WalkState::default()
    };
    let mut converted = HashMap::new();
    let mut stack = vec![Visit::Enter(root, 0)];

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
                            | NodeValue::FootnoteDefinition(_)
                    );
                    let is_block = value.block()
                        && !matches!(value, NodeValue::Document | NodeValue::FrontMatter(_));
                    (is_container, is_block)
                };
                if depth > limits.max_nesting_depth {
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
                let child_depth = depth + u16::from(is_container);
                let children: Vec<_> = node.children().collect();
                stack.extend(
                    children
                        .into_iter()
                        .rev()
                        .map(|child| Visit::Enter(child, child_depth)),
                );
            }
            Visit::Exit(node) => {
                let children = node
                    .children()
                    .map(|child| {
                        converted
                            .remove(&node_key(child))
                            .unwrap_or(ConvertedNode::Skip)
                    })
                    .collect::<Vec<_>>();
                let output = convert_node(node, children, &mut state);
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
        ..Document::default()
    };
    Ok(ReadOutput { document, warnings })
}

fn node_key(node: &AstNode<'_>) -> usize {
    std::ptr::from_ref(node).cast::<()>() as usize
}

fn convert_node(
    node: &AstNode<'_>,
    children: Vec<ConvertedNode>,
    state: &mut WalkState,
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
        NodeValue::Link(link) => inline(Inline::Link {
            url: link.url.clone(),
            title: nonempty(&link.title),
            content: inlines(children),
        }),
        NodeValue::Image(link) => {
            let alt = nfc(&inline_text(&inlines(children)));
            inline(Inline::Image {
                target: AssetRef::Url {
                    href: link.url.clone(),
                },
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

fn nonempty(value: &str) -> Option<String> {
    (!value.is_empty()).then(|| value.to_owned())
}

fn text_inlines(text: &str, state: &WalkState) -> Vec<Inline> {
    let mut inlines = Vec::new();
    let mut cursor = 0;
    while let Some(relative) = text[cursor..].find("[^") {
        let start = cursor + relative;
        let label_start = start + 2;
        let Some(relative_end) = text[label_start..].find(']') else {
            break;
        };
        let end = label_start + relative_end;
        let label = &text[label_start..end];
        let normalized = normalize_footnote(label);
        if label.is_empty()
            || !state.footnote_references.contains(&normalized)
            || state.footnote_definitions.contains(&normalized)
        {
            cursor = label_start;
            continue;
        }

        if start > cursor {
            inlines.push(Inline::Text {
                text: nfc(&text[cursor..start]),
            });
        }
        inlines.push(Inline::FootnoteRef {
            id: label.to_owned(),
        });
        cursor = end + 1;
    }

    if cursor < text.len() {
        inlines.push(Inline::Text {
            text: nfc(&text[cursor..]),
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
    let end = remainder.find("]:")?;
    let label = &remainder[..end];
    (!label.is_empty()).then_some(label)
}

fn scan_footnote_references(
    line: &str,
    inline_code_ticks: &mut usize,
    references: &mut BTreeSet<String>,
) {
    let bytes = line.as_bytes();
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
        if *inline_code_ticks == 0
            && line[cursor..].starts_with("[^")
            && let Some(relative_end) = line[cursor + 2..].find(']')
        {
            let end = cursor + 2 + relative_end;
            let label = &line[cursor + 2..end];
            if !label.is_empty() {
                references.insert(normalize_footnote(label));
            }
            cursor = end + 1;
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

fn nfc(value: &str) -> String {
    value.nfc().collect()
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
    let mut bracket_depth = 0_u16;
    let mut emphasis_depth = 0_u16;

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

        check_container_depth(line, limit)?;
        check_inline_depth(line, limit, &mut bracket_depth, &mut emphasis_depth)?;
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

fn check_inline_depth(
    line: &str,
    limit: u16,
    bracket_depth: &mut u16,
    emphasis_depth: &mut u16,
) -> Result<(), ReadError> {
    let chars: Vec<_> = line.chars().collect();
    let mut cursor = 0;
    let mut inline_code_ticks = 0_usize;
    while cursor < chars.len() {
        if chars[cursor] == '\\' {
            cursor = (cursor + 2).min(chars.len());
            continue;
        }
        if chars[cursor] == '`' {
            let length = run_length(&chars, cursor, '`');
            if inline_code_ticks == 0 {
                inline_code_ticks = length;
            } else if inline_code_ticks == length {
                inline_code_ticks = 0;
            }
            cursor += length;
            continue;
        }
        if inline_code_ticks != 0 {
            cursor += 1;
            continue;
        }

        match chars[cursor] {
            '[' => {
                *bracket_depth = bracket_depth.saturating_add(1);
                if *bracket_depth > limit {
                    return Err(ReadError::NestingTooDeep { limit });
                }
                cursor += 1;
            }
            ']' => {
                *bracket_depth = bracket_depth.saturating_sub(1);
                cursor += 1;
            }
            '*' | '_' => {
                let marker = chars[cursor];
                let length = run_length(&chars, cursor, marker);
                if length > usize::from(limit) {
                    return Err(ReadError::NestingTooDeep { limit });
                }
                let previous_space = cursor == 0 || chars[cursor - 1].is_whitespace();
                let after = cursor + length;
                let next_space = after == chars.len() || chars[after].is_whitespace();
                if previous_space && !next_space {
                    *emphasis_depth = emphasis_depth.saturating_add(1);
                    if *emphasis_depth > limit {
                        return Err(ReadError::NestingTooDeep { limit });
                    }
                } else if !previous_space && next_space {
                    *emphasis_depth = emphasis_depth.saturating_sub(1);
                }
                cursor += length;
            }
            _ => cursor += 1,
        }
    }
    Ok(())
}

fn run_length(chars: &[char], start: usize, marker: char) -> usize {
    chars[start..]
        .iter()
        .take_while(|character| **character == marker)
        .count()
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
    fn rejects_hostile_nesting_before_calling_the_parser() {
        let cases = [
            format!("{} text", ">".repeat(100_000)),
            format!("{}item", "- ".repeat(100_000)),
            "[".repeat(50_000),
            "*".repeat(50_000),
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
}
