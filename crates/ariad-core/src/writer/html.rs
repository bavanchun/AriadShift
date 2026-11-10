//! Standalone HTML5 document writer.

use std::collections::BTreeSet;

use base64::engine::{Engine as _, general_purpose::STANDARD};
use unicode_normalization::UnicodeNormalization;

use crate::{
    ir::{
        Alignment, AssetRef, AssetStore, Block, CharacterStyle, Document, FontRef, Inline,
        ListItem, ParagraphAlignment, ParagraphStyle, RawFormat, StyleSheet, TableCell,
    },
    limits::Limits,
    warning::{Warning, WarningCode},
    writer::{WriteOutput, sanitize},
};

const DEFAULT_CSS: &str = r#"body {
  font-family: system-ui, -apple-system, sans-serif;
  line-height: 1.6;
  max-width: 48rem;
  margin: 2rem auto;
  padding: 0 1rem;
  color: #1a1a1a;
  background: #fff;
}
pre, code {
  font-family: ui-monospace, monospace;
}
pre {
  padding: 1rem;
  background: #f4f4f4;
  overflow-x: auto;
  border-radius: 4px;
}
table {
  border-collapse: collapse;
  width: 100%;
  margin: 1rem 0;
}
th, td {
  border: 1px solid #ddd;
  padding: 0.5rem;
  text-align: left;
}
th {
  background: #f8f8f8;
}
blockquote {
  border-left: 4px solid #ddd;
  margin: 1rem 0;
  padding-left: 1rem;
  color: #555;
}
img {
  max-width: 100%;
  height: auto;
}"#;

/// Writes a document to a standalone HTML5 document.
#[must_use]
pub fn write(document: &Document) -> WriteOutput {
    write_with_title_fallback(document, None)
}

/// Writes a document to a standalone HTML5 document with an optional title fallback.
#[must_use]
pub fn write_with_title_fallback(document: &Document, title_fallback: Option<&str>) -> WriteOutput {
    if super::markdown::check_output_size(document, title_fallback).is_none() {
        return super::markdown::output_ceiling_exceeded();
    }
    let prohibited_fonts = document
        .assets
        .values()
        .any(|asset| !asset.can_export_bytes());
    let normalized_doc = super::normalize_document(document);
    let document = &normalized_doc;
    let normalized_title_fallback = title_fallback.map(super::normalize_singleline);
    let title_fallback = normalized_title_fallback.as_deref();

    let mut out = String::new();
    let mut warnings = Vec::new();
    if prohibited_fonts {
        warnings.push(Warning::new(WarningCode::FontEmbeddingRestricted,
            "Restricted or unknown embedded font bytes were omitted; use a permitted family fallback."));
    }

    let lang = document.meta.language.as_deref().unwrap_or("und");

    out.push_str("<!DOCTYPE html>\n");
    out.push_str(&format!("<html lang=\"{}\">\n<head>\n", escape_attr(lang)));
    out.push_str("  <meta charset=\"utf-8\">\n");
    out.push_str("  <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");

    let title_candidate = document
        .meta
        .title
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .or(title_fallback.map(str::trim).filter(|t| !t.is_empty()));

    if let Some(title) = title_candidate {
        out.push_str(&format!("  <title>{}</title>\n", escape_html(title)));
    }

    if !document.meta.authors.is_empty() {
        let authors = document.meta.authors.join(", ");
        out.push_str(&format!(
            "  <meta name=\"author\" content=\"{}\">\n",
            escape_attr(&authors)
        ));
    }
    if !document.meta.keywords.is_empty() {
        let keywords = document.meta.keywords.join(", ");
        out.push_str(&format!(
            "  <meta name=\"keywords\" content=\"{}\">\n",
            escape_attr(&keywords)
        ));
    }
    if let Some(ref date) = document.meta.date {
        out.push_str(&format!(
            "  <meta name=\"date\" content=\"{}\">\n",
            escape_attr(date)
        ));
    }
    if let Some(ref subject) = document.meta.subject {
        out.push_str(&format!(
            "  <meta name=\"subject\" content=\"{}\">\n",
            escape_attr(subject)
        ));
    }

    out.push_str("  <style>\n");
    out.push_str(DEFAULT_CSS);
    out.push_str("\n  </style>\n</head>\n<body>\n");

    // Separate footnotes from other body blocks
    let mut main_blocks = Vec::new();
    let mut footnotes = Vec::new();

    for block in &document.body {
        if let Block::Footnote {
            note_id: id,
            blocks,
            ..
        } = block
        {
            footnotes.push((id.clone(), blocks.clone()));
        } else {
            main_blocks.push(block);
        }
    }

    let mut ctx = HtmlContext {
        styles: &document.styles,
        character: document.styles.defaults.character.clone(),
        table_character: None,
        table_paragraph: None,
        assets: &document.assets,
        embedded_assets: BTreeSet::new(),
        repeated_assets_warned: BTreeSet::new(),
        warnings: &mut warnings,
        raw_dropped_warned: false,
    };

    for block in main_blocks {
        write_block(block, &mut ctx, &mut out);
    }

    for block in &document.furniture {
        write_block(block, &mut ctx, &mut out);
    }

    // Footnotes section
    if !footnotes.is_empty() {
        out.push_str("<section role=\"doc-endnotes\">\n<hr>\n");
        for (id, blocks) in &footnotes {
            out.push_str(&format!(
                "<div role=\"doc-footnote\" id=\"{}\">\n",
                escape_attr(id)
            ));
            for block in blocks {
                write_block(block, &mut ctx, &mut out);
            }
            out.push_str("</div>\n");
        }
        out.push_str("</section>\n");
    }

    out.push_str("</body>\n</html>\n");

    if out.len() > Limits::MAX_TEXT_OUTPUT_BYTES {
        return super::markdown::output_ceiling_exceeded();
    }
    WriteOutput {
        content: out,
        warnings,
    }
}

