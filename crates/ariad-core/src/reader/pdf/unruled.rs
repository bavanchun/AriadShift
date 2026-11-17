//! Whitespace-aligned table detection.
//!
//! A table drawn without ruling lines still leaves vertical stripes of empty space between its
//! columns. The detector works on the text rows of one reading-order flow (see `layout`). A row
//! whose glyphs fall into three or more clusters is table-like; consecutive table-like rows are a
//! candidate when the stripes that stay empty in all of them separate at least three columns.
//! Short rows next to or between those rows, such as a head with spanning labels or a row with a
//! missing cell, join the candidate when each of their clusters sits inside one column.
//!
//! The detector favours precision over recall. A candidate is dropped, never repaired, when
//!
//! - the columns are not aligned, most cells are empty, or the cells read like running text;
//! - the text is monospaced, as in code listings;
//! - the neighbouring flow holds short rows on the same baselines, which means the table may
//!   continue across the column gutter;
//! - it overlaps a ruled table or another candidate.
//!
//! All distances scale with the median glyph advance of the flow, so the thresholds follow the
//! text size. Cells are assigned to columns by the centre point of each glyph, as ruled tables
//! are, and the resulting [`Grid`] goes through the same layout and emission as a ruled one.

use std::ops::Range;

use super::tables::{Cell, Grid, MAX_COLUMNS, MAX_ROWS};

/// Gap, in median glyph advances, that separates two clusters of a row.
const CLUSTER_GAP: f64 = 2.0;

/// Narrowest stripe, in median glyph advances, that stays empty in every table-like row.
const STRIPE: f64 = 1.5;

/// How far, in median glyph advances, the left edges, right edges or centres of the cells of a
/// column may drift from one row to another.
const ALIGN: f64 = 1.0;

/// Fewest clusters of a table-like row, and therefore fewest columns.
const MIN_COLUMNS: usize = 3;

/// Fewest table-like rows of a candidate.
const MIN_ROWS: usize = 3;

/// Most baseline distance, in font sizes, between consecutive rows of a candidate.
const MAX_PITCH: f64 = 2.5;

/// Most rows between two table-like rows that are not table-like themselves.
const MAX_BRIDGE: usize = 2;

/// Most rows attached above or below the table-like rows.
const MAX_ATTACHED: usize = 3;

/// Most baseline distance, in median row pitches, between a table row and an attached row.
const ATTACH_PITCH: f64 = 1.6;

/// How far, in median glyph advances, an attached row may reach past the table-like rows.
const ATTACH_REACH: f64 = 4.0;

/// Largest relative size difference between an attached row and the table.
const ATTACH_SIZE: f64 = 0.2;

/// Share of the cells of table-like rows that must hold text, and the same share over all rows.
/// A row with the fewest clusters of a five-column table already fills 0.6 of it.
const MIN_FILLED_CORE: f64 = 0.6;
const MIN_FILLED_ALL: f64 = 0.5;

/// Share of the cells of a column whose left edge, right edge or centre agree.
const MIN_ALIGNED: f64 = 0.7;

/// Fewest cells of a column for the alignment test to apply.
const MIN_ALIGNED_CELLS: usize = 3;

/// Cells longer than this many glyphs read like running text.
const LONG_CELL: usize = 40;
const MAX_CELL: usize = 80;
/// Median cell length, in glyphs, above which the candidate reads like running text.
const MAX_MEDIAN_CELL: usize = 20;
/// Share of the cells that may be long.
const MAX_LONG_SHARE: f64 = 0.1;

/// Fewest letters for the monospace test, and the share that must share one advance.
const MONO_LETTERS: usize = 24;
const MONO_SHARE: f64 = 0.95;

/// Rows of the neighbouring flow with at most this many glyphs count as short.
const SHORT_ROW: usize = 40;

/// Share of a candidate's table-like rows with a short neighbour on the same baseline at which the
/// candidate may continue in the neighbouring flow.
const NEIGHBOUR_SHARE: f64 = 0.6;

/// One glyph of a text row, in display space with text running left to right.
#[derive(Clone, Copy, Debug)]
pub(super) struct GlyphBox {
    pub(super) x0: f64,
    pub(super) x1: f64,
    /// Index into the page's glyphs.
    pub(super) index: usize,
    /// Draws ink: not a space and carrying text.
    pub(super) visible: bool,
    /// A base letter, which sets the advance of a monospaced face.
    pub(super) letter: bool,
}

