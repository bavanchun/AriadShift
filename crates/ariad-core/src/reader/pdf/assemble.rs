//! Document-level assembly: running headers and footers, headings, lists, and the IR blocks with
//! their layout positions.

use std::collections::{BTreeMap, HashMap, HashSet};

use super::{
    ReadError,
    layout::{self, Script, Span},
    lists,
};
use crate::{
    ir::{Block, BlockPosition, BoundingBox, Inline, ListItem},
    limits::Limits,
    reader::common::nfc,
};

/// Share of the page height, from the top and from the bottom, where running headers, footers
/// and page numbers are looked for.
const FURNITURE_ZONE: f64 = 0.12;

/// Font size relative to the body text from which a short block reads as a heading.
const HEADING_RATIO: f64 = 1.15;

/// The laid-out blocks of one page.
pub(super) struct PageBlocks {
    pub(super) page_no: u32,
    pub(super) height: f64,
    pub(super) blocks: Vec<layout::Block>,
}

/// Body and furniture blocks with their positions.
#[derive(Default)]
pub(super) struct Assembled {
    pub(super) body: Vec<Block>,
    pub(super) furniture: Vec<Block>,
    pub(super) positions: BTreeMap<String, BlockPosition>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum Zone {
    Top,
    Bottom,
}

enum Kind {
    Paragraph,
    Heading,
    Item(lists::Marker),
}

/// A list being collected from consecutive item blocks.
struct OpenList {
    ordered: bool,
    start: Option<u64>,
    /// The number the next item of a digit-labelled list must carry to continue it.
    next_number: Option<u64>,
    left: f64,
    size: f64,
    items: Vec<ListItem>,
    page_no: u32,
    bbox: [f64; 4],
}

/// A paragraph that may still continue in the next column or on the next page.
struct OpenParagraph {
    spans: Vec<Span>,
    page_no: u32,
    bbox: [f64; 4],
    size: f64,
    chars: usize,
}

/// Collects body blocks in reading order.
struct Emitter<'a> {
    out: Assembled,
    list: Option<OpenList>,
    paragraph: Option<OpenParagraph>,
    deferred: Vec<(Block, u32, [f64; 4])>,
    limits: &'a Limits,
    vocabulary: &'a HashSet<String>,
}

impl Emitter<'_> {
    fn push_body(&mut self, block: Block, page_no: u32, bbox: [f64; 4]) {
        if block_is_empty(&block) {
            return;
        }
        let path = format!("body/{}", self.out.body.len());
        self.out.positions.insert(path, position(page_no, bbox));
        self.out.body.push(block);
    }

    fn flush_paragraph(&mut self) -> Result<(), ReadError> {
        if let Some(paragraph) = self.paragraph.take() {
            let content = inlines(&paragraph.spans, false, self.limits, 0)?;
            self.push_body(
                Block::Paragraph { content },
                paragraph.page_no,
                paragraph.bbox,
            );
        }
        for (block, page_no, bbox) in std::mem::take(&mut self.deferred) {
            self.push_body(block, page_no, bbox);
        }
        Ok(())
    }

    fn flush(&mut self) -> Result<(), ReadError> {
        self.flush_paragraph()?;
        if let Some(list) = self.list.take() {
            self.push_body(
                Block::List {
                    ordered: list.ordered,
                    start: list.start,
                    tight: true,
                    items: list.items,
                },
                list.page_no,
                list.bbox,
            );
        }
        Ok(())
    }

    fn heading(
        &mut self,
        spans: &[Span],
        level: u8,
        page_no: u32,
        bbox: [f64; 4],
    ) -> Result<(), ReadError> {
        self.flush()?;
        let content = inlines(spans, true, self.limits, 0)?;
        self.push_body(Block::Heading { level, content }, page_no, bbox);
        Ok(())
    }

    fn paragraph(
        &mut self,
        spans: Vec<Span>,
        block: &layout::Block,
        page_no: u32,
    ) -> Result<(), ReadError> {
        if !block.upright && self.paragraph.is_some() {
            // Rotated margin text (such as an arXiv identifier) waits until the paragraph it
            // interrupts is complete.
            let content = inlines(&spans, false, self.limits, 0)?;
            self.deferred
                .push((Block::Paragraph { content }, page_no, block.bbox));
            return Ok(());
        }
        if let Some(open) = &mut self.paragraph
            && continues_paragraph(open, &spans, block, page_no)
        {
            append_line(&mut open.spans, &spans, self.vocabulary);
            if open.page_no == page_no {
                open.bbox = union(open.bbox, block.bbox);
            }
            open.chars += block.chars;
            return Ok(());
        }
        self.flush()?;
        self.paragraph = Some(OpenParagraph {
            spans,
            page_no,
            bbox: block.bbox,
            size: block.size,
            chars: block.chars,
        });
        Ok(())
    }

    fn item(
        &mut self,
        spans: Vec<Span>,
        marker: lists::Marker,
        block: &layout::Block,
        page_no: u32,
    ) -> Result<(), ReadError> {
        self.flush_paragraph()?;
        let content = inlines(&strip_marker(spans, marker.len), false, self.limits, 1)?;
        let item = ListItem {
            checked: None,
            blocks: vec![Block::Paragraph { content }],
        };
        let next_number = marker
            .number
            .filter(|_| marker.arabic)
            .and_then(|number| number.checked_add(1));
        if let Some(list) = &mut self.list
            && list.ordered == marker.ordered
            && (list.left - block.bbox[0]).abs() <= 2.0 * list.size.max(block.size)
            // A restart or a gap in the numbers starts another list that keeps its own numbers.
            && !(marker.arabic && list.next_number.is_some_and(|next| marker.number != Some(next)))
        {
            if list.page_no == page_no {
                list.bbox = union(list.bbox, block.bbox);
            }
            list.next_number = next_number;
            list.items.push(item);
            return Ok(());
        }
        self.flush()?;
        self.list = Some(OpenList {
            ordered: marker.ordered,
            start: if marker.ordered { marker.number } else { None },
            next_number,
            left: block.bbox[0],
            size: block.size,
            items: vec![item],
            page_no,
            bbox: block.bbox,
        });
        Ok(())
    }
}