struct HtmlContext<'a> {
    styles: &'a StyleSheet,
    character: CharacterStyle,
    table_character: Option<CharacterStyle>,
    table_paragraph: Option<ParagraphStyle>,
    assets: &'a AssetStore,
    embedded_assets: BTreeSet<String>,
    repeated_assets_warned: BTreeSet<String>,
    warnings: &'a mut Vec<Warning>,
    raw_dropped_warned: bool,
}

fn write_block(block: &Block, ctx: &mut HtmlContext<'_>, out: &mut String) {
    let parent = ctx.character.clone();
    let parent_table = ctx.table_character.clone();
    let parent_paragraph = ctx.table_paragraph.clone();
    let resolved = if matches!(block, Block::Table { .. }) {
        ctx.styles
            .resolve_table(block.style().as_ref(), block.direct(), 100)
            .map(|table| {
                ctx.table_character = Some(table.character.clone());
                ctx.table_paragraph = Some(table.paragraph.clone());
                let mut css = String::new();
                if let Some(background) = &table.background {
                    css.push_str(&format!("background-color:{};", background.as_str()));
                }
                (table.paragraph, css)
            })
    } else {
        ctx.styles
            .resolve_paragraph_with_baseline(
                block.style().as_ref(),
                block.direct(),
                ctx.table_paragraph.as_ref(),
                100,
            )
            .map(|p| (p, String::new()))
    };
    let mut css = String::new();
    match resolved {
        Ok((paragraph, extra)) => {
            ctx.character.overlay(&CharacterStyle {
                font: paragraph.font.clone(),
                size_pt: paragraph.size_pt,
                color: paragraph.color.clone(),
                ..Default::default()
            });
            if let Some(table) = &ctx.table_character {
                ctx.character.overlay(table);
                let direct = if block.direct().based_on.is_some() {
                    &paragraph
                } else {
                    block.direct()
                };
                ctx.character.overlay(&CharacterStyle {
                    font: direct.font.clone(),
                    size_pt: direct.size_pt,
                    color: direct.color.clone(),
                    ..Default::default()
                });
            }
            css.push_str(&paragraph_css(&paragraph));
            css.push_str(&extra);
        }
        Err(error) => ctx
            .warnings
            .push(Warning::new(WarningCode::UnsupportedNode, error)),
    }
    // Apply to the element itself, retaining the native structure for read-back.
    let start = out.len();
    write_block_content(block, ctx, out);
    if let Some(end) = out[start..].find('>') {
        let mut attrs = String::new();
        if let Some(style) = block.style() {
            attrs.push_str(&format!(
                " data-ariad-style=\"{}\"",
                escape_attr(style.as_str())
            ));
        }
        if !css.is_empty() {
            attrs.push_str(&format!(" style=\"{}\"", escape_attr(&css)));
        }
        out.insert_str(start + end, &attrs);
    }
    ctx.character = parent;
    ctx.table_character = parent_table;
    ctx.table_paragraph = parent_paragraph;
}