/// A line of text with all its glyphs, spaces included, sorted by `x0`.
#[derive(Debug)]
pub(super) struct Row {
    pub(super) base: f64,
    pub(super) size: f64,
    pub(super) top: f64,
    pub(super) bottom: f64,
    pub(super) glyphs: Vec<GlyphBox>,
}

/// What may still be added to the page.
#[derive(Clone, Copy, Debug)]
pub(super) struct Budget {
    pub(super) tables: usize,
    pub(super) cells: usize,
}

/// A table found in one flow.
#[derive(Debug)]
pub(super) struct Found {
    pub(super) grid: Grid,
    pub(super) flow: usize,
    /// Rows of the flow that belong to the table.
    pub(super) rows: Range<usize>,
}

/// Outcome of scanning the flows of a page.
#[derive(Debug, Default)]
pub(super) struct Detected {
    pub(super) found: Vec<Found>,
    /// A limit stopped detection, so some tables may be missing.
    pub(super) capped: bool,
}

#[derive(Clone, Copy, Debug)]
struct Cluster {
    x0: f64,
    x1: f64,
    glyphs: usize,
}

struct Candidate {
    found: Found,
    /// Flow rows that are table-like.
    seeds: Vec<usize>,
}

/// Finds whitespace-aligned tables among the rows of `flows`, none of which may overlap `avoid`
/// (the boxes of ruled tables).
pub(super) fn detect(flows: &[Vec<Row>], avoid: &[[f64; 4]], budget: Budget) -> Detected {
    let mut candidates = Vec::new();
    for (flow, rows) in flows.iter().enumerate() {
        if let Some(advance) = median_advance(rows) {
            scan_flow(flow, rows, advance, &mut candidates);
        }
    }
    let short = short_baselines(flows);
    let mut detected = Detected::default();
    let mut kept: Vec<Candidate> = Vec::new();
    let mut cells_left = budget.cells;
    for candidate in candidates {
        let bbox = candidate.found.grid.bbox;
        if continues_in_neighbour(flows, &short, &candidate)
            || avoid.iter().any(|other| overlaps(other, &bbox))
            || kept
                .iter()
                .any(|other| overlaps(&other.found.grid.bbox, &bbox))
        {
            continue;
        }
        let cells = candidate.found.grid.cells.len();
        if kept.len() >= budget.tables || cells > cells_left {
            detected.capped = true;
            continue;
        }
        cells_left -= cells;
        kept.push(candidate);
    }
    detected.found = kept.into_iter().map(|candidate| candidate.found).collect();
    detected
}

fn overlaps(a: &[f64; 4], b: &[f64; 4]) -> bool {
    a[0] < b[2] && b[0] < a[2] && a[1] < b[3] && b[1] < a[3]
}

/// Median advance of the visible glyphs of the rows.
fn median_advance(rows: &[Row]) -> Option<f64> {
    let mut advances: Vec<f64> = rows
        .iter()
        .flat_map(|row| &row.glyphs)
        .filter(|glyph| glyph.visible && glyph.x1 > glyph.x0)
        .map(|glyph| glyph.x1 - glyph.x0)
        .collect();
    if advances.is_empty() {
        return None;
    }
    advances.sort_by(f64::total_cmp);
    Some(advances[advances.len() / 2])
}

/// Groups the visible glyphs of a row into clusters separated by gaps of at least `gap`.
fn clusters(row: &Row, gap: f64) -> Vec<Cluster> {
    let mut out: Vec<Cluster> = Vec::new();
    for glyph in row.glyphs.iter().filter(|glyph| glyph.visible) {
        match out.last_mut() {
            Some(last) if glyph.x0 - last.x1 < gap => {
                last.x1 = last.x1.max(glyph.x1);
                last.glyphs += 1;
            }
            _ => out.push(Cluster {
                x0: glyph.x0,
                x1: glyph.x1,
                glyphs: 1,
            }),
        }
    }
    out
}

fn pitch_fits(upper: &Row, lower: &Row) -> bool {
    let distance = lower.base - upper.base;
    distance > 0.0 && distance <= MAX_PITCH * upper.size.max(lower.size)
}

