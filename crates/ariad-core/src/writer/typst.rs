//! Typst markup writer: turns document IR into a Typst source that compiles to PDF.
//!
//! The writer is pure string generation, so it stays I/O-free and WASM-safe; the host compiles
//! the markup with the `typst` crate. Every piece of IR text reaches the markup inside a Typst
//! string literal (`#"…"`, `raw("…")`, `link("…")`) whose escaping is complete, so document
//! content can never become Typst code. Structure is emitted only as function calls the writer
//! builds itself (`#heading(level: 2)[…]`, `#list(…)`, `#table(…)`), never as markup shorthand.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
};

use sha2::{Digest, Sha256};

use crate::{
    ir::{
        Alignment, AssetRef, AssetStore, Block, ColumnSpec, Document, Inline, ListItem, Metadata,
        RawFormat, TableCell,
    },
    links,
    warning::{Warning, WarningCode},
    writer::{normalize_singleline, strip_html_tags},
};

/// Typst stops at a show-rule nesting depth of 64 and a parser depth of 256. Every container the
/// writer opens adds its measured cost toward this budget (with headroom for headings, figures and
/// the preamble's show rules); content that would exceed it is flattened to plain paragraphs or
/// text instead of failing to compile.
const MAX_NESTING_COST: usize = 48;
const QUOTE_COST: usize = 4;
const LIST_COST: usize = 2;
const TABLE_COST: usize = 6;
const FOOTNOTE_COST: usize = 4;
const INLINE_COST: usize = 1;

/// Footnotes referenced from inside footnotes are expanded at most this deep.
const MAX_FOOTNOTE_NESTING: usize = 4;

/// Tables wider than this many columns keep their first columns; the cells that do not fit are
/// written as paragraphs below the table.
const MAX_TABLE_COLUMNS: usize = 128;

/// Upper bound for a cell's row span; the span is further clipped to its table section.
const MAX_ROW_SPAN: usize = 65_534;

/// Typst integers are `i64`, so larger list numbers cannot be written.
const MAX_LIST_START: u64 = i64::MAX as u64;

/// Box glyphs for task-list items; Source Code Pro carries both.
const TASK_CHECKED: &str = "\u{2611}";
const TASK_UNCHECKED: &str = "\u{2610}";

const PREAMBLE_STYLE: &str = r##"#set page(paper: "a4", margin: (x: 2.2cm, y: 2.5cm), numbering: "1")
#show raw: set text(font: "Source Code Pro")
#show raw.where(block: true): set block(fill: luma(246), inset: 8pt, radius: 2pt, width: 100%)
#show math.equation: set text(font: "Libertinus Math")
#show link: set text(fill: rgb("#1f4e8c"))
#show link: underline
#set table(stroke: 0.5pt + luma(160), inset: 5pt)
#show figure: set block(breakable: true)
#show figure.where(kind: table): set figure.caption(position: top)
"##;

/// Options the host passes to the writer.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TypstOptions {
    /// Title used when the document metadata has none.
    pub title_fallback: Option<String>,
    /// Document date as `(year, month, day)`, normally derived from `SOURCE_DATE_EPOCH` or the
    /// conversion time; `None` writes `date: none`.
    pub date: Option<(i32, u8, u8)>,
    /// Asset ids whose bytes the compiler cannot decode; their images fall back to alt text.
    pub rejected_assets: BTreeSet<String>,
}

/// The Typst source and the virtual files it references.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TypstOutput {
    /// The main Typst source.
    pub markup: String,
    /// Virtual files keyed by their path in the Typst world (for example `assets/<id>.png`).
    pub files: BTreeMap<String, Vec<u8>>,
    pub warnings: Vec<Warning>,
}

/// The file extension under which Typst 0.15 decodes an image of this media type, if it can.
#[must_use]
pub fn image_extension(media_type: &str) -> Option<&'static str> {
    let essence = media_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    match essence.as_str() {
        "image/png" => Some("png"),
        "image/jpeg" | "image/jpg" | "image/pjpeg" => Some("jpg"),
        "image/gif" => Some("gif"),
        "image/svg+xml" => Some("svg"),
        "image/webp" => Some("webp"),
        _ => None,
    }
}

/// Writes a document as Typst markup.
#[must_use]
pub fn write(document: &Document, options: &TypstOptions) -> TypstOutput {
    let document = super::normalize_document(document);
    let mut writer = Writer::new(&document, options);

    let mut markup = preamble(&document.meta, options);
    markup.push('\n');
    let mut body = String::new();
    writer.blocks(&document.body, &mut body);
    markup.push_str(body.trim_end());
    markup.push('\n');

    writer.report_unused_footnotes();
    TypstOutput {
        markup,
        files: writer.files,
        warnings: writer.warnings,
    }
}

