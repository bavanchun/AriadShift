//! Ruled-table detection.
//!
//! Horizontal and vertical rules painted by a page (see [`super::rules`]) are merged into lines,
//! lines that touch form connected components, and a component whose lines make a closed grid is
//! a table candidate. Cells come from the grid intersections; a cell edge that carries no rule
//! joins its two neighbours into one rectangular merged cell. Glyphs are assigned to cells by
//! their centre point.
//!
//! Two tables that share a border are one connected component, so they read as one table; with
//! aligned columns that is indistinguishable from one table with a heavy rule between rows, and
//! the detector does not split them. Tables with a gap between them stay separate.
//!
//! The detector favours precision. A candidate is dropped, never repaired, when its outer border
//! is not closed, when a line dangles past the grid, when merged cells are not rectangles, when it
//! covers most of the page, when most of its text is not upright, when fewer than two rows hold
//! two or more plain cells (the shape of a page frame with header, footer and column rules), when
//! too few cells hold text, or when text crosses cell borders. Tables without rules are not
//! detected.

use super::{
    interpreter::{Glyph, PageContent, SPACE},
    rules::{Rule, RulePaint, RuleShape},
};

/// Distance, in points, within which positions count as the same line and lines count as touching.
const TOL: f64 = 2.0;

/// Smallest row height or column width, in points, of a candidate grid.
const MIN_CELL: f64 = 4.0;

/// Shortest rule, in points, that can belong to a grid.
const MIN_RULE: f64 = 3.0;

/// Most merged horizontal or vertical lines on a page. More means a drawing, not a table, and
/// bounds the pairwise intersection work.
const MAX_LINES: usize = 800;

/// Most tables kept for one page.
const MAX_TABLES: usize = 32;

/// Most grid cells, merged or not, for one page.
const MAX_CELLS: usize = 4_096;

/// Largest grid, in rows and columns.
const MAX_ROWS: usize = 200;
const MAX_COLUMNS: usize = 40;

/// Share of the page area above which a candidate is a page frame or a layout grid, not a table.
const MAX_PAGE_SHARE: f64 = 0.5;

/// Fewest rows of a grid that must hold at least [`MIN_PLAIN_CELLS`] cells spanning one row and
/// one column. Header, footer and column rules around prose merge every other row into one cell.
const MIN_PLAIN_ROWS: usize = 2;
const MIN_PLAIN_CELLS: usize = 2;

/// Fewest cells of a grid that must hold text, as a share of its merged cells.
const MIN_FILLED_SHARE: f64 = 0.3;

/// Share of glyphs allowed to cross their cell border before the candidate is dropped.
const MAX_STRADDLE_SHARE: f64 = 0.05;

/// How much thicker than the median rule the rule under the first row must be to mark a head.
const HEAD_RULE_RATIO: f64 = 1.5;

/// A table found on a page, in display space.
#[derive(Debug)]
pub(super) struct Grid {
    /// `[left, top, right, bottom]`.
    pub(super) bbox: [f64; 4],
    pub(super) rows: usize,
    pub(super) columns: usize,
    /// Merged cells in reading order.
    pub(super) cells: Vec<Cell>,
    /// Whether the rule under the first row is distinctly thicker than the others.
    pub(super) heavy_head_rule: bool,
}

/// A merged cell of a [`Grid`].
#[derive(Debug)]
pub(super) struct Cell {
    pub(super) row: usize,
    pub(super) column: usize,
    pub(super) rowspan: usize,
    pub(super) colspan: usize,
    /// Indices into [`PageContent::glyphs`], ascending.
    pub(super) glyphs: Vec<usize>,
}

/// Outcome of scanning one page.
#[derive(Debug, Default)]
pub(super) struct Detected {
    pub(super) grids: Vec<Grid>,
    /// A limit stopped detection on this page, so some tables may be missing.
    pub(super) capped: bool,
}

/// A merged rule: `pos` is the y of a horizontal line or the x of a vertical one.
#[derive(Clone, Copy, Debug)]
struct Line {
    pos: f64,
    lo: f64,
    hi: f64,
    width: f64,
}