/// Whether a paragraph interrupted by a column or page break continues in `block`: the open
/// paragraph stops mid-sentence and the block starts in lowercase.
fn continues_paragraph(
    open: &OpenParagraph,
    spans: &[Span],
    block: &layout::Block,
    page_no: u32,
) -> bool {
    let tail = open
        .spans
        .iter()
        .rev()
        .find_map(|span| span.text.trim_end().chars().last());
    let head = spans
        .iter()
        .find_map(|span| span.text.trim_start().chars().next());
    let (Some(tail), Some(head)) = (tail, head) else {
        return false;
    };
    // A column or page break: the block starts on a later page, higher up (the next column),
    // or below the paragraph (after a figure); never beside it.
    let broken = page_no > open.page_no
        || block.bbox[1] < open.bbox[1] - 1.0
        || block.bbox[1] > open.bbox[3];
    open.chars >= 30
        && broken
        && block.upright
        && page_no <= open.page_no + 1
        && (open.size - block.size).abs() <= 0.15 * open.size.max(block.size)
        && !matches!(tail, '.' | '!' | '?' | ':' | ';' | '"' | '”' | '»' | '…')
        && (head.is_lowercase() || (tail == '-' && head.is_alphanumeric()))
}

pub(super) fn assemble(pages: &[PageBlocks], limits: &Limits) -> Result<Assembled, ReadError> {
    let furniture = find_furniture(pages);
    let body_size = body_size(pages, &furniture);
    let vocabulary = vocabulary(pages, &furniture);
    let mut kinds: HashMap<(usize, usize), Kind> = HashMap::new();
    let mut heading_sizes: Vec<f64> = Vec::new();
    for (page_index, page) in pages.iter().enumerate() {
        for (block_index, block) in page.blocks.iter().enumerate() {
            if furniture.contains(&(page_index, block_index)) {
                continue;
            }
            let kind = classify(block, body_size);
            if let Kind::Heading = kind {
                heading_sizes.push(round_size(block.size));
            }
            kinds.insert((page_index, block_index), kind);
        }
    }
    heading_sizes.sort_by(|a, b| b.total_cmp(a));
    heading_sizes.dedup();

    let mut emitter = Emitter {
        out: Assembled::default(),
        list: None,
        paragraph: None,
        deferred: Vec::new(),
        limits,
        vocabulary: &vocabulary,
    };
    for (page_index, page) in pages.iter().enumerate() {
        for (block_index, block) in page.blocks.iter().enumerate() {
            let spans = join_lines(&block.lines, &vocabulary);
            let Some(kind) = kinds.get(&(page_index, block_index)) else {
                let content = inlines(&spans, false, limits, 0)?;
                if !content.is_empty() {
                    let out = &mut emitter.out;
                    let path = format!("furniture/{}", out.furniture.len());
                    out.positions
                        .insert(path, position(page.page_no, block.bbox));
                    out.furniture.push(Block::Paragraph { content });
                }
                continue;
            };
            match kind {
                Kind::Item(marker) => emitter.item(spans, *marker, block, page.page_no)?,
                Kind::Heading => {
                    let rank = heading_sizes
                        .iter()
                        .position(|&size| size <= round_size(block.size))
                        .unwrap_or(0);
                    let level = u8::try_from(rank + 1).unwrap_or(3).min(3);
                    emitter.heading(&spans, level, page.page_no, block.bbox)?;
                }
                Kind::Paragraph => emitter.paragraph(spans, block, page.page_no)?,
            }
        }
    }
    emitter.flush()?;
    Ok(emitter.out)
}