fn preamble(meta: &Metadata, options: &TypstOptions) -> String {
    let mut out = String::from("#set document(");
    let title = meta.title.as_deref().and_then(metadata_text).or_else(|| {
        options
            .title_fallback
            .as_deref()
            .map(normalize_singleline)
            .as_deref()
            .and_then(metadata_text)
    });
    if let Some(title) = title {
        out.push_str("title: ");
        push_literal(&mut out, &title);
        out.push_str(", ");
    }
    push_string_array(&mut out, "author", &meta.authors);
    push_string_array(&mut out, "keywords", &meta.keywords);
    if let Some(subject) = meta.subject.as_deref().and_then(metadata_text) {
        out.push_str("description: ");
        push_literal(&mut out, &subject);
        out.push_str(", ");
    }
    match options
        .date
        .filter(|&(year, month, day)| is_valid_date(year, month, day))
    {
        Some((year, month, day)) => {
            let _ = write!(
                out,
                "date: datetime(year: {year}, month: {month}, day: {day})"
            );
        }
        None => out.push_str("date: none"),
    }
    out.push_str(")\n");

    out.push_str("#set text(font: \"Libertinus Serif\", size: 11pt");
    if let Some((lang, region)) = meta.language.as_deref().and_then(language_tag) {
        out.push_str(", lang: ");
        push_literal(&mut out, &lang);
        if let Some(region) = region {
            out.push_str(", region: ");
            push_literal(&mut out, &region);
        }
    }
    out.push_str(")\n");
    out.push_str(PREAMBLE_STYLE);
    out
}

/// A metadata string without control characters, or `None` when it is blank.
fn metadata_text(value: &str) -> Option<String> {
    let mut text = String::new();
    push_prose(&mut text, value);
    let trimmed = text.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

fn push_string_array(out: &mut String, name: &str, values: &[String]) {
    let values: Vec<String> = values
        .iter()
        .filter_map(|value| metadata_text(value))
        .collect();
    if values.is_empty() {
        return;
    }
    out.push_str(name);
    out.push_str(": (");
    for value in &values {
        push_literal(out, value);
        out.push_str(", ");
    }
    out.push_str("), ");
}

/// Splits a BCP 47-like tag into an ISO 639 language code and an optional ISO 3166 region.
fn language_tag(tag: &str) -> Option<(String, Option<String>)> {
    let mut parts = tag.trim().split(['-', '_']);
    let primary = parts.next()?;
    if !(2..=3).contains(&primary.len()) || !primary.chars().all(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    let region = parts
        .find(|part| part.len() == 2 && part.chars().all(|c| c.is_ascii_alphabetic()))
        .map(str::to_ascii_uppercase);
    Some((primary.to_ascii_lowercase(), region))
}

fn is_valid_date(year: i32, month: u8, day: u8) -> bool {
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    (1..=9999).contains(&year) && (1..=days).contains(&day)
}

/// Writes `text` as a Typst string literal; the escaping covers every character the lexer treats
/// specially inside strings and spells out invisible characters.
fn push_literal(out: &mut String, text: &str) {
    out.push('"');
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch.is_control() || is_invisible_format(ch) => {
                let _ = write!(out, "\\u{{{:x}}}", u32::from(ch));
            }
            ch => out.push(ch),
        }
    }
    out.push('"');
}

/// Line and paragraph separators, bidirectional controls and other zero-width format characters.
fn is_invisible_format(ch: char) -> bool {
    matches!(
        ch,
        '\u{00ad}'
            | '\u{061c}'
            | '\u{180e}'
            | '\u{200b}'..='\u{200f}'
            | '\u{2028}'..='\u{202e}'
            | '\u{2060}'..='\u{206f}'
            | '\u{feff}'
            | '\u{fff9}'..='\u{fffb}'
    )
}

/// Prose whitespace collapses to spaces and other control characters are dropped.
fn push_prose(buffer: &mut String, text: &str) {
    for ch in text.chars() {
        match ch {
            '\t' | '\n' | '\u{000b}' | '\u{000c}' | '\u{2028}' | '\u{2029}' => buffer.push(' '),
            ch if ch.is_control() => {}
            ch => buffer.push(ch),
        }
    }
}

/// Code keeps tabs and line breaks; other control characters are dropped.
fn clean_code(text: &str) -> String {
    text.chars()
        .filter_map(|ch| match ch {
            '\u{000b}' | '\u{000c}' | '\u{2028}' | '\u{2029}' => Some('\n'),
            '\t' | '\n' => Some(ch),
            ch if ch.is_control() => None,
            ch => Some(ch),
        })
        .collect()
}

/// Emits buffered prose as one `#"…"` text run.
fn flush_text(buffer: &mut String, out: &mut String) {
    if !buffer.is_empty() {
        out.push('#');
        push_literal(out, buffer);
        buffer.clear();
    }
}

fn push_prose_run(out: &mut String, text: &str) {
    let mut buffer = String::new();
    push_prose(&mut buffer, text);
    flush_text(&mut buffer, out);
}

/// Keeps `[A-Za-z0-9_+-]` of a code block language tag.
fn sanitize_lang(lang: &str) -> String {
    lang.chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '+' | '-'))
        .take(64)
        .collect()
}

/// A virtual file name for an asset id: lowercase hex ids (the IR's SHA-256 ids) are used as
/// is, anything else is hashed so the name stays a plain, collision-free path segment.
fn asset_file_stem(id: &str) -> String {
    if !id.is_empty()
        && id.len() <= 128
        && id.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    {
        id.to_owned()
    } else {
        format!("x{}", hex::encode(Sha256::digest(id.as_bytes())))
    }
}