fn write_block_content(block: &Block, ctx: &mut HtmlContext<'_>, out: &mut String) {
    match block {
        Block::Heading { level, content, .. } => {
            let clamped = (*level).clamp(1, 6);
            out.push_str(&format!("<h{clamped}>"));
            out.push_str(&render_inlines(content, ctx));
            out.push_str(&format!("</h{clamped}>\n"));
        }
        Block::Paragraph { content, .. } => {
            out.push_str("<p>");
            out.push_str(&render_inlines(content, ctx));
            out.push_str("</p>\n");
        }
        Block::Code { lang, text, .. } => {
            out.push_str("<pre>");
            if let Some(l) = lang {
                out.push_str(&format!("<code class=\"language-{}\">", escape_attr(l)));
            } else {
                out.push_str("<code>");
            }
            out.push_str(&escape_html(text));
            out.push_str("</code></pre>\n");
        }
        Block::Math { tex, .. } => {
            out.push_str(&format!(
                "<math display=\"block\"><semantics><mtext>{}</mtext><annotation encoding=\"application/x-tex\">{}</annotation></semantics></math>\n",
                escape_html(tex),
                escape_html(tex)
            ));
        }
        Block::Quote { blocks, .. } => {
            out.push_str("<blockquote>\n");
            for inner in blocks {
                write_block(inner, ctx, out);
            }
            out.push_str("</blockquote>\n");
        }
        Block::PageBreak { .. } => {
            out.push_str("<hr>\n");
        }
        Block::List {
            ordered,
            start,
            tight: _,
            items,
            ..
        } => {
            if *ordered {
                if let Some(s) = start {
                    out.push_str(&format!("<ol start=\"{s}\">\n"));
                } else {
                    out.push_str("<ol>\n");
                }
            } else {
                out.push_str("<ul>\n");
            }

            for item in items {
                write_list_item(item, ctx, out);
            }

            if *ordered {
                out.push_str("</ol>\n");
            } else {
                out.push_str("</ul>\n");
            }
        }
        Block::Table {
            caption,
            columns,
            head,
            body,
            footnotes,
            ..
        } => {
            write_table(caption.as_deref(), columns, head, body, ctx, out);
            if !footnotes.is_empty() {
                out.push_str("<p>");
                out.push_str(&render_inlines(footnotes, ctx));
                out.push_str("</p>\n");
            }
        }
        Block::Figure { asset, caption, .. } => {
            out.push_str("<figure>\n");
            let alt = plain_text(caption);
            let img = render_image(asset, &alt, None, ctx);
            out.push_str(&img);
            out.push('\n');
            if !caption.is_empty() {
                out.push_str("<figcaption>");
                out.push_str(&render_inlines(caption, ctx));
                out.push_str("</figcaption>\n");
            }
            out.push_str("</figure>\n");
        }
        Block::Footnote {
            note_id: id,
            blocks,
            ..
        } => {
            out.push_str(&format!(
                "<div role=\"doc-footnote\" id=\"{}\">\n",
                escape_attr(id)
            ));
            for inner in blocks {
                write_block(inner, ctx, out);
            }
            out.push_str("</div>\n");
        }
        Block::Raw { format, text, .. } => match format {
            RawFormat::Html => {
                if !ctx.raw_dropped_warned {
                    ctx.warnings.push(Warning::new(
                        WarningCode::RawDropped,
                        "raw HTML content was sanitized",
                    ));
                    ctx.raw_dropped_warned = true;
                }
                let sanitized = sanitize::sanitize_raw_html(text);
                out.push_str(&sanitized);
                out.push('\n');
            }
            RawFormat::Tex => {
                out.push_str(&escape_html(text));
                out.push('\n');
            }
        },
    }
}

