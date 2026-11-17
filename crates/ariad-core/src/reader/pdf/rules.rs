//! Straight rules painted by a page's path operators.
//!
//! Path construction and painting are tracked in display space so a later table detector can read
//! ruled grids. Only horizontal and vertical segments and axis-aligned rectangles are kept; curves,
//! diagonals and clipping paths are ignored.

use super::interpreter::Matrix;

/// Most rules recorded for one page. Recording stops at the cap and flags the page, but the read
/// continues.
pub(super) const MAX_PAGE_RULES: usize = 50_000;

/// Largest deviation, in points, still treated as axis-aligned or degenerate.
const AXIS_TOLERANCE: f64 = 1e-3;

/// Thickest filled rectangle, in points, that counts as a rule rather than a shaded area.
const MAX_FILLED_RULE_THICKNESS: f64 = 3.0;

/// Geometry of a recorded rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RuleShape {
    /// A horizontal or vertical line; one pair of bounds coincides.
    Segment,
    /// An axis-aligned rectangle.
    Rect,
}

/// How a rule was painted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RulePaint {
    Stroke,
    Fill,
    FillStroke,
}

/// One straight rule in page display space (points, origin top-left, y down).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Rule {
    pub(super) shape: RuleShape,
    pub(super) paint: RulePaint,
    /// Bounds with `x0 <= x1` and `y0 <= y1`.
    pub(super) x0: f64,
    pub(super) y0: f64,
    pub(super) x1: f64,
    pub(super) y1: f64,
    /// Stroke width in display space; `0.0` for fill-only rules.
    pub(super) width: f64,
}

/// A path piece waiting for its painting operator.
#[derive(Clone, Copy, Debug)]
struct Piece {
    shape: RuleShape,
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
}

/// The path under construction.
#[derive(Debug, Default)]
pub(super) struct PathBuilder {
    pieces: Vec<Piece>,
    start: Option<(f64, f64)>,
    current: Option<(f64, f64)>,
    overflowed: bool,
}

impl PathBuilder {
    pub(super) fn move_to(&mut self, ctm: &Matrix, x: f64, y: f64) {
        let point = ctm.apply(x, y);
        self.start = Some(point);
        self.current = Some(point);
    }

    pub(super) fn line_to(&mut self, ctm: &Matrix, x: f64, y: f64) {
        let point = ctm.apply(x, y);
        self.line_to_point(point);
    }

    /// A curve contributes no rule, but its end point still starts the next segment.
    pub(super) fn curve_to(&mut self, ctm: &Matrix, x: f64, y: f64) {
        self.current = Some(ctm.apply(x, y));
    }

    pub(super) fn rect(&mut self, ctm: &Matrix, x: f64, y: f64, width: f64, height: f64) {
        let p0 = ctm.apply(x, y);
        let p1 = ctm.apply(x + width, y);
        let p2 = ctm.apply(x + width, y + height);
        self.start = Some(p0);
        self.current = Some(p0);
        let aligned = |a: (f64, f64), b: (f64, f64)| {
            (a.0 - b.0).abs() <= AXIS_TOLERANCE || (a.1 - b.1).abs() <= AXIS_TOLERANCE
        };
        if ![p0, p1, p2]
            .iter()
            .all(|p| p.0.is_finite() && p.1.is_finite())
            || !aligned(p0, p1)
            || !aligned(p1, p2)
        {
            return;
        }
        self.push(Piece {
            shape: RuleShape::Rect,
            x0: p0.0.min(p2.0),
            y0: p0.1.min(p2.1),
            x1: p0.0.max(p2.0),
            y1: p0.1.max(p2.1),
        });
    }

    pub(super) fn close(&mut self) {
        if let Some(start) = self.start {
            self.line_to_point(start);
        }
    }

    fn line_to_point(&mut self, point: (f64, f64)) {
        let Some(from) = self.current.replace(point) else {
            return;
        };
        if ![from.0, from.1, point.0, point.1]
            .iter()
            .all(|value| value.is_finite())
        {
            return;
        }
        let (dx, dy) = ((point.0 - from.0).abs(), (point.1 - from.1).abs());
        let piece = if dy <= AXIS_TOLERANCE && dx > AXIS_TOLERANCE {
            let y = f64::midpoint(from.1, point.1);
            Piece {
                shape: RuleShape::Segment,
                x0: from.0.min(point.0),
                y0: y,
                x1: from.0.max(point.0),
                y1: y,
            }
        } else if dx <= AXIS_TOLERANCE && dy > AXIS_TOLERANCE {
            let x = f64::midpoint(from.0, point.0);
            Piece {
                shape: RuleShape::Segment,
                x0: x,
                y0: from.1.min(point.1),
                x1: x,
                y1: from.1.max(point.1),
            }
        } else {
            return;
        };
        self.push(piece);
    }

