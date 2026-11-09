//! Serializers from document IR to text and markup formats.

pub mod csv;
pub mod html;
pub mod markdown;
pub mod sanitize;
pub mod text;
pub mod typst;
pub mod xlsx;

use crate::{
    ir::{AssetRef, Block, Document, Inline, ListItem, Metadata, TableCell},
    warning::Warning,
};

/// Result of serializing an IR document.
#[derive(Clone, Debug, PartialEq)]
pub struct WriteOutput {
    pub content: String,
    pub warnings: Vec<Warning>,
}

/// Result of serializing an IR document to a binary format.
#[derive(Clone, Debug, PartialEq)]
pub struct WriteBinaryOutput {
    pub bytes: Vec<u8>,
    pub warnings: Vec<Warning>,
}

/// Replaces line breaks (`\r\n`, lone `\r`, `\u{2028}`, `\u{0085}`) with `\n` in multiline text.
#[must_use]
pub(crate) fn normalize_multiline(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                out.push('\n');
            }
            '\u{2028}' | '\u{0085}' => {
                out.push('\n');
            }
            other => out.push(other),
        }
    }
    out
}

/// Replaces line breaks (`\r\n`, lone `\r`, `\n`, `\u{2028}`, `\u{0085}`) with space in single-line strings.
#[must_use]
pub(crate) fn normalize_singleline(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                out.push(' ');
            }
            '\n' | '\u{2028}' | '\u{0085}' => {
                out.push(' ');
            }
            other => out.push(other),
        }
    }
    out
}

/// Checks whether a slice starting with `<` is an HTML tag opener (or comment / doctype / pi).
#[must_use]
pub(crate) fn is_tag_opener(slice: &str) -> bool {
    if slice.starts_with("<!--") || slice.starts_with("<!") || slice.starts_with("<?") {
        return true;
    }
    if let Some(after_slash) = slice.strip_prefix("</") {
        return after_slash
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic());
    }
    if let Some(after_bracket) = slice.strip_prefix('<') {
        return after_bracket
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic());
    }
    false
}

/// Strips HTML tags, comments, and declarations while preserving text and bare `<` characters.
///
/// The bodies of `<script>` and `<style>` elements are discarded to prevent script code
/// or CSS declarations from being emitted as raw text.
#[doc(hidden)]
#[must_use]
pub fn strip_html_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    let mut in_comment = false;
    let mut special_skip_tag: Option<&'static str> = None;
    let mut quote: Option<char> = None;
    let mut chars = html.char_indices().peekable();

    while let Some((idx, ch)) = chars.next() {
        if let Some(tag_name) = special_skip_tag {
            let rest = &html[idx..];
            if let Some(after_close) = rest.strip_prefix("</")
                && after_close
                    .as_bytes()
                    .get(..tag_name.len())
                    .is_some_and(|prefix| prefix.eq_ignore_ascii_case(tag_name.as_bytes()))
            {
                let next_b = after_close.as_bytes().get(tag_name.len()).copied();
                if next_b.is_none()
                    || matches!(next_b, Some(b' ' | b'\t' | b'\n' | b'\r' | b'>' | b'/'))
                {
                    for (_, c) in chars.by_ref() {
                        if c == '>' {
                            break;
                        }
                    }
                    special_skip_tag = None;
                }
            }
            continue;
        }

        if in_comment {
            if ch == '-' && html[idx..].starts_with("-->") {
                in_comment = false;
                chars.next(); // skip second '-'
                chars.next(); // skip '>'
            }
        } else if in_tag {
            if let Some(q) = quote {
                if ch == q {
                    quote = None;
                }
            } else if ch == '"' || ch == '\'' {
                quote = Some(ch);
            } else if ch == '>' {
                in_tag = false;
            }
        } else if ch == '<' && is_tag_opener(&html[idx..]) {
            let rest = &html[idx..];
            if rest.starts_with("<!--") {
                in_comment = true;
                chars.next(); // '!'
                chars.next(); // '-'
                chars.next(); // '-'
            } else {
                let after_open = &rest[1..];
                if after_open
                    .as_bytes()
                    .get(..6)
                    .is_some_and(|prefix| prefix.eq_ignore_ascii_case(b"script"))
                    && matches!(
                        after_open.as_bytes().get(6).copied(),
                        None | Some(b' ' | b'\t' | b'\n' | b'\r' | b'>' | b'/')
                    )
                {
                    special_skip_tag = Some("script");
                    for (_, c) in chars.by_ref() {
                        if c == '>' {
                            break;
                        }
                    }
                } else if after_open
                    .as_bytes()
                    .get(..5)
                    .is_some_and(|prefix| prefix.eq_ignore_ascii_case(b"style"))
                    && matches!(
                        after_open.as_bytes().get(5).copied(),
                        None | Some(b' ' | b'\t' | b'\n' | b'\r' | b'>' | b'/')
                    )
                {
                    special_skip_tag = Some("style");
                    for (_, c) in chars.by_ref() {
                        if c == '>' {
                            break;
                        }
                    }
                } else {
                    in_tag = true;
                    quote = None;
                }
            }
        } else {
            out.push(ch);
        }
    }
    out
}