fn write_list_item(item: &ListItem, ctx: &mut HtmlContext<'_>, out: &mut String) {
    let has_checkbox = item.checked.is_some();
    if has_checkbox {
        out.push_str("<li class=\"task-list-item\">");
        if item.checked == Some(true) {
            out.push_str("<input type=\"checkbox\" checked disabled> ");
        } else {
            out.push_str("<input type=\"checkbox\" disabled> ");
        }
    } else {
        out.push_str("<li>");
    }

    if item.blocks.len() == 1
        && item.blocks[0].style().is_none()
        && item.blocks[0].direct().is_empty()
        && ctx.styles.defaults.is_empty()
        && let Block::Paragraph { content, .. } = &item.blocks[0]
    {
        out.push_str(&render_inlines(content, ctx));
    } else {
        for inner in &item.blocks {
            write_block(inner, ctx, out);
        }
    }
    out.push_str("</li>\n");
}

fn write_table(
    caption: Option<&[Inline]>,
    columns: &[crate::ir::ColumnSpec],
    head: &[Vec<TableCell>],
    body: &[Vec<TableCell>],
    ctx: &mut HtmlContext<'_>,
    out: &mut String,
) {
    out.push_str("<table>\n");
    if let Some(cap) = caption
        && !cap.is_empty()
    {
        out.push_str("<caption>");
        out.push_str(&render_inlines(cap, ctx));
        out.push_str("</caption>\n");
    }

    if !head.is_empty() {
        out.push_str("<thead>\n");
        for row in head {
            out.push_str("<tr>\n");
            for (col_idx, cell) in row.iter().enumerate() {
                let align = columns.get(col_idx).map_or(Alignment::Default, |c| c.align);
                write_table_cell(cell, true, align, ctx, out);
            }
            out.push_str("</tr>\n");
        }
        out.push_str("</thead>\n");
    }

    if !body.is_empty() {
        out.push_str("<tbody>\n");
        for row in body {
            out.push_str("<tr>\n");
            for (col_idx, cell) in row.iter().enumerate() {
                let align = columns.get(col_idx).map_or(Alignment::Default, |c| c.align);
                write_table_cell(cell, cell.header == Some(true), align, ctx, out);
            }
            out.push_str("</tr>\n");
        }
        out.push_str("</tbody>\n");
    }

    out.push_str("</table>\n");
}

fn write_table_cell(
    cell: &TableCell,
    is_header: bool,
    align: Alignment,
    ctx: &mut HtmlContext<'_>,
    out: &mut String,
) {
    let tag = if is_header { "th" } else { "td" };
    out.push_str(&format!("<{tag}"));

    if cell.colspan > 1 {
        out.push_str(&format!(" colspan=\"{}\"", cell.colspan));
    }
    if cell.rowspan > 1 {
        out.push_str(&format!(" rowspan=\"{}\"", cell.rowspan));
    }
    match align {
        Alignment::Left => out.push_str(" align=\"left\""),
        Alignment::Center => out.push_str(" align=\"center\""),
        Alignment::Right => out.push_str(" align=\"right\""),
        Alignment::Default => {}
    }
    out.push('>');

    if cell.blocks.len() == 1
        && cell.blocks[0].style().is_none()
        && cell.blocks[0].direct().is_empty()
        && ctx.styles.defaults.is_empty()
        && let Block::Paragraph { content, .. } = &cell.blocks[0]
    {
        out.push_str(&render_inlines(content, ctx));
    } else {
        for inner in &cell.blocks {
            write_block(inner, ctx, out);
        }
    }
    out.push_str(&format!("</{tag}>\n"));
}