/// Plain text of inline content, gathered without recursion.
fn plain_text(inlines: &[Inline]) -> String {
    let mut text = String::new();
    let mut stack: Vec<&Inline> = inlines.iter().rev().collect();
    while let Some(inline) = stack.pop() {
        match inline {
            Inline::Text { text: value } | Inline::Code { text: value } => text.push_str(value),
            Inline::Math { tex, .. } => text.push_str(tex),
            Inline::Emph { content }
            | Inline::Strong { content }
            | Inline::Strikeout { content }
            | Inline::Superscript { content }
            | Inline::Subscript { content }
            | Inline::Link { content, .. } => stack.extend(content.iter().rev()),
            Inline::Image { alt, .. } => text.push_str(alt),
            Inline::SoftBreak {} | Inline::LineBreak {} => text.push(' '),
            Inline::Raw {
                format: RawFormat::Html,
                text: html,
            } => text.push_str(&strip_html_tags(html)),
            Inline::FootnoteRef { .. }
            | Inline::Raw {
                format: RawFormat::Tex,
                ..
            } => {}
        }
    }
    text
}

/// Plain text of every leaf block, one entry per paragraph, gathered without recursion.
fn plain_paragraphs(blocks: &[Block]) -> Vec<String> {
    let mut paragraphs = Vec::new();
    let mut stack: Vec<&Block> = blocks.iter().rev().collect();
    while let Some(block) = stack.pop() {
        match block {
            Block::Heading { content, .. } | Block::Paragraph { content } => {
                paragraphs.push(plain_text(content));
            }
            Block::Code { text, .. } => paragraphs.push(text.clone()),
            Block::Math { tex, .. } => paragraphs.push(tex.clone()),
            Block::Quote { blocks } => stack.extend(blocks.iter().rev()),
            Block::List { items, .. } => {
                stack.extend(items.iter().rev().flat_map(|item| item.blocks.iter().rev()));
            }
            Block::Table {
                caption,
                head,
                body,
                footnotes,
                ..
            } => {
                let mut texts = Vec::new();
                if let Some(caption) = caption {
                    texts.push(plain_text(caption));
                }
                let cells = head.iter().chain(body).flatten();
                texts.extend(cells.map(|cell| plain_paragraphs(&cell.blocks).join(" ")));
                texts.push(plain_text(footnotes));
                paragraphs.push(texts.join(" "));
            }
            Block::Figure { caption, .. } => paragraphs.push(plain_text(caption)),
            Block::Raw {
                format: RawFormat::Html,
                text,
            } => paragraphs.push(strip_html_tags(text)),
            Block::PageBreak {}
            | Block::Footnote { .. }
            | Block::Raw {
                format: RawFormat::Tex,
                ..
            } => {}
        }
    }
    paragraphs
}

/// Footnote definitions anywhere in the body; the first definition of an id wins.
fn collect_footnotes(blocks: &[Block]) -> BTreeMap<&str, &[Block]> {
    let mut footnotes = BTreeMap::new();
    let mut stack: Vec<&Block> = blocks.iter().rev().collect();
    while let Some(block) = stack.pop() {
        match block {
            Block::Footnote { id, blocks } => {
                footnotes.entry(id.as_str()).or_insert(blocks.as_slice());
                stack.extend(blocks.iter().rev());
            }
            Block::Quote { blocks } => stack.extend(blocks.iter().rev()),
            Block::List { items, .. } => {
                stack.extend(items.iter().rev().flat_map(|item| item.blocks.iter().rev()));
            }
            Block::Table { head, body, .. } => {
                stack.extend(
                    head.iter()
                        .chain(body)
                        .flatten()
                        .flat_map(|cell| &cell.blocks),
                );
            }
            _ => {}
        }
    }
    footnotes
}

/// A table cell placed on the grid.
struct PlacedCell<'a> {
    cell: &'a TableCell,
    colspan: usize,
    rowspan: usize,
    header: bool,
}

/// One grid position of a table row.
enum Slot<'a> {
    /// A cell starts here.
    Cell(PlacedCell<'a>),
    /// A cell from this or an earlier row spans over this position.
    Covered,
    /// Nothing covers this position; an empty cell fills it.
    Empty,
}

/// A table normalized so that every grid row is completely filled, which keeps Typst's automatic
/// cell placement identical to the IR's row structure.
struct TableGrid<'a> {
    columns: usize,
    head: Vec<Vec<Slot<'a>>>,
    body: Vec<Vec<Slot<'a>>>,
    overflow: Vec<&'a TableCell>,
}

impl<'a> TableGrid<'a> {
    fn new(columns: &[ColumnSpec], head: &'a [Vec<TableCell>], body: &'a [Vec<TableCell>]) -> Self {
        // A span never reaches past the declared columns or the longest row, so one absurd
        // colspan cannot stretch the table to the column cap.
        let longest_row = head.iter().chain(body).map(Vec::len).max().unwrap_or(0);
        let max_colspan = columns.len().max(longest_row).clamp(1, MAX_TABLE_COLUMNS);
        let width = columns
            .len()
            .min(MAX_TABLE_COLUMNS)
            .max(section_width(head, max_colspan))
            .max(section_width(body, max_colspan));
        let mut overflow = Vec::new();
        let head = place_section(head, width, max_colspan, true, &mut overflow);
        let body = place_section(body, width, max_colspan, false, &mut overflow);
        Self {
            columns: width,
            head,
            body,
            overflow,
        }
    }
}