pub(super) fn detect(content: &PageContent, page_width: f64, page_height: f64) -> Detected {
    let mut detected = Detected::default();
    if content.rules.is_empty() || content.glyphs.is_empty() {
        return detected;
    }
    let (horizontal, vertical) = lines_of(&content.rules, page_width, page_height);
    let (horizontal, vertical) = (merge(horizontal), merge(vertical));
    if horizontal.len() > MAX_LINES || vertical.len() > MAX_LINES {
        detected.capped = true;
        return detected;
    }
    let mut cells_left = MAX_CELLS;
    let mut glyph_index = None;
    for (h_set, v_set) in components(&horizontal, &vertical) {
        if detected.grids.len() >= MAX_TABLES {
            detected.capped = true;
            break;
        }
        let Some(layout) = build_grid(&h_set, &v_set) else {
            continue;
        };
        if covers_most_of_page(&layout, page_width, page_height) {
            continue;
        }
        if layout.cells.len() > cells_left {
            detected.capped = true;
            continue;
        }
        let index = glyph_index.get_or_insert_with(|| GlyphIndex::new(content));
        let Some(grid) = fill_grid(&layout, content, index) else {
            continue;
        };
        if detected
            .grids
            .iter()
            .any(|other| overlaps(&other.bbox, &grid.bbox))
        {
            continue;
        }
        cells_left -= layout.cells.len();
        detected.grids.push(grid);
    }
    detected
        .grids
        .sort_by(|a, b| a.bbox[1].total_cmp(&b.bbox[1]));
    detected
}

fn covers_most_of_page(layout: &Layout, page_width: f64, page_height: f64) -> bool {
    let width = layout.xs[layout.xs.len() - 1] - layout.xs[0];
    let height = layout.ys[layout.ys.len() - 1] - layout.ys[0];
    width * height > MAX_PAGE_SHARE * page_width * page_height
}

fn overlaps(a: &[f64; 4], b: &[f64; 4]) -> bool {
    a[0] < b[2] && b[0] < a[2] && a[1] < b[3] && b[1] < a[3]
}

/// Splits rules into horizontal and vertical lines. Rectangles that cover nearly the whole page
/// are page frames, not cell borders, and are ignored.
fn lines_of(rules: &[Rule], page_width: f64, page_height: f64) -> (Vec<Line>, Vec<Line>) {
    let mut horizontal = Vec::new();
    let mut vertical = Vec::new();
    let mut add = |x0: f64, y0: f64, x1: f64, y1: f64, width: f64| {
        let (w, h) = (x1 - x0, y1 - y0);
        if h <= w {
            if w >= MIN_RULE {
                horizontal.push(Line {
                    pos: f64::midpoint(y0, y1),
                    lo: x0,
                    hi: x1,
                    width: width.max(h),
                });
            }
        } else if h >= MIN_RULE {
            vertical.push(Line {
                pos: f64::midpoint(x0, x1),
                lo: y0,
                hi: y1,
                width: width.max(w),
            });
        }
    };
    for rule in rules {
        match (rule.shape, rule.paint) {
            (RuleShape::Segment, _) | (RuleShape::Rect, RulePaint::Fill) => {
                add(rule.x0, rule.y0, rule.x1, rule.y1, rule.width);
            }
            (RuleShape::Rect, _) => {
                if rule.x1 - rule.x0 >= 0.9 * page_width && rule.y1 - rule.y0 >= 0.9 * page_height {
                    continue;
                }
                let w = rule.width;
                add(rule.x0, rule.y0, rule.x1, rule.y0, w);
                add(rule.x0, rule.y1, rule.x1, rule.y1, w);
                add(rule.x0, rule.y0, rule.x0, rule.y1, w);
                add(rule.x1, rule.y0, rule.x1, rule.y1, w);
            }
        }
    }
    (horizontal, vertical)
}

/// Joins lines on one position whose extents overlap or nearly touch.
fn merge(mut lines: Vec<Line>) -> Vec<Line> {
    lines.retain(|line| line.pos.is_finite() && line.lo.is_finite() && line.hi.is_finite());
    lines.sort_by(|a, b| a.pos.total_cmp(&b.pos));
    let mut merged: Vec<Line> = Vec::new();
    let mut cluster: Vec<Line> = Vec::new();
    let flush = |cluster: &mut Vec<Line>, merged: &mut Vec<Line>| {
        cluster.sort_by(|a, b| a.lo.total_cmp(&b.lo));
        let pos = cluster.iter().map(|line| line.pos).sum::<f64>() / cluster.len().max(1) as f64;
        let mut current: Option<Line> = None;
        for line in cluster.drain(..) {
            match &mut current {
                Some(open) if line.lo <= open.hi + TOL => {
                    open.hi = open.hi.max(line.hi);
                    open.width = open.width.max(line.width);
                }
                _ => {
                    if let Some(done) = current.take() {
                        merged.push(done);
                    }
                    current = Some(Line { pos, ..line });
                }
            }
        }
        merged.extend(current);
    };
    let mut last_pos = f64::NEG_INFINITY;
    for line in lines {
        if !cluster.is_empty() && line.pos - last_pos > TOL {
            flush(&mut cluster, &mut merged);
        }
        last_pos = line.pos;
        cluster.push(line);
    }
    if !cluster.is_empty() {
        flush(&mut cluster, &mut merged);
    }
    merged
}