fn render_single_inline(inline: &Inline, ctx: &mut HtmlContext<'_>) -> String {
    let content = render_single_inline_content(inline, ctx);
    if !ctx.character.is_empty()
        && !matches!(
            inline,
            Inline::Styled { .. }
                | Inline::Emph { .. }
                | Inline::Strong { .. }
                | Inline::Strikeout { .. }
                | Inline::Superscript { .. }
                | Inline::Subscript { .. }
                | Inline::Link { .. }
        )
    {
        format!(
            "<span style=\"{}\">{content}</span>",
            escape_attr(&character_css(&ctx.character))
        )
    } else {
        content
    }
}
fn render_semantic_character(
    content: &[Inline],
    direct: CharacterStyle,
    ctx: &mut HtmlContext<'_>,
) -> String {
    let parent = ctx.character.clone();
    ctx.character.overlay(&direct);
    let result = render_inlines(content, ctx);
    ctx.character = parent;
    result
}
fn render_single_inline_content(inline: &Inline, ctx: &mut HtmlContext<'_>) -> String {
    match inline {
        Inline::Text { text } => escape_prose(text),
        Inline::Styled {
            style,
            direct,
            content,
        } => {
            let parent = ctx.character.clone();
            let resolved = ctx
                .styles
                .resolve_character(style.as_ref(), direct, Some(&parent));
            let result = match resolved {
                Ok(character) => {
                    ctx.character = character;
                    let attrs = style
                        .as_ref()
                        .map(|id| format!(" data-ariad-style=\"{}\"", escape_attr(id.as_str())))
                        .unwrap_or_default();
                    format!("<span{attrs}>{}</span>", render_inlines(content, ctx))
                }
                Err(error) => {
                    ctx.warnings
                        .push(Warning::new(WarningCode::UnsupportedNode, error));
                    render_inlines(content, ctx)
                }
            };
            ctx.character = parent;
            result
        }
        Inline::Emph { content } if !ctx.character.is_empty() || contains_styled(content) => {
            render_semantic_character(
                content,
                CharacterStyle {
                    italic: Some(true),
                    ..Default::default()
                },
                ctx,
            )
        }
        Inline::Emph { content } => {
            format!("<em>{}</em>", render_inlines(content, ctx))
        }
        Inline::Strong { content } if !ctx.character.is_empty() || contains_styled(content) => {
            render_semantic_character(
                content,
                CharacterStyle {
                    weight: Some(700),
                    ..Default::default()
                },
                ctx,
            )
        }
        Inline::Strong { content } => {
            format!("<strong>{}</strong>", render_inlines(content, ctx))
        }
        Inline::Strikeout { content } if !ctx.character.is_empty() || contains_styled(content) => {
            render_semantic_character(
                content,
                CharacterStyle {
                    strike: Some(true),
                    ..Default::default()
                },
                ctx,
            )
        }
        Inline::Strikeout { content } => {
            format!("<del>{}</del>", render_inlines(content, ctx))
        }
        Inline::Superscript { content } => {
            format!("<sup>{}</sup>", render_inlines(content, ctx))
        }
        Inline::Subscript { content } => {
            format!("<sub>{}</sub>", render_inlines(content, ctx))
        }
        Inline::Code { text } => {
            format!("<code>{}</code>", escape_html(text))
        }
        Inline::SoftBreak {} => "\n".to_owned(),
        Inline::LineBreak {} => "<br>\n".to_owned(),
        Inline::Math { tex, display } => {
            if *display {
                format!(
                    "<math display=\"block\"><semantics><mtext>{}</mtext><annotation encoding=\"application/x-tex\">{}</annotation></semantics></math>",
                    escape_html(tex),
                    escape_html(tex)
                )
            } else {
                format!(
                    "<math><semantics><mtext>{}</mtext><annotation encoding=\"application/x-tex\">{}</annotation></semantics></math>",
                    escape_html(tex),
                    escape_html(tex)
                )
            }
        }
        Inline::FootnoteRef { id } => {
            format!(
                "<a role=\"doc-noteref\" href=\"#{}\"><sup>{}</sup></a>",
                escape_attr(id),
                escape_html(id)
            )
        }
        Inline::Raw { format, text } => match format {
            RawFormat::Html => {
                if !ctx.raw_dropped_warned {
                    ctx.warnings.push(Warning::new(
                        WarningCode::RawDropped,
                        "raw HTML content was stripped",
                    ));
                    ctx.raw_dropped_warned = true;
                }
                let stripped = super::strip_html_tags(text);
                escape_prose(&stripped)
            }
            RawFormat::Tex => escape_html(text),
        },
        Inline::Link {
            url,
            title,
            content,
        } => {
            if !crate::links::allowed_link(url) {
                ctx.warnings.push(Warning::new(
                    WarningCode::LinkDropped,
                    format!("link target `{url}` was reduced to its text"),
                ));
                render_inlines(content, ctx)
            } else {
                let mut s = format!(
                    "<a href=\"{}\" rel=\"noopener noreferrer\"",
                    escape_attr(url)
                );
                if let Some(t) = title {
                    s.push_str(&format!(" title=\"{}\"", escape_attr(t)));
                }
                s.push('>');
                s.push_str(&render_inlines(content, ctx));
                s.push_str("</a>");
                s
            }
        }
        Inline::Image { target, alt, title } => render_image(target, alt, title.as_deref(), ctx),
    }
}

