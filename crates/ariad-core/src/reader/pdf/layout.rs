//! Page layout analysis.
//!
//! Glyphs become runs (neighbours in content order), runs become segments (line fragments on
//! one baseline, split at wide gaps such as column gutters and table cells), segments are put in
//! reading order (single column, or two columns with spanning lines between bands), and
//! consecutive segments are grouped into blocks by line pitch, indentation, short lines, list
//! markers and font changes.
//!
//! All geometry is computed in a canonical frame where text runs left to right and y grows
//! downward; text in other orientations is rotated into that frame and back.

use std::collections::HashSet;

use unicode_normalization::char::is_combining_mark;

use super::{
    interpreter::{CHUNK_START, FontStyle, Glyph, PageContent, SPACE, STROKED},
    lists,
};

/// Gap, in font sizes, beyond which glyphs on one baseline belong to separate segments.
const SEGMENT_GAP: f64 = 1.5;
/// Gap, in font sizes, that separates two words when no space glyph does.
const WORD_GAP: f64 = 0.11;
/// Ascent and descent of the glyph box, in font sizes.
const ASCENT: f64 = 0.8;
const DESCENT: f64 = 0.2;
/// Largest baseline difference, in font sizes, between runs or segments of one row.
const ROW_TOLERANCE: f64 = 0.5;
/// Fraction of a block's characters that must be bold for the block to count as bold.
const BOLD_SHARE: f64 = 0.9;

/// Vertical position of a glyph run relative to its line.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Script {
    Normal,
    Super,
    Sub,
}

/// Text with one style.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Span {
    pub(super) text: String,
    pub(super) bold: bool,
    pub(super) italic: bool,
    pub(super) script: Script,
}

/// A text block on a page, in reading order.
#[derive(Clone, Debug)]
pub(super) struct Block {
    /// Lines of styled text; spaces between words are part of the span text.
    pub(super) lines: Vec<Vec<Span>>,
    /// `[left, top, right, bottom]` in page display space.
    pub(super) bbox: [f64; 4],
    /// Dominant font size.
    pub(super) size: f64,
    pub(super) bold: bool,
    pub(super) chars: usize,
    /// Whether the text runs left to right on the displayed page.
    pub(super) upright: bool,
}

/// Lays out one page whose display box is `width` by `height` points.
pub(super) fn layout_page(
    content: &PageContent,
    styles: &[FontStyle],
    width: f64,
    height: f64,
) -> Vec<Block> {
    let mut groups: [Vec<CGlyph>; 4] = Default::default();
    let mut seen: HashSet<(&str, i64, i64, u8)> = HashSet::new();
    for (index, glyph) in content.glyphs.iter().enumerate() {
        let margin = glyph.size;
        if glyph.x < -margin
            || glyph.y < -margin
            || glyph.x > width + margin
            || glyph.y > height + margin
            || glyph.size > 4.0 * height.max(width)
        {
            continue;
        }
        let text = content.glyph_text(glyph);
        let orientation = orientation(glyph);
        if !text.is_empty()
            && !seen.insert((
                text,
                (glyph.x * 2.0).round() as i64,
                (glyph.y * 2.0).round() as i64,
                orientation,
            ))
        {
            // The same text painted again at the same spot, as producers do to fake bold.
            continue;
        }
        groups[usize::from(orientation)].push(CGlyph::new(glyph, index, text, orientation));
    }

    let mut blocks = Vec::new();
    for (orientation, glyphs) in groups.iter().enumerate() {
        if glyphs.is_empty() {
            continue;
        }
        let runs = build_runs(glyphs);
        let segments = build_segments(glyphs, &runs, content, styles);
        if segments.is_empty() {
            continue;
        }
        let flows = if orientation == 0 {
            order_flows(&segments)
        } else {
            vec![rows_of(&segments, (0..segments.len()).collect())]
        };
        let pitch = typical_pitch(&segments, &flows);
        for flow in &flows {
            group_blocks(&segments, flow, pitch, orientation as u8, &mut blocks);
        }
    }
    blocks
}

fn orientation(glyph: &Glyph) -> u8 {
    if glyph.ux.abs() >= glyph.uy.abs() {
        if glyph.ux >= 0.0 { 0 } else { 2 }
    } else if glyph.uy > 0.0 {
        1
    } else {
        3
    }
}

/// Display coordinates to the canonical frame of an orientation.
fn to_canonical(orientation: u8, x: f64, y: f64) -> (f64, f64) {
    match orientation {
        1 => (y, -x),
        2 => (-x, -y),
        3 => (-y, x),
        _ => (x, y),
    }
}

/// Canonical coordinates back to display space.
fn to_display(orientation: u8, u: f64, v: f64) -> (f64, f64) {
    match orientation {
        1 => (-v, u),
        2 => (-u, -v),
        3 => (v, -u),
        _ => (u, v),
    }
}