fn touches(h: &Line, v: &Line) -> bool {
    v.pos >= h.lo - TOL && v.pos <= h.hi + TOL && h.pos >= v.lo - TOL && h.pos <= v.hi + TOL
}

/// Connected sets of horizontal and vertical lines that touch each other.
fn components(horizontal: &[Line], vertical: &[Line]) -> Vec<(Vec<Line>, Vec<Line>)> {
    let count = horizontal.len() + vertical.len();
    let mut parent: Vec<usize> = (0..count).collect();
    for (i, h) in horizontal.iter().enumerate() {
        for (j, v) in vertical.iter().enumerate() {
            if touches(h, v) {
                union(&mut parent, i, horizontal.len() + j);
            }
        }
    }
    let mut sets: std::collections::BTreeMap<usize, (Vec<Line>, Vec<Line>)> = Default::default();
    for (i, line) in horizontal.iter().enumerate() {
        sets.entry(find(&mut parent, i)).or_default().0.push(*line);
    }
    for (j, line) in vertical.iter().enumerate() {
        let root = find(&mut parent, horizontal.len() + j);
        sets.entry(root).or_default().1.push(*line);
    }
    sets.into_values()
        .filter(|(h, v)| h.len() >= 3 && v.len() >= 3)
        .collect()
}

fn find(parent: &mut [usize], mut node: usize) -> usize {
    while parent[node] != node {
        parent[node] = parent[parent[node]];
        node = parent[node];
    }
    node
}

fn union(parent: &mut [usize], a: usize, b: usize) {
    let (a, b) = (find(parent, a), find(parent, b));
    if a != b {
        parent[a.max(b)] = a.min(b);
    }
}

/// Positions of the lines, with values closer than [`TOL`] counted once.
fn positions(lines: &[Line]) -> Vec<f64> {
    let mut all: Vec<f64> = lines.iter().map(|line| line.pos).collect();
    all.sort_by(f64::total_cmp);
    let mut out: Vec<f64> = Vec::new();
    for value in all {
        match out.last() {
            Some(last) if value - last <= TOL => {}
            _ => out.push(value),
        }
    }
    out
}

/// The rule geometry of a candidate before text is assigned.
struct Layout {
    xs: Vec<f64>,
    ys: Vec<f64>,
    cells: Vec<Cell>,
    heavy_head_rule: bool,
}

/// Whether some line at `pos` covers `lo..=hi`.
fn covered(lines: &[Line], pos: f64, lo: f64, hi: f64) -> bool {
    lines
        .iter()
        .any(|line| (line.pos - pos).abs() <= TOL && line.lo <= lo + TOL && line.hi >= hi - TOL)
}