fn block_is_empty(block: &Block) -> bool {
    match block {
        Block::Heading { content, .. } | Block::Paragraph { content } => content.is_empty(),
        _ => false,
    }
}

fn position(page_no: u32, bbox: [f64; 4]) -> BlockPosition {
    BlockPosition {
        page_no,
        bbox: BoundingBox {
            left: bbox[0],
            top: bbox[1],
            right: bbox[2],
            bottom: bbox[3],
        },
    }
}

fn union(a: [f64; 4], b: [f64; 4]) -> [f64; 4] {
    [
        a[0].min(b[0]),
        a[1].min(b[1]),
        a[2].max(b[2]),
        a[3].max(b[3]),
    ]
}

fn round_size(size: f64) -> f64 {
    (size * 2.0).round() / 2.0
}

fn plain_text(lines: &[Vec<Span>]) -> String {
    let mut text = String::new();
    for line in lines {
        if !text.is_empty() {
            text.push(' ');
        }
        for span in line {
            text.push_str(&span.text);
        }
    }
    text
}

/// Finds running headers, running footers and page numbers: short blocks near the top or
/// bottom edge whose text, ignoring digits, repeats at a similar height on several pages.
fn find_furniture(pages: &[PageBlocks]) -> HashSet<(usize, usize)> {
    struct Candidate {
        key: (usize, usize),
        centre: f64,
        tolerance: f64,
        page: usize,
    }
    let mut furniture = HashSet::new();
    let mut groups: HashMap<(Zone, String), Vec<Candidate>> = HashMap::new();
    let text_pages = pages.iter().filter(|page| !page.blocks.is_empty()).count();
    for (page_index, page) in pages.iter().enumerate() {
        for (block_index, block) in page.blocks.iter().enumerate() {
            if block.lines.len() > 2 || block.chars > 200 || page.height <= 0.0 {
                continue;
            }
            let zone = if block.bbox[3] <= FURNITURE_ZONE * page.height {
                Zone::Top
            } else if block.bbox[1] >= (1.0 - FURNITURE_ZONE) * page.height {
                Zone::Bottom
            } else {
                continue;
            };
            let text = plain_text(&block.lines);
            if is_page_number(&text) {
                furniture.insert((page_index, block_index));
                continue;
            }
            let key = normalize_running_text(&text);
            if key.chars().filter(|ch| ch.is_alphabetic()).count() < 2 {
                continue;
            }
            groups.entry((zone, key)).or_default().push(Candidate {
                key: (page_index, block_index),
                centre: (block.bbox[1] + block.bbox[3]) / 2.0,
                tolerance: (0.02 * page.height).max(2.0 * block.size),
                page: page_index,
            });
        }
    }
    let needed = 2.max(text_pages.div_ceil(5));
    for candidates in groups.values() {
        let mut centres: Vec<f64> = candidates.iter().map(|c| c.centre).collect();
        centres.sort_by(f64::total_cmp);
        let middle = centres[centres.len() / 2];
        let matching: Vec<&Candidate> = candidates
            .iter()
            .filter(|candidate| (candidate.centre - middle).abs() <= candidate.tolerance)
            .collect();
        let mut distinct: Vec<usize> = matching.iter().map(|candidate| candidate.page).collect();
        distinct.dedup();
        if distinct.len() >= needed {
            for candidate in matching {
                furniture.insert(candidate.key);
            }
        }
    }
    furniture
}