fn render_inlines(inlines: &[Inline], ctx: &mut HtmlContext<'_>) -> String {
    let mut out = String::new();
    for inline in inlines {
        out.push_str(&render_single_inline(inline, ctx));
    }
    out
}

fn render_image(
    target: &AssetRef,
    alt: &str,
    title: Option<&str>,
    ctx: &mut HtmlContext<'_>,
) -> String {
    if let AssetRef::Asset { id } = target
        && ctx.embedded_assets.contains(id)
    {
        if ctx.repeated_assets_warned.insert(id.clone()) {
            ctx.warnings.push(Warning::new(
                WarningCode::ImageNotEmbedded,
                format!("repeated asset `{id}` was reduced to alt text to avoid duplicating embedded data"),
            ));
        }
        return escape_prose(alt);
    }
    let (url, warn) = match target {
        AssetRef::Asset { id } => {
            if let Some(asset) = ctx.assets.get(id) {
                ctx.embedded_assets.insert(id.clone());
                let b64 = STANDARD.encode(&asset.bytes);
                (
                    Some(format!("data:{};base64,{}", asset.media_type, b64)),
                    None,
                )
            } else {
                (
                    None,
                    Some(Warning::new(
                        WarningCode::ImageNotEmbedded,
                        format!("asset `{id}` was not found; image was reduced to its alt text"),
                    )),
                )
            }
        }
        AssetRef::Url { href } => {
            if is_allowed_image_url(href) {
                (Some(href.clone()), None)
            } else {
                (
                    None,
                    Some(Warning::new(
                        WarningCode::ImageNotEmbedded,
                        format!("image URL `{href}` was reduced to its alt text"),
                    )),
                )
            }
        }
    };

    if let Some(warning) = warn {
        ctx.warnings.push(warning);
    }

    if let Some(url) = url {
        let mut img = format!(
            "<img src=\"{}\" alt=\"{}\"",
            escape_attr(&url),
            escape_attr(alt)
        );
        if let Some(t) = title {
            img.push_str(&format!(" title=\"{}\"", escape_attr(t)));
        }
        img.push('>');
        img
    } else {
        escape_prose(alt)
    }
}

fn is_allowed_image_url(url: &str) -> bool {
    if url.trim() != url || url.is_empty() {
        return false;
    }
    if let Some((scheme, _)) = url.split_once(':') {
        if scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https") {
            return true;
        }
        if scheme.eq_ignore_ascii_case("data")
            && let Some(rest) = url.strip_prefix("data:")
        {
            return rest.starts_with("image/");
        }
        false
    } else {
        true
    }
}

fn escape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            other => out.push(other),
        }
    }
    out
}

fn escape_prose(text: &str) -> String {
    escape_html(text).nfc().collect()
}

fn escape_attr(attr: &str) -> String {
    escape_html(attr)
}

