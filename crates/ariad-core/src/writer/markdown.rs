//! Markdown document writer producing GitHub Flavored Markdown (GFM).

use base64::engine::{Engine as _, general_purpose::STANDARD};
use unicode_normalization::UnicodeNormalization;

use crate::{
    ir::{
        Alignment, AssetRef, AssetStore, Block, Document, Inline, ListItem, Metadata, RawFormat,
        TableCell,
    },
    warning::{Warning, WarningCode},
    writer::WriteOutput,
};

struct MarkdownContext<'a> {
    assets: &'a AssetStore,
    warnings: &'a mut Vec<Warning>,
    raw_dropped_warned: bool,
    math_fallback_warned: bool,
    task_checkbox_dropped_warned: bool,
}

/// Serializes an IR document to GitHub Flavored Markdown.
#[must_use]
pub fn write(document: &Document) -> WriteOutput {
    let normalized_doc = super::normalize_document(document);
    let document = &normalized_doc;

    let mut out = String::new();
    let mut warnings = Vec::new();

    // 1. YAML front matter
    write_front_matter(&document.meta, &mut out);

    let mut ctx = MarkdownContext {
        assets: &document.assets,
        warnings: &mut warnings,
        raw_dropped_warned: false,
        math_fallback_warned: false,
        task_checkbox_dropped_warned: false,
    };

    // 2. Body blocks
    for block in &document.body {
        write_block(block, &mut ctx, &mut out);
    }

    // 3. Furniture blocks (if any)
    for block in &document.furniture {
        write_block(block, &mut ctx, &mut out);
    }

    WriteOutput {
        content: out,
        warnings,
    }
}

fn write_front_matter(meta: &Metadata, out: &mut String) {
    let has_title = meta.title.is_some();
    let has_authors = !meta.authors.is_empty();
    let has_lang = meta.language.is_some();
    let has_date = meta.date.is_some();
    let has_subject = meta.subject.is_some();
    let has_keywords = !meta.keywords.is_empty();

    if !has_title && !has_authors && !has_lang && !has_date && !has_subject && !has_keywords {
        return;
    }

    out.push_str("---\n");
    if let Some(ref title) = meta.title {
        out.push_str("title: ");
        out.push_str(&format_yaml_string(title));
        out.push('\n');
    }
    if !meta.authors.is_empty() {
        out.push_str("author:\n");
        for author in &meta.authors {
            out.push_str("  - ");
            out.push_str(&format_yaml_string(author));
            out.push('\n');
        }
    }
    if let Some(ref lang) = meta.language {
        out.push_str("lang: ");
        out.push_str(&format_yaml_string(lang));
        out.push('\n');
    }
    if let Some(ref date) = meta.date {
        out.push_str("date: ");
        out.push_str(&format_yaml_string(date));
        out.push('\n');
    }
    if let Some(ref subject) = meta.subject {
        out.push_str("subject: ");
        out.push_str(&format_yaml_string(subject));
        out.push('\n');
    }
    if !meta.keywords.is_empty() {
        out.push_str("keywords:\n");
        for keyword in &meta.keywords {
            out.push_str("  - ");
            out.push_str(&format_yaml_string(keyword));
            out.push('\n');
        }
    }
    out.push_str("---\n\n");
}

fn format_yaml_string(val: &str) -> String {
    let is_simple = !val.is_empty()
        && val
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        && !matches!(
            val.to_ascii_lowercase().as_str(),
            "true" | "false" | "yes" | "no" | "null" | "y" | "n" | "on" | "off"
        )
        && !val.starts_with(|c: char| c.is_ascii_digit())
        && !val.starts_with('-');
    if is_simple {
        val.to_owned()
    } else {
        serde_json::to_string(val).unwrap_or_else(|_| format!("\"{val}\""))
    }
}