/// A glyph in the canonical frame.
#[derive(Clone, Copy, Debug)]
struct CGlyph {
    x0: f64,
    x1: f64,
    base: f64,
    size: f64,
    index: usize,
    flags: u8,
    /// A combining mark, which always attaches to the preceding glyph.
    mark: bool,
}

impl CGlyph {
    fn new(glyph: &Glyph, index: usize, text: &str, orientation: u8) -> Self {
        let (x0, base) = to_canonical(orientation, glyph.x, glyph.y);
        Self {
            x0,
            // Broken width tables must not stretch a glyph across a column gutter.
            x1: x0 + glyph.width.min(3.0 * glyph.size),
            base,
            size: glyph.size,
            index,
            flags: glyph.flags,
            mark: text.chars().next().is_some_and(is_combining_mark),
        }
    }

    fn top(&self) -> f64 {
        self.base - ASCENT * self.size
    }

    fn bottom(&self) -> f64 {
        self.base + DESCENT * self.size
    }

    const fn is_space(&self) -> bool {
        self.flags & SPACE != 0
    }
}

fn overlap(top_a: f64, bottom_a: f64, top_b: f64, bottom_b: f64) -> f64 {
    (bottom_a.min(bottom_b) - top_a.max(top_b)).max(0.0)
}

/// Whether two glyphs sit on the same line: their boxes overlap by half the smaller height.
fn same_line(a: &CGlyph, b: &CGlyph) -> bool {
    let shared = overlap(a.top(), a.bottom(), b.top(), b.bottom());
    shared >= 0.5 * a.size.min(b.size) * (ASCENT + DESCENT)
}

/// Consecutive glyphs in content order that continue one line.
#[derive(Clone, Debug)]
struct Run {
    start: usize,
    end: usize,
    /// Last glyph that is not a combining mark.
    last: usize,
    x0: f64,
    x1: f64,
    base: f64,
    size: f64,
    glyph_count: usize,
}

impl Run {
    fn new(index: usize, glyph: &CGlyph) -> Self {
        Self {
            start: index,
            end: index + 1,
            last: index,
            x0: glyph.x0,
            x1: glyph.x1,
            base: glyph.base,
            size: glyph.size,
            glyph_count: 1,
        }
    }
}

fn build_runs(glyphs: &[CGlyph]) -> Vec<Run> {
    let mut runs: Vec<Run> = Vec::new();
    for (index, glyph) in glyphs.iter().enumerate() {
        if let Some(run) = runs.last_mut() {
            if glyph.mark {
                run.end = index + 1;
                continue;
            }
            let previous = &glyphs[run.last];
            let gap = glyph.x0 - previous.x1;
            if same_line(previous, glyph)
                && gap >= -0.5 * previous.size.max(glyph.size)
                && gap <= SEGMENT_GAP * previous.size.min(glyph.size)
            {
                run.end = index + 1;
                run.last = index;
                run.glyph_count += 1;
                if !glyph.is_space() {
                    if glyph.size > run.size {
                        run.size = glyph.size;
                        run.base = glyph.base;
                    }
                    run.x0 = run.x0.min(glyph.x0);
                    run.x1 = run.x1.max(glyph.x1);
                }
                continue;
            }
        }
        runs.push(Run::new(index, glyph));
    }
    runs
}

/// A line fragment: runs on one baseline without a wide gap between them.
#[derive(Clone, Debug)]
pub(super) struct Segment {
    x0: f64,
    x1: f64,
    base: f64,
    size: f64,
    spans: Vec<Span>,
    chars: usize,
    bold_chars: usize,
    /// Width of the first word, to test whether it would have fit on the previous line.
    first_word_width: f64,
    /// Start of the text after a leading list marker.
    text_x: Option<f64>,
    marker: lists::MarkerStrength,
}

impl Segment {
    fn top(&self) -> f64 {
        self.base - ASCENT * self.size
    }

    fn bottom(&self) -> f64 {
        self.base + DESCENT * self.size
    }

    fn all_bold(&self) -> bool {
        self.chars > 0 && self.bold_chars as f64 >= BOLD_SHARE * self.chars as f64
    }

    fn plain_text(&self) -> String {
        self.spans.iter().map(|span| span.text.as_str()).collect()
    }
}