fn scan_flow(flow: usize, rows: &[Row], advance: f64, out: &mut Vec<Candidate>) {
    let clustered: Vec<Vec<Cluster>> = rows
        .iter()
        .map(|row| clusters(row, CLUSTER_GAP * advance))
        .collect();
    let mut start = 0;
    while start < rows.len() {
        if clustered[start].len() < MIN_COLUMNS {
            start += 1;
            continue;
        }
        let mut seeds = vec![start];
        let mut last = start;
        'grow: loop {
            for next in last + 1..rows.len().min(last + 2 + MAX_BRIDGE) {
                if !pitch_fits(&rows[next - 1], &rows[next]) {
                    break;
                }
                if clustered[next].len() >= MIN_COLUMNS {
                    seeds.push(next);
                    last = next;
                    continue 'grow;
                }
            }
            break;
        }
        if seeds.len() >= MIN_ROWS
            && let Some(found) = build(flow, rows, &clustered, &seeds, advance)
        {
            out.push(Candidate { found, seeds });
        }
        start = last + 1;
    }
}

/// Positions between the columns: the midpoints of the stripes that stay empty in every row.
fn stripe_midpoints(rows: &[&Row], advance: f64) -> Vec<f64> {
    let mut spans: Vec<(f64, f64)> = rows
        .iter()
        .flat_map(|row| &row.glyphs)
        .filter(|glyph| glyph.visible)
        .map(|glyph| (glyph.x0, glyph.x1))
        .collect();
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
    let Some(&(_, first_end)) = spans.first() else {
        return Vec::new();
    };
    let mut mids = Vec::new();
    let mut reach = first_end;
    for &(x0, x1) in &spans[1..] {
        if x0 - reach >= STRIPE * advance {
            mids.push(f64::midpoint(reach, x0));
        }
        reach = reach.max(x1);
    }
    mids
}

/// Index of the column that holds `x`.
fn column_of(mids: &[f64], x: f64) -> usize {
    mids.partition_point(|&mid| mid <= x)
}

/// Whether every cluster of the row lies inside one column within `[left, right]`.
fn fits_columns(clusters: &[Cluster], mids: &[f64], left: f64, right: f64) -> bool {
    !clusters.is_empty()
        && clusters.iter().all(|cluster| {
            cluster.x0 >= left
                && cluster.x1 <= right
                && cluster.glyphs <= MAX_CELL
                && column_of(mids, cluster.x0) == column_of(mids, cluster.x1)
        })
}

fn median_of(mut values: Vec<f64>) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    Some(values[values.len() / 2])
}

/// Extent and glyph count of the visible text of one cell.
#[derive(Clone, Copy, Debug, Default)]
struct CellStat {
    glyphs: usize,
    x0: f64,
    x1: f64,
}