fn write_block(block: &Block, ctx: &mut MarkdownContext, out: &mut String) {
    match block {
        Block::Heading { level, content } => {
            let clamped = (*level).clamp(1, 6) as usize;
            out.push_str(&"#".repeat(clamped));
            out.push(' ');
            let mut heading_text = render_inlines(content, false, true, ctx);
            // Trailing # preceded by space in heading would be treated as closing ATX hashes:
            // Escape trailing # as \#
            if heading_text.ends_with('#') {
                let chars: Vec<char> = heading_text.chars().collect();
                let mut j = chars.len() - 1;
                while j > 0 && chars[j] == '#' {
                    j -= 1;
                }
                if chars[j] == ' ' || chars[j] == '\t' {
                    let prefix: String = chars[..=j].iter().collect();
                    let count = chars.len() - 1 - j;
                    heading_text = format!("{prefix}{}", "\\#".repeat(count));
                }
            }
            out.push_str(&heading_text);
            out.push_str("\n\n");
        }
        Block::Paragraph { content } => {
            out.push_str(&render_inlines(content, false, false, ctx));
            out.push_str("\n\n");
        }
        Block::Code { lang, text } => {
            out.push_str(&format_fenced_block(lang.as_deref(), text));
        }
        Block::Math { tex, display } => {
            let trimmed = tex.trim_matches(|c| c == '\r' || c == '\n');
            let has_internal_newline = trimmed.contains('\n') || trimmed.contains('\r');
            if *display {
                if has_internal_newline || !is_safe_tex(trimmed) {
                    if !ctx.math_fallback_warned {
                        ctx.warnings.push(Warning::new(
                            WarningCode::UnsupportedNode,
                            "math expression was formatted as a code span or code block",
                        ));
                        ctx.math_fallback_warned = true;
                    }
                    out.push_str(&format_fenced_block(Some("math"), tex));
                } else {
                    out.push_str(&format!("$${trimmed}$$\n\n"));
                }
            } else if !has_internal_newline && is_safe_tex(trimmed) {
                out.push_str(&format!("${trimmed}$\n\n"));
            } else {
                if !ctx.math_fallback_warned {
                    ctx.warnings.push(Warning::new(
                        WarningCode::UnsupportedNode,
                        "math expression was formatted as a code span or code block",
                    ));
                    ctx.math_fallback_warned = true;
                }
                out.push_str(&format_fenced_block(Some("math"), tex));
            }
        }
        Block::Quote { blocks } => {
            let mut quote_out = String::new();
            for inner in blocks {
                write_block(inner, ctx, &mut quote_out);
            }
            let trimmed = quote_out.trim_end_matches('\n');
            for line in trimmed.lines() {
                if line.is_empty() {
                    out.push('>');
                } else {
                    out.push_str("> ");
                    out.push_str(line);
                }
                out.push('\n');
            }
            out.push('\n');
        }
        Block::PageBreak {} => {
            out.push_str("***\n\n");
        }
        Block::List {
            ordered,
            start,
            tight,
            items,
        } => {
            write_list(
                &ListContext {
                    ordered: *ordered,
                    start: *start,
                    tight: *tight,
                    items,
                    base_indent: String::new(),
                },
                ctx,
                out,
            );
        }
        Block::Table {
            columns,
            head,
            body,
            ..
        } => {
            write_table(columns, head, body, ctx, out);
        }
        Block::Figure { asset, caption } => {
            let alt = plain_text(caption);
            let img = render_image(asset, &alt, None, false, ctx);
            out.push_str(&img);
            out.push_str("\n\n");
        }
        Block::Footnote { id, blocks } => {
            write_footnote(id, blocks, ctx, out);
        }
        Block::Raw { format, text } => match format {
            RawFormat::Html => {
                if !ctx.raw_dropped_warned {
                    ctx.warnings.push(Warning::new(
                        WarningCode::RawDropped,
                        "raw HTML content was stripped",
                    ));
                    ctx.raw_dropped_warned = true;
                }
                let stripped = super::strip_html_tags(text);
                let escaped = escape_markdown_text(&stripped, EscapeOptions::default());
                if !escaped.trim().is_empty() {
                    out.push_str(&escaped);
                    out.push_str("\n\n");
                }
            }
            RawFormat::Tex => {
                ctx.warnings.push(Warning::new(
                    WarningCode::RawDropped,
                    "raw TeX content was formatted as a code block",
                ));
                out.push_str(&format_fenced_block(Some("tex"), text));
            }
        },
    }
}

struct ListContext<'a> {
    ordered: bool,
    start: Option<u64>,
    tight: bool,
    items: &'a [ListItem],
    base_indent: String,
}