/// Groups runs into rows by baseline, then splits rows at wide gaps into segments.
fn build_segments(
    glyphs: &[CGlyph],
    runs: &[Run],
    content: &PageContent,
    styles: &[FontStyle],
) -> Vec<Segment> {
    let mut order: Vec<usize> = (0..runs.len()).collect();
    order.sort_by(|&a, &b| runs[a].base.total_cmp(&runs[b].base));

    let mut rows: Vec<(usize, Vec<usize>)> = Vec::new();
    for index in order {
        let run = &runs[index];
        if let Some((dominant, members)) = rows.last_mut() {
            let lead = &runs[*dominant];
            if (run.base - lead.base).abs() <= ROW_TOLERANCE * lead.size.min(run.size) {
                members.push(index);
                if (run.size, run.glyph_count) > (lead.size, lead.glyph_count) {
                    *dominant = index;
                }
                continue;
            }
        }
        rows.push((index, vec![index]));
    }

    let mut segments = Vec::new();
    for (_, mut members) in rows {
        members.sort_by(|&a, &b| runs[a].x0.total_cmp(&runs[b].x0));
        let mut current: Vec<usize> = Vec::new();
        let mut right = f64::NEG_INFINITY;
        // Size of the run that reaches furthest right, which the next gap is measured from.
        let mut edge_size: f64 = 0.0;
        for index in members {
            let run = &runs[index];
            if !current.is_empty() && run.x0 - right > SEGMENT_GAP * edge_size.min(run.size) {
                segments.extend(assemble_segment(glyphs, runs, &current, content, styles));
                current.clear();
                right = f64::NEG_INFINITY;
            }
            current.push(index);
            if run.x1 > right {
                right = run.x1;
                edge_size = run.size;
            }
        }
        if !current.is_empty() {
            segments.extend(assemble_segment(glyphs, runs, &current, content, styles));
        }
    }
    merge_markers(segments)
}

/// Builds a segment's styled text from its runs, inserting spaces at word gaps.
fn assemble_segment(
    glyphs: &[CGlyph],
    runs: &[Run],
    members: &[usize],
    content: &PageContent,
    styles: &[FontStyle],
) -> Option<Segment> {
    let dominant = members.iter().map(|&index| &runs[index]).max_by(|a, b| {
        (a.size, a.glyph_count)
            .partial_cmp(&(b.size, b.glyph_count))
            .unwrap_or(std::cmp::Ordering::Equal)
    })?;
    let sequence: Vec<&CGlyph> = members
        .iter()
        .flat_map(|&index| &glyphs[runs[index].start..runs[index].end])
        .collect();
    // The median glyph size resists the per-word size noise of OCR text layers and the odd
    // large initial.
    let mut sizes: Vec<f64> = sequence
        .iter()
        .filter(|glyph| !glyph.is_space() && !glyph.mark)
        .map(|glyph| glyph.size)
        .collect();
    sizes.sort_by(f64::total_cmp);
    let base = dominant.base;
    let size = sizes.get(sizes.len() / 2).copied().unwrap_or(dominant.size);
    let threshold = word_gap(&sequence);

    let mut segment = Segment {
        x0: f64::INFINITY,
        x1: f64::NEG_INFINITY,
        base,
        size,
        spans: Vec::new(),
        chars: 0,
        bold_chars: 0,
        first_word_width: 0.0,
        text_x: None,
        marker: lists::MarkerStrength::None,
    };
    let mut previous: Option<&CGlyph> = None;
    let mut pending_space = false;
    let mut first_word_end: Option<f64> = None;
    let mut pending_accent: Option<char> = None;
    for (position, glyph) in sequence.iter().enumerate() {
        let source = &content.glyphs[glyph.index];
        let text = content.glyph_text(source);
        if glyph.is_space() {
            pending_space |= previous.is_some();
            continue;
        }
        if glyph.mark {
            if let Some(span) = segment.spans.last_mut() {
                span.text.push_str(text);
            }
            continue;
        }
        // A spacing accent drawn over its letter, as TeX composes accented characters.
        if let Some(mark) = combining_accent(text) {
            let centre = (glyph.x0 + glyph.x1) / 2.0;
            let over = |other: &CGlyph| {
                centre >= other.x0 - 0.1 * other.size && centre <= other.x1 + 0.1 * other.size
            };
            let next = sequence[position + 1..]
                .iter()
                .find(|other| !other.is_space() && !other.mark);
            if let Some(next) = next
                && over(next)
            {
                pending_accent = Some(mark);
                continue;
            }
            if let Some(previous) = previous
                && over(previous)
                && let Some(span) = segment.spans.last_mut()
            {
                span.text.push(mark);
                continue;
            }
        }
        if let Some(previous) = previous {
            let scale = previous.size.min(glyph.size);
            let gap = glyph.x0 - previous.x1;
            let boundary = pending_space
                || gap > threshold * scale
                || gap < -0.5 * scale
                || (glyph.flags & CHUNK_START != 0 && gap < -0.15 * scale);
            if boundary && !segment.spans.is_empty() {
                if first_word_end.is_none() {
                    first_word_end = Some(previous.x1);
                    segment.text_x = Some(glyph.x0);
                }
                if let Some(span) = segment.spans.last_mut()
                    && !span.text.ends_with(' ')
                {
                    span.text.push(' ');
                }
            }
        }
        pending_space = false;
        previous = Some(glyph);
        segment.x0 = segment.x0.min(glyph.x0);
        segment.x1 = segment.x1.max(glyph.x1);
        if text.is_empty() {
            continue;
        }
        let style = styles
            .get(source.font as usize)
            .copied()
            .unwrap_or_default();
        let bold = style.bold || glyph.flags & STROKED != 0;
        let script = script_of(glyph, base, size);
        let count = text.chars().count();
        segment.chars += count;
        if bold {
            segment.bold_chars += count;
        }
        match segment.spans.last_mut() {
            Some(span)
                if span.bold == bold && span.italic == style.italic && span.script == script =>
            {
                span.text.push_str(text);
            }
            _ => segment.spans.push(Span {
                text: text.to_owned(),
                bold,
                italic: style.italic,
                script,
            }),
        }
        if let Some(mark) = pending_accent.take()
            && let Some(span) = segment.spans.last_mut()
        {
            span.text.push(mark);
        }
    }
    if segment.chars == 0 {
        return None;
    }
    // Raised or lowered text longer than a few words is a smaller line of its own, not a
    // superscript or subscript.
    let mut spans: Vec<Span> = Vec::with_capacity(segment.spans.len());
    for mut span in std::mem::take(&mut segment.spans) {
        let trimmed = span.text.trim();
        if span.script != Script::Normal
            && (trimmed.chars().count() > 12 || trimmed.split_whitespace().count() > 3)
        {
            span.script = Script::Normal;
        }
        match spans.last_mut() {
            Some(last)
                if last.bold == span.bold
                    && last.italic == span.italic
                    && last.script == span.script =>
            {
                last.text.push_str(&span.text);
            }
            _ => spans.push(span),
        }
    }
    for span in &mut spans {
        span.text = crate::reader::common::nfc(&span.text);
    }
    segment.spans = spans;
    if let Some(span) = segment.spans.last_mut() {
        let trimmed = span.text.trim_end().len();
        span.text.truncate(trimmed);
    }
    segment.first_word_width = first_word_end.unwrap_or(segment.x1) - segment.x0;
    segment.marker = lists::marker_strength(&segment.plain_text());
    Some(segment)
}