fn normalize_running_text(text: &str) -> String {
    let mut out = String::new();
    let mut in_digits = false;
    for ch in text.chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_digit() {
            if !in_digits {
                out.push('#');
            }
            in_digits = true;
            continue;
        }
        in_digits = false;
        if ch.is_whitespace() {
            if !out.ends_with(' ') {
                out.push(' ');
            }
        } else {
            out.push(ch);
        }
    }
    out.trim().to_owned()
}

/// Whether text is a page number such as `12`, `- 12 -`, `Page 3 of 9`, `Trang 2` or `iv`.
fn is_page_number(text: &str) -> bool {
    let cleaned: String = text
        .chars()
        .map(|ch| {
            if "-–—()[]|·".contains(ch) {
                ' '
            } else {
                ch
            }
        })
        .collect::<String>()
        .to_lowercase();
    let words: Vec<&str> = cleaned.split_whitespace().collect();
    let number = |word: &str| {
        (1..=4).contains(&word.len()) && word.bytes().all(|byte| byte.is_ascii_digit())
            || roman_page(word)
    };
    let label = |word: &str| matches!(word, "page" | "trang" | "p." | "pg." | "seite");
    let of = |word: &str| matches!(word, "of" | "/" | "trên" | "von");
    match words.as_slice() {
        [single] => {
            number(single)
                || single
                    .split_once('/')
                    .is_some_and(|(page, total)| number(page) && number(total))
        }
        [first, second] => label(first) && number(second),
        [first, middle, last] => number(first) && of(middle) && number(last),
        [first, second, third, fourth] => {
            label(first) && number(second) && of(third) && number(fourth)
        }
        _ => false,
    }
}

/// Whether a word is a canonical lowercase roman numeral up to 50, as used for front matter.
fn roman_page(word: &str) -> bool {
    word.len() <= 7 && (1..=50).any(|value| canonical_roman(value) == word)
}

fn canonical_roman(mut value: u32) -> String {
    let mut out = String::new();
    for (amount, numeral) in [
        (50, "l"),
        (40, "xl"),
        (10, "x"),
        (9, "ix"),
        (5, "v"),
        (4, "iv"),
        (1, "i"),
    ] {
        while value >= amount {
            out.push_str(numeral);
            value -= amount;
        }
    }
    out
}

/// The dominant font size of body text, weighted by character count.
fn body_size(pages: &[PageBlocks], furniture: &HashSet<(usize, usize)>) -> f64 {
    let mut counts: HashMap<i64, usize> = HashMap::new();
    for (page_index, page) in pages.iter().enumerate() {
        for (block_index, block) in page.blocks.iter().enumerate() {
            if block.upright && !furniture.contains(&(page_index, block_index)) {
                *counts.entry((block.size * 2.0).round() as i64).or_default() += block.chars;
            }
        }
    }
    counts
        .into_iter()
        .max_by_key(|&(size, count)| (count, -size))
        .map_or(0.0, |(size, _)| size as f64 / 2.0)
}

fn classify(block: &layout::Block, body_size: f64) -> Kind {
    if !block.upright {
        return Kind::Paragraph;
    }
    let text = plain_text(&block.lines);
    let trimmed = text.trim();
    let letters = trimmed.chars().filter(|ch| ch.is_alphabetic()).count();
    let short = letters >= 2 && block.chars <= 200 && block.lines.len() <= 3 && body_size > 0.0;
    let ratio = if body_size > 0.0 {
        block.size / body_size
    } else {
        1.0
    };
    // Headings start with a capital or a digit and never end inside a hyphenated word.
    let plausible = short
        && !trimmed.ends_with(['-', ','])
        && trimmed
            .chars()
            .find(|ch| ch.is_alphanumeric())
            .is_some_and(|ch| !ch.is_lowercase());
    if plausible && ratio >= HEADING_RATIO {
        return Kind::Heading;
    }
    if let Some(marker) = lists::parse(&text) {
        return Kind::Item(marker);
    }
    if !plausible {
        return Kind::Paragraph;
    }
    let terminal = trimmed.ends_with(['.', ',', ';']);
    if block.bold && ratio >= 0.95 && block.lines.len() <= 2 && block.chars <= 120 && !terminal {
        return Kind::Heading;
    }
    Kind::Paragraph
}