fn write_list(list: &ListContext<'_>, ctx: &mut MarkdownContext, out: &mut String) {
    for (idx, item) in list.items.iter().enumerate() {
        let first_block_is_non_paragraph = item
            .blocks
            .first()
            .is_some_and(|b| !matches!(b, Block::Paragraph { .. }));
        let drop_task_checkbox = item.checked.is_some() && first_block_is_non_paragraph;
        if drop_task_checkbox && !ctx.task_checkbox_dropped_warned {
            ctx.warnings.push(Warning::new(
                WarningCode::UnsupportedNode,
                "task list checkbox was omitted because the item does not start with a paragraph",
            ));
            ctx.task_checkbox_dropped_warned = true;
        }
        let checked = if drop_task_checkbox {
            None
        } else {
            item.checked
        };

        let (marker, base_marker_len) = if list.ordered {
            let num = list.start.unwrap_or(1) + idx as u64;
            let base = format!("{num}. ");
            let base_len = base.len();
            let marker = match checked {
                Some(true) => format!("{base}[x] "),
                Some(false) => format!("{base}[ ] "),
                None => base,
            };
            (marker, base_len)
        } else {
            let base_len = 2;
            let marker = match checked {
                Some(true) => "- [x] ".to_owned(),
                Some(false) => "- [ ] ".to_owned(),
                None => "- ".to_owned(),
            };
            (marker, base_len)
        };

        let item_prefix = format!("{}{marker}", list.base_indent);
        let cont_prefix = format!("{}{}", list.base_indent, " ".repeat(base_marker_len));

        if item.blocks.is_empty() {
            out.push_str(&item_prefix);
            out.push('\n');
            continue;
        }

        let mut first = true;
        for inner in &item.blocks {
            if !first && (!matches!(inner, Block::List { .. }) || !list.tight) {
                out.push('\n');
            }

            match inner {
                Block::Paragraph { content } => {
                    let text = render_inlines(content, false, false, ctx);
                    let mut lines = text.lines();
                    if first {
                        if let Some(first_line) = lines.next() {
                            out.push_str(&item_prefix);
                            out.push_str(first_line);
                            out.push('\n');
                        }
                    } else if let Some(first_line) = lines.next() {
                        out.push_str(&cont_prefix);
                        out.push_str(first_line);
                        out.push('\n');
                    }
                    for line in lines {
                        out.push_str(&cont_prefix);
                        out.push_str(line);
                        out.push('\n');
                    }
                    first = false;
                }
                Block::List {
                    ordered: inner_ordered,
                    start: inner_start,
                    tight: inner_tight,
                    items: inner_items,
                } => {
                    if first {
                        out.push_str(&item_prefix);
                        out.push('\n');
                        first = false;
                    }
                    write_list(
                        &ListContext {
                            ordered: *inner_ordered,
                            start: *inner_start,
                            tight: *inner_tight,
                            items: inner_items,
                            base_indent: cont_prefix.clone(),
                        },
                        ctx,
                        out,
                    );
                }
                other => {
                    let mut block_out = String::new();
                    write_block(other, ctx, &mut block_out);
                    let trimmed = block_out.trim_end_matches('\n');
                    if first {
                        if drop_task_checkbox {
                            out.push_str(&item_prefix);
                            out.push('\n');
                            for line in trimmed.lines() {
                                out.push_str(&cont_prefix);
                                out.push_str(line);
                                out.push('\n');
                            }
                        } else {
                            let mut lines = trimmed.lines();
                            if let Some(first_line) = lines.next() {
                                out.push_str(&item_prefix);
                                out.push_str(first_line);
                                out.push('\n');
                            }
                            for line in lines {
                                out.push_str(&cont_prefix);
                                out.push_str(line);
                                out.push('\n');
                            }
                        }
                        first = false;
                    } else {
                        for line in trimmed.lines() {
                            out.push_str(&cont_prefix);
                            out.push_str(line);
                            out.push('\n');
                        }
                    }
                }
            }
        }

        if !list.tight {
            out.push('\n');
        }
    }
    if list.base_indent.is_empty() && list.tight {
        out.push('\n');
    }
}

fn write_table(
    columns: &[crate::ir::ColumnSpec],
    head: &[Vec<TableCell>],
    body: &[Vec<TableCell>],
    ctx: &mut MarkdownContext,
    out: &mut String,
) {
    // Check for complex non-pipe-representable table features
    let mut has_complex = false;
    for row in head.iter().chain(body.iter()) {
        for cell in row {
            if cell.rowspan > 1 || cell.colspan > 1 || cell.blocks.len() > 1 {
                has_complex = true;
            } else if let Some(first) = cell.blocks.first()
                && !matches!(first, Block::Paragraph { .. } | Block::Heading { .. })
            {
                has_complex = true;
            }
        }
    }
    if has_complex {
        ctx.warnings.push(Warning::new(
            WarningCode::UnsupportedNode,
            "table with cell spans or block cells was simplified to a pipe table",
        ));
    }

    let mut col_count = columns.len();
    if let Some(first) = head.first() {
        col_count = col_count.max(first.len());
    }
    if let Some(first) = body.first() {
        col_count = col_count.max(first.len());
    }
    if col_count == 0 {
        return;
    }

    // 1. Header row
    out.push_str("| ");
    let head_row = head.first();
    for col_idx in 0..col_count {
        if col_idx > 0 {
            out.push_str(" | ");
        }
        if let Some(cell) = head_row.and_then(|r| r.get(col_idx)) {
            out.push_str(&render_cell_content(cell, ctx));
        }
    }
    out.push_str(" |\n");

    // 2. Delimiter row
    out.push_str("| ");
    for col_idx in 0..col_count {
        if col_idx > 0 {
            out.push_str(" | ");
        }
        let align = columns
            .get(col_idx)
            .map_or(Alignment::Default, |col| col.align);
        match align {
            Alignment::Left => out.push_str(":---"),
            Alignment::Center => out.push_str(":---:"),
            Alignment::Right => out.push_str("---:"),
            Alignment::Default => out.push_str("---"),
        }
    }
    out.push_str(" |\n");

    // 3. Body rows
    for row in body {
        out.push_str("| ");
        for col_idx in 0..col_count {
            if col_idx > 0 {
                out.push_str(" | ");
            }
            if let Some(cell) = row.get(col_idx) {
                out.push_str(&render_cell_content(cell, ctx));
            }
        }
        out.push_str(" |\n");
    }
    out.push('\n');
}