fn build_grid(horizontal: &[Line], vertical: &[Line]) -> Option<Layout> {
    let xs = positions(vertical);
    let ys = positions(horizontal);
    let (columns, rows) = (xs.len().checked_sub(1)?, ys.len().checked_sub(1)?);
    if rows < 2 || columns < 2 || rows > MAX_ROWS || columns > MAX_COLUMNS {
        return None;
    }
    if xs.windows(2).any(|w| w[1] - w[0] < MIN_CELL)
        || ys.windows(2).any(|w| w[1] - w[0] < MIN_CELL)
    {
        return None;
    }
    let (left, right) = (xs[0], xs[columns]);
    let (top, bottom) = (ys[0], ys[rows]);
    // A line that runs past the outer border is not part of this grid.
    if horizontal
        .iter()
        .any(|line| line.lo < left - TOL || line.hi > right + TOL)
        || vertical
            .iter()
            .any(|line| line.lo < top - TOL || line.hi > bottom + TOL)
    {
        return None;
    }
    let h_edge = |r: usize, c: usize| covered(horizontal, ys[r], xs[c], xs[c + 1]);
    let v_edge = |r: usize, c: usize| covered(vertical, xs[c], ys[r], ys[r + 1]);
    if (0..columns).any(|c| !h_edge(0, c) || !h_edge(rows, c))
        || (0..rows).any(|r| !v_edge(r, 0) || !v_edge(r, columns))
    {
        return None;
    }

    let mut parent: Vec<usize> = (0..rows * columns).collect();
    for r in 0..rows {
        for c in 0..columns {
            if r > 0 && !h_edge(r, c) {
                union(&mut parent, (r - 1) * columns + c, r * columns + c);
            }
            if c > 0 && !v_edge(r, c) {
                union(&mut parent, r * columns + c - 1, r * columns + c);
            }
        }
    }
    // Every merged group must fill its bounding rectangle exactly.
    let mut groups: std::collections::BTreeMap<usize, [usize; 5]> = Default::default();
    for r in 0..rows {
        for c in 0..columns {
            let root = find(&mut parent, r * columns + c);
            let entry = groups.entry(root).or_insert([r, c, r, c, 0]);
            entry[0] = entry[0].min(r);
            entry[1] = entry[1].min(c);
            entry[2] = entry[2].max(r);
            entry[3] = entry[3].max(c);
            entry[4] += 1;
        }
    }
    let mut cells = Vec::with_capacity(groups.len());
    for [r0, c0, r1, c1, count] in groups.into_values() {
        if count != (r1 - r0 + 1) * (c1 - c0 + 1) {
            return None;
        }
        cells.push(Cell {
            row: r0,
            column: c0,
            rowspan: r1 - r0 + 1,
            colspan: c1 - c0 + 1,
            glyphs: Vec::new(),
        });
    }
    if cells.len() < 4 {
        return None;
    }
    cells.sort_by_key(|cell| (cell.row, cell.column));
    // A row that starts no cell is a full-width merge, which tables do not use.
    if (0..rows).any(|r| cells.iter().all(|cell| cell.row != r)) {
        return None;
    }
    let plain_rows = (0..rows)
        .filter(|&r| {
            let plain = cells
                .iter()
                .filter(|cell| cell.row == r && cell.rowspan == 1 && cell.colspan == 1);
            plain.count() >= MIN_PLAIN_CELLS
        })
        .count();
    if plain_rows < MIN_PLAIN_ROWS {
        return None;
    }

    let mut widths: Vec<f64> = horizontal.iter().map(|line| line.width).collect();
    widths.sort_by(f64::total_cmp);
    let median = widths[widths.len() / 2];
    let under_head = horizontal
        .iter()
        .filter(|line| (line.pos - ys[1]).abs() <= TOL)
        .map(|line| line.width)
        .fold(0.0, f64::max);
    Some(Layout {
        xs,
        ys,
        cells,
        heavy_head_rule: median > 0.0 && under_head >= HEAD_RULE_RATIO * median,
    })
}

/// Where a glyph is centred, in display space.
fn centre(glyph: &Glyph) -> (f64, f64) {
    let half = glyph.width / 2.0;
    (
        glyph.x + glyph.ux * half + glyph.uy * 0.3 * glyph.size,
        glyph.y + glyph.uy * half - glyph.ux * 0.3 * glyph.size,
    )
}

/// Index of the interval of `edges` that holds `value`.
fn interval(edges: &[f64], value: f64) -> Option<usize> {
    let first = *edges.first()?;
    let last = *edges.last()?;
    if value < first || value >= last {
        return None;
    }
    Some(edges.partition_point(|&edge| edge <= value) - 1)
}

/// Glyph centres, with the glyphs ordered by x so that a grid only visits the glyphs in its box.
struct GlyphIndex {
    centres: Vec<(f64, f64)>,
    by_x: Vec<usize>,
}

impl GlyphIndex {
    fn new(content: &PageContent) -> Self {
        let centres: Vec<(f64, f64)> = content.glyphs.iter().map(centre).collect();
        let mut by_x: Vec<usize> = (0..centres.len())
            .filter(|&i| centres[i].0.is_finite() && centres[i].1.is_finite())
            .collect();
        by_x.sort_by(|&a, &b| centres[a].0.total_cmp(&centres[b].0));
        Self { centres, by_x }
    }

    /// Glyphs centred in `[left, right) x [top, bottom)`, in ascending glyph order.
    fn within(&self, [left, top, right, bottom]: [f64; 4]) -> Vec<usize> {
        let from = self.by_x.partition_point(|&i| self.centres[i].0 < left);
        let to = self.by_x.partition_point(|&i| self.centres[i].0 < right);
        let mut found: Vec<usize> = self.by_x[from..to]
            .iter()
            .copied()
            .filter(|&i| (top..bottom).contains(&self.centres[i].1))
            .collect();
        found.sort_unstable();
        found
    }
}