/// The number of columns a section needs, capped at [`MAX_TABLE_COLUMNS`].
fn section_width(rows: &[Vec<TableCell>], max_colspan: usize) -> usize {
    let mut occupied = vec![0_usize; MAX_TABLE_COLUMNS];
    let mut width = 0;
    for (row_index, row) in rows.iter().enumerate() {
        let remaining_rows = rows.len() - row_index;
        let mut x = 0;
        for cell in row {
            while x < MAX_TABLE_COLUMNS && occupied[x] > 0 {
                x += 1;
            }
            if x >= MAX_TABLE_COLUMNS {
                break;
            }
            let colspan = free_run(&occupied, x, MAX_TABLE_COLUMNS, cell.colspan, max_colspan);
            let rowspan = clamp_rowspan(cell.rowspan, remaining_rows);
            for slot in &mut occupied[x..x + colspan] {
                *slot = rowspan;
            }
            x += colspan;
        }
        let used = occupied
            .iter()
            .rposition(|&rows| rows > 0)
            .map_or(0, |i| i + 1);
        width = width.max(x).max(used);
        for slot in &mut occupied {
            *slot = slot.saturating_sub(1);
        }
    }
    width
}

fn place_section<'a>(
    rows: &'a [Vec<TableCell>],
    columns: usize,
    max_colspan: usize,
    header: bool,
    overflow: &mut Vec<&'a TableCell>,
) -> Vec<Vec<Slot<'a>>> {
    let mut occupied = vec![0_usize; columns];
    let mut grid = Vec::with_capacity(rows.len());
    for (row_index, row) in rows.iter().enumerate() {
        let remaining_rows = rows.len() - row_index;
        let mut slots: Vec<Slot<'a>> = occupied
            .iter()
            .map(|&rows| if rows > 0 { Slot::Covered } else { Slot::Empty })
            .collect();
        let mut x = 0;
        for cell in row {
            while x < columns && occupied[x] > 0 {
                x += 1;
            }
            if x >= columns {
                overflow.push(cell);
                continue;
            }
            let colspan = free_run(&occupied, x, columns, cell.colspan, max_colspan);
            let rowspan = clamp_rowspan(cell.rowspan, remaining_rows);
            for slot in &mut occupied[x..x + colspan] {
                *slot = rowspan;
            }
            slots[x] = Slot::Cell(PlacedCell {
                cell,
                colspan,
                rowspan,
                header: header || cell.header == Some(true),
            });
            for slot in &mut slots[x + 1..x + colspan] {
                *slot = Slot::Covered;
            }
            x += colspan;
        }
        grid.push(slots);
        for slot in &mut occupied {
            *slot = slot.saturating_sub(1);
        }
    }
    grid
}

/// The column span that fits into the free slots starting at `x`.
fn free_run(occupied: &[usize], x: usize, columns: usize, colspan: u32, max: usize) -> usize {
    let wanted = usize::try_from(colspan).unwrap_or(usize::MAX).min(max);
    let free = occupied[x..columns]
        .iter()
        .take_while(|&&rows| rows == 0)
        .count();
    wanted.min(free).max(1)
}

fn clamp_rowspan(rowspan: u32, remaining_rows: usize) -> usize {
    usize::try_from(rowspan)
        .unwrap_or(usize::MAX)
        .clamp(1, remaining_rows.clamp(1, MAX_ROW_SPAN))
}

struct Writer<'a> {
    assets: &'a AssetStore,
    rejected_assets: &'a BTreeSet<String>,
    footnotes: BTreeMap<&'a str, &'a [Block]>,
    /// Footnotes already written, with the number of their label.
    footnote_labels: BTreeMap<&'a str, usize>,
    referenced_footnotes: BTreeSet<&'a str>,
    footnote_stack: Vec<&'a str>,
    /// Footnote references met inside a link; they are written after the link.
    deferred_footnotes: Vec<&'a str>,
    files: BTreeMap<String, Vec<u8>>,
    warnings: Vec<Warning>,
    /// Nesting cost of the containers currently open.
    nesting_cost: usize,
    in_link: bool,
    warned_raw_html: bool,
    warned_raw_tex: bool,
    warned_math: bool,
    warned_nesting: bool,
    warned_table_overflow: bool,
    warned_missing_footnotes: BTreeSet<&'a str>,
}

impl<'a> Writer<'a> {
    fn new(document: &'a Document, options: &'a TypstOptions) -> Self {
        Self {
            assets: &document.assets,
            rejected_assets: &options.rejected_assets,
            footnotes: collect_footnotes(&document.body),
            footnote_labels: BTreeMap::new(),
            referenced_footnotes: BTreeSet::new(),
            footnote_stack: Vec::new(),
            deferred_footnotes: Vec::new(),
            files: BTreeMap::new(),
            warnings: Vec::new(),
            nesting_cost: 0,
            in_link: false,
            warned_raw_html: false,
            warned_raw_tex: false,
            warned_math: false,
            warned_nesting: false,
            warned_table_overflow: false,
            warned_missing_footnotes: BTreeSet::new(),
        }
    }

    fn warn(&mut self, code: WarningCode, message: impl Into<String>) {
        self.warnings.push(Warning::new(code, message));
    }