fn render_cell_content(cell: &TableCell, ctx: &mut MarkdownContext) -> String {
    let mut inlines = Vec::new();
    for block in &cell.blocks {
        match block {
            Block::Paragraph { content } | Block::Heading { content, .. } => {
                inlines.extend(content.clone());
            }
            _ => {}
        }
    }
    let rendered = render_inlines(&inlines, true, false, ctx);
    rendered.replace('\n', " ").trim().to_owned()
}

fn format_footnote_id(id: &str) -> String {
    let mut out = String::with_capacity(id.len());
    for ch in id.chars() {
        match ch {
            '\\' => out.push_str("%5C"),
            '[' => out.push_str("%5B"),
            ']' => out.push_str("%5D"),
            ' ' => out.push_str("%20"),
            '\n' => out.push_str("%0A"),
            '\r' => out.push_str("%0D"),
            c => out.push(c),
        }
    }
    out
}

fn write_footnote(id: &str, blocks: &[Block], ctx: &mut MarkdownContext, out: &mut String) {
    let safe_id = format_footnote_id(id);
    out.push_str(&format!("[^{safe_id}]: "));
    if blocks.is_empty() {
        out.push_str("\n\n");
        return;
    }

    if blocks.len() == 1
        && let Block::Paragraph { content } = &blocks[0]
    {
        out.push_str(&render_inlines(content, false, false, ctx));
        out.push_str("\n\n");
        return;
    }

    let mut first = true;
    for block in blocks {
        let mut block_out = String::new();
        write_block(block, ctx, &mut block_out);
        let trimmed = block_out.trim_end_matches('\n');
        if first {
            let mut lines = trimmed.lines();
            if let Some(first_line) = lines.next() {
                out.push_str(first_line);
                out.push('\n');
            }
            for line in lines {
                out.push_str("    ");
                out.push_str(line);
                out.push('\n');
            }
            first = false;
        } else {
            out.push('\n');
            for line in trimmed.lines() {
                out.push_str("    ");
                out.push_str(line);
                out.push('\n');
            }
        }
    }
    out.push('\n');
}

/// Returns the length of the longest consecutive run of `target` character in `s`.
pub(crate) fn longest_run(s: &str, target: char) -> usize {
    let mut max_run = 0;
    let mut current_run = 0;
    for ch in s.chars() {
        if ch == target {
            current_run += 1;
            if current_run > max_run {
                max_run = current_run;
            }
        } else {
            current_run = 0;
        }
    }
    max_run
}

/// Computes delimiter string and formatted content for fenced blocks and code spans.
///
/// For fences (`is_fence = true`), delimiter is at least 3 backticks, and strictly longer than
/// any backtick run and any tilde run in `text`.
/// For inline code spans (`is_fence = false`), delimiter is at least 1 backtick, and strictly longer than
/// any backtick run in `text`.
///
/// Applies CommonMark padding rule: if `text` starts or ends with a backtick or a space
/// (and is non-empty and not entirely spaces), pads with one space on each side.
#[doc(hidden)]
#[must_use]
pub fn format_delimiter_and_content(text: &str, is_fence: bool) -> (String, String) {
    if is_fence {
        let max_ticks = longest_run(text, '`');
        let max_tildes = longest_run(text, '~');
        let max_run = std::cmp::max(max_ticks, max_tildes);
        let fence_len = std::cmp::max(3, max_run + 1);
        let fence = "`".repeat(fence_len);
        (fence, text.to_owned())
    } else {
        let max_ticks = longest_run(text, '`');
        let delim_len = if max_ticks == 0 { 1 } else { max_ticks + 1 };
        let delim = "`".repeat(delim_len);
        let is_all_spaces = !text.is_empty() && text.chars().all(|c| c == ' ');
        let pad = !is_all_spaces
            && (text.starts_with('`')
                || text.ends_with('`')
                || text.starts_with(' ')
                || text.ends_with(' '));
        let content = if pad {
            format!(" {text} ")
        } else {
            text.to_owned()
        };
        (delim, content)
    }
}