/// The combining form of a spacing accent glyph.
fn combining_accent(text: &str) -> Option<char> {
    let mut chars = text.chars();
    let accent = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    Some(match accent {
        '\u{00B4}' => '\u{0301}',
        '`' => '\u{0300}',
        '\u{00A8}' => '\u{0308}',
        '\u{02C6}' => '\u{0302}',
        '\u{02DC}' => '\u{0303}',
        '\u{00AF}' => '\u{0304}',
        '\u{02D8}' => '\u{0306}',
        '\u{02D9}' => '\u{0307}',
        '\u{02DA}' => '\u{030A}',
        '\u{00B8}' => '\u{0327}',
        '\u{02DB}' => '\u{0328}',
        '\u{02C7}' => '\u{030C}',
        '\u{02DD}' => '\u{030B}',
        _ => return None,
    })
}

/// The word-gap threshold of a segment, in font sizes. Letter-spaced text raises it by the
/// typical gap between letters.
fn word_gap(sequence: &[&CGlyph]) -> f64 {
    let mut gaps: Vec<f64> = Vec::new();
    let mut previous: Option<&CGlyph> = None;
    for glyph in sequence {
        if glyph.mark {
            continue;
        }
        if glyph.is_space() {
            previous = None;
            continue;
        }
        if let Some(previous) = previous {
            gaps.push((glyph.x0 - previous.x1) / previous.size.min(glyph.size));
        }
        previous = Some(glyph);
    }
    if gaps.len() < 8 {
        return WORD_GAP;
    }
    gaps.sort_by(f64::total_cmp);
    let typical = gaps[gaps.len() * 3 / 10];
    WORD_GAP + typical.clamp(0.0, 0.35)
}

fn script_of(glyph: &CGlyph, base: f64, size: f64) -> Script {
    if glyph.size > 0.85 * size {
        return Script::Normal;
    }
    let raised = base - glyph.base;
    if raised >= 0.2 * size {
        Script::Super
    } else if raised <= -0.1 * size {
        Script::Sub
    } else {
        Script::Normal
    }
}

/// Joins a segment holding only a list marker with the text that follows it on the same row.
fn merge_markers(segments: Vec<Segment>) -> Vec<Segment> {
    let mut merged: Vec<Segment> = Vec::with_capacity(segments.len());
    for segment in segments {
        if let Some(previous) = merged.last_mut()
            && lists::is_marker_only(&previous.plain_text())
            && (segment.base - previous.base).abs() < 0.3 * previous.size.max(segment.size)
            && segment.x0 > previous.x1
            && segment.x0 - previous.x1 < 6.0 * previous.size.max(segment.size)
        {
            previous.text_x = Some(segment.x0);
            previous.first_word_width = previous.x1 - previous.x0;
            previous.x1 = segment.x1;
            previous.chars += segment.chars;
            previous.bold_chars += segment.bold_chars;
            if previous.size < segment.size {
                previous.size = segment.size;
                previous.base = segment.base;
            }
            if let Some(span) = previous.spans.last_mut() {
                span.text.push(' ');
            }
            previous.spans.extend(segment.spans);
            previous.marker = lists::marker_strength(&previous.plain_text());
            continue;
        }
        merged.push(segment);
    }
    merged
}