    fn warn_raw_html(&mut self) {
        if !self.warned_raw_html {
            self.warned_raw_html = true;
            self.warn(WarningCode::RawDropped, "raw HTML content was stripped");
        }
    }

    fn warn_raw_tex(&mut self) {
        if !self.warned_raw_tex {
            self.warned_raw_tex = true;
            self.warn(WarningCode::RawDropped, "raw TeX content was dropped");
        }
    }

    fn warn_math(&mut self) {
        if !self.warned_math {
            self.warned_math = true;
            self.warn(
                WarningCode::MathNotConverted,
                "TeX math was written as source text because it cannot be converted to Typst math",
            );
        }
    }

    fn report_unused_footnotes(&mut self) {
        let unused: Vec<&str> = self
            .footnotes
            .keys()
            .copied()
            .filter(|id| !self.referenced_footnotes.contains(id))
            .collect();
        for id in unused {
            self.warn(
                WarningCode::FootnoteUnused,
                format!("footnote `{id}` is never referenced and was dropped"),
            );
        }
    }

    /// Opens a container of the given cost when it fits the nesting budget; otherwise the caller
    /// flattens the container's content.
    fn enter(&mut self, cost: usize) -> bool {
        if self.nesting_cost + cost > MAX_NESTING_COST {
            if !self.warned_nesting {
                self.warned_nesting = true;
                self.warn(
                    WarningCode::UnsupportedNode,
                    "deeply nested content was flattened to plain text",
                );
            }
            false
        } else {
            self.nesting_cost += cost;
            true
        }
    }

    fn leave(&mut self, cost: usize) {
        self.nesting_cost -= cost;
    }

    // Blocks.

    fn blocks(&mut self, blocks: &'a [Block], out: &mut String) {
        for block in blocks {
            self.block(block, out);
        }
    }

    /// Renders blocks as the trimmed content of a container.
    fn content(&mut self, blocks: &'a [Block]) -> String {
        let mut out = String::new();
        self.blocks(blocks, &mut out);
        out.truncate(out.trim_end().len());
        out
    }

    /// Writes the text of blocks as plain paragraphs, without recursion.
    fn flatten(blocks: &[Block], out: &mut String) {
        for paragraph in plain_paragraphs(blocks) {
            let start = out.len();
            push_prose_run(out, &paragraph);
            if out.len() > start {
                out.push_str("\n\n");
            }
        }
    }

    fn flattened(blocks: &[Block]) -> String {
        let mut out = String::new();
        Self::flatten(blocks, &mut out);
        out.truncate(out.trim_end().len());
        out
    }

    fn block(&mut self, block: &'a Block, out: &mut String) {
        match block {
            Block::Heading { level, content } => {
                let _ = write!(out, "#heading(level: {})[", (*level).clamp(1, 6));
                self.inlines(content, out);
                out.push_str("]\n\n");
            }
            Block::Paragraph { content } => {
                let start = out.len();
                self.inlines(content, out);
                if out.len() > start {
                    out.push_str("\n\n");
                }
            }
            Block::Code { lang, text } => {
                out.push_str("#raw(block: true, ");
                if let Some(lang) = lang.as_deref().map(sanitize_lang)
                    && !lang.is_empty()
                {
                    out.push_str("lang: ");
                    push_literal(out, &lang);
                    out.push_str(", ");
                }
                push_literal(out, &clean_code(text));
                out.push_str(")\n\n");
            }
            Block::Math { tex, .. } => {
                self.warn_math();
                out.push_str("#align(center, raw(block: true, lang: \"latex\", ");
                push_literal(out, &clean_code(tex));
                out.push_str("))\n\n");
            }
            Block::Quote { blocks } => {
                if self.enter(QUOTE_COST) {
                    let inner = self.content(blocks);
                    self.leave(QUOTE_COST);
                    out.push_str("#quote(block: true)[");
                    out.push_str(&inner);
                    out.push_str("]\n\n");
                } else {
                    Self::flatten(blocks, out);
                }
            }
            Block::PageBreak {} => {
                // Typst rejects page breaks inside containers, where they carry no meaning anyway.
                if self.nesting_cost == 0 {
                    out.push_str("#pagebreak(weak: true)\n\n");
                }
            }
            Block::List {
                ordered,
                start,
                tight,
                items,
            } => {
                if items.is_empty() {
                    return;
                }
                if self.enter(LIST_COST) {
                    self.list(*ordered, *start, *tight, items, out);
                    self.leave(LIST_COST);
                } else {
                    Self::flatten(std::slice::from_ref(block), out);
                }
            }
            Block::Table {
                caption,
                columns,
                head,
                body,
                footnotes,
            } => {
                if self.enter(TABLE_COST) {
                    self.table(caption.as_deref(), columns, head, body, footnotes, out);
                    self.leave(TABLE_COST);
                } else {
                    Self::flatten(std::slice::from_ref(block), out);
                }
            }
            Block::Figure { asset, caption } => self.figure(asset, caption, out),
            // Footnote bodies are written where they are referenced.
            Block::Footnote { .. } => {}
            Block::Raw { format, text } => match format {
                RawFormat::Html => {
                    self.warn_raw_html();
                    let mut buffer = String::new();
                    push_prose(&mut buffer, &strip_html_tags(text));
                    if !buffer.trim().is_empty() {
                        flush_text(&mut buffer, out);
                        out.push_str("\n\n");
                    }
                }
                RawFormat::Tex => self.warn_raw_tex(),
            },
        }
    }