fn build(
    flow: usize,
    rows: &[Row],
    clustered: &[Vec<Cluster>],
    seeds: &[usize],
    advance: f64,
) -> Option<Found> {
    let (first, last) = (seeds[0], *seeds.last()?);
    let seed_rows: Vec<&Row> = seeds.iter().map(|&index| &rows[index]).collect();
    let mids = stripe_midpoints(&seed_rows, advance);
    let columns = mids.len() + 1;
    if !(MIN_COLUMNS..=MAX_COLUMNS).contains(&columns) {
        return None;
    }
    let left = seeds
        .iter()
        .flat_map(|&index| &clustered[index])
        .map(|cluster| cluster.x0)
        .fold(f64::INFINITY, f64::min)
        - ATTACH_REACH * advance;
    let right = seeds
        .iter()
        .flat_map(|&index| &clustered[index])
        .map(|cluster| cluster.x1)
        .fold(f64::NEG_INFINITY, f64::max)
        + ATTACH_REACH * advance;
    let fits = |index: usize| fits_columns(&clustered[index], &mids, left, right);
    // A row between table-like rows that straddles a column edge breaks the table.
    if (first..=last).any(|index| !fits(index)) {
        return None;
    }

    let pitch = median_of(
        (first + 1..=last)
            .map(|index| rows[index].base - rows[index - 1].base)
            .collect(),
    )?;
    let size = median_of(seed_rows.iter().map(|row| row.size).collect())?;
    // A short row that follows a long line at ordinary line spacing is the end of a paragraph.
    let ends_paragraph = |index: usize, beyond: Option<usize>| {
        beyond.is_some_and(|beyond| {
            let (row, other) = (&rows[index], &rows[beyond]);
            other.glyphs.iter().filter(|glyph| glyph.visible).count() > SHORT_ROW
                && (row.base - other.base).abs() <= MAX_PITCH * row.size.max(other.size)
        })
    };
    let attachable = |near: &Row, index: usize, beyond: Option<usize>| {
        let row = &rows[index];
        (row.base - near.base).abs() <= ATTACH_PITCH * pitch
            && (row.size - size).abs() <= ATTACH_SIZE * size
            && fits(index)
            && !ends_paragraph(index, beyond)
    };
    let mut top = first;
    while top > 0
        && first - top < MAX_ATTACHED
        && attachable(&rows[top], top - 1, (top - 1).checked_sub(1))
    {
        top -= 1;
    }
    let mut bottom = last;
    while bottom + 1 < rows.len()
        && bottom - last < MAX_ATTACHED
        && attachable(
            &rows[bottom],
            bottom + 1,
            Some(bottom + 2).filter(|&b| b < rows.len()),
        )
    {
        bottom += 1;
    }

    let table_rows = &rows[top..=bottom];
    let mut cells: Vec<Cell> = Vec::with_capacity(table_rows.len() * columns);
    let mut stats = vec![CellStat::default(); table_rows.len() * columns];
    let mut letters: Vec<f64> = Vec::new();
    let (mut x0, mut x1) = (f64::INFINITY, f64::NEG_INFINITY);
    for (r, row) in table_rows.iter().enumerate() {
        for c in 0..columns {
            cells.push(Cell {
                row: r,
                column: c,
                rowspan: 1,
                colspan: 1,
                glyphs: Vec::new(),
            });
        }
        for glyph in &row.glyphs {
            let c = column_of(&mids, f64::midpoint(glyph.x0, glyph.x1));
            cells[r * columns + c].glyphs.push(glyph.index);
            if glyph.visible {
                let stat = &mut stats[r * columns + c];
                if stat.glyphs == 0 {
                    stat.x0 = glyph.x0;
                    stat.x1 = glyph.x1;
                }
                stat.glyphs += 1;
                stat.x0 = stat.x0.min(glyph.x0);
                stat.x1 = stat.x1.max(glyph.x1);
                x0 = x0.min(glyph.x0);
                x1 = x1.max(glyph.x1);
                if glyph.letter {
                    letters.push(glyph.x1 - glyph.x0);
                }
            }
        }
    }
    let core: Vec<usize> = seeds.iter().map(|&index| index - top).collect();
    if !reads_as_table(&stats, &core, columns, advance) || is_monospaced(&letters) {
        return None;
    }
    for cell in &mut cells {
        cell.glyphs.sort_unstable();
    }
    let bbox = [
        x0,
        table_rows
            .iter()
            .map(|row| row.top)
            .fold(f64::INFINITY, f64::min),
        x1,
        table_rows
            .iter()
            .map(|row| row.bottom)
            .fold(f64::NEG_INFINITY, f64::max),
    ];
    if table_rows.len() > MAX_ROWS || !bbox.iter().all(|value| value.is_finite()) {
        return None;
    }
    Some(Found {
        grid: Grid {
            bbox,
            rows: table_rows.len(),
            columns,
            cells,
            heavy_head_rule: false,
        },
        flow,
        rows: top..bottom + 1,
    })
}