/// Assigns glyphs to cells and applies the text checks.
fn fill_grid(layout: &Layout, content: &PageContent, index: &GlyphIndex) -> Option<Grid> {
    let Layout {
        xs,
        ys,
        cells,
        heavy_head_rule,
    } = layout;
    let columns = xs.len() - 1;
    // Cell index of each grid slot.
    let mut slot = vec![0_usize; (ys.len() - 1) * columns];
    for (index, cell) in cells.iter().enumerate() {
        for r in cell.row..cell.row + cell.rowspan {
            for c in cell.column..cell.column + cell.colspan {
                slot[r * columns + c] = index;
            }
        }
    }
    let mut filled: Vec<Cell> = cells
        .iter()
        .map(|cell| Cell {
            row: cell.row,
            column: cell.column,
            rowspan: cell.rowspan,
            colspan: cell.colspan,
            glyphs: Vec::new(),
        })
        .collect();
    let (mut counted, mut straddling, mut text_glyphs) = (0_usize, 0_usize, 0_usize);
    let inside = index.within([xs[0], ys[0], xs[columns], ys[ys.len() - 1]]);
    for glyph_index in inside {
        let glyph = &content.glyphs[glyph_index];
        let (cx, cy) = index.centres[glyph_index];
        let (Some(c), Some(r)) = (interval(xs, cx), interval(ys, cy)) else {
            continue;
        };
        let cell = slot[r * columns + c];
        filled[cell].glyphs.push(glyph_index);
        if glyph.flags & SPACE != 0 || glyph.text_end <= glyph.text_start {
            continue;
        }
        text_glyphs += 1;
        if glyph.ux.abs() >= glyph.uy.abs() && glyph.ux > 0.0 {
            counted += 1;
            let origin = &cells[cell];
            let (x0, x1) = (xs[origin.column], xs[origin.column + origin.colspan]);
            let slack = 0.3 * glyph.size;
            let end = glyph.x + glyph.width.min(3.0 * glyph.size);
            if glyph.x < x0 - slack || end > x1 + slack {
                straddling += 1;
            }
        }
    }
    let with_text = filled
        .iter()
        .filter(|cell| {
            cell.glyphs.iter().any(|&index| {
                let glyph = &content.glyphs[index];
                glyph.flags & SPACE == 0 && glyph.text_end > glyph.text_start
            })
        })
        .count();
    if with_text < 2 || (with_text as f64) < MIN_FILLED_SHARE * filled.len() as f64 {
        return None;
    }
    // The border-crossing check only measures upright text, and a sideways or upside-down table
    // would come out transposed or reversed.
    if counted * 2 <= text_glyphs {
        return None;
    }
    if (straddling as f64) > MAX_STRADDLE_SHARE * counted as f64 {
        return None;
    }
    Some(Grid {
        bbox: [xs[0], ys[0], xs[columns], ys[ys.len() - 1]],
        rows: ys.len() - 1,
        columns,
        cells: filled,
        heavy_head_rule: *heavy_head_rule,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reader::pdf::interpreter::Glyph;

    fn segment(x0: f64, y0: f64, x1: f64, y1: f64) -> Rule {
        Rule {
            shape: RuleShape::Segment,
            paint: RulePaint::Stroke,
            x0,
            y0,
            x1,
            y1,
            width: 0.5,
        }
    }

    /// A complete grid of rules with the given column and row edges.
    fn grid_rules(xs: &[f64], ys: &[f64]) -> Vec<Rule> {
        let mut rules = Vec::new();
        for &y in ys {
            rules.push(segment(xs[0], y, xs[xs.len() - 1], y));
        }
        for &x in xs {
            rules.push(segment(x, ys[0], x, ys[ys.len() - 1]));
        }
        rules
    }

    fn glyph_at(page: &mut PageContent, ch: char, x: f64, y: f64) {
        let start = page.text.len() as u32;
        page.text.push(ch);
        page.glyphs.push(Glyph {
            x,
            y,
            ux: 1.0,
            uy: 0.0,
            width: 5.0,
            size: 10.0,
            text_start: start,
            text_end: page.text.len() as u32,
            font: 0,
            color: None,
            flags: 0,
        });
    }

    /// One glyph near the left of every cell of the grid.
    fn page_with_grid(xs: &[f64], ys: &[f64]) -> PageContent {
        let mut page = PageContent {
            rules: grid_rules(xs, ys),
            ..PageContent::default()
        };
        for bottom in &ys[1..] {
            for left in &xs[..xs.len() - 1] {
                glyph_at(&mut page, 'a', left + 3.0, bottom - 4.0);
            }
        }
        page
    }

    const XS: [f64; 4] = [100.0, 160.0, 220.0, 280.0];
    const YS: [f64; 4] = [100.0, 120.0, 140.0, 160.0];

    #[test]
    fn builds_a_grid_from_rules() {
        let page = page_with_grid(&XS, &YS);
        let detected = detect(&page, 400.0, 400.0);
        assert!(!detected.capped);
        let [grid] = detected.grids.as_slice() else {
            panic!("expected one grid, got {:?}", detected.grids);
        };
        assert_eq!((grid.rows, grid.columns), (3, 3));
        assert_eq!(grid.cells.len(), 9);
        assert_eq!(grid.bbox, [100.0, 100.0, 280.0, 160.0]);
        assert!(grid.cells.iter().all(|cell| cell.glyphs.len() == 1));
        assert_eq!(grid.cells[4].glyphs, vec![4]);
    }

    #[test]
    fn stroked_rectangles_make_the_same_grid() {
        let mut page = page_with_grid(&XS, &YS);
        page.rules = Vec::new();
        for r in 0..3 {
            for c in 0..3 {
                page.rules.push(Rule {
                    shape: RuleShape::Rect,
                    paint: RulePaint::Stroke,
                    x0: XS[c],
                    y0: YS[r],
                    x1: XS[c + 1],
                    y1: YS[r + 1],
                    width: 0.5,
                });
            }
        }
        let detected = detect(&page, 400.0, 400.0);
        assert_eq!(detected.grids.len(), 1);
        assert_eq!(detected.grids[0].cells.len(), 9);
    }

    #[test]
    fn a_missing_interior_edge_merges_two_cells() {
        let mut page = page_with_grid(&XS, &YS);
        // Drop the vertical rule between columns 1 and 2 in the middle row only.
        page.rules.retain(|rule| {
            !(rule.x0 == XS[2] && rule.x1 == XS[2] && rule.y0 == YS[0] && rule.y1 == YS[3])
        });
        page.rules.push(segment(XS[2], YS[0], XS[2], YS[1]));
        page.rules.push(segment(XS[2], YS[2], XS[2], YS[3]));
        let detected = detect(&page, 400.0, 400.0);
        let [grid] = detected.grids.as_slice() else {
            panic!("expected one grid");
        };
        assert_eq!(grid.cells.len(), 8);
        let merged = grid
            .cells
            .iter()
            .find(|cell| cell.colspan == 2)
            .expect("one merged cell");
        assert_eq!((merged.row, merged.column, merged.rowspan), (1, 1, 1));
        assert_eq!(merged.glyphs.len(), 2);
    }

    #[test]
    fn an_irregular_grid_is_skipped() {
        let mut page = page_with_grid(&XS, &YS);
        // The missing rule below the top-middle cell and the missing rule between the two
        // middle-row cells on its right join three cells into an L shape.
        page.rules = vec![
            segment(XS[0], YS[0], XS[3], YS[0]),
            segment(XS[0], YS[3], XS[3], YS[3]),
            segment(XS[0], YS[0], XS[0], YS[3]),
            segment(XS[3], YS[0], XS[3], YS[3]),
            segment(XS[1], YS[0], XS[1], YS[3]),
            segment(XS[2], YS[0], XS[2], YS[1]),
            segment(XS[2], YS[2], XS[2], YS[3]),
            segment(XS[0], YS[1], XS[1], YS[1]),
            segment(XS[2], YS[1], XS[3], YS[1]),
            segment(XS[0], YS[2], XS[3], YS[2]),
        ];
        assert!(detect(&page, 400.0, 400.0).grids.is_empty());
    }

    #[test]
    fn an_open_border_is_skipped() {
        let mut page = page_with_grid(&XS, &YS);
        page.rules
            .retain(|rule| !(rule.y0 == YS[3] && rule.y1 == YS[3]));
        assert!(detect(&page, 400.0, 400.0).grids.is_empty());
    }

    #[test]
    fn a_line_past_the_border_is_skipped() {
        let mut page = page_with_grid(&XS, &YS);
        page.rules.push(segment(XS[0], YS[1], XS[3] + 30.0, YS[1]));
        assert!(detect(&page, 400.0, 400.0).grids.is_empty());
    }

    #[test]
    fn a_grid_without_text_is_skipped() {
        let mut page = page_with_grid(&XS, &YS);
        page.glyphs.truncate(1);
        assert!(detect(&page, 400.0, 400.0).grids.is_empty());
    }

    #[test]
    fn text_crossing_cell_borders_is_skipped() {
        let mut page = page_with_grid(&XS, &YS);
        for glyph in &mut page.glyphs {
            glyph.x += 47.0;
            glyph.width = 15.0;
        }
        assert!(detect(&page, 400.0, 400.0).grids.is_empty());
    }

    #[test]
    fn a_page_frame_rectangle_is_ignored() {
        let mut page = page_with_grid(&XS, &YS);
        page.rules.push(Rule {
            shape: RuleShape::Rect,
            paint: RulePaint::Stroke,
            x0: 0.0,
            y0: 0.0,
            x1: 400.0,
            y1: 400.0,
            width: 1.0,
        });
        assert_eq!(detect(&page, 400.0, 400.0).grids.len(), 1);
    }

    #[test]
    fn two_stacked_tables_sharing_a_border_read_as_one() {
        // The shared rule joins both sets of lines into one component, and a grid whose rows
        // continue the columns of the table above is the same picture as one longer table.
        let lower = [YS[3], YS[3] + 20.0, YS[3] + 40.0];
        let ys: Vec<f64> = YS
            .iter()
            .copied()
            .chain(lower[1..].iter().copied())
            .collect();
        let page = page_with_grid(&XS, &ys);
        let detected = detect(&page, 400.0, 400.0);
        let [grid] = detected.grids.as_slice() else {
            panic!("expected one grid, got {:?}", detected.grids);
        };
        assert_eq!((grid.rows, grid.columns), (5, 3));
    }

    #[test]
    fn two_stacked_tables_with_a_gap_stay_separate() {
        let mut page = page_with_grid(&XS, &YS);
        let lower = page_with_grid(
            &XS,
            &[YS[3] + 20.0, YS[3] + 40.0, YS[3] + 60.0, YS[3] + 80.0],
        );
        let offset = page.glyphs.len();
        page.rules.extend(lower.rules);
        for glyph in &lower.glyphs {
            let start = page.text.len() as u32;
            page.text.push('b');
            page.glyphs.push(Glyph {
                text_start: start,
                text_end: page.text.len() as u32,
                ..*glyph
            });
        }
        assert_eq!(page.glyphs.len(), offset + lower.glyphs.len());
        assert_eq!(detect(&page, 400.0, 400.0).grids.len(), 2);
    }

    #[test]
    fn glyphs_outside_the_grid_are_left_alone() {
        let mut page = page_with_grid(&XS, &YS);
        // One glyph beyond each side of the grid; the nine glyphs of its cells are the only ones
        // it claims.
        glyph_at(&mut page, 'l', XS[0] - 20.0, 130.0);
        glyph_at(&mut page, 'r', XS[3] + 20.0, 130.0);
        glyph_at(&mut page, 'u', 130.0, YS[0] - 20.0);
        glyph_at(&mut page, 'd', 130.0, YS[3] + 20.0);
        let detected = detect(&page, 400.0, 400.0);
        let claimed: usize = detected.grids[0]
            .cells
            .iter()
            .map(|cell| cell.glyphs.len())
            .sum();
        assert_eq!(claimed, 9);
    }

    #[test]
    fn a_grid_covering_most_of_the_page_is_skipped() {
        let page = page_with_grid(&XS, &YS);
        // The grid is 180 by 60 points: 18% of this page, but 54% of a smaller one.
        assert_eq!(detect(&page, 300.0, 200.0).grids.len(), 1);
        assert!(detect(&page, 200.0, 100.0).grids.is_empty());
    }

    #[test]
    fn a_banded_page_layout_is_skipped() {
        // Frame, header rule and footer rule, with one column rule between header and footer:
        // the header and footer rows are single merged cells, so only the body row has two cells.
        let mut page = page_with_grid(&XS, &YS);
        page.rules = vec![
            segment(XS[0], YS[0], XS[2], YS[0]),
            segment(XS[0], YS[1], XS[2], YS[1]),
            segment(XS[0], YS[2], XS[2], YS[2]),
            segment(XS[0], YS[3], XS[2], YS[3]),
            segment(XS[0], YS[0], XS[0], YS[3]),
            segment(XS[2], YS[0], XS[2], YS[3]),
            segment(XS[1], YS[1], XS[1], YS[2]),
        ];
        assert!(detect(&page, 400.0, 400.0).grids.is_empty());
        // Give the header row a second cell and it is a table again.
        page.rules.push(segment(XS[1], YS[0], XS[1], YS[1]));
        assert_eq!(detect(&page, 400.0, 400.0).grids.len(), 1);
    }

    #[test]
    fn a_sideways_grid_is_skipped() {
        let mut page = page_with_grid(&XS, &YS);
        for glyph in &mut page.glyphs {
            (glyph.ux, glyph.uy) = (0.0, 1.0);
        }
        assert!(detect(&page, 400.0, 400.0).grids.is_empty());
        // Upside-down text is not upright either.
        for glyph in &mut page.glyphs {
            (glyph.ux, glyph.uy) = (-1.0, 0.0);
        }
        assert!(detect(&page, 400.0, 400.0).grids.is_empty());
    }

    #[test]
    fn a_few_turned_labels_do_not_hide_a_table() {
        let mut page = page_with_grid(&XS, &YS);
        // Three of nine cells carry a vertical label, as in a narrow column head.
        for glyph in page.glyphs.iter_mut().take(3) {
            (glyph.ux, glyph.uy) = (0.0, 1.0);
        }
        assert_eq!(detect(&page, 400.0, 400.0).grids.len(), 1);
    }

    #[test]
    fn glyphs_are_assigned_by_centre_point() {
        let mut page = page_with_grid(&XS, &YS);
        // Starts left of the column border but is centred inside the column to its right.
        glyph_at(&mut page, 'z', XS[1] - 2.0, YS[1] - 4.0);
        let detected = detect(&page, 400.0, 400.0);
        let grid = &detected.grids[0];
        let cell = grid
            .cells
            .iter()
            .find(|cell| (cell.row, cell.column) == (0, 1))
            .unwrap();
        assert!(cell.glyphs.contains(&(page.glyphs.len() - 1)));
    }

    #[test]
    fn too_many_lines_cap_the_page_without_failing() {
        let mut page = page_with_grid(&XS, &YS);
        for i in 0..=MAX_LINES {
            let y = 200.0 + 5.0 * i as f64;
            page.rules.push(segment(10.0, y, 90.0, y));
        }
        let detected = detect(&page, 400.0, 6_000.0);
        assert!(detected.capped);
        assert!(detected.grids.is_empty());
    }

    #[test]
    fn the_table_count_is_capped() {
        let mut page = PageContent::default();
        for t in 0..=MAX_TABLES {
            let y0 = 10.0 + 70.0 * t as f64;
            let ys = [y0, y0 + 20.0, y0 + 40.0];
            let part = page_with_grid(&[100.0, 160.0, 220.0], &ys);
            page.rules.extend(part.rules);
            for glyph in &part.glyphs {
                let start = page.text.len() as u32;
                page.text.push('a');
                page.glyphs.push(Glyph {
                    text_start: start,
                    text_end: page.text.len() as u32,
                    ..*glyph
                });
            }
        }
        let detected = detect(&page, 400.0, 3_000.0);
        assert_eq!(detected.grids.len(), MAX_TABLES);
        assert!(detected.capped);
    }

    #[test]
    fn the_cell_cap_skips_oversized_pages() {
        let xs: Vec<f64> = (0..=MAX_COLUMNS).map(|i| 10.0 + 10.0 * i as f64).collect();
        let ys: Vec<f64> = (0..=MAX_ROWS).map(|i| 10.0 + 10.0 * i as f64).collect();
        // 40 x 200 = 8,000 cells, above the page cap of 4,096.
        let page = page_with_grid(&xs, &ys);
        let detected = detect(&page, 600.0, 3_000.0);
        assert!(detected.capped);
        assert!(detected.grids.is_empty());
    }

    #[test]
    fn a_heavier_rule_under_the_first_row_is_reported() {
        let mut page = page_with_grid(&XS, &YS);
        for rule in &mut page.rules {
            if rule.y0 == YS[1] && rule.y1 == YS[1] {
                rule.width = 2.0;
            }
        }
        let detected = detect(&page, 400.0, 400.0);
        assert!(detected.grids[0].heavy_head_rule);
        assert!(!detect(&page_with_grid(&XS, &YS), 400.0, 400.0).grids[0].heavy_head_rule);
    }
}