/// Rows of segment indices in top-to-bottom order, each row sorted left to right.
type Flow = Vec<Vec<usize>>;

fn rows_of(segments: &[Segment], mut members: Vec<usize>) -> Flow {
    members.sort_by(|&a, &b| segments[a].base.total_cmp(&segments[b].base));
    let mut rows: Flow = Vec::new();
    for index in members {
        let segment = &segments[index];
        if let Some(row) = rows.last_mut() {
            let lead = &segments[row[0]];
            if (segment.base - lead.base).abs() <= ROW_TOLERANCE * lead.size.min(segment.size) {
                row.push(index);
                continue;
            }
        }
        rows.push(vec![index]);
    }
    for row in &mut rows {
        row.sort_by(|&a, &b| segments[a].x0.total_cmp(&segments[b].x0));
    }
    rows
}

/// Splits a page into reading-order flows: one flow for a single-column page; for a page with
/// two columns, the bands between full-width lines read left column first, then right.
fn order_flows(segments: &[Segment]) -> Vec<Flow> {
    let all: Vec<usize> = (0..segments.len()).collect();
    let Some(gutter) = find_gutter(segments) else {
        return vec![rows_of(segments, all)];
    };
    let mut spanning: Vec<usize> = all
        .iter()
        .copied()
        .filter(|&index| segments[index].x0 < gutter && segments[index].x1 > gutter)
        .collect();
    spanning.sort_by(|&a, &b| segments[a].base.total_cmp(&segments[b].base));
    let centre = |index: usize| (segments[index].top() + segments[index].bottom()) / 2.0;

    // Band k holds the column segments between spanning segments k - 1 and k.
    let mut bands: Vec<(Vec<usize>, Vec<usize>)> =
        vec![(Vec::new(), Vec::new()); spanning.len() + 1];
    for &index in &all {
        let segment = &segments[index];
        if segment.x0 < gutter && segment.x1 > gutter {
            continue;
        }
        let band = spanning.partition_point(|&span| centre(span) < centre(index));
        if segment.x1 <= gutter {
            bands[band].0.push(index);
        } else {
            bands[band].1.push(index);
        }
    }

    let mut flows = Vec::new();
    let mut spanning_run: Vec<usize> = Vec::new();
    for (band, (left, right)) in bands.into_iter().enumerate() {
        if !left.is_empty() || !right.is_empty() {
            if !spanning_run.is_empty() {
                flows.push(rows_of(segments, std::mem::take(&mut spanning_run)));
            }
            if !left.is_empty() {
                flows.push(rows_of(segments, left));
            }
            if !right.is_empty() {
                flows.push(rows_of(segments, right));
            }
        }
        if let Some(&span) = spanning.get(band) {
            spanning_run.push(span);
        }
    }
    if !spanning_run.is_empty() {
        flows.push(rows_of(segments, spanning_run));
    }
    flows
}

/// Finds the x position of a column gutter, if the page has two columns of text.
fn find_gutter(segments: &[Segment]) -> Option<f64> {
    const BINS: usize = 240;
    if segments.len() < 8 {
        return None;
    }
    let left = segments.iter().map(|s| s.x0).fold(f64::INFINITY, f64::min);
    let right = segments
        .iter()
        .map(|s| s.x1)
        .fold(f64::NEG_INFINITY, f64::max);
    let span = right - left;
    if !span.is_finite() || span <= 0.0 {
        return None;
    }
    let bin_width = span / BINS as f64;
    let bin_of = |x: f64| (((x - left) / bin_width).floor().max(0.0) as usize).min(BINS - 1);
    let mut delta = vec![0_i64; BINS + 1];
    // Page numbers and figure labels may sit inside a gutter; only text lines delimit it.
    for segment in segments.iter().filter(|segment| segment.chars >= 4) {
        delta[bin_of(segment.x0)] += 1;
        delta[bin_of(segment.x1) + 1] -= 1;
    }
    let mut coverage = Vec::with_capacity(BINS);
    let mut running = 0_i64;
    for value in &delta[..BINS] {
        running += value;
        coverage.push(running);
    }
    let (from, to) = (BINS * 3 / 10, BINS * 7 / 10);
    let minimum = *coverage[from..to].iter().min()?;
    // The widest stretch of bins at the minimum coverage.
    let (mut best_start, mut best_len, mut start) = (0, 0, None);
    for (bin, &count) in coverage.iter().enumerate().take(to + 1).skip(from) {
        let at_minimum = bin < to && count == minimum;
        match (at_minimum, start) {
            (true, None) => start = Some(bin),
            (false, Some(first)) => {
                if bin - first > best_len {
                    (best_start, best_len) = (first, bin - first);
                }
                start = None;
            }
            _ => {}
        }
    }
    if best_len == 0 {
        return None;
    }
    let gutter = left + (best_start as f64 + best_len as f64 / 2.0) * bin_width;
    let sizes: Vec<f64> = segments.iter().map(|s| s.size).collect();
    let size = median(&sizes)?;
    if (best_len as f64) * bin_width < 0.5 * size {
        return None;
    }

    let half_left = gutter - left;
    let half_right = right - gutter;
    let long_left = segments
        .iter()
        .filter(|s| s.x1 <= gutter && s.chars >= 8 && s.x1 - s.x0 >= 0.6 * half_left)
        .count();
    let long_right = segments
        .iter()
        .filter(|s| s.x0 >= gutter && s.chars >= 8 && s.x1 - s.x0 >= 0.6 * half_right)
        .count();
    let crossing = segments
        .iter()
        .filter(|s| s.x0 < gutter && s.x1 > gutter)
        .count();
    (long_left >= 3 && long_right >= 3 && crossing <= long_left + long_right).then_some(gutter)
}