/// Normalizes prose and display attributes once at writer entry.
///
/// Converts CRLF, lone CR, U+2028, and U+0085 into LF (for multiline content)
/// or space (for single-line attributes). Opaque code, raw markup, math and
/// destinations retain their bytes; each target writer handles its own escaping.
#[must_use]
pub(crate) fn normalize_document(doc: &Document) -> Document {
    Document {
        version: doc.version.clone(),
        meta: Metadata {
            title: doc.meta.title.as_deref().map(normalize_singleline),
            authors: doc
                .meta
                .authors
                .iter()
                .map(|s| normalize_singleline(s))
                .collect(),
            language: doc.meta.language.as_deref().map(normalize_singleline),
            date: doc.meta.date.as_deref().map(normalize_singleline),
            subject: doc.meta.subject.as_deref().map(normalize_singleline),
            keywords: doc
                .meta
                .keywords
                .iter()
                .map(|s| normalize_singleline(s))
                .collect(),
            source_format: doc.meta.source_format,
        },
        body: normalize_blocks(&doc.body),
        assets: doc
            .assets
            .iter()
            .filter(|(_, asset)| asset.can_export_bytes())
            .map(|(id, asset)| (id.clone(), asset.clone()))
            .collect(),
        layout: doc.layout.clone(),
        provenance: doc.provenance.clone(),
        furniture: normalize_blocks(&doc.furniture),
        styles: doc.styles.clone(),
        sections: doc.sections.clone(),
        source_map: doc.source_map.clone(),
    }
}

// Clone containers one level at a time. A recursive subtree clone can exhaust
// the stack before a writer applies its own depth-dependent fallback.
fn normalize_blocks(blocks: &[Block]) -> Vec<Block> {
    let mut result: Vec<_> = blocks.iter().map(normalize_block_shallow).collect();
    let mut pending: Vec<_> = blocks.iter().zip(result.iter_mut()).collect();
    while let Some((source, target)) = pending.pop() {
        match (source, target) {
            (Block::Quote { blocks: source, .. }, Block::Quote { blocks: target, .. })
            | (Block::Footnote { blocks: source, .. }, Block::Footnote { blocks: target, .. }) => {
                *target = source.iter().map(normalize_block_shallow).collect();
                pending.extend(source.iter().zip(target.iter_mut()));
            }
            (Block::List { items: source, .. }, Block::List { items: target, .. }) => {
                *target = source
                    .iter()
                    .map(|item| ListItem {
                        checked: item.checked,
                        blocks: item.blocks.iter().map(normalize_block_shallow).collect(),
                    })
                    .collect();
                for (source, target) in source.iter().zip(target) {
                    pending.extend(source.blocks.iter().zip(target.blocks.iter_mut()));
                }
            }
            (
                Block::Table {
                    head: source_head,
                    body: source_body,
                    ..
                },
                Block::Table {
                    head: target_head,
                    body: target_body,
                    ..
                },
            ) => {
                let rows = |rows: &[Vec<TableCell>]| {
                    rows.iter()
                        .map(|row| {
                            row.iter()
                                .map(|cell| TableCell {
                                    rowspan: cell.rowspan,
                                    colspan: cell.colspan,
                                    header: cell.header,
                                    blocks: cell
                                        .blocks
                                        .iter()
                                        .map(normalize_block_shallow)
                                        .collect(),
                                })
                                .collect()
                        })
                        .collect()
                };
                *target_head = rows(source_head);
                *target_body = rows(source_body);
                for (source, target) in source_head
                    .iter()
                    .chain(source_body)
                    .zip(target_head.iter_mut().chain(target_body))
                {
                    for (source, target) in source.iter().zip(target) {
                        pending.extend(source.blocks.iter().zip(target.blocks.iter_mut()));
                    }
                }
            }
            _ => {}
        }
    }
    result
}

