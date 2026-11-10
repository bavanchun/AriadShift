//! Measured document fidelity, independent of planner route estimates and I/O.
//!
//! Exact NFC/whitespace-normalized semantic keys consume hypothesis observations
//! once. Block identities locate evidence within an import, never across imports.

use std::collections::{BTreeMap, VecDeque};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use unicode_normalization::UnicodeNormalization;

use crate::ir::{Block, BlockPosition, CharacterStyle, Document, Inline};

pub const METRIC_VERSION: &str = "fidelity/2";
const MAX_ITEMS: usize = 4096;
const MAX_TEXT_CHARS: usize = 100_000;
const MAX_CER_CELLS: usize = 4_000_000;
const STYLE_FIELDS: [&str; 7] = [
    "size_pt",
    "weight",
    "color",
    "font_family",
    "italic",
    "underline",
    "strike",
];

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FidelityBasis {
    RuntimeReadback,
    IndependentReference,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FidelityStatus {
    Measured,
    Unavailable,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Coverage {
    pub reference: usize,
    pub hypothesis: usize,
    pub matched: usize,
    pub observed: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct StructureScore {
    #[serde(flatten)]
    pub counts: Coverage,
    pub precision: Option<f64>,
    pub recall: Option<f64>,
    pub f1: Option<f64>,
}

/// A measurement, including the denominators behind every applicable component.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FidelityReport {
    pub metric_version: String,
    pub status: FidelityStatus,
    pub basis: FidelityBasis,
    pub score: Option<f64>,
    pub text_cer: Option<f64>,
    pub structure_f1: Option<f64>,
    pub structure_by_kind: BTreeMap<String, StructureScore>,
    pub style_accuracy: Option<f64>,
    pub layout_iou: Option<f64>,
    pub coverage: BTreeMap<String, Coverage>,
    pub unavailable_reasons: BTreeMap<String, String>,
}

impl FidelityReport {
    pub fn unavailable(basis: FidelityBasis, reason: impl Into<String>) -> Self {
        Self {
            metric_version: METRIC_VERSION.into(),
            status: FidelityStatus::Unavailable,
            basis,
            score: None,
            text_cer: None,
            structure_f1: None,
            structure_by_kind: BTreeMap::new(),
            style_accuracy: None,
            layout_iou: None,
            coverage: BTreeMap::new(),
            unavailable_reasons: BTreeMap::from([("report".into(), reason.into())]),
        }
    }
}

/// Portable observations consumed by Rust and Python conformance tests separately.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Observations {
    pub text: String,
    pub structures: Vec<Structure>,
    pub runs: Vec<StyleRun>,
    pub boxes: Vec<LayoutBox>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Structure {
    pub kind: String,
    pub key: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StyleRun {
    pub text: String,
    pub style: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LayoutBox {
    pub text: String,
    #[serde(flatten)]
    pub position: BlockPosition,
}

fn clean(text: &str) -> String {
    text.nfc()
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn pairs<T>(reference: &[T], hypothesis: &[T], key: impl Fn(&T) -> &str) -> Vec<(usize, usize)> {
    let mut indexes: BTreeMap<&str, VecDeque<usize>> = BTreeMap::new();
    for (i, item) in hypothesis.iter().enumerate() {
        indexes.entry(key(item)).or_default().push_back(i);
    }
    reference
        .iter()
        .enumerate()
        .filter_map(|(i, item)| {
            indexes
                .get_mut(key(item))
                .and_then(VecDeque::pop_front)
                .map(|j| (i, j))
        })
        .collect()
}

fn distance(a: &[char], b: &[char]) -> usize {
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ac) in a.iter().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, bc) in b.iter().enumerate() {
            let previous = row[j + 1];
            row[j + 1] = (diagonal + usize::from(ac != bc))
                .min(row[j] + 1)
                .min(previous + 1);
            diagonal = previous;
        }
    }
    row[b.len()]
}

fn style_equal(field: &str, a: &Value, b: Option<&Value>) -> bool {
    let Some(b) = b else { return false };
    match field {
        "size_pt" => a
            .as_f64()
            .zip(b.as_f64())
            .is_some_and(|(a, b)| (a - b).abs() <= 0.5),
        "color" => {
            let parse = |v: &Value| -> Option<[u8; 3]> {
                let s = v.as_str()?;
                if s.len() != 7 || !s.starts_with('#') || !s.is_ascii() {
                    return None;
                }
                Some([
                    u8::from_str_radix(&s[1..3], 16).ok()?,
                    u8::from_str_radix(&s[3..5], 16).ok()?,
                    u8::from_str_radix(&s[5..7], 16).ok()?,
                ])
            };
            parse(a)
                .zip(parse(b))
                .is_some_and(|(a, b)| a.iter().zip(b).all(|(a, b)| a.abs_diff(b) <= 2))
        }
        _ => a == b,
    }
}

fn area(position: &BlockPosition) -> f64 {
    let b = &position.bbox;
    if [b.left, b.top, b.right, b.bottom]
        .iter()
        .any(|v| !v.is_finite())
    {
        return 0.0;
    }
    let area = (b.right - b.left).max(0.0) * (b.bottom - b.top).max(0.0);
    if area.is_finite() { area } else { 0.0 }
}

fn iou(a: &BlockPosition, b: &BlockPosition) -> f64 {
    if a.page_no != b.page_no {
        return 0.0;
    }
    let (area_a, area_b) = (area(a), area(b));
    if area_a == 0.0 || area_b == 0.0 {
        return 0.0;
    }
    let (x, y) = (&a.bbox, &b.bbox);
    let intersection = (x.right.min(y.right) - x.left.max(y.left)).max(0.0)
        * (x.bottom.min(y.bottom) - x.top.max(y.top)).max(0.0);
    intersection / (area_a + area_b - intersection)
}

fn layout_pairs(reference: &[LayoutBox], hypothesis: &[LayoutBox]) -> Vec<(usize, usize)> {
    let mut by_page: BTreeMap<(&str, u32), VecDeque<usize>> = BTreeMap::new();
    for (j, item) in hypothesis.iter().enumerate() {
        by_page
            .entry((&item.text, item.position.page_no))
            .or_default()
            .push_back(j);
    }
    let mut used = vec![false; hypothesis.len()];
    let mut matched = Vec::new();
    let mut missing = Vec::new();
    for (i, item) in reference.iter().enumerate() {
        if let Some(j) = by_page
            .get_mut(&(item.text.as_str(), item.position.page_no))
            .and_then(VecDeque::pop_front)
        {
            matched.push((i, j));
            used[j] = true;
        } else {
            missing.push(i);
        }
    }
    let mut remaining: BTreeMap<&str, VecDeque<usize>> = BTreeMap::new();
    for (j, item) in hypothesis.iter().enumerate().filter(|(j, _)| !used[*j]) {
        remaining.entry(&item.text).or_default().push_back(j);
    }
    for i in missing {
        if let Some(j) = remaining
            .get_mut(reference[i].text.as_str())
            .and_then(VecDeque::pop_front)
        {
            matched.push((i, j));
        }
    }
    matched
}

/// Score captured observations with bounded work, preserving raw CER above one.
#[must_use]
pub fn score(
    reference: &Observations,
    hypothesis: &Observations,
    basis: FidelityBasis,
) -> FidelityReport {
    let (r, h) = (reference, hypothesis);
    if [
        r.structures.len(),
        h.structures.len(),
        r.runs.len(),
        h.runs.len(),
        r.boxes.len(),
        h.boxes.len(),
    ]
    .iter()
    .any(|n| *n > MAX_ITEMS)
    {
        return FidelityReport::unavailable(basis, "observation_limit");
    }
    if r.text.chars().take(MAX_TEXT_CHARS + 1).count() > MAX_TEXT_CHARS
        || h.text.chars().take(MAX_TEXT_CHARS + 1).count() > MAX_TEXT_CHARS
    {
        return FidelityReport::unavailable(basis, "text_work_limit");
    }
    let (rt, ht): (Vec<char>, Vec<char>) = (r.text.chars().collect(), h.text.chars().collect());
    let prefix = rt.iter().zip(&ht).take_while(|(a, b)| a == b).count();
    let (ra, ha) = (&rt[prefix..], &ht[prefix..]);
    let suffix = ra
        .iter()
        .rev()
        .zip(ha.iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let (ra, ha) = (&ra[..ra.len() - suffix], &ha[..ha.len() - suffix]);
    if rt.len().max(ht.len()) > MAX_TEXT_CHARS || ra.len().saturating_mul(ha.len()) > MAX_CER_CELLS
    {
        return FidelityReport::unavailable(basis, "text_work_limit");
    }
    let edits = distance(ra, ha);
    let cer = if rt.is_empty() {
        f64::from(!ht.is_empty())
    } else {
        edits as f64 / rt.len() as f64
    };
    let mut result = FidelityReport {
        metric_version: METRIC_VERSION.into(),
        status: FidelityStatus::Measured,
        basis,
        score: None,
        text_cer: Some(cer),
        structure_f1: None,
        structure_by_kind: BTreeMap::new(),
        style_accuracy: None,
        layout_iou: None,
        coverage: BTreeMap::new(),
        unavailable_reasons: BTreeMap::new(),
    };
    result.coverage.insert(
        "text".into(),
        Coverage {
            reference: rt.len(),
            hypothesis: ht.len(),
            matched: rt.len().saturating_sub(edits),
            observed: ht.len(),
        },
    );
    let mut components = vec![(1.0 - cer).max(0.0)];
    let mut f1s = Vec::new();
    for kind in ["heading", "list", "table"] {
        let rs: Vec<_> = r
            .structures
            .iter()
            .filter(|s| s.kind == kind)
            .cloned()
            .collect();
        let hs: Vec<_> = h
            .structures
            .iter()
            .filter(|s| s.kind == kind)
            .cloned()
            .collect();
        let matched = pairs(&rs, &hs, |s| &s.key).len();
        let f1 =
            (rs.len() + hs.len() > 0).then(|| 2.0 * matched as f64 / (rs.len() + hs.len()) as f64);
        if let Some(f1) = f1 {
            f1s.push(f1);
        }
        result.structure_by_kind.insert(
            kind.into(),
            StructureScore {
                counts: Coverage {
                    reference: rs.len(),
                    hypothesis: hs.len(),
                    matched,
                    observed: hs.len(),
                },
                precision: (!hs.is_empty()).then(|| matched as f64 / hs.len() as f64),
                recall: (!rs.is_empty()).then(|| matched as f64 / rs.len() as f64),
                f1,
            },
        );
    }
    if f1s.is_empty() {
        result
            .unavailable_reasons
            .insert("structure".into(), "no_structure_observations".into());
    } else {
        let f1 = f1s.iter().sum::<f64>() / f1s.len() as f64;
        result.structure_f1 = Some(f1);
        components.push(f1);
    }
    let run_pairs: BTreeMap<_, _> = pairs(&r.runs, &h.runs, |s| &s.text).into_iter().collect();
    let hypothesis_attributes = h
        .runs
        .iter()
        .map(|run| {
            STYLE_FIELDS
                .iter()
                .filter(|f| run.style.get(**f).is_some_and(|v| !v.is_null()))
                .count()
        })
        .sum();
    let mut style = Coverage {
        hypothesis: hypothesis_attributes,
        ..Coverage::default()
    };
    for (i, run) in r.runs.iter().enumerate() {
        for field in STYLE_FIELDS {
            if let Some(value) = run.style.get(field).filter(|v| !v.is_null()) {
                style.reference += 1;
                let other = run_pairs.get(&i).and_then(|j| h.runs[*j].style.get(field));
                style.observed += usize::from(other.is_some_and(|v| !v.is_null()));
                style.matched += usize::from(style_equal(field, value, other));
            }
        }
    }
    if style.reference > 0 {
        let accuracy = style.matched as f64 / style.reference as f64;
        result.style_accuracy = Some(accuracy);
        components.push(accuracy);
    } else {
        result
            .unavailable_reasons
            .insert("style".into(), "no_reference_style_observations".into());
    }
    result.coverage.insert("style".into(), style);
    let box_pairs = layout_pairs(&r.boxes, &h.boxes);
    result.coverage.insert(
        "layout".into(),
        Coverage {
            reference: r.boxes.len(),
            hypothesis: h.boxes.len(),
            matched: box_pairs
                .iter()
                .filter(|(i, j)| r.boxes[*i].position.page_no == h.boxes[*j].position.page_no)
                .count(),
            observed: box_pairs.len(),
        },
    );
    if r.boxes.is_empty() {
        result
            .unavailable_reasons
            .insert("layout".into(), "no_reference_layout_observations".into());
    } else {
        let layout = box_pairs
            .iter()
            .map(|(i, j)| iou(&r.boxes[*i].position, &h.boxes[*j].position))
            .sum::<f64>()
            / r.boxes.len() as f64;
        result.layout_iou = Some(layout);
        components.push(layout);
        if r.boxes
            .iter()
            .chain(&h.boxes)
            .any(|b| area(&b.position) == 0.0)
        {
            result.unavailable_reasons.insert(
                "layout_geometry".into(),
                "degenerate_box_scores_zero".into(),
            );
        }
    }
    result.score = Some(components.iter().sum::<f64>() / components.len() as f64);
    result
}

fn children(block: &Block) -> Vec<&Block> {
    match block {
        Block::Quote { blocks, .. } | Block::Footnote { blocks, .. } => {
            blocks.iter().take(MAX_ITEMS + 1).collect()
        }
        Block::List { items, .. } => items
            .iter()
            .flat_map(|i| &i.blocks)
            .take(MAX_ITEMS + 1)
            .collect(),
        Block::Table { head, body, .. } => head
            .iter()
            .chain(body)
            .flatten()
            .flat_map(|c| &c.blocks)
            .take(MAX_ITEMS + 1)
            .collect(),
        _ => Vec::new(),
    }
}

fn content(block: &Block) -> Option<&[Inline]> {
    match block {
        Block::Heading { content, .. } | Block::Paragraph { content, .. } => Some(content),
        Block::Table { caption, .. } => Some(caption.as_deref().unwrap_or_default()),
        Block::Figure { caption, .. } => Some(caption),
        _ => None,
    }
}

fn inline_text(nodes: &[Inline]) -> String {
    nodes
        .iter()
        .map(|n| match n {
            Inline::Text { text } | Inline::Code { text } | Inline::Raw { text, .. } => {
                text.clone()
            }
            Inline::Math { tex, .. } => tex.clone(),
            Inline::Image { alt, .. } => alt.clone(),
            Inline::SoftBreak {} | Inline::LineBreak {} => " ".into(),
            Inline::FootnoteRef { .. } => String::new(),
            Inline::Styled { content, .. }
            | Inline::Emph { content }
            | Inline::Strong { content }
            | Inline::Strikeout { content }
            | Inline::Superscript { content }
            | Inline::Subscript { content }
            | Inline::Link { content, .. } => inline_text(content),
        })
        .collect()
}

fn block_text(block: &Block) -> String {
    if let Block::Table { caption, .. } = block {
        return clean(
            &std::iter::once(inline_text(caption.as_deref().unwrap_or_default()))
                .chain(children(block).iter().map(|b| block_text(b)))
                .collect::<Vec<_>>()
                .join(" "),
        );
    }
    if let Some(nodes) = content(block) {
        return clean(&inline_text(nodes));
    }
    match block {
        Block::Code { text, .. } | Block::Raw { text, .. } => clean(text),
        Block::Math { tex, .. } => clean(tex),
        _ => clean(
            &children(block)
                .iter()
                .map(|b| block_text(b))
                .collect::<Vec<_>>()
                .join(" "),
        ),
    }
}

fn observed_style(style: &CharacterStyle) -> BTreeMap<String, Value> {
    let mut result = BTreeMap::new();
    let value = serde_json::to_value(style).expect("finite validated character style");
    for field in STYLE_FIELDS {
        if let Some(value) = value.get(field) {
            result.insert(field.into(), value.clone());
        }
    }
    if let Some(font) = &style.font {
        result.insert("font_family".into(), json!(font.family));
    }
    result
}

fn runs(
    nodes: &[Inline],
    parent: &CharacterStyle,
    doc: &Document,
    out: &mut Vec<StyleRun>,
) -> Result<(), String> {
    for node in nodes {
        let mut style = parent.clone();
        let nested = match node {
            Inline::Styled {
                style: named,
                direct,
                content,
            } => {
                style = doc
                    .styles
                    .resolve_character(named.as_ref(), direct, Some(parent))?;
                Some(content)
            }
            Inline::Strong { content } => {
                style.weight = Some(700);
                Some(content)
            }
            Inline::Emph { content } => {
                style.italic = Some(true);
                Some(content)
            }
            Inline::Strikeout { content } => {
                style.strike = Some(true);
                Some(content)
            }
            Inline::Superscript { content }
            | Inline::Subscript { content }
            | Inline::Link { content, .. } => Some(content),
            _ => None,
        };
        if let Some(nested) = nested {
            runs(nested, &style, doc, out)?;
        } else {
            let text = inline_text(std::slice::from_ref(node));
            let style = observed_style(&style);
            if let Some(previous) = out.last_mut().filter(|r| r.style == style) {
                previous.text.push_str(&text);
            } else {
                out.push(StyleRun { text, style });
            }
        }
    }
    Ok(())
}

/// Extract effective character styles, structure and top-left point geometry.
pub fn observe(document: &Document) -> Result<Observations, String> {
    if document.body.len() > MAX_ITEMS {
        return Err("observation_limit".into());
    }
    // Bound the input walk before normalization, semantic keys or run copies allocate.
    let mut pending: Vec<(&Block, usize)> = document.body.iter().map(|block| (block, 0)).collect();
    let mut nodes = 0;
    let mut characters = 0;
    let mut count_text = |text: &str| -> Result<(), String> {
        characters += text.chars().take(MAX_TEXT_CHARS + 1 - characters).count();
        if characters > MAX_TEXT_CHARS {
            Err("text_work_limit".into())
        } else {
            Ok(())
        }
    };
    while let Some((block, depth)) = pending.pop() {
        if depth > 64 {
            return Err("observation_limit".into());
        }
        nodes += 1;
        match block {
            Block::Table { head, body, .. } => {
                nodes += head
                    .iter()
                    .chain(body)
                    .take(MAX_ITEMS + 1)
                    .map(Vec::len)
                    .sum::<usize>()
            }
            Block::List { items, .. } => nodes += items.len(),
            _ => {}
        }
        if nodes > MAX_ITEMS {
            return Err("observation_limit".into());
        }
        let nested = children(block);
        if nodes + pending.len() + nested.len() > MAX_ITEMS {
            return Err("observation_limit".into());
        }
        pending.extend(nested.into_iter().map(|block| (block, depth + 1)));
        let content = content(block).unwrap_or_default();
        if content.len() > MAX_ITEMS {
            return Err("observation_limit".into());
        }
        let mut inline: Vec<(&Inline, usize)> =
            content.iter().map(|item| (item, depth + 1)).collect();
        if let Block::Table { footnotes, .. } = block {
            if inline.len() + footnotes.len() > MAX_ITEMS {
                return Err("observation_limit".into());
            }
            inline.extend(footnotes.iter().map(|item| (item, depth + 1)));
        }
        match block {
            Block::Code { text, .. } | Block::Raw { text, .. } => count_text(text)?,
            Block::Math { tex, .. } => count_text(tex)?,
            _ => {}
        }
        while let Some((item, inline_depth)) = inline.pop() {
            if inline_depth > 64 {
                return Err("observation_limit".into());
            }
            nodes += 1;
            let content = match item {
                Inline::Styled { content, .. }
                | Inline::Strong { content }
                | Inline::Emph { content }
                | Inline::Strikeout { content }
                | Inline::Superscript { content }
                | Inline::Subscript { content }
                | Inline::Link { content, .. } => content.as_slice(),
                Inline::Text { text } | Inline::Code { text } | Inline::Raw { text, .. } => {
                    count_text(text)?;
                    &[]
                }
                Inline::Math { tex, .. } => {
                    count_text(tex)?;
                    &[]
                }
                Inline::Image { alt, .. } => {
                    count_text(alt)?;
                    &[]
                }
                _ => &[],
            };
            if nodes + inline.len() + content.len() > MAX_ITEMS {
                return Err("observation_limit".into());
            }
            inline.extend(content.iter().map(|item| (item, inline_depth + 1)));
        }
    }
    fn walk(
        blocks: Vec<&Block>,
        inherited: (&CharacterStyle, Option<&crate::ir::ParagraphStyle>),
        doc: &Document,
        out: &mut Observations,
        pieces: &mut Vec<String>,
        headings: &mut Vec<u8>,
        depth: usize,
    ) -> Result<(), String> {
        let (parent, baseline) = inherited;
        if depth > 64
            || out
                .structures
                .len()
                .max(out.runs.len())
                .max(out.boxes.len())
                > MAX_ITEMS
        {
            return Err("observation_limit".into());
        }
        for block in blocks {
            let (para, mut style) = if matches!(block, Block::Table { .. }) {
                let table = doc
                    .styles
                    .resolve_table(block.style().as_ref(), block.direct(), 64)?;
                let mut style =
                    doc.styles
                        .resolve_character(None, &CharacterStyle::default(), Some(parent))?;
                style.overlay(&table.character);
                (table.paragraph, style)
            } else {
                (
                    doc.styles.resolve_paragraph_with_baseline(
                        block.style().as_ref(),
                        block.direct(),
                        baseline,
                        64,
                    )?,
                    doc.styles
                        .resolve_character(None, &CharacterStyle::default(), Some(parent))?,
                )
            };
            let child_baseline = if matches!(block, Block::Table { .. }) {
                Some(para.clone())
            } else {
                baseline.cloned()
            };
            if para.font.is_some() {
                style.font = para.font;
            }
            if para.size_pt.is_some() {
                style.size_pt = para.size_pt;
            }
            if para.color.is_some() {
                style.color = para.color;
            }
            let text = block_text(block);
            let structure = match block {
                Block::Heading { level, .. } => {
                    while headings.last().is_some_and(|h| h >= level) {
                        headings.pop();
                    }
                    headings.push(*level);
                    Some(("heading", json!([headings, text])))
                }
                Block::List { ordered, items, .. } => Some((
                    "list",
                    json!([
                        ordered,
                        items
                            .iter()
                            .map(|i| clean(
                                &i.blocks
                                    .iter()
                                    .map(block_text)
                                    .collect::<Vec<_>>()
                                    .join(" ")
                            ))
                            .collect::<Vec<_>>()
                    ]),
                )),
                Block::Table { head, body, .. } => {
                    let rows: Vec<Vec<Value>> = [(true, head), (false, body)]
                        .into_iter()
                        .flat_map(|(header, rows)| {
                            rows.iter().map(move |row| {
                                row.iter()
                                    .map(|c| {
                                        json!([
                                            c.rowspan,
                                            c.colspan,
                                            c.header.unwrap_or(header),
                                            clean(
                                                &c.blocks
                                                    .iter()
                                                    .map(block_text)
                                                    .collect::<Vec<_>>()
                                                    .join(" ")
                                            )
                                        ])
                                    })
                                    .collect()
                            })
                        })
                        .collect();
                    Some(("table", json!(rows)))
                }
                _ => None,
            };
            if let Some((kind, key)) = structure {
                out.structures.push(Structure {
                    kind: kind.into(),
                    key: key.to_string(),
                });
            }
            if let Some(positions) = doc
                .source_map
                .get(block.id())
                .filter(|positions| !positions.is_empty())
            {
                out.boxes.extend(positions.iter().map(|position| LayoutBox {
                    text: text.clone(),
                    position: position.clone(),
                }));
            } else if let Some(position) =
                doc.layout.as_ref().and_then(|l| l.blocks.get(block.id()))
            {
                out.boxes.push(LayoutBox {
                    text: text.clone(),
                    position: position.clone(),
                });
            }
            if let Some(nodes) = content(block) {
                let inline_value = clean(&inline_text(nodes));
                if !inline_value.is_empty() {
                    pieces.push(inline_value);
                }
                let mut block_runs = Vec::new();
                runs(nodes, &style, doc, &mut block_runs)?;
                out.runs.extend(block_runs.into_iter().filter_map(|mut r| {
                    r.text = clean(&r.text);
                    (!r.text.is_empty()).then_some(r)
                }));
            } else if matches!(
                block,
                Block::Code { .. } | Block::Math { .. } | Block::Raw { .. }
            ) && !text.is_empty()
            {
                pieces.push(text);
            }
            walk(
                children(block),
                (&style, child_baseline.as_ref()),
                doc,
                out,
                pieces,
                headings,
                depth + 1,
            )?;
            if let Block::Table { footnotes, .. } = block {
                let text = clean(&inline_text(footnotes));
                if !text.is_empty() {
                    pieces.push(text);
                }
                let mut footnote_runs = Vec::new();
                runs(footnotes, &style, doc, &mut footnote_runs)?;
                out.runs
                    .extend(footnote_runs.into_iter().filter_map(|mut r| {
                        r.text = clean(&r.text);
                        (!r.text.is_empty()).then_some(r)
                    }));
            }
            if pieces.iter().map(String::len).sum::<usize>() > MAX_TEXT_CHARS * 4 {
                return Err("text_work_limit".into());
            }
        }
        Ok(())
    }
    let mut out = Observations::default();
    let mut pieces = Vec::new();
    walk(
        document.body.iter().collect(),
        (&CharacterStyle::default(), None),
        document,
        &mut out,
        &mut pieces,
        &mut Vec::new(),
        0,
    )?;
    out.text = pieces.join("\n");
    Ok(out)
}

/// Compare real document observations without filesystem or engine dependencies.
#[must_use]
pub fn compare(
    reference: &Document,
    hypothesis: &Document,
    basis: FidelityBasis,
) -> FidelityReport {
    match (observe(reference), observe(hypothesis)) {
        (Ok(r), Ok(h)) => score(&r, &h, basis),
        (Err(reason), _) | (_, Err(reason)) => FidelityReport::unavailable(basis, reason),
    }
}
