//! Standalone HTML5 document writer.

use base64::engine::{Engine as _, general_purpose::STANDARD};
use unicode_normalization::UnicodeNormalization;

use crate::{
    ir::{
        Alignment, AssetRef, AssetStore, Block, Document, Inline, ListItem, RawFormat, TableCell,
    },
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
    let normalized_doc = super::normalize_document(document);
    let document = &normalized_doc;
    let normalized_title_fallback = title_fallback.map(super::normalize_singleline);
    let title_fallback = normalized_title_fallback.as_deref();

    let mut out = String::new();
    let mut warnings = Vec::new();

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
        if let Block::Footnote { id, blocks } = block {
            footnotes.push((id.clone(), blocks.clone()));
        } else {
            main_blocks.push(block);
        }
    }

    let mut ctx = HtmlContext {
        assets: &document.assets,
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

    WriteOutput {
        content: out,
        warnings,
    }
}

struct HtmlContext<'a> {
    assets: &'a AssetStore,
    warnings: &'a mut Vec<Warning>,
    raw_dropped_warned: bool,
}

fn write_block(block: &Block, ctx: &mut HtmlContext<'_>, out: &mut String) {
    match block {
        Block::Heading { level, content } => {
            let clamped = (*level).clamp(1, 6);
            out.push_str(&format!("<h{clamped}>"));
            out.push_str(&render_inlines(content, ctx));
            out.push_str(&format!("</h{clamped}>\n"));
        }
        Block::Paragraph { content } => {
            out.push_str("<p>");
            out.push_str(&render_inlines(content, ctx));
            out.push_str("</p>\n");
        }
        Block::Code { lang, text } => {
            out.push_str("<pre>");
            if let Some(l) = lang {
                out.push_str(&format!("<code class=\"language-{}\">", escape_attr(l)));
            } else {
                out.push_str("<code>");
            }
            out.push_str(&escape_html(text));
            out.push_str("</code></pre>\n");
        }
        Block::Math { tex, display: _ } => {
            out.push_str(&format!(
                "<math display=\"block\"><semantics><mtext>{}</mtext><annotation encoding=\"application/x-tex\">{}</annotation></semantics></math>\n",
                escape_html(tex),
                escape_html(tex)
            ));
        }
        Block::Quote { blocks } => {
            out.push_str("<blockquote>\n");
            for inner in blocks {
                write_block(inner, ctx, out);
            }
            out.push_str("</blockquote>\n");
        }
        Block::PageBreak {} => {
            out.push_str("<hr>\n");
        }
        Block::List {
            ordered,
            start,
            tight: _,
            items,
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
            ..
        } => {
            write_table(caption.as_deref(), columns, head, body, ctx, out);
        }
        Block::Figure { asset, caption } => {
            out.push_str("<figure>\n");
            let alt = plain_text(caption);
            let img = render_image(asset, &alt, None, ctx.assets, ctx.warnings);
            out.push_str(&img);
            out.push('\n');
            if !caption.is_empty() {
                out.push_str("<figcaption>");
                out.push_str(&render_inlines(caption, ctx));
                out.push_str("</figcaption>\n");
            }
            out.push_str("</figure>\n");
        }
        Block::Footnote { id, blocks } => {
            out.push_str(&format!(
                "<div role=\"doc-footnote\" id=\"{}\">\n",
                escape_attr(id)
            ));
            for inner in blocks {
                write_block(inner, ctx, out);
            }
            out.push_str("</div>\n");
        }
        Block::Raw { format, text } => match format {
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
        && let Block::Paragraph { content } = &item.blocks[0]
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
        && let Block::Paragraph { content } = &cell.blocks[0]
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
    match inline {
        Inline::Text { text } => escape_prose(text),
        Inline::Emph { content } => {
            format!("<em>{}</em>", render_inlines(content, ctx))
        }
        Inline::Strong { content } => {
            format!("<strong>{}</strong>", render_inlines(content, ctx))
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
        Inline::Image { target, alt, title } => {
            render_image(target, alt, title.as_deref(), ctx.assets, ctx.warnings)
        }
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
    assets: &AssetStore,
    warnings: &mut Vec<Warning>,
) -> String {
    let (url, warn) = match target {
        AssetRef::Asset { id } => {
            if let Some(asset) = assets.get(id) {
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
        warnings.push(warning);
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