fn median(values: &[f64]) -> Option<f64> {
    let mut sorted: Vec<f64> = values.iter().copied().filter(|v| v.is_finite()).collect();
    if sorted.is_empty() {
        return None;
    }
    sorted.sort_by(f64::total_cmp);
    Some(sorted[sorted.len() / 2])
}

/// Typical baseline distance between lines of one paragraph, in font sizes.
fn typical_pitch(segments: &[Segment], flows: &[Flow]) -> f64 {
    let mut pitches = Vec::new();
    for flow in flows {
        for pair in flow.windows(2) {
            let ([upper], [lower]) = (pair[0].as_slice(), pair[1].as_slice()) else {
                continue;
            };
            let (upper, lower) = (&segments[*upper], &segments[*lower]);
            let size = upper.size.max(lower.size);
            if (upper.size - lower.size).abs() > 0.15 * size {
                continue;
            }
            let pitch = (lower.base - upper.base) / size;
            if pitch > 0.5 && pitch < 3.0 {
                pitches.push(pitch);
            }
        }
    }
    if pitches.len() < 3 {
        return 1.2;
    }
    pitches.sort_by(f64::total_cmp);
    pitches[pitches.len() * 2 / 5]
}

/// A block under construction.
struct Builder {
    segments: Vec<usize>,
    right: f64,
    /// Whether every line so far is bold.
    bold: bool,
}

/// Left and right text margins of a flow, from its longer lines.
#[derive(Clone, Copy)]
struct Margins {
    left: f64,
    right: f64,
}

fn margins(segments: &[Segment], flow: &Flow) -> Margins {
    let members = || flow.iter().flatten().map(|&index| &segments[index]);
    let mut lefts: Vec<f64> = members().filter(|s| s.chars >= 12).map(|s| s.x0).collect();
    let mut rights: Vec<f64> = members().filter(|s| s.chars >= 12).map(|s| s.x1).collect();
    lefts.sort_by(f64::total_cmp);
    rights.sort_by(f64::total_cmp);
    Margins {
        left: lefts
            .get(lefts.len() / 10)
            .copied()
            .unwrap_or_else(|| members().map(|s| s.x0).fold(f64::INFINITY, f64::min)),
        right: rights
            .get(rights.len() * 9 / 10)
            .copied()
            .unwrap_or_else(|| members().map(|s| s.x1).fold(f64::NEG_INFINITY, f64::max)),
    }
}

fn group_blocks(
    segments: &[Segment],
    flow: &Flow,
    pitch: f64,
    orientation: u8,
    blocks: &mut Vec<Block>,
) {
    let margins = margins(segments, flow);
    let mut current: Option<Builder> = None;
    for row in flow {
        if let [index] = row.as_slice() {
            let segment = &segments[*index];
            if let Some(builder) = &mut current
                && continues(segments, builder, segment, pitch, margins)
            {
                builder.segments.push(*index);
                builder.right = builder.right.max(segment.x1);
                builder.bold &= segment.all_bold();
                continue;
            }
            if let Some(builder) = current.take() {
                blocks.push(finish(segments, &builder.segments, orientation));
            }
            current = Some(Builder {
                segments: vec![*index],
                right: segment.x1,
                bold: segment.all_bold(),
            });
        } else {
            if let Some(builder) = current.take() {
                blocks.push(finish(segments, &builder.segments, orientation));
            }
            for &index in row {
                blocks.push(finish(segments, &[index], orientation));
            }
        }
    }
    if let Some(builder) = current.take() {
        blocks.push(finish(segments, &builder.segments, orientation));
    }
}