/// The cell statistics must describe a table: cells filled, columns aligned, text short.
fn reads_as_table(stats: &[CellStat], core: &[usize], columns: usize, advance: f64) -> bool {
    let rows = stats.len() / columns;
    let filled = |cell: &CellStat| cell.glyphs > 0;
    let core_cells = core.len() * columns;
    let core_filled = core
        .iter()
        .flat_map(|&r| &stats[r * columns..(r + 1) * columns])
        .filter(|cell| filled(cell))
        .count();
    let all_filled = stats.iter().filter(|cell| filled(cell)).count();
    if (core_filled as f64) < MIN_FILLED_CORE * core_cells as f64
        || (all_filled as f64) < MIN_FILLED_ALL * (rows * columns) as f64
    {
        return false;
    }

    // Running text: long cells, or a long typical cell.
    let lengths: Vec<usize> = stats
        .iter()
        .filter(|cell| filled(cell))
        .map(|cell| cell.glyphs)
        .collect();
    let mut sorted = lengths.clone();
    sorted.sort_unstable();
    let long = lengths.iter().filter(|&&glyphs| glyphs > LONG_CELL).count();
    if sorted[sorted.len() / 2] > MAX_MEDIAN_CELL
        || (long as f64) > MAX_LONG_SHARE * lengths.len() as f64
    {
        return false;
    }

    // Each column of the table-like rows keeps one alignment.
    let tolerance = ALIGN * advance;
    (0..columns).all(|c| {
        let cells: Vec<&CellStat> = core
            .iter()
            .map(|&r| &stats[r * columns + c])
            .filter(|cell| filled(cell))
            .collect();
        if cells.len() < MIN_ALIGNED_CELLS {
            return true;
        }
        let anchors: [fn(&CellStat) -> f64; 3] = [
            |cell| cell.x0,
            |cell| cell.x1,
            |cell| f64::midpoint(cell.x0, cell.x1),
        ];
        anchors.iter().any(|anchor| {
            let values: Vec<f64> = cells.iter().map(|cell| anchor(cell)).collect();
            let Some(middle) = median_of(values.clone()) else {
                return false;
            };
            let near = values
                .iter()
                .filter(|value| (**value - middle).abs() <= tolerance)
                .count();
            (near as f64) >= MIN_ALIGNED * cells.len() as f64
        })
    })
}

/// Whether the letters share one advance, as in a monospaced code listing.
fn is_monospaced(letters: &[f64]) -> bool {
    if letters.len() < MONO_LETTERS {
        return false;
    }
    let Some(typical) = median_of(letters.to_vec()) else {
        return false;
    };
    let same = letters
        .iter()
        .filter(|advance| (**advance - typical).abs() <= 0.01 * typical)
        .count();
    (same as f64) >= MONO_SHARE * letters.len() as f64
}

/// Baselines of the short rows of every flow, ascending: rows with few enough glyphs to be table
/// cells rather than running text.
fn short_baselines(flows: &[Vec<Row>]) -> Vec<Vec<f64>> {
    flows
        .iter()
        .map(|rows| {
            let mut bases: Vec<f64> = rows
                .iter()
                .filter(|row| {
                    let glyphs = row.glyphs.iter().filter(|glyph| glyph.visible).count();
                    (1..=SHORT_ROW).contains(&glyphs)
                })
                .map(|row| row.base)
                .collect();
            bases.sort_by(f64::total_cmp);
            bases
        })
        .collect()
}