#[doc(hidden)]
#[must_use]
pub fn format_fenced_block(lang: Option<&str>, text: &str) -> String {
    let (fence, content) = format_delimiter_and_content(text, true);
    let mut out = String::new();
    out.push_str(&fence);
    if let Some(l) = lang {
        let safe_lang: String = l
            .lines()
            .next()
            .unwrap_or("")
            .chars()
            .filter(|&c| c != '`' && c != '~')
            .collect();
        out.push_str(&safe_lang);
    }
    out.push('\n');
    out.push_str(&content);
    if !content.is_empty() && !content.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&fence);
    out.push_str("\n\n");
    out
}

#[doc(hidden)]
#[must_use]
pub fn format_code_span(text: &str, in_table: bool) -> String {
    let mut normalized = text.replace("\r\n", " ").replace(['\r', '\n'], " ");
    if in_table && normalized.contains('|') {
        normalized = normalized.replace('|', "\\|");
    }
    let (delim, content) = format_delimiter_and_content(&normalized, false);
    format!("{delim}{content}{delim}")
}

/// Returns true if the TeX expression satisfies the safe inline math criteria.
///
/// Must be non-empty, trimmed, not end with a backslash, contain no `$`, backtick, newline,
/// `<`, `>`, `|`, `[`, `]`, `!`, and allow only ASCII letters, digits, internal spaces,
/// `+ - * / = ^ _ { } ( ) , . ; : ' ~`, and `\` followed by an ASCII letter.
#[doc(hidden)]
#[must_use]
pub fn is_safe_tex(tex: &str) -> bool {
    if tex.is_empty() || tex != tex.trim() || tex.ends_with('\\') {
        return false;
    }
    let lower = tex.to_ascii_lowercase();
    if lower.contains("https://")
        || lower.contains("http://")
        || lower.contains("www.")
        || lower.contains('@')
    {
        return false;
    }
    let mut chars = tex.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            'a'..='z'
            | 'A'..='Z'
            | '0'..='9'
            | ' '
            | '+'
            | '-'
            | '*'
            | '/'
            | '='
            | '^'
            | '_'
            | '{'
            | '}'
            | '('
            | ')'
            | ','
            | '.'
            | ';'
            | ':'
            | '\''
            | '~' => {
                // Allowed
            }
            '\\' => {
                // A backslash must be immediately followed by an ASCII letter
                match chars.peek() {
                    Some(&next_ch) if next_ch.is_ascii_alphabetic() => {
                        // Allowed command prefix
                    }
                    _ => return false,
                }
            }
            _ => return false,
        }
    }
    true
}

/// Checks whether an inline node's rendered output starts with an ASCII digit.
fn starts_with_digit(inline: &Inline) -> bool {
    match inline {
        Inline::Text { text } => text.chars().next().is_some_and(|c| c.is_ascii_digit()),
        Inline::Raw {
            format: RawFormat::Html,
            text,
        } => {
            let stripped = super::strip_html_tags(text);
            stripped
                .trim_start()
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_digit())
        }
        _ => false,
    }
}

fn format_link_destination(url: &str, in_table: bool) -> String {
    let target_url = if in_table && url.contains('|') {
        url.replace('|', "%7C")
    } else {
        url.to_owned()
    };
    let url = target_url.as_str();

    let needs_angle_brackets = url.is_empty()
        || url.chars().any(|c| {
            c.is_whitespace()
                || c == '('
                || c == ')'
                || c == '<'
                || c == '>'
                || c == '\\'
                || c == '"'
                || c.is_ascii_control()
        });
    if needs_angle_brackets {
        let mut escaped = String::with_capacity(url.len() + 2);
        escaped.push('<');
        for ch in url.chars() {
            match ch {
                '\\' => escaped.push_str("\\\\"),
                '<' => escaped.push_str("\\<"),
                '>' => escaped.push_str("\\>"),
                '\n' => escaped.push_str("%0A"),
                '\r' => escaped.push_str("%0D"),
                c if c.is_ascii_control() => {
                    escaped.push_str(&format!("%{:02X}", c as u8));
                }
                c => escaped.push(c),
            }
        }
        escaped.push('>');
        escaped
    } else {
        url.to_owned()
    }
}

fn format_link_title(title: &str, in_table: bool) -> String {
    let mut escaped = String::with_capacity(title.len());
    for ch in title.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("&#10;"),
            '\r' => escaped.push_str("&#13;"),
            '|' if in_table => escaped.push_str("\\|"),
            c => escaped.push(c),
        }
    }
    escaped
}