/// Whether `segment` continues the block being built.
fn continues(
    segments: &[Segment],
    builder: &Builder,
    segment: &Segment,
    pitch: f64,
    margins: Margins,
) -> bool {
    let first = &segments[builder.segments[0]];
    let Some(previous) = builder.segments.last().map(|&index| &segments[index]) else {
        return false;
    };
    let size = previous.size.max(segment.size);
    if (segment.size - previous.size).abs() > 0.15 * size {
        return false;
    }
    let distance = segment.base - previous.base;
    if distance <= 0.2 * size || distance > (pitch * 1.15 + 0.15) * size {
        return false;
    }
    if segment.x0 >= previous.x1 || segment.x1 <= previous.x0 {
        return false;
    }
    if segment.all_bold() != builder.bold && (builder.segments.len() == 1 || builder.bold) {
        return false;
    }
    let centred = ((segment.x0 + segment.x1) - (previous.x0 + previous.x1)).abs() / 2.0 < size
        && (segment.x0 - previous.x0).abs() > size;
    // The previous line ended early although the next line's first word would have fit: within
    // the block's own width, or within the flow's width when both lines share a first-line
    // indent (paragraphs of one line each).
    let needed = segment.first_word_width + 0.5 * size;
    let local_right = previous
        .x1
        .max(segment.x1)
        .max(if builder.segments.len() >= 2 {
            builder.right
        } else {
            f64::NEG_INFINITY
        });
    let indented_alike =
        previous.x0 - margins.left > 0.8 * size && (segment.x0 - previous.x0).abs() < 0.3 * size;
    let short = !centred
        && (local_right - previous.x1 > needed
            || (indented_alike && margins.right - previous.x1 > needed));
    match segment.marker {
        lists::MarkerStrength::Strong => return false,
        lists::MarkerStrength::Weak
            if short
                || (segment.x0 - previous.x0).abs() > 0.5 * size
                || (first.marker != lists::MarkerStrength::None
                    && (segment.x0 - first.x0).abs() < 0.5 * size) =>
        {
            return false;
        }
        _ => {}
    }
    // Hanging indent (bibliography entries): continuation lines are indented, and a line back at
    // the first line's start begins the next entry.
    if builder.segments.len() >= 2
        && previous.x0 > first.x0 + 0.8 * size
        && segment.x0 < previous.x0 - 0.8 * size
        && segment.x0 <= first.x0 + 0.5 * size
    {
        return false;
    }
    if !centred && segment.x0 > previous.x0 + 0.8 * size {
        let hanging = first.text_x.is_some_and(|text_x| {
            first.marker != lists::MarkerStrength::None && (segment.x0 - text_x).abs() < size
        });
        if !hanging {
            return false;
        }
    }
    !short
}

fn finish(segments: &[Segment], members: &[usize], orientation: u8) -> Block {
    let mut bounds = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    let mut lines = Vec::with_capacity(members.len());
    let mut chars = 0;
    let mut bold_chars = 0;
    let mut sizes: Vec<(f64, usize)> = Vec::with_capacity(members.len());
    for &index in members {
        let segment = &segments[index];
        bounds[0] = bounds[0].min(segment.x0);
        bounds[1] = bounds[1].min(segment.top());
        bounds[2] = bounds[2].max(segment.x1);
        bounds[3] = bounds[3].max(segment.bottom());
        lines.push(segment.spans.clone());
        chars += segment.chars;
        bold_chars += segment.bold_chars;
        sizes.push((segment.size, segment.chars));
    }
    sizes.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut remaining = chars / 2;
    let mut size = sizes.last().map_or(0.0, |&(size, _)| size);
    for &(value, count) in &sizes {
        if count > remaining {
            size = value;
            break;
        }
        remaining -= count;
    }
    let corners = [
        to_display(orientation, bounds[0], bounds[1]),
        to_display(orientation, bounds[2], bounds[3]),
    ];
    let bbox = [
        corners[0].0.min(corners[1].0),
        corners[0].1.min(corners[1].1),
        corners[0].0.max(corners[1].0),
        corners[0].1.max(corners[1].1),
    ];
    Block {
        lines,
        bbox,
        size,
        bold: chars > 0 && bold_chars as f64 >= BOLD_SHARE * chars as f64,
        chars,
        upright: orientation == 0,
    }
}

#[cfg(test)]
mod tests {
    use super::{Block, layout_page};
    use crate::reader::pdf::interpreter::{FontStyle, Glyph, PageContent, SPACE};

    /// Places `text` as glyphs of `width` per character starting at `(x, y)`.
    fn place(page: &mut PageContent, text: &str, x: f64, y: f64, size: f64, font: u32) {
        let advance = size * 0.5;
        for (offset, ch) in text.chars().enumerate() {
            let start = page.text.len() as u32;
            let mut flags = 0;
            if ch == ' ' {
                flags |= SPACE;
            } else {
                page.text.push(ch);
            }
            page.glyphs.push(Glyph {
                x: x + offset as f64 * advance,
                y,
                ux: 1.0,
                uy: 0.0,
                width: advance,
                size,
                text_start: start,
                text_end: page.text.len() as u32,
                font,
                flags,
            });
        }
    }

    fn text(block: &Block) -> Vec<String> {
        block
            .lines
            .iter()
            .map(|line| line.iter().map(|span| span.text.as_str()).collect())
            .collect()
    }

