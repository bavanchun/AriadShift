//! Serializers from document IR to text and markup formats.

pub mod html;
pub mod markdown;
pub mod sanitize;

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
                && after_close.len() >= tag_name.len()
                && after_close[..tag_name.len()].eq_ignore_ascii_case(tag_name)
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
                if after_open.len() >= 6
                    && after_open[..6].eq_ignore_ascii_case("script")
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
                } else if after_open.len() >= 5
                    && after_open[..5].eq_ignore_ascii_case("style")
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

/// Normalizes all string fields of a `Document` once at writer entry.
///
/// Converts CRLF, lone CR, U+2028, and U+0085 into LF (for multiline content)
/// or space (for single-line attributes and URLs).
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
        body: doc.body.iter().map(normalize_block).collect(),
        assets: doc.assets.clone(),
        layout: doc.layout.clone(),
        provenance: doc.provenance.clone(),
        furniture: doc.furniture.iter().map(normalize_block).collect(),
    }
}

fn normalize_block(b: &Block) -> Block {
    match b {
        Block::Heading { level, content } => Block::Heading {
            level: *level,
            content: content.iter().map(normalize_inline).collect(),
        },
        Block::Paragraph { content } => Block::Paragraph {
            content: content.iter().map(normalize_inline).collect(),
        },
        Block::Code { lang, text } => Block::Code {
            lang: lang.as_deref().map(normalize_singleline),
            text: normalize_multiline(text),
        },
        Block::Math { tex, display } => Block::Math {
            tex: normalize_multiline(tex),
            display: *display,
        },
        Block::Quote { blocks } => Block::Quote {
            blocks: blocks.iter().map(normalize_block).collect(),
        },
        Block::PageBreak {} => Block::PageBreak {},
        Block::List {
            ordered,
            start,
            tight,
            items,
        } => Block::List {
            ordered: *ordered,
            start: *start,
            tight: *tight,
            items: items.iter().map(normalize_list_item).collect(),
        },
        Block::Table {
            caption,
            columns,
            head,
            body,
            footnotes,
        } => Block::Table {
            caption: caption
                .as_ref()
                .map(|c| c.iter().map(normalize_inline).collect()),
            columns: columns.clone(),
            head: head
                .iter()
                .map(|row| row.iter().map(normalize_cell).collect())
                .collect(),
            body: body
                .iter()
                .map(|row| row.iter().map(normalize_cell).collect())
                .collect(),
            footnotes: footnotes.iter().map(normalize_inline).collect(),
        },
        Block::Figure { asset, caption } => Block::Figure {
            asset: normalize_asset_ref(asset),
            caption: caption.iter().map(normalize_inline).collect(),
        },
        Block::Footnote { id, blocks } => Block::Footnote {
            id: normalize_singleline(id),
            blocks: blocks.iter().map(normalize_block).collect(),
        },
        Block::Raw { format, text } => Block::Raw {
            format: *format,
            text: normalize_multiline(text),
        },
    }
}

fn normalize_list_item(item: &ListItem) -> ListItem {
    ListItem {
        checked: item.checked,
        blocks: item.blocks.iter().map(normalize_block).collect(),
    }
}

fn normalize_cell(cell: &TableCell) -> TableCell {
    TableCell {
        rowspan: cell.rowspan,
        colspan: cell.colspan,
        header: cell.header,
        blocks: cell.blocks.iter().map(normalize_block).collect(),
    }
}

fn normalize_inline(i: &Inline) -> Inline {
    match i {
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
        Inline::Code { text } => Inline::Code {
            text: normalize_multiline(text),
        },
        Inline::Link {
            url,
            title,
            content,
        } => Inline::Link {
            url: normalize_singleline(url),
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
            tex: normalize_multiline(tex),
            display: *display,
        },
        Inline::FootnoteRef { id } => Inline::FootnoteRef {
            id: normalize_singleline(id),
        },
        Inline::Raw { format, text } => Inline::Raw {
            format: *format,
            text: normalize_multiline(text),
        },
    }
}

fn normalize_asset_ref(a: &AssetRef) -> AssetRef {
    match a {
        AssetRef::Asset { id } => AssetRef::Asset {
            id: normalize_singleline(id),
        },
        AssetRef::Url { href } => AssetRef::Url {
            href: normalize_singleline(href),
        },
    }
}