fn render_delimited(
    inner: &str,
    delim: &str,
    inlines: &[Inline],
    idx: usize,
    _in_table: bool,
    out: &mut String,
) {
    if inner.trim().is_empty() {
        out.push_str(inner);
        return;
    }

    let trimmed_start = inner.trim_start_matches(char::is_whitespace);
    let leading_bytes = inner.len() - trimmed_start.len();
    let leading = &inner[..leading_bytes];

    let trimmed_end = trimmed_start.trim_end_matches(char::is_whitespace);
    let trailing = &trimmed_start[trimmed_end.len()..];
    let mut core = trimmed_end;

    out.push_str(leading);

    let mut trailing_punct = "";
    if let Some(next) = inlines.get(idx + 1)
        && let Inline::Text { text } = next
        && text.starts_with(|c: char| c.is_alphanumeric())
        && core.ends_with(|c: char| c.is_ascii_punctuation())
        && let Some((last_char_idx, _)) = core.char_indices().last()
    {
        trailing_punct = &core[last_char_idx..];
        core = &core[..last_char_idx];
    }

    let mut leading_punct = "";
    if idx > 0
        && let Inline::Text { text } = &inlines[idx - 1]
        && text.ends_with(|c: char| c.is_alphanumeric())
        && core.starts_with(|c: char| c.is_ascii_punctuation())
        && let Some(first_char) = core.chars().next()
    {
        let first_char_len = first_char.len_utf8();
        leading_punct = &core[..first_char_len];
        core = &core[first_char_len..];
    }

    out.push_str(leading_punct);
    out.push_str(delim);
    out.push_str(core);
    out.push_str(delim);
    out.push_str(trailing_punct);
    out.push_str(trailing);
}

fn render_inlines(
    inlines: &[Inline],
    in_table: bool,
    in_heading: bool,
    ctx: &mut MarkdownContext,
) -> String {
    let mut out = String::new();

    for (i, inline) in inlines.iter().enumerate() {
        let trailing_bang_before_link = inlines
            .get(i + 1)
            .is_some_and(|next| matches!(next, Inline::Link { .. } | Inline::Image { .. }));
        let escape_opts = EscapeOptions {
            in_table,
            trailing_bang_before_link,
        };

        match inline {
            Inline::Text { text } => {
                out.push_str(&escape_markdown_text(text, escape_opts));
            }
            Inline::Emph { content } => {
                let inner = render_inlines(content, in_table, in_heading, ctx);
                render_delimited(&inner, "*", inlines, i, in_table, &mut out);
            }
            Inline::Strong { content } => {
                let inner = render_inlines(content, in_table, in_heading, ctx);
                render_delimited(&inner, "**", inlines, i, in_table, &mut out);
            }
            Inline::Strikeout { content } => {
                let inner = render_inlines(content, in_table, in_heading, ctx);
                render_delimited(&inner, "~~", inlines, i, in_table, &mut out);
            }
            Inline::Superscript { content } => {
                out.push_str("<sup>");
                out.push_str(&render_inlines(content, in_table, in_heading, ctx));
                out.push_str("</sup>");
            }
            Inline::Subscript { content } => {
                out.push_str("<sub>");
                out.push_str(&render_inlines(content, in_table, in_heading, ctx));
                out.push_str("</sub>");
            }
            Inline::Code { text } => {
                if out.ends_with('`') {
                    out.push_str("<!---->");
                }
                out.push_str(&format_code_span(text, in_table));
            }
            Inline::SoftBreak {} => {
                if in_table || in_heading {
                    out.push(' ');
                } else {
                    out.push('\n');
                }
            }
            Inline::LineBreak {} => {
                if in_table {
                    out.push_str("<br />");
                } else if in_heading {
                    out.push(' ');
                } else {
                    out.push_str("  \n");
                }
            }
            Inline::Math { tex, display } => {
                let trimmed = tex.trim_matches(|c| c == '\r' || c == '\n');
                let has_internal_newline = trimmed.contains('\n') || trimmed.contains('\r');
                let is_safe = !has_internal_newline && is_safe_tex(trimmed);
                if *display {
                    if is_safe {
                        out.push_str(&format!("$${trimmed}$$"));
                    } else {
                        if !ctx.math_fallback_warned {
                            ctx.warnings.push(Warning::new(
                                WarningCode::UnsupportedNode,
                                "math expression was formatted as a code span or code block",
                            ));
                            ctx.math_fallback_warned = true;
                        }
                        if out.ends_with('`') {
                            out.push_str("<!---->");
                        }
                        out.push_str(&format_code_span(tex, in_table));
                    }
                } else {
                    let next_starts_with_digit = inlines.get(i + 1).is_some_and(starts_with_digit);
                    if is_safe && !next_starts_with_digit {
                        out.push_str(&format!("${trimmed}$"));
                    } else {
                        if !ctx.math_fallback_warned {
                            ctx.warnings.push(Warning::new(
                                WarningCode::UnsupportedNode,
                                "math expression was formatted as a code span or code block",
                            ));
                            ctx.math_fallback_warned = true;
                        }
                        if out.ends_with('`') {
                            out.push_str("<!---->");
                        }
                        out.push_str(&format_code_span(tex, in_table));
                    }
                }
            }
            Inline::FootnoteRef { id } => {
                let safe_id = format_footnote_id(id);
                out.push_str(&format!("[^{safe_id}]"));
            }
            Inline::Raw {
                format: RawFormat::Tex,
                text,
            } => {
                ctx.warnings.push(Warning::new(
                    WarningCode::RawDropped,
                    "raw TeX content was formatted as a code span",
                ));
                if out.ends_with('`') {
                    out.push_str("<!---->");
                }
                out.push_str(&format_code_span(text, in_table));
            }
            Inline::Raw {
                format: RawFormat::Html,
                text,
            } => {
                if !ctx.raw_dropped_warned {
                    ctx.warnings.push(Warning::new(
                        WarningCode::RawDropped,
                        "raw HTML content was stripped",
                    ));
                    ctx.raw_dropped_warned = true;
                }
                let stripped = super::strip_html_tags(text);
                out.push_str(&escape_markdown_text(&stripped, escape_opts));
            }
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
                    out.push_str(&render_inlines(content, in_table, in_heading, ctx));
                } else {
                    let is_autolink = title.is_none()
                        && content.len() == 1
                        && matches!(&content[0], Inline::Text { text } if text == url)
                        && (url.starts_with("http://") || url.starts_with("https://"))
                        && !url.chars().any(|c| {
                            c.is_whitespace()
                                || c == '<'
                                || c == '>'
                                || c == '\\'
                                || c.is_ascii_control()
                        });
                    if is_autolink {
                        out.push('<');
                        out.push_str(url);
                        out.push('>');
                    } else {
                        let text = render_inlines(content, in_table, in_heading, ctx);
                        let dest = format_link_destination(url, in_table);
                        if let Some(t) = title {
                            let escaped_title = format_link_title(t, in_table);
                            out.push_str(&format!("[{text}]({dest} \"{escaped_title}\")"));
                        } else {
                            out.push_str(&format!("[{text}]({dest})"));
                        }
                    }
                }
            }
            Inline::Image { target, alt, title } => {
                out.push_str(&render_image(target, alt, title.as_deref(), in_table, ctx));
            }
        }
    }
    out
}