    fn styles() -> Vec<FontStyle> {
        vec![
            FontStyle::default(),
            FontStyle {
                bold: true,
                italic: false,
            },
        ]
    }

    #[test]
    fn splits_paragraphs_by_gap_indent_and_short_lines() {
        let mut page = PageContent::default();
        place(&mut page, "Title here", 100.0, 60.0, 20.0, 1);
        let body = [
            ("alpha beta gamma delta epsilon zeta", 72.0, 100.0),
            ("eta theta iota kappa lambda mu nu xi", 72.0, 112.0),
            ("omicron pi.", 72.0, 124.0),
            ("rho sigma tau upsilon phi chi psi aa", 72.0, 136.0),
            ("omega end of the second paragraph.", 72.0, 148.0),
            ("  third starts with an indent here.", 72.0, 172.0),
        ];
        for (line, x, y) in body {
            place(&mut page, line, x, y, 10.0, 0);
        }
        let blocks = layout_page(&page, &styles(), 400.0, 400.0);
        let texts: Vec<Vec<String>> = blocks.iter().map(text).collect();
        assert_eq!(texts.len(), 4, "{texts:?}");
        assert_eq!(texts[0], vec!["Title here"]);
        assert!(blocks[0].bold);
        assert_eq!(texts[1].len(), 3);
        assert_eq!(texts[2].len(), 2);
        assert_eq!(texts[3], vec!["third starts with an indent here."]);
        assert!(blocks[1].bbox[0] >= 71.0 && blocks[1].bbox[1] < 100.0);
    }

    #[test]
    fn reads_two_columns_left_then_right() {
        let mut page = PageContent::default();
        place(
            &mut page,
            "A Spanning Title Over Both Columns",
            120.0,
            40.0,
            14.0,
            0,
        );
        for row in 0..6 {
            let y = 80.0 + f64::from(row) * 12.0;
            place(
                &mut page,
                &format!("left column line {row} with more words"),
                50.0,
                y,
                10.0,
                0,
            );
            place(
                &mut page,
                &format!("right column line {row} has more words"),
                300.0,
                y,
                10.0,
                0,
            );
        }
        let blocks = layout_page(&page, &styles(), 612.0, 792.0);
        let texts: Vec<String> = blocks.iter().map(|b| text(b).join("|")).collect();
        assert_eq!(texts.len(), 3, "{texts:?}");
        assert!(texts[0].starts_with("A Spanning"));
        assert!(
            texts[1].starts_with("left column line 0")
                && texts[1].ends_with("line 5 with more words")
        );
        assert!(texts[2].starts_with("right column line 0"));
    }

    #[test]
    fn keeps_words_whole_across_fonts_and_splits_on_gaps() {
        let mut page = PageContent::default();
        place(&mut page, "TH", 10.0, 50.0, 10.0, 0);
        place(&mut page, "Ủ", 20.0, 50.0, 10.0, 1);
        place(&mut page, "TƯỚNG", 32.0, 50.0, 10.0, 0);
        let blocks = layout_page(&page, &styles(), 200.0, 200.0);
        let line: String = blocks[0].lines[0].iter().map(|s| s.text.as_str()).collect();
        assert_eq!(line, "THỦ TƯỚNG");
    }

    #[test]
    fn detects_superscripts_and_combining_marks() {
        let mut page = PageContent::default();
        place(&mut page, "note", 10.0, 50.0, 10.0, 0);
        place(&mut page, "1", 30.0, 46.0, 6.0, 0);
        place(&mut page, "e", 40.0, 50.0, 10.0, 0);
        place(&mut page, "\u{0301}", 41.0, 50.0, 10.0, 0);
        let blocks = layout_page(&page, &styles(), 200.0, 200.0);
        let spans = &blocks[0].lines[0];
        assert_eq!(spans[0].text, "note");
        assert_eq!(spans[1].text, "1 ");
        assert_eq!(spans[1].script, super::Script::Super);
        assert_eq!(spans[2].text, "é");
    }

    #[test]
    fn rotated_text_keeps_reading_order_within_its_frame() {
        let mut page = PageContent::default();
        for (offset, ch) in "UP".chars().enumerate() {
            let start = page.text.len() as u32;
            page.text.push(ch);
            page.glyphs.push(Glyph {
                x: 20.0,
                y: 300.0 - offset as f64 * 6.0,
                ux: 0.0,
                uy: -1.0,
                width: 6.0,
                size: 10.0,
                text_start: start,
                text_end: page.text.len() as u32,
                font: 0,
                flags: 0,
            });
        }
        let blocks = layout_page(&page, &styles(), 400.0, 400.0);
        assert_eq!(text(&blocks[0]), vec!["UP"]);
        let [left, top, right, bottom] = blocks[0].bbox;
        assert!(left < 20.0 && right > 20.0 && top < 300.0 && bottom > 290.0);
    }
}