/// Words that occur inside lines anywhere in the body, lowercased, to decide whether a
/// line-final hyphen belongs to the word or only marks hyphenation.
fn vocabulary(pages: &[PageBlocks], furniture: &HashSet<(usize, usize)>) -> HashSet<String> {
    let mut words = HashSet::new();
    for (page_index, page) in pages.iter().enumerate() {
        for (block_index, block) in page.blocks.iter().enumerate() {
            if furniture.contains(&(page_index, block_index)) {
                continue;
            }
            for line in &block.lines {
                let text: String = line.iter().map(|span| span.text.as_str()).collect();
                let mut parts = text.split_whitespace().peekable();
                while let Some(word) = parts.next() {
                    if parts.peek().is_none() && word.ends_with(HYPHENS) {
                        break;
                    }
                    let core = word.trim_matches(|ch: char| !ch.is_alphanumeric());
                    if core.chars().count() >= 2 {
                        words.insert(core.to_lowercase());
                    }
                }
            }
        }
    }
    words
}

const HYPHENS: [char; 3] = ['-', '\u{00AD}', '\u{2010}'];

/// Longest word read to the left of a line-end hyphen. Longer runs keep their hyphens.
const MAX_JOIN_WORD: usize = 64;

/// How two lines meet.
#[derive(Debug, Eq, PartialEq)]
enum Join {
    /// Separate words.
    Space,
    /// A compound word broken after its hyphen: keep the hyphen, no space.
    Hyphen,
    /// A word hyphenated at the line end: drop the hyphen.
    Merge,
}

fn line_join(previous: &str, next: &str, vocabulary: &HashSet<String>) -> Join {
    let previous = previous.trim_end();
    let Some(stem) = previous.strip_suffix(HYPHENS) else {
        return Join::Space;
    };
    if !stem.chars().next_back().is_some_and(char::is_alphanumeric) {
        return Join::Space;
    }
    if previous.ends_with('\u{00AD}') {
        return Join::Merge;
    }
    let Some(left) = last_word(stem) else {
        return Join::Hyphen;
    };
    let left = left.trim_start_matches(|ch: char| !ch.is_alphanumeric());
    let right = next.split_whitespace().next().unwrap_or_default();
    let right = right.trim_end_matches(|ch: char| !ch.is_alphanumeric());
    let Some(head) = right.chars().next().filter(|ch| ch.is_alphanumeric()) else {
        return Join::Space;
    };
    if vocabulary.contains(&format!("{left}{right}").to_lowercase()) {
        return Join::Merge;
    }
    if vocabulary.contains(&format!("{left}-{right}").to_lowercase())
        || left.contains('-')
        || right.contains('-')
    {
        return Join::Hyphen;
    }
    if left.chars().last().is_some_and(char::is_lowercase) && head.is_lowercase() {
        Join::Merge
    } else {
        Join::Hyphen
    }
}

/// Returns the last word, reading at most `MAX_JOIN_WORD + 1` characters.
fn last_word(text: &str) -> Option<&str> {
    let mut start = text.len();
    for (count, (index, ch)) in text.char_indices().rev().enumerate() {
        if ch.is_whitespace() {
            break;
        }
        if count == MAX_JOIN_WORD {
            return None;
        }
        start = index;
    }
    Some(&text[start..])
}

/// Appends a line (or a continued block) to spans, joining across the line break.
fn append_line(out: &mut Vec<Span>, line: &[Span], vocabulary: &HashSet<String>) {
    let next: String = line.iter().map(|span| span.text.as_str()).collect();
    if let Some(last) = out.last_mut() {
        let trimmed = last.text.trim_end().len();
        last.text.truncate(trimmed);
        match line_join(&last.text, &next, vocabulary) {
            Join::Space => last.text.push(' '),
            Join::Hyphen => {}
            Join::Merge => {
                last.text.pop();
            }
        }
    }
    for span in line {
        push_span(out, span.clone());
    }
}

/// Joins the lines of a block into one run of spans.
fn join_lines(lines: &[Vec<Span>], vocabulary: &HashSet<String>) -> Vec<Span> {
    let mut out: Vec<Span> = Vec::new();
    for line in lines {
        append_line(&mut out, line, vocabulary);
    }
    out
}

fn push_span(out: &mut Vec<Span>, span: Span) {
    match out.last_mut() {
        Some(last)
            if last.bold == span.bold
                && last.italic == span.italic
                && last.script == span.script =>
        {
            last.text.push_str(&span.text);
        }
        _ => out.push(span),
    }
}