    fn push(&mut self, piece: Piece) {
        if self.pieces.len() >= MAX_PAGE_RULES {
            self.overflowed = true;
        } else {
            self.pieces.push(piece);
        }
    }

    /// Discards the path under construction without painting it.
    pub(super) fn reset(&mut self) {
        *self = Self::default();
    }

    /// Ends the path. A painted path appends its rules to `rules`; `widths` holds the display-space
    /// stroke width of a horizontal and of a vertical rule. Reaching [`MAX_PAGE_RULES`] sets
    /// `truncated`.
    pub(super) fn paint(
        &mut self,
        stroke: bool,
        fill: bool,
        widths: StrokeWidths,
        rules: &mut Vec<Rule>,
        truncated: &mut bool,
    ) {
        let record = stroke || fill;
        let overflowed = self.overflowed;
        let pieces = std::mem::take(&mut self.pieces);
        self.start = None;
        self.current = None;
        self.overflowed = false;
        if !record {
            return;
        }
        *truncated |= overflowed;
        for piece in pieces {
            let Some(rule) = rule_for(piece, stroke, fill, widths) else {
                continue;
            };
            if rules.len() >= MAX_PAGE_RULES {
                *truncated = true;
                return;
            }
            rules.push(rule);
        }
    }
}

/// Display-space stroke width of a horizontal rule and of a vertical rule.
#[derive(Clone, Copy, Debug)]
pub(super) struct StrokeWidths {
    pub(super) horizontal: f64,
    pub(super) vertical: f64,
}

fn rule_for(piece: Piece, stroke: bool, fill: bool, widths: StrokeWidths) -> Option<Rule> {
    let Piece {
        shape,
        x0,
        y0,
        x1,
        y1,
    } = piece;
    let (w, h) = (x1 - x0, y1 - y0);
    let degenerate = w <= AXIS_TOLERANCE || h <= AXIS_TOLERANCE;
    let horizontal = h <= AXIS_TOLERANCE;
    let line_width = if horizontal {
        widths.horizontal
    } else {
        widths.vertical
    };
    // A rectangle's edges can differ under a non-uniform scale; the thinner one is recorded.
    let stroke_width = match shape {
        RuleShape::Segment => line_width,
        RuleShape::Rect => widths.horizontal.min(widths.vertical),
    };
    if stroke && !stroke_width.is_finite() {
        return None;
    }
    let (shape, paint, width) = match shape {
        // A segment paints only when stroked.
        RuleShape::Segment if stroke => (RuleShape::Segment, RulePaint::Stroke, stroke_width),
        RuleShape::Segment => return None,
        // A rectangle collapsed to a point has no direction; one collapsed to a line is a stroke.
        RuleShape::Rect if w <= AXIS_TOLERANCE && h <= AXIS_TOLERANCE => return None,
        RuleShape::Rect if degenerate && stroke => {
            (RuleShape::Segment, RulePaint::Stroke, line_width)
        }
        RuleShape::Rect if degenerate => return None,
        RuleShape::Rect if stroke && fill => (RuleShape::Rect, RulePaint::FillStroke, stroke_width),
        RuleShape::Rect if stroke => (RuleShape::Rect, RulePaint::Stroke, stroke_width),
        RuleShape::Rect if w.min(h) <= MAX_FILLED_RULE_THICKNESS => {
            (RuleShape::Rect, RulePaint::Fill, 0.0)
        }
        RuleShape::Rect => return None,
    };
    Some(Rule {
        shape,
        paint,
        x0,
        y0,
        x1,
        y1,
        width,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paint(path: &mut PathBuilder, stroke: bool, fill: bool) -> (Vec<Rule>, bool) {
        let mut rules = Vec::new();
        let mut truncated = false;
        path.paint(
            stroke,
            fill,
            StrokeWidths {
                horizontal: 1.0,
                vertical: 1.0,
            },
            &mut rules,
            &mut truncated,
        );
        (rules, truncated)
    }

    #[test]
    fn rule_count_is_capped_without_failing() {
        let mut path = PathBuilder::default();
        for _ in 0..=MAX_PAGE_RULES {
            path.rect(&Matrix::IDENTITY, 0.0, 0.0, 9.0, 1.0);
        }
        let (rules, truncated) = paint(&mut path, false, true);
        assert_eq!(rules.len(), MAX_PAGE_RULES);
        assert!(truncated);
    }

    #[test]
    fn large_fills_are_not_rules() {
        let mut path = PathBuilder::default();
        path.rect(&Matrix::IDENTITY, 0.0, 0.0, 100.0, 50.0);
        assert!(paint(&mut path, false, true).0.is_empty());
    }
}