    /// Writes a list whose nesting cost the caller has already entered.
    fn list(
        &mut self,
        ordered: bool,
        start: Option<u64>,
        tight: bool,
        items: &'a [ListItem],
        out: &mut String,
    ) {
        if ordered {
            out.push_str("#enum(");
            if let Some(start) = start {
                let _ = write!(out, "start: {}, ", start.min(MAX_LIST_START));
            }
        } else {
            out.push_str("#list(");
        }
        let _ = write!(out, "tight: {tight}");
        for item in items {
            out.push_str(", [");
            let content = self.list_item(item);
            out.push_str(&content);
            out.push(']');
        }
        out.push_str(")\n\n");
    }

    fn list_item(&mut self, item: &'a ListItem) -> String {
        let Some(checked) = item.checked else {
            return self.content(&item.blocks);
        };
        let mut out = String::from("#text(font: \"Source Code Pro\", ");
        push_literal(
            &mut out,
            if checked {
                TASK_CHECKED
            } else {
                TASK_UNCHECKED
            },
        );
        out.push_str(")#\" \"");
        let rest = match item.blocks.split_first() {
            Some((Block::Paragraph { content }, rest)) => {
                self.inlines(content, &mut out);
                rest
            }
            _ => item.blocks.as_slice(),
        };
        let rest = self.content(rest);
        if !rest.is_empty() {
            out.push_str("\n\n");
            out.push_str(&rest);
        }
        out
    }

    /// Writes a table whose nesting cost the caller has already entered.
    fn table(
        &mut self,
        caption: Option<&'a [Inline]>,
        columns: &[ColumnSpec],
        head: &'a [Vec<TableCell>],
        body: &'a [Vec<TableCell>],
        footnotes: &'a [Inline],
        out: &mut String,
    ) {
        let grid = TableGrid::new(columns, head, body);
        let has_rows = !grid.head.is_empty() || !grid.body.is_empty();
        let caption = caption.filter(|caption| !caption.is_empty());

        if grid.columns > 0 && has_rows {
            let mut table = format!("table(columns: {}", grid.columns);
            if columns
                .iter()
                .any(|column| column.align != Alignment::Default)
            {
                table.push_str(", align: (");
                for index in 0..grid.columns {
                    let align = columns.get(index).map_or(Alignment::Default, |c| c.align);
                    table.push_str(match align {
                        Alignment::Default => "auto, ",
                        Alignment::Left => "left, ",
                        Alignment::Center => "center, ",
                        Alignment::Right => "right, ",
                    });
                }
                table.push(')');
            }
            if !grid.head.is_empty() {
                let cells: Vec<String> = grid
                    .head
                    .iter()
                    .flat_map(|row| self.table_row(row))
                    .collect();
                table.push_str(", table.header(");
                table.push_str(&cells.join(", "));
                table.push(')');
            }
            for row in &grid.body {
                for cell in self.table_row(row) {
                    table.push_str(", ");
                    table.push_str(&cell);
                }
            }
            table.push(')');

            if let Some(caption) = caption {
                out.push_str("#figure(");
                out.push_str(&table);
                out.push_str(", caption: [");
                self.inlines(caption, out);
                out.push_str("])\n\n");
            } else {
                out.push('#');
                out.push_str(&table);
                out.push_str("\n\n");
            }
        } else if let Some(caption) = caption {
            self.inlines(caption, out);
            out.push_str("\n\n");
        }

        if !grid.overflow.is_empty() {
            if !self.warned_table_overflow {
                self.warned_table_overflow = true;
                self.warn(
                    WarningCode::UnsupportedNode,
                    format!(
                        "table cells beyond {MAX_TABLE_COLUMNS} columns were written below the table"
                    ),
                );
            }
            for cell in grid.overflow {
                let content = self.content(&cell.blocks);
                if !content.is_empty() {
                    out.push_str(&content);
                    out.push_str("\n\n");
                }
            }
        }

        if !footnotes.is_empty() {
            out.push_str("#text(size: 0.85em)[");
            self.inlines(footnotes, out);
            out.push_str("]\n\n");
        }
    }