/// Whether the table may continue in a neighbouring flow: many of its table-like rows have a
/// short row on the same baseline in another flow, as the halves of a table that spans both
/// columns of a page do.
fn continues_in_neighbour(flows: &[Vec<Row>], short: &[Vec<f64>], candidate: &Candidate) -> bool {
    let rows = &flows[candidate.found.flow];
    let shared = candidate
        .seeds
        .iter()
        .filter(|&&index| {
            let row = &rows[index];
            let tolerance = 0.1 * row.size;
            short.iter().enumerate().any(|(flow, bases)| {
                flow != candidate.found.flow && {
                    let from = bases.partition_point(|&base| base < row.base - tolerance);
                    bases
                        .get(from)
                        .is_some_and(|&base| base <= row.base + tolerance)
                }
            })
        })
        .count();
    (shared as f64) >= NEIGHBOUR_SHARE * candidate.seeds.len() as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A row of 10 pt text with the given `(x, text)` cells. Letters advance by slightly
    /// different amounts, as in a proportional face; `mono` gives them all the same advance.
    fn make_row(base: f64, cells: &[(f64, &str)], mono: bool) -> Row {
        let mut glyphs = Vec::new();
        for &(x, text) in cells {
            let mut at = x;
            for ch in text.chars() {
                let advance = if mono || !ch.is_alphabetic() {
                    5.0
                } else {
                    4.0 + f64::from(ch as u32 % 5) * 0.5
                };
                glyphs.push(GlyphBox {
                    x0: at,
                    x1: at + advance,
                    index: glyphs.len(),
                    visible: true,
                    letter: ch.is_alphabetic(),
                });
                at += advance;
            }
        }
        Row {
            base,
            size: 10.0,
            top: base - 8.0,
            bottom: base + 2.0,
            glyphs,
        }
    }

    fn row(base: f64, cells: &[(f64, &str)]) -> Row {
        make_row(base, cells, false)
    }

    const ROOM: Budget = Budget {
        tables: 8,
        cells: 1_000,
    };

    fn detect_one(rows: Vec<Row>) -> Detected {
        detect(&[rows], &[], ROOM)
    }

    /// The rows moved down by `distance`.
    fn lowered(mut rows: Vec<Row>, distance: f64) -> Vec<Row> {
        for row in &mut rows {
            row.base += distance;
            row.top += distance;
            row.bottom += distance;
        }
        rows
    }

    fn plain_table() -> Vec<Row> {
        vec![
            row(100.0, &[(50.0, "Name"), (150.0, "Size"), (220.0, "Cost")]),
            row(112.0, &[(50.0, "Alpha"), (150.0, "12"), (220.0, "3.5")]),
            row(124.0, &[(50.0, "Beta"), (150.0, "48"), (220.0, "1.2")]),
            row(136.0, &[(50.0, "Gamma"), (150.0, "96"), (220.0, "7.7")]),
        ]
    }

    #[test]
    fn rows_with_aligned_gaps_make_a_table() {
        let detected = detect_one(plain_table());
        assert!(!detected.capped);
        let [found] = detected.found.as_slice() else {
            panic!("expected one table, got {detected:?}");
        };
        assert_eq!(found.rows, 0..4);
        assert_eq!((found.grid.rows, found.grid.columns), (4, 3));
        assert_eq!(found.grid.cells.len(), 12);
        // Row-major cells; "Name" is the first four glyphs of the page.
        assert_eq!(found.grid.cells[0].glyphs, vec![0, 1, 2, 3]);
        assert!(found.grid.cells.iter().all(|cell| cell.glyphs.len() > 1));
        assert!(!found.grid.heavy_head_rule);
        assert_eq!(found.grid.bbox[0], 50.0);
        assert!(found.grid.bbox[2] > 235.0 && found.grid.bbox[2] < 240.0);
        assert_eq!(found.grid.bbox[1], 92.0);
    }

    #[test]
    fn two_columns_are_not_enough() {
        let rows = (0..5)
            .map(|i| {
                row(
                    100.0 + 12.0 * f64::from(i),
                    &[(50.0, "Alpha"), (200.0, "Beta")],
                )
            })
            .collect();
        assert!(detect_one(rows).found.is_empty());
    }

    #[test]
    fn two_rows_are_not_enough() {
        let mut rows = plain_table();
        rows.truncate(2);
        assert!(detect_one(rows).found.is_empty());
    }

    #[test]
    fn columns_that_drift_from_row_to_row_are_not_a_table() {
        let rows = (0..6)
            .map(|i| {
                let drift = 13.0 * f64::from(i);
                row(
                    100.0 + 12.0 * f64::from(i),
                    &[
                        (50.0 + drift, "alpha"),
                        (150.0 + 2.0 * drift, "beta"),
                        (260.0, "g"),
                    ],
                )
            })
            .collect();
        assert!(detect_one(rows).found.is_empty());
    }

    #[test]
    fn rows_far_apart_do_not_chain() {
        let mut rows = plain_table();
        for (index, row) in rows.iter_mut().enumerate() {
            row.base += 40.0 * index as f64;
        }
        assert!(detect_one(rows).found.is_empty());
    }

    #[test]
    fn a_missing_cell_keeps_its_row() {
        let mut rows = plain_table();
        rows.push(row(148.0, &[(50.0, "Delta"), (220.0, "9.1")]));
        rows.push(row(160.0, &[(50.0, "Eta"), (150.0, "5"), (220.0, "2.2")]));
        let detected = detect_one(rows);
        let [found] = detected.found.as_slice() else {
            panic!("expected one table");
        };
        assert_eq!((found.grid.rows, found.grid.columns), (6, 3));
        // The empty cell is part of the grid and holds no glyphs.
        assert!(found.grid.cells[4 * 3 + 1].glyphs.is_empty());
    }

    #[test]
    fn a_short_row_between_table_rows_joins_when_it_fits_one_column() {
        let mut rows = plain_table();
        rows.insert(2, row(118.0, &[(150.0, "ab")]));
        for row in &mut rows[3..] {
            row.base += 6.0;
        }
        let detected = detect_one(rows);
        let [found] = detected.found.as_slice() else {
            panic!("expected one table");
        };
        assert_eq!(found.grid.rows, 5);
    }

    #[test]
    fn a_row_between_table_rows_that_crosses_a_column_edge_breaks_the_table() {
        let mut rows = plain_table();
        rows.insert(2, row(118.0, &[(100.0, "spans two columns")]));
        for row in &mut rows[3..] {
            row.base += 6.0;
        }
        assert!(detect_one(rows).found.is_empty());
    }

    #[test]
    fn head_rows_attach_when_each_cluster_fits_a_column() {
        let mut rows = vec![row(88.0, &[(150.0, "Group")])];
        rows.extend(plain_table());
        let detected = detect_one(rows);
        let [found] = detected.found.as_slice() else {
            panic!("expected one table");
        };
        assert_eq!(found.rows, 0..5);
        assert_eq!(found.grid.rows, 5);
        assert!(found.grid.cells[1].glyphs.len() == 5);
    }

    #[test]
    fn a_head_label_that_spans_columns_stays_out() {
        let mut rows = vec![row(88.0, &[(100.0, "Spanning label")])];
        rows.extend(plain_table());
        let detected = detect_one(rows);
        let [found] = detected.found.as_slice() else {
            panic!("expected one table");
        };
        assert_eq!(found.rows, 1..5);
    }

    #[test]
    fn a_paragraph_after_the_table_is_not_attached() {
        let mut rows = plain_table();
        rows.push(row(
            148.0,
            &[(50.0, "Note: the figures are provisional and may change.")],
        ));
        let detected = detect_one(rows);
        assert_eq!(detected.found[0].rows, 0..4);
    }

    #[test]
    fn the_last_line_of_a_paragraph_above_is_not_attached() {
        let prose = "a line of running prose that is long enough to fill a column";
        let mut rows = vec![
            row(76.0, &[(50.0, prose)]),
            row(88.0, &[(50.0, "the end.")]),
        ];
        rows.extend(plain_table());
        let detected = detect_one(rows);
        assert_eq!(detected.found.len(), 1);
        assert_eq!(detected.found[0].rows, 2..6);
    }

    #[test]
    fn a_short_row_below_that_follows_a_long_line_is_not_attached() {
        let mut rows = plain_table();
        rows.push(row(148.0, &[(50.0, "a")]));
        rows.push(row(
            160.0,
            &[(
                50.0,
                "a line of running prose that is long enough to fill a column",
            )],
        ));
        assert_eq!(detect_one(rows).found[0].rows, 0..4);
    }

    #[test]
    fn a_distant_row_is_not_attached() {
        let mut rows = vec![row(60.0, &[(150.0, "Group")])];
        rows.extend(plain_table());
        assert_eq!(detect_one(rows).found[0].rows, 1..5);
    }

    #[test]
    fn right_aligned_and_centred_columns_count_as_aligned() {
        // Digits advance by 5. Column 1 is centred on 172 and column 2 ends on 240.
        let centred = |text: &str| 172.0 - 2.5 * text.len() as f64;
        let ended = |text: &str| 240.0 - 5.0 * text.len() as f64;
        let rows: Vec<Row> = [
            ("Name", "2023", "2024"),
            ("Alpha", "7", "13.50"),
            ("Beta", "48", "1.20"),
            ("Gamma", "512", "7.7"),
        ]
        .iter()
        .enumerate()
        .map(|(i, &(name, one, two))| {
            row(
                100.0 + 12.0 * i as f64,
                &[(50.0, name), (centred(one), one), (ended(two), two)],
            )
        })
        .collect();
        let detected = detect_one(rows);
        assert_eq!(detected.found.len(), 1);
        assert_eq!(detected.found[0].grid.columns, 3);
    }

    #[test]
    fn text_that_reads_like_sentences_is_not_a_table() {
        let rows = (0..4)
            .map(|i| {
                row(
                    100.0 + 12.0 * f64::from(i),
                    &[
                        (50.0, "the committee met in the old hall"),
                        (300.0, "and agreed to publish the list of"),
                        (550.0, "names before the next vote was held"),
                    ],
                )
            })
            .collect();
        assert!(detect_one(rows).found.is_empty());
    }

    #[test]
    fn monospaced_text_is_not_a_table() {
        let cells = [
            (50.0, "count_total"),
            (200.0, "= items.len();"),
            (350.0, "// number"),
        ];
        let mono: Vec<Row> = (0..5)
            .map(|i| make_row(100.0 + 12.0 * f64::from(i), &cells, true))
            .collect();
        assert!(detect_one(mono).found.is_empty());
        let proportional: Vec<Row> = (0..5)
            .map(|i| make_row(100.0 + 12.0 * f64::from(i), &cells, false))
            .collect();
        assert_eq!(detect_one(proportional).found.len(), 1);
    }

    #[test]
    fn sparse_grids_are_not_tables() {
        let at = |columns: &[usize]| -> Vec<(f64, &'static str)> {
            columns
                .iter()
                .map(|&c| (50.0 + 70.0 * c as f64, "ab"))
                .collect()
        };
        // Six columns, but each row fills only three of them.
        let rows = [[0, 1, 2], [3, 4, 5], [0, 2, 4], [1, 3, 5]]
            .iter()
            .enumerate()
            .map(|(i, columns)| row(100.0 + 12.0 * i as f64, &at(columns)))
            .collect();
        assert!(detect_one(rows).found.is_empty());
    }

    #[test]
    fn short_rows_on_the_same_baselines_in_a_neighbouring_flow_make_it_ambiguous() {
        let left = plain_table();
        let halves: Vec<Row> = (0..4)
            .map(|i| {
                row(
                    100.0 + 12.0 * f64::from(i),
                    &[(400.0, "1.5"), (460.0, "2.5")],
                )
            })
            .collect();
        let detected = detect(&[left, halves], &[], ROOM);
        assert!(detected.found.is_empty());

        // Long lines in the neighbouring flow are prose, not a continuation.
        let prose: Vec<Row> = (0..4)
            .map(|i| {
                row(
                    100.0 + 12.0 * f64::from(i),
                    &[(
                        400.0,
                        "a long line of running prose that fills the whole column width",
                    )],
                )
            })
            .collect();
        let detected = detect(&[plain_table(), prose], &[], ROOM);
        assert_eq!(detected.found.len(), 1);
        assert_eq!(detected.found[0].flow, 0);
    }

    #[test]
    fn a_table_beside_unrelated_rows_survives() {
        let other: Vec<Row> = (0..4)
            .map(|i| row(103.0 + 12.0 * f64::from(i), &[(400.0, "12"), (460.0, "34")]))
            .collect();
        let detected = detect(&[plain_table(), other], &[], ROOM);
        assert_eq!(detected.found.len(), 1);
    }

    #[test]
    fn ruled_tables_keep_priority() {
        let bbox = detect_one(plain_table()).found[0].grid.bbox;
        let detected = detect(&[plain_table()], &[bbox], ROOM);
        assert!(detected.found.is_empty());
        let far = [1_000.0, 1_000.0, 1_100.0, 1_100.0];
        assert_eq!(detect(&[plain_table()], &[far], ROOM).found.len(), 1);
    }

    #[test]
    fn the_budget_caps_tables_and_cells() {
        let mut rows = plain_table();
        rows.extend(lowered(plain_table(), 100.0));
        let none = Budget {
            tables: 1,
            cells: 1_000,
        };
        let detected = detect(&[rows], &[], none);
        assert_eq!(detected.found.len(), 1);
        assert!(detected.capped);

        let small = Budget {
            tables: 8,
            cells: 11,
        };
        let detected = detect(&[plain_table()], &[], small);
        assert!(detected.found.is_empty());
        assert!(detected.capped);
    }

    #[test]
    fn two_tables_in_one_flow_are_found_separately() {
        let mut rows = plain_table();
        rows.extend(lowered(plain_table(), 100.0));
        let detected = detect_one(rows);
        assert_eq!(detected.found.len(), 2);
        assert_eq!(detected.found[0].rows, 0..4);
        assert_eq!(detected.found[1].rows, 4..8);
    }

    #[test]
    fn empty_input_finds_nothing() {
        assert!(detect(&[], &[], ROOM).found.is_empty());
        assert!(detect(&[Vec::new()], &[], ROOM).found.is_empty());
    }
}