fn render_image(
    target: &AssetRef,
    alt: &str,
    title: Option<&str>,
    in_table: bool,
    ctx: &mut MarkdownContext,
) -> String {
    let (url, warn) = match target {
        AssetRef::Asset { id } => {
            if let Some(asset) = ctx.assets.get(id) {
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

    let escape_opts = EscapeOptions {
        in_table,
        trailing_bang_before_link: false,
    };

    if let Some(url) = url {
        let escaped_alt = escape_markdown_text(alt, escape_opts);
        let dest = format_link_destination(&url, in_table);
        if let Some(t) = title {
            let escaped_title = format_link_title(t, in_table);
            format!("![{escaped_alt}]({dest} \"{escaped_title}\")")
        } else {
            format!("![{escaped_alt}]({dest})")
        }
    } else {
        escape_markdown_text(alt, escape_opts)
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
        // Relative URL (no scheme)
        true
    }
}

/// Options controlling Markdown text escaping.
#[derive(Clone, Copy, Debug, Default)]
pub struct EscapeOptions {
    /// True when escaping text inside a Markdown pipe table cell.
    pub in_table: bool,
    /// True when a trailing exclamation mark in the text node is immediately
    /// followed by an inline link or image, which would otherwise become `![...]`.
    pub trailing_bang_before_link: bool,
}

/// Escapes Markdown-active characters in text with context awareness.
#[must_use]
pub fn escape_markdown_text(text: &str, options: EscapeOptions) -> String {
    let mut out = String::with_capacity(text.len());
    let mut at_line_start = true;

    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let ch = chars[i];

        if at_line_start {
            if ch == '\t' {
                out.push_str("&#9;");
                i += 1;
                continue;
            }
            if ch == ' ' {
                let mut spaces = 0;
                while i + spaces < chars.len() && chars[i + spaces] == ' ' {
                    spaces += 1;
                }
                if spaces >= 4 {
                    out.push_str("&#32;   ");
                    i += 4;
                    continue;
                }

                // 1 <= spaces <= 3: check if followed by a block marker
                let next_idx = i + spaces;
                if next_idx < chars.len() {
                    let next_ch = chars[next_idx];
                    let is_marker = matches!(
                        next_ch,
                        '-' | '=' | '+' | '#' | '>' | '|' | '*' | '~' | '<' | '`'
                    ) || next_ch.is_ascii_digit();
                    if is_marker {
                        for _ in 0..spaces {
                            out.push(' ');
                        }
                        i = next_idx;
                        continue;
                    }
                }

                for _ in 0..spaces {
                    out.push(' ');
                }
                i += spaces;
                at_line_start = false;
                continue;
            }

            let is_block_marker = match ch {
                '-' | '=' | '+' | '#' | '>' | '|' | '*' | '~' | '<' | '`' => {
                    out.push('\\');
                    out.push(ch);
                    true
                }
                _ if ch.is_ascii_digit() => {
                    let mut j = i;
                    while j < chars.len() && chars[j].is_ascii_digit() {
                        j += 1;
                    }
                    if j < chars.len()
                        && (chars[j] == '.' || chars[j] == ')')
                        && (j + 1 == chars.len() || chars[j + 1].is_whitespace())
                    {
                        for &c in &chars[i..j] {
                            out.push(c);
                        }
                        out.push('\\');
                        out.push(chars[j]);
                        i = j + 1;
                        at_line_start = false;
                        continue;
                    }
                    false
                }
                _ => false,
            };

            if is_block_marker {
                i += 1;
                at_line_start = false;
                continue;
            }
        }

        match ch {
            '\\' => out.push_str("\\\\"),
            '*' => out.push_str("\\*"),
            '_' => out.push_str("\\_"),
            '~' => out.push_str("\\~"),
            '`' => out.push_str("\\`"),
            '[' => out.push_str("\\["),
            ']' => out.push_str("\\]"),
            '<' => out.push_str("\\<"),
            '>' => {
                if at_line_start {
                    out.push_str("\\>");
                } else {
                    out.push('>');
                }
            }
            '$' => out.push_str("\\$"),
            '^' => out.push_str("\\^"),
            '|' if options.in_table || at_line_start => out.push_str("\\|"),
            '!' => {
                if (i + 1 < chars.len() && chars[i + 1] == '[')
                    || (i + 1 == chars.len() && options.trailing_bang_before_link)
                {
                    out.push_str("\\!");
                } else {
                    out.push('!');
                }
            }
            '&' => {
                if i + 1 < chars.len()
                    && (chars[i + 1].is_ascii_alphabetic() || chars[i + 1] == '#')
                {
                    out.push_str("\\&");
                } else {
                    out.push('&');
                }
            }
            ':' => {
                let mut is_shortcode = false;
                let mut j = i + 1;
                while j < chars.len()
                    && (chars[j].is_ascii_alphanumeric() || chars[j] == '_' || chars[j] == '+')
                {
                    j += 1;
                }
                if j > i + 1 && j < chars.len() && chars[j] == ':' {
                    is_shortcode = true;
                }

                let is_url_colon = i + 2 < chars.len()
                    && chars[i + 1] == '/'
                    && chars[i + 2] == '/'
                    && ((i >= 4
                        && chars[i - 4..i] == ['h', 't', 't', 'p']
                        && (i == 4 || !chars[i - 5].is_ascii_alphanumeric()))
                        || (i >= 5
                            && chars[i - 5..i] == ['h', 't', 't', 'p', 's']
                            && (i == 5 || !chars[i - 6].is_ascii_alphanumeric()))
                        || (i >= 3
                            && chars[i - 3..i] == ['f', 't', 'p']
                            && (i == 3 || !chars[i - 4].is_ascii_alphanumeric())));

                if is_shortcode || is_url_colon {
                    out.push_str("\\:");
                } else {
                    out.push(':');
                }
            }
            '@' => {
                if i > 0
                    && chars[i - 1].is_ascii_alphanumeric()
                    && i + 1 < chars.len()
                    && chars[i + 1].is_ascii_alphanumeric()
                {
                    out.push_str("\\@");
                } else {
                    out.push('@');
                }
            }
            '.' => {
                if i >= 3
                    && chars[i - 3] == 'w'
                    && chars[i - 2] == 'w'
                    && chars[i - 1] == 'w'
                    && (i == 3 || !chars[i - 4].is_ascii_alphanumeric())
                {
                    out.push_str("\\.");
                } else {
                    out.push('.');
                }
            }
            '\n' => {
                at_line_start = true;
                out.push('\n');
                i += 1;
                continue;
            }
            '\r' => {
                i += 1;
                continue;
            }
            other => {
                out.push(other);
            }
        }
        at_line_start = false;
        i += 1;
    }
    out
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