    /// The cell expressions of one grid row; covered positions produce nothing.
    fn table_row(&mut self, row: &[Slot<'a>]) -> Vec<String> {
        let mut cells = Vec::with_capacity(row.len());
        for slot in row {
            match slot {
                Slot::Cell(placed) => {
                    let content = self.content(&placed.cell.blocks);
                    let mut cell = String::new();
                    if placed.colspan > 1 || placed.rowspan > 1 {
                        let _ = write!(
                            cell,
                            "table.cell(colspan: {}, rowspan: {})",
                            placed.colspan, placed.rowspan
                        );
                    }
                    cell.push('[');
                    if placed.header && !content.is_empty() {
                        cell.push_str("#strong[");
                        cell.push_str(&content);
                        cell.push(']');
                    } else {
                        cell.push_str(&content);
                    }
                    cell.push(']');
                    cells.push(cell);
                }
                Slot::Covered => {}
                Slot::Empty => cells.push("[]".to_owned()),
            }
        }
        cells
    }

    fn figure(&mut self, asset: &AssetRef, caption: &'a [Inline], out: &mut String) {
        match self.embed_image(asset) {
            Some(path) => {
                out.push_str("#figure(image(");
                push_literal(out, &path);
                push_alt(out, &plain_text(caption));
                out.push(')');
                if !caption.is_empty() {
                    out.push_str(", caption: [");
                    self.inlines(caption, out);
                    out.push(']');
                }
                out.push_str(")\n\n");
            }
            None => {
                let start = out.len();
                self.inlines(caption, out);
                if out.len() > start {
                    out.push_str("\n\n");
                }
            }
        }
    }

    /// Registers an embeddable image as a virtual file and returns its path, or warns.
    fn embed_image(&mut self, target: &AssetRef) -> Option<String> {
        let id = match target {
            AssetRef::Url { href } => {
                self.warn(
                    WarningCode::ImageNotEmbedded,
                    format!("image URL `{href}` was reduced to its alt text"),
                );
                return None;
            }
            AssetRef::Asset { id } => id,
        };
        let Some(asset) = self.assets.get(id) else {
            self.warn(
                WarningCode::ImageNotEmbedded,
                format!("asset `{id}` was not found; image was reduced to its alt text"),
            );
            return None;
        };
        let Some(extension) = image_extension(&asset.media_type) else {
            self.warn(
                WarningCode::ImageNotEmbedded,
                format!(
                    "asset `{id}` has media type `{}`, which PDF output cannot embed; image was reduced to its alt text",
                    asset.media_type
                ),
            );
            return None;
        };
        if self.rejected_assets.contains(id) {
            self.warn(
                WarningCode::ImageNotEmbedded,
                format!("asset `{id}` could not be decoded; image was reduced to its alt text"),
            );
            return None;
        }
        let path = format!("assets/{}.{extension}", asset_file_stem(id));
        self.files
            .entry(path.clone())
            .or_insert_with(|| asset.bytes.clone());
        Some(path)
    }

    // Inlines.

    fn inlines(&mut self, inlines: &'a [Inline], out: &mut String) {
        let mut buffer = String::new();
        for inline in inlines {
            match inline {
                Inline::Text { text } => push_prose(&mut buffer, text),
                Inline::SoftBreak {} => buffer.push(' '),
                other => {
                    flush_text(&mut buffer, out);
                    self.inline(other, out);
                }
            }
        }
        flush_text(&mut buffer, out);
    }

    /// Writes `#function[…]` around inline children, or only their text past the budget.
    fn styled(&mut self, function: &str, content: &'a [Inline], out: &mut String) {
        if self.enter(INLINE_COST) {
            out.push('#');
            out.push_str(function);
            out.push('[');
            self.inlines(content, out);
            out.push(']');
            self.leave(INLINE_COST);
        } else {
            push_prose_run(out, &plain_text(content));
        }
    }

    fn inline(&mut self, inline: &'a Inline, out: &mut String) {
        match inline {
            Inline::Text { text } => push_prose_run(out, text),
            Inline::SoftBreak {} => out.push_str("#\" \""),
            Inline::LineBreak {} => out.push_str("#linebreak()"),
            Inline::Emph { content } => self.styled("emph", content, out),
            Inline::Strong { content } => self.styled("strong", content, out),
            Inline::Strikeout { content } => self.styled("strike", content, out),
            Inline::Superscript { content } => self.styled("super", content, out),
            Inline::Subscript { content } => self.styled("sub", content, out),
            Inline::Code { text } => {
                out.push_str("#raw(");
                push_literal(out, &clean_code(text));
                out.push(')');
            }
            Inline::Math { tex, .. } => {
                self.warn_math();
                out.push_str("#raw(lang: \"latex\", ");
                push_literal(out, &clean_code(tex));
                out.push(')');
            }
            Inline::Link { url, content, .. } => {
                if self.enter(INLINE_COST) {
                    self.link(url, content, out);
                    self.leave(INLINE_COST);
                } else {
                    push_prose_run(out, &plain_text(content));
                }
            }
            Inline::Image { target, alt, .. } => match self.embed_image(target) {
                Some(path) => {
                    out.push_str("#box(image(");
                    push_literal(out, &path);
                    push_alt(out, alt);
                    out.push_str("))");
                }
                None => push_prose_run(out, alt),
            },
            Inline::FootnoteRef { id } => {
                if self.in_link {
                    self.deferred_footnotes.push(id);
                } else {
                    self.footnote_ref(id, out);
                }
            }
            Inline::Raw { format, text } => match format {
                RawFormat::Html => {
                    self.warn_raw_html();
                    push_prose_run(out, &strip_html_tags(text));
                }
                RawFormat::Tex => self.warn_raw_tex(),
            },
        }
    }

    /// Writes a link whose nesting cost the caller has already entered.
    fn link(&mut self, url: &str, content: &'a [Inline], out: &mut String) {
        if !links::allowed_link(url) {
            self.warn(
                WarningCode::LinkDropped,
                format!("link target `{url}` was reduced to its text"),
            );
            self.inlines(content, out);
            return;
        }
        if url.starts_with('#') {
            // The IR has no anchors, so an in-document target has nothing to point at.
            self.warn(
                WarningCode::LinkDropped,
                format!("in-document link `{url}` was reduced to its text"),
            );
            self.inlines(content, out);
            return;
        }
        if self.in_link {
            // A link inside a link keeps only its text.
            self.inlines(content, out);
            return;
        }
        self.in_link = true;
        out.push_str("#link(");
        push_literal(out, url);
        out.push(')');
        if !content.is_empty() {
            out.push('[');
            self.inlines(content, out);
            out.push(']');
        }
        self.in_link = false;
        // Footnote markers are links themselves, so they follow the link instead of nesting.
        for id in std::mem::take(&mut self.deferred_footnotes) {
            self.footnote_ref(id, out);
        }
    }

    fn footnote_ref(&mut self, id: &'a str, out: &mut String) {
        self.referenced_footnotes.insert(id);
        if let Some(label) = self.footnote_labels.get(id) {
            let _ = write!(out, "#footnote(<ariad-fn-{label}>)");
            return;
        }
        let Some(blocks) = self.footnotes.get(id).copied() else {
            if self.warned_missing_footnotes.insert(id) {
                self.warn(
                    WarningCode::FootnoteMissing,
                    format!("footnote `{id}` is referenced but never defined"),
                );
            }
            return;
        };
        if self.footnote_stack.contains(&id) || self.footnote_stack.len() >= MAX_FOOTNOTE_NESTING {
            self.warn(
                WarningCode::UnsupportedNode,
                format!("footnote `{id}` refers to itself or nests too deeply and was dropped"),
            );
            return;
        }
        self.footnote_stack.push(id);
        let body = if self.enter(FOOTNOTE_COST) {
            let body = self.content(blocks);
            self.leave(FOOTNOTE_COST);
            body
        } else {
            Self::flattened(blocks)
        };
        self.footnote_stack.pop();
        let label = self.footnote_labels.len() + 1;
        self.footnote_labels.insert(id, label);
        out.push_str("#footnote[");
        out.push_str(&body);
        let _ = write!(out, "]<ariad-fn-{label}>");
    }
}

/// Writes `, alt: "…"` for non-blank alt text.
fn push_alt(out: &mut String, alt: &str) {
    let mut buffer = String::new();
    push_prose(&mut buffer, alt);
    let alt = buffer.trim();
    if !alt.is_empty() {
        out.push_str(", alt: ");
        push_literal(out, alt);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        asset_file_stem, image_extension, is_valid_date, language_tag, push_literal, sanitize_lang,
    };

    fn literal(text: &str) -> String {
        let mut out = String::new();
        push_literal(&mut out, text);
        out
    }

    #[test]
    fn string_literals_escape_quotes_backslashes_and_invisible_characters() {
        assert_eq!(literal(r#"a"b\c"#), r#""a\"b\\c""#);
        assert_eq!(literal("x\ny\tz\r"), r#""x\ny\tz\r""#);
        assert_eq!(
            literal("\u{0}\u{2028}\u{202e}"),
            r#""\u{0}\u{2028}\u{202e}""#
        );
        assert_eq!(literal("Việt #$*_"), "\"Việt #$*_\"");
    }

    #[test]
    fn language_tags_keep_iso_codes_only() {
        assert_eq!(
            language_tag("vi-VN"),
            Some(("vi".to_owned(), Some("VN".to_owned())))
        );
        assert_eq!(
            language_tag("EN_gb"),
            Some(("en".to_owned(), Some("GB".to_owned())))
        );
        assert_eq!(
            language_tag("zh-Hant-TW"),
            Some(("zh".to_owned(), Some("TW".to_owned())))
        );
        assert_eq!(language_tag("fil"), Some(("fil".to_owned(), None)));
        assert_eq!(language_tag("english"), None);
        assert_eq!(language_tag("v1"), None);
        assert_eq!(language_tag(""), None);
    }

    #[test]
    fn dates_are_checked_against_the_calendar() {
        assert!(is_valid_date(2024, 2, 29));
        assert!(!is_valid_date(2023, 2, 29));
        assert!(!is_valid_date(1900, 2, 29));
        assert!(is_valid_date(2000, 2, 29));
        assert!(!is_valid_date(2024, 13, 1));
        assert!(!is_valid_date(2024, 4, 31));
        assert!(!is_valid_date(0, 1, 1));
        assert!(!is_valid_date(10_000, 1, 1));
    }

    #[test]
    fn media_types_map_to_typst_image_extensions() {
        assert_eq!(image_extension("image/png"), Some("png"));
        assert_eq!(image_extension("IMAGE/JPEG; q=1"), Some("jpg"));
        assert_eq!(image_extension("image/svg+xml"), Some("svg"));
        assert_eq!(image_extension("image/webp"), Some("webp"));
        assert_eq!(image_extension("image/gif"), Some("gif"));
        assert_eq!(image_extension("image/tiff"), None);
        assert_eq!(image_extension("application/pdf"), None);
    }

    #[test]
    fn asset_names_and_languages_are_plain_path_segments() {
        assert_eq!(asset_file_stem("00ff"), "00ff");
        let hashed = asset_file_stem("../../etc/passwd");
        assert!(hashed.starts_with('x'));
        assert!(hashed[1..].bytes().all(|b| b.is_ascii_hexdigit()));
        assert_eq!(sanitize_lang("rust\"); #eval("), "rusteval");
        assert_eq!(sanitize_lang("c++"), "c++");
    }
}