fn normalize_block_shallow(block: &Block) -> Block {
    let id = block.id().clone();
    let style = block.style().clone();
    let direct = block.direct().clone();
    match block {
        Block::Heading { level, content, .. } => Block::Heading {
            id,
            style,
            direct,
            level: *level,
            content: content.iter().map(normalize_inline).collect(),
        },
        Block::Paragraph { content, .. } => Block::Paragraph {
            id,
            style,
            direct,
            content: content.iter().map(normalize_inline).collect(),
        },
        Block::Quote { .. } => Block::Quote {
            id,
            style,
            direct,
            blocks: vec![],
        },
        Block::Footnote { note_id, .. } => Block::Footnote {
            id,
            style,
            direct,
            note_id: note_id.clone(),
            blocks: vec![],
        },
        Block::List {
            ordered,
            start,
            tight,
            ..
        } => Block::List {
            id,
            style,
            direct,
            ordered: *ordered,
            start: *start,
            tight: *tight,
            items: vec![],
        },
        Block::Table {
            caption,
            columns,
            footnotes,
            ..
        } => Block::Table {
            id,
            style,
            direct,
            caption: caption
                .as_ref()
                .map(|v| v.iter().map(normalize_inline).collect()),
            columns: columns.clone(),
            head: vec![],
            body: vec![],
            footnotes: footnotes.iter().map(normalize_inline).collect(),
        },
        Block::Figure { asset, caption, .. } => Block::Figure {
            id,
            style,
            direct,
            asset: asset.clone(),
            caption: caption.iter().map(normalize_inline).collect(),
        },
        _ => block.clone(),
    }
}

fn normalize_inline(i: &Inline) -> Inline {
    match i {
        Inline::Styled {
            style,
            direct,
            content,
        } => Inline::Styled {
            style: style.clone(),
            direct: direct.clone(),
            content: content.iter().map(normalize_inline).collect(),
        },
        Inline::Text { text } => Inline::Text {
            text: normalize_multiline(text),
        },
        Inline::Emph { content } => Inline::Emph {
            content: content.iter().map(normalize_inline).collect(),
        },
        Inline::Strong { content } => Inline::Strong {
            content: content.iter().map(normalize_inline).collect(),
        },
        Inline::Strikeout { content } => Inline::Strikeout {
            content: content.iter().map(normalize_inline).collect(),
        },
        Inline::Superscript { content } => Inline::Superscript {
            content: content.iter().map(normalize_inline).collect(),
        },
        Inline::Subscript { content } => Inline::Subscript {
            content: content.iter().map(normalize_inline).collect(),
        },
        Inline::Code { text } => Inline::Code { text: text.clone() },
        Inline::Link {
            url,
            title,
            content,
        } => Inline::Link {
            url: url.clone(),
            title: title.as_deref().map(normalize_singleline),
            content: content.iter().map(normalize_inline).collect(),
        },
        Inline::Image { target, alt, title } => Inline::Image {
            target: normalize_asset_ref(target),
            alt: normalize_singleline(alt),
            title: title.as_deref().map(normalize_singleline),
        },
        Inline::SoftBreak {} => Inline::SoftBreak {},
        Inline::LineBreak {} => Inline::LineBreak {},
        Inline::Math { tex, display } => Inline::Math {
            tex: tex.clone(),
            display: *display,
        },
        Inline::FootnoteRef { id } => Inline::FootnoteRef { id: id.clone() },
        Inline::Raw { format, text } => Inline::Raw {
            format: *format,
            text: text.clone(),
        },
    }
}

fn normalize_asset_ref(a: &AssetRef) -> AssetRef {
    match a {
        AssetRef::Asset { id } => AssetRef::Asset { id: id.clone() },
        AssetRef::Url { href } => AssetRef::Url { href: href.clone() },
    }
}