/// Removes the first `len` bytes of text (a list marker) from the spans.
fn strip_marker(spans: Vec<Span>, len: usize) -> Vec<Span> {
    let mut remaining = len;
    let mut out = Vec::with_capacity(spans.len());
    for mut span in spans {
        if remaining > 0 {
            if span.text.len() <= remaining {
                remaining -= span.text.len();
                continue;
            }
            let mut cut = remaining;
            while !span.text.is_char_boundary(cut) {
                cut += 1;
            }
            span.text.drain(..cut);
            remaining = 0;
        }
        out.push(span);
    }
    out
}

/// Converts spans to NFC inline content, wrapping styled runs in `Strong`, `Emph`,
/// `Superscript` and `Subscript`, and checking the nesting limit.
fn inlines(
    spans: &[Span],
    heading: bool,
    limits: &Limits,
    block_depth: u16,
) -> Result<Vec<Inline>, ReadError> {
    let mut out: Vec<Inline> = Vec::new();
    let mut text = String::new();
    // Collapse whitespace across span boundaries and trim the block.
    let mut cleaned: Vec<(String, &Span)> = Vec::new();
    for span in spans {
        let mut value = String::with_capacity(span.text.len());
        for ch in span.text.chars() {
            if ch == '\u{00AD}' {
                continue;
            }
            if ch.is_whitespace() {
                let previous_space = value
                    .chars()
                    .last()
                    .or_else(|| cleaned.last().and_then(|(text, _)| text.chars().last()))
                    .is_none_or(char::is_whitespace);
                if !previous_space {
                    value.push(' ');
                }
            } else {
                value.push(ch);
            }
        }
        cleaned.push((value, span));
    }
    if let Some((last, _)) = cleaned.iter_mut().rev().find(|(text, _)| !text.is_empty()) {
        let trimmed = last.trim_end().len();
        last.truncate(trimmed);
    }

    for (value, span) in cleaned {
        if value.is_empty() {
            continue;
        }
        let bold = span.bold && !heading;
        let styled = bold || span.italic || span.script != Script::Normal;
        if !styled {
            text.push_str(&value);
            continue;
        }
        let core = value.trim();
        let leading = &value[..value.len() - value.trim_start().len()];
        let trailing = &value[value.trim_end().len()..];
        text.push_str(leading);
        if core.is_empty() {
            continue;
        }
        if !text.is_empty() {
            out.push(Inline::Text {
                text: nfc(&std::mem::take(&mut text)),
            });
        }
        let mut depth = block_depth;
        let mut inline = Inline::Text { text: nfc(core) };
        if span.italic {
            inline = Inline::Emph {
                content: vec![inline],
            };
            depth += 1;
        }
        if bold {
            inline = Inline::Strong {
                content: vec![inline],
            };
            depth += 1;
        }
        match span.script {
            Script::Super => {
                inline = Inline::Superscript {
                    content: vec![inline],
                };
                depth += 1;
            }
            Script::Sub => {
                inline = Inline::Subscript {
                    content: vec![inline],
                };
                depth += 1;
            }
            Script::Normal => {}
        }
        if depth > limits.max_nesting_depth {
            return Err(ReadError::NestingTooDeep {
                limit: limits.max_nesting_depth,
            });
        }
        out.push(inline);
        text.push_str(trailing);
    }
    if !text.is_empty() {
        out.push(Inline::Text { text: nfc(&text) });
    }
    if block_depth > limits.max_nesting_depth {
        return Err(ReadError::NestingTooDeep {
            limit: limits.max_nesting_depth,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashSet,
        time::{Duration, Instant},
    };

    use super::{Join, PageBlocks, assemble, is_page_number, join_lines, line_join};
    use crate::{
        ir::{Block, Inline},
        limits::Limits,
        reader::pdf::{
            ReadError,
            layout::{self, Script, Span},
        },
    };

    fn span(text: &str) -> Span {
        Span {
            text: text.to_owned(),
            bold: false,
            italic: false,
            script: Script::Normal,
        }
    }

    fn block(lines: &[&str], top: f64, size: f64, bold: bool) -> layout::Block {
        layout::Block {
            lines: lines.iter().map(|line| vec![span(line)]).collect(),
            bbox: [72.0, top, 500.0, top + size * lines.len() as f64],
            size,
            bold,
            chars: lines.iter().map(|line| line.chars().count()).sum(),
            upright: true,
        }
    }

    fn texts(blocks: &[Block]) -> Vec<String> {
        fn flatten(content: &[Inline], out: &mut String) {
            for inline in content {
                match inline {
                    Inline::Text { text } => out.push_str(text),
                    Inline::Strong { content }
                    | Inline::Emph { content }
                    | Inline::Superscript { content } => flatten(content, out),
                    _ => {}
                }
            }
        }
        blocks
            .iter()
            .map(|block| {
                let mut out = String::new();
                match block {
                    Block::Heading { level, content } => {
                        out.push_str(&format!("h{level}:"));
                        flatten(content, &mut out);
                    }
                    Block::Paragraph { content } => flatten(content, &mut out),
                    Block::List { items, ordered, .. } => {
                        out.push_str(if *ordered { "ol:" } else { "ul:" });
                        for item in items {
                            if let [Block::Paragraph { content }] = item.blocks.as_slice() {
                                flatten(content, &mut out);
                                out.push('|');
                            }
                        }
                    }
                    _ => {}
                }
                out
            })
            .collect()
    }

    #[test]
    fn builds_headings_lists_and_furniture() {
        let page = |page_no: u32, extra: Vec<layout::Block>| {
            let mut blocks = vec![block(&["Annual report 2024"], 30.0, 9.0, false)];
            blocks.extend(extra);
            blocks.push(block(&[&page_no.to_string()], 770.0, 9.0, false));
            PageBlocks {
                page_no,
                height: 792.0,
                blocks,
            }
        };
        let pages = vec![
            page(
                1,
                vec![
                    block(&["Big Title"], 100.0, 20.0, true),
                    block(&["Section"], 140.0, 14.0, true),
                    block(
                        &[
                            "Body text that is long enough to be the body",
                            "of the para-",
                            "graph continues.",
                        ],
                        160.0,
                        10.0,
                        false,
                    ),
                    block(&["• first item"], 220.0, 10.0, false),
                    block(&["• second item"], 232.0, 10.0, false),
                    block(&["1. numbered"], 250.0, 10.0, false),
                ],
            ),
            page(
                2,
                vec![block(
                    &["More body text on the second page here."],
                    100.0,
                    10.0,
                    false,
                )],
            ),
        ];
        let out = assemble(&pages, &Limits::local()).unwrap();
        assert_eq!(
            texts(&out.body),
            vec![
                "h1:Big Title",
                "h2:Section",
                "Body text that is long enough to be the body of the paragraph continues.",
                "ul:first item|second item|",
                "ol:numbered|",
                "More body text on the second page here.",
            ]
        );
        assert_eq!(
            texts(&out.furniture),
            vec!["Annual report 2024", "1", "Annual report 2024", "2"]
        );
        assert_eq!(out.positions["body/3"].page_no, 1);
        assert_eq!(out.positions["body/5"].page_no, 2);
        assert_eq!(out.positions["furniture/3"].bbox.top, 770.0);
    }

    #[test]
    fn numbered_lists_split_on_restarts_and_gaps_and_keep_their_numbers() {
        let pages = vec![PageBlocks {
            page_no: 1,
            height: 792.0,
            blocks: vec![
                block(&["1. one"], 100.0, 10.0, false),
                block(&["2. two"], 112.0, 10.0, false),
                block(&["4. four"], 124.0, 10.0, false),
                block(&["1. again"], 136.0, 10.0, false),
            ],
        }];
        assert_eq!(
            ordered_lists(&pages),
            vec![(Some(1), 2), (Some(4), 1), (Some(1), 1)]
        );
    }

    #[test]
    fn roman_and_lettered_lists_are_not_split_by_their_numeric_value() {
        let pages = vec![PageBlocks {
            page_no: 1,
            height: 792.0,
            blocks: vec![
                block(&["iv) fourth"], 100.0, 10.0, false),
                block(&["v) fifth"], 112.0, 10.0, false),
                block(&["vi) sixth"], 124.0, 10.0, false),
            ],
        }];
        assert_eq!(ordered_lists(&pages), vec![(Some(4), 3)]);
    }

    fn ordered_lists(pages: &[PageBlocks]) -> Vec<(Option<u64>, usize)> {
        assemble(pages, &Limits::local())
            .unwrap()
            .body
            .iter()
            .filter_map(|block| match block {
                Block::List {
                    ordered: true,
                    start,
                    items,
                    ..
                } => Some((*start, items.len())),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn styled_spans_become_nested_inlines_within_the_depth_limit() {
        let mut bold = span("Bold ");
        bold.bold = true;
        let mut sup = span("1");
        sup.script = Script::Super;
        let pages = vec![PageBlocks {
            page_no: 1,
            height: 792.0,
            blocks: vec![layout::Block {
                lines: vec![vec![bold, span("text"), sup]],
                bbox: [0.0, 300.0, 100.0, 310.0],
                size: 10.0,
                bold: false,
                chars: 10,
                upright: true,
            }],
        }];
        let out = assemble(&pages, &Limits::local()).unwrap();
        let Block::Paragraph { content } = &out.body[0] else {
            panic!("expected a paragraph");
        };
        assert_eq!(content.len(), 3);
        assert!(matches!(&content[0], Inline::Strong { .. }));
        assert_eq!(
            content[1],
            Inline::Text {
                text: " text".to_owned()
            }
        );
        assert!(matches!(&content[2], Inline::Superscript { .. }));

        let limits = Limits {
            max_nesting_depth: 0,
            ..Limits::local()
        };
        assert_eq!(
            assemble(&pages, &limits).err(),
            Some(ReadError::NestingTooDeep { limit: 0 })
        );
    }

    #[test]
    fn hyphenation_and_page_numbers() {
        let words: HashSet<String> = ["fintabnet", "table-structure"]
            .into_iter()
            .map(str::to_owned)
            .collect();
        assert_eq!(line_join("of the para-", "graph is", &words), Join::Merge);
        assert_eq!(line_join("the FinTab-", "Net [36]", &words), Join::Merge);
        assert_eq!(
            line_join("a table-", "structure model", &words),
            Join::Hyphen
        );
        assert_eq!(line_join("end-to-", "end model", &words), Join::Hyphen);
        assert_eq!(line_join("state-", "of-the-art", &words), Join::Hyphen);
        assert_eq!(line_join("COVID-", "19 cases", &words), Join::Hyphen);
        assert_eq!(line_join("Hà Nội -", "Tự do", &words), Join::Space);
        assert_eq!(line_join("plain", "text", &words), Join::Space);
        let joined = join_lines(&[vec![span("non-")], vec![span("trivial")]], &words);
        assert_eq!(joined[0].text, "nontrivial");
        for text in [
            "12",
            "- 3 -",
            "Page 4",
            "Page 4 of 10",
            "Trang 2",
            "iv",
            "3/10",
            "4 of 9",
        ] {
            assert!(is_page_number(text), "{text}");
        }
        for text in ["Chapter 2", "2024 Report", "12345", "Civil", "mix", "dim"] {
            assert!(!is_page_number(text), "{text}");
        }
    }

    #[test]
    fn hyphen_joins_read_at_most_64_characters_of_the_left_word() {
        let words = HashSet::new();
        let word = "a".repeat(64);
        assert_eq!(line_join(&format!("x {word}-"), "b", &words), Join::Merge);
        assert_eq!(line_join(&format!("{word}a-"), "b", &words), Join::Hyphen);
        assert_eq!(
            line_join(&format!("{word}a\u{00AD}"), "b", &words),
            Join::Merge
        );
        assert_eq!(line_join(&format!("{word}a -"), "b", &words), Join::Space);
        let word = "ế".repeat(64);
        assert_eq!(line_join(&format!("{word}-"), "b", &words), Join::Merge);
        assert_eq!(line_join(&format!("{word}ế-"), "b", &words), Join::Hyphen);
    }

    #[test]
    fn long_runs_of_hyphenated_lines_join_in_linear_time() {
        // Every `ab-` line merged into the one before, so each join rescanned, copied and
        // lowercased the whole accumulated word: quadratic in the number of lines.
        const LINES: usize = 20_000;
        let lines = vec![vec![span("ab-")]; LINES];
        let started = Instant::now();
        let joined = join_lines(&lines, &HashSet::new());
        let elapsed = started.elapsed();
        eprintln!("hyphen join debug elapsed: {elapsed:?}");
        assert!(elapsed < Duration::from_secs(1), "joining took {elapsed:?}");
        // Merging stops once the word passes 64 characters; later joins keep their hyphens.
        let expected = format!("{}{}-", "ab".repeat(33), "-ab".repeat(LINES - 33));
        assert_eq!(joined.len(), 1);
        assert_eq!(joined[0].text, expected);
    }
}