fn plain_text(inlines: &[Inline]) -> String {
    let mut text = String::new();
    let mut stack: Vec<&Inline> = inlines.iter().rev().collect();
    while let Some(inline) = stack.pop() {
        match inline {
            Inline::Text { text: val } | Inline::Code { text: val } => text.push_str(val),
            Inline::Math { tex, .. } => text.push_str(tex),
            Inline::Link { content, .. }
            | Inline::Styled { content, .. }
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
    text.nfc().collect()
}

// Escape every non-word character inside a CSS string. Attribute escaping is a
// separate final step, so neither quotes nor resource syntax can escape the value.
fn css_string(value: &str) -> String {
    use std::fmt::Write as _;
    let mut out = String::from("\"");
    for ch in value.chars() {
        if ch.is_alphanumeric() || ch == ' ' || ch == '-' {
            out.push(ch);
        } else {
            let _ = write!(out, "\\{:x} ", u32::from(ch));
        }
    }
    out.push('"');
    out
}
fn font_css(font: &FontRef) -> String {
    let names: Vec<_> = std::iter::once(&font.family)
        .chain(&font.fallback)
        .map(|v| css_string(v))
        .collect();
    format!("font-family:{};", names.join(","))
}
fn paragraph_css(style: &ParagraphStyle) -> String {
    let mut out = String::new();
    if let Some(font) = &style.font {
        out.push_str(&font_css(font));
    }
    for (name, value) in [
        ("font-size", style.size_pt),
        ("margin-top", style.space_before_pt),
        ("margin-bottom", style.space_after_pt),
        ("text-indent", style.indent_first_pt),
        ("margin-left", style.indent_left_pt),
        ("margin-right", style.indent_right_pt),
    ] {
        if let Some(value) = value {
            out.push_str(&format!("{name}:{value}pt;"));
        }
    }
    if let Some(value) = style.line_height {
        out.push_str(&format!("line-height:{value};"));
    }
    if let Some(value) = style.align {
        out.push_str(match value {
            ParagraphAlignment::Left => "text-align:left;",
            ParagraphAlignment::Center => "text-align:center;",
            ParagraphAlignment::Right => "text-align:right;",
            ParagraphAlignment::Justify => "text-align:justify;",
        });
    }
    if let Some(value) = style.keep_with_next {
        out.push_str(if value {
            "break-after:avoid;"
        } else {
            "break-after:auto;"
        });
    }
    if let Some(value) = style.page_break_before {
        out.push_str(if value {
            "break-before:page;"
        } else {
            "break-before:auto;"
        });
    }
    if let Some(value) = &style.color {
        out.push_str(&format!("color:{};", value.as_str()));
    }
    if let Some(value) = &style.background {
        out.push_str(&format!("background-color:{};", value.as_str()));
    }
    out
}
fn character_css(style: &CharacterStyle) -> String {
    let mut out = String::new();
    if let Some(font) = &style.font {
        out.push_str(&font_css(font));
    }
    for (name, value) in [
        ("font-size", style.size_pt),
        ("letter-spacing", style.letter_spacing),
    ] {
        if let Some(value) = value {
            out.push_str(&format!("{name}:{value}pt;"));
        }
    }
    if let Some(value) = style.weight {
        out.push_str(&format!("font-weight:{value};"));
    }
    if let Some(value) = style.italic {
        out.push_str(if value {
            "font-style:italic;"
        } else {
            "font-style:normal;"
        });
    }
    if style.underline.is_some() || style.strike.is_some() {
        let mut decorations = Vec::new();
        if style.underline == Some(true) {
            decorations.push("underline");
        }
        if style.strike == Some(true) {
            decorations.push("line-through");
        }
        if decorations.is_empty() {
            decorations.push("none");
        }
        out.push_str(&format!("text-decoration-line:{};", decorations.join(" ")));
    }
    if let Some(value) = style.small_caps {
        out.push_str(if value {
            "font-variant-caps:small-caps;"
        } else {
            "font-variant-caps:normal;"
        });
    }
    if let Some(value) = style.baseline_shift {
        out.push_str(&format!("vertical-align:{value}pt;"));
    }
    if let Some(value) = &style.color {
        out.push_str(&format!("color:{};", value.as_str()));
    }
    if let Some(value) = &style.highlight {
        out.push_str(&format!("background-color:{};", value.as_str()));
    }
    out
}

fn contains_styled(inlines: &[Inline]) -> bool {
    let mut pending: Vec<_> = inlines.iter().collect();
    while let Some(inline) = pending.pop() {
        match inline {
            Inline::Styled {
                style,
                direct,
                content,
            } => {
                if style.is_some() || !direct.is_empty() {
                    return true;
                }
                pending.extend(content);
            }
            Inline::Emph { content }
            | Inline::Strong { content }
            | Inline::Strikeout { content }
            | Inline::Superscript { content }
            | Inline::Subscript { content }
            | Inline::Link { content, .. } => pending.extend(content),
            _ => {}
        }
    }
    false
}
