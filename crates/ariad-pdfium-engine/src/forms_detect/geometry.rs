//! Geometric signal processing for field proposals in PDF user space.

use ariad_core::tools::params::{
    form_model::FieldType,
    forms::{DetectedField, NewField},
};
use std::collections::HashMap;

#[derive(Clone, Debug)]
pub(super) struct Label {
    pub text: String,
    pub rect: [f64; 4],
    pub size: f64,
}
#[derive(Clone, Copy, Debug)]
pub(super) struct Edge {
    pub a: [f64; 2],
    pub b: [f64; 2],
}
#[derive(Clone, Debug)]
pub(super) struct Candidate {
    pub rect: [f64; 4],
    pub kind: FieldType,
    pub confidence: f64,
    pub reason: &'static str,
}

pub(super) fn area(r: [f64; 4]) -> f64 {
    (r[2] - r[0]).max(0.0) * (r[3] - r[1]).max(0.0)
}
pub(super) fn intersection(a: [f64; 4], b: [f64; 4]) -> f64 {
    area([
        a[0].max(b[0]),
        a[1].max(b[1]),
        a[2].min(b[2]),
        a[3].min(b[3]),
    ])
}
pub(super) fn iou(a: [f64; 4], b: [f64; 4]) -> f64 {
    let overlap = intersection(a, b);
    let union = area(a) + area(b) - overlap;
    if union > 0.0 { overlap / union } else { 0.0 }
}
pub(super) fn overlaps_widget(
    rect: [f64; 4],
    widgets: &[[f64; 4]],
    ctx: &crate::output::ToolContext<'_>,
) -> Result<bool, ariad_engine_io::EngineFailure> {
    for (index, widget) in widgets.iter().enumerate() {
        if index % 1024 == 0 {
            ctx.check()?;
        }
        if iou(rect, *widget) > 0.3 {
            return Ok(true);
        }
    }
    Ok(false)
}
fn horizontal(e: Edge) -> Option<(f64, f64, f64)> {
    if (e.a[1] - e.b[1]).abs() > 1.25 {
        return None;
    }
    Some((
        e.a[0].min(e.b[0]),
        e.a[0].max(e.b[0]),
        (e.a[1] + e.b[1]) / 2.0,
    ))
}
fn vertical(e: Edge) -> Option<(f64, f64, f64)> {
    if (e.a[0] - e.b[0]).abs() > 1.25 {
        return None;
    }
    Some((
        (e.a[0] + e.b[0]) / 2.0,
        e.a[1].min(e.b[1]),
        e.a[1].max(e.b[1]),
    ))
}
fn near(a: f64, b: f64) -> bool {
    (a - b).abs() <= 2.0
}
fn supported_box(r: [f64; 4]) -> Option<FieldType> {
    let w = r[2] - r[0];
    let h = r[3] - r[1];
    if (6.0..=20.0).contains(&w) && (6.0..=20.0).contains(&h) && (w - h).abs() <= w.max(h) * 0.22 {
        Some(FieldType::Checkbox)
    } else if w >= 36.0 && h >= 12.0 && h <= 300.0 {
        Some(FieldType::Text)
    } else {
        None
    }
}
fn has_text_inside(
    r: [f64; 4],
    words: &[[f64; 4]],
    ctx: &crate::output::ToolContext<'_>,
) -> Result<bool, ariad_engine_io::EngineFailure> {
    for (index, word) in words.iter().enumerate() {
        if index % 1024 == 0 {
            ctx.check()?;
        }
        let center = [(word[0] + word[2]) / 2.0, (word[1] + word[3]) / 2.0];
        if center[0] > r[0] + 1.0
            && center[0] < r[2] - 1.0
            && center[1] > r[1] + 1.0
            && center[1] < r[3] - 1.0
        {
            return Ok(true);
        }
    }
    Ok(false)
}
fn nearest<'a>(
    rect: [f64; 4],
    labels: &'a [Label],
    allow_right: bool,
    ctx: &crate::output::ToolContext<'_>,
) -> Result<Option<&'a Label>, ariad_engine_io::EngineFailure> {
    let mut best: Option<(f64, &Label)> = None;
    for (index, label) in labels.iter().enumerate() {
        if index % 1024 == 0 {
            ctx.check()?;
        }
        let cy = (rect[1] + rect[3]) / 2.0;
        let ly = (label.rect[1] + label.rect[3]) / 2.0;
        let dx = rect[0] - label.rect[2];
        let dy = (cy - ly).abs();
        let score = if (-3.0..=200.0).contains(&dx)
            && dy <= (rect[3] - rect[1]).max(label.size) * 0.9 + 4.0
        {
            Some(dx.max(0.0) + dy * 2.0)
        } else if allow_right
            && (0.0..=200.0).contains(&(label.rect[0] - rect[2]))
            && dy <= (rect[3] - rect[1]).max(label.size) * 0.9 + 4.0
        {
            Some(label.rect[0] - rect[2] + dy * 2.0)
        } else {
            let gap = label.rect[1] - rect[3];
            let overlap = (rect[2].min(label.rect[2]) - rect[0].max(label.rect[0])).max(0.0);
            if (0.0..=24.0).contains(&gap) && overlap > 0.0 {
                Some(gap * 2.0 + 20.0)
            } else {
                None
            }
        };
        if let Some(score) = score {
            if best
                .as_ref()
                .is_none_or(|(old, _)| score.total_cmp(old).is_lt())
            {
                best = Some((score, label));
            }
        }
    }
    Ok(best.map(|(_, label)| label))
}
pub(super) fn slug(label: &str) -> String {
    ariad_pdf::naming::field_name_from_label(label)
}

fn signature(label: &str) -> bool {
    let s = slug(label);
    s.contains("signature")
        || s.contains("sign_here")
        || s.contains("chu_ky")
        || s.contains("ky_ten")
}

/// Recognize rectangles formed from separate edges and rules not used as borders.
pub(super) fn candidates(
    edges: &[Edge],
    direct_boxes: &[[f64; 4]],
    labels: &[Label],
    words: &[[f64; 4]],
    widgets: &[[f64; 4]],
    ctx: &crate::output::ToolContext<'_>,
) -> Result<(Vec<Candidate>, bool), ariad_engine_io::EngineFailure> {
    const MAX_WORK: usize = 1_000_000;
    let mut boxes: Vec<_> = direct_boxes.iter().copied().take(5000).collect();
    let h: Vec<_> = edges
        .iter()
        .filter_map(|e| horizontal(*e))
        .filter(|(l, r, _)| r - l >= 6.0)
        .take(5000)
        .collect();
    let v: Vec<_> = edges
        .iter()
        .filter_map(|e| vertical(*e))
        .filter(|(_, b, t)| t - b >= 6.0)
        .take(5000)
        .collect();
    let mut work = 0usize;
    let mut truncated = direct_boxes.len() > 5000 || edges.len() > 5000;
    'outer: for (i, &(l, r, y)) in h.iter().enumerate() {
        for &(l2, r2, y2) in h.iter().skip(i + 1) {
            work += 1;
            if work % 1024 == 0 {
                ctx.check()?;
            }
            if work > MAX_WORK {
                truncated = true;
                break 'outer;
            }
            if !near(l, l2) || !near(r, r2) || (y - y2).abs() < 6.0 {
                continue;
            }
            let b = y.min(y2);
            let t = y.max(y2);
            let mut left = false;
            let mut right = false;
            for &(x, yb, yt) in &v {
                work += 1;
                if work % 1024 == 0 {
                    ctx.check()?;
                }
                if work > MAX_WORK {
                    truncated = true;
                    break 'outer;
                }
                if yb <= b + 2.0 && yt >= t - 2.0 {
                    left |= near(x, l);
                    right |= near(x, r);
                    if left && right {
                        break;
                    }
                }
            }
            if left && right {
                boxes.push([l, b, r, t]);
                if boxes.len() >= 5000 {
                    truncated = true;
                    break 'outer;
                }
            }
        }
    }
    let mut output = Vec::new();
    for (i, rect) in boxes.iter().enumerate() {
        if i % 256 == 0 {
            ctx.check()?;
        }
        if let Some(kind) = supported_box(*rect) {
            if has_text_inside(*rect, words, ctx)? || overlaps_widget(*rect, widgets, ctx)? {
                continue;
            }
            let Some(label) = nearest(*rect, labels, kind == FieldType::Checkbox, ctx)? else {
                continue;
            };
            let rect = [rect[0] + 1.0, rect[1] + 1.0, rect[2] - 1.0, rect[3] - 1.0];
            if area(rect) == 0.0 {
                continue;
            }
            output.push(Candidate {
                rect,
                kind,
                confidence: if label.text.is_empty() { 0.55 } else { 0.9 },
                reason: if kind == FieldType::Checkbox {
                    "square"
                } else {
                    "empty_box"
                },
            });
            if output.len() > 1000 {
                truncated = true;
                break;
            }
        }
    }
    for (index, (l, r, y)) in h.into_iter().enumerate() {
        if index % 256 == 0 {
            ctx.check()?;
        }
        if r - l < 36.0 {
            continue;
        }
        let mut border = false;
        for (box_index, b) in boxes.iter().enumerate() {
            if box_index % 1024 == 0 {
                ctx.check()?;
            }
            if near(l, b[0]) && near(r, b[2]) && (near(y, b[1]) || near(y, b[3])) {
                border = true;
                break;
            }
        }
        if border {
            continue;
        }
        let provisional = [l, y, r, y + 14.4];
        let Some(label) = nearest(provisional, labels, false, ctx)? else {
            continue;
        };
        let rect = [l, y, r, y + (label.size * 1.2).clamp(10.0, 30.0)];
        if overlaps_widget(rect, widgets, ctx)? {
            continue;
        }
        output.push(Candidate {
            rect,
            kind: if signature(&label.text) {
                FieldType::Signature
            } else {
                FieldType::Text
            },
            confidence: 0.88,
            reason: "horizontal_rule",
        });
        if output.len() > 1000 {
            truncated = true;
            break;
        }
    }
    Ok((output, truncated))
}

/// Merge overlaps and turn geometry into validated form-creation proposals.
pub(super) fn proposals(
    mut candidates: Vec<Candidate>,
    labels: &[Label],
    page: u32,
    min_confidence: f64,
    names: &mut HashMap<String, usize>,
    ctx: &crate::output::ToolContext<'_>,
) -> Result<Vec<DetectedField>, ariad_engine_io::EngineFailure> {
    candidates.sort_by(|a, b| b.confidence.total_cmp(&a.confidence));
    let mut accepted: Vec<Candidate> = Vec::new();
    for (index, candidate) in candidates.into_iter().enumerate() {
        if index % 256 == 0 {
            ctx.check()?;
        }
        if candidate.confidence < min_confidence
            || accepted
                .iter()
                .any(|other| iou(candidate.rect, other.rect) > 0.5)
        {
            continue;
        }
        accepted.push(candidate);
    }
    accepted.sort_by(|a, b| {
        b.rect[3]
            .total_cmp(&a.rect[3])
            .then_with(|| a.rect[0].total_cmp(&b.rect[0]))
    });
    let mut output = Vec::new();
    for candidate in accepted {
        let label = nearest(
            candidate.rect,
            labels,
            candidate.kind == FieldType::Checkbox,
            ctx,
        )?
        .map(|v| v.text.trim().to_owned())
        .filter(|s| !s.is_empty());
        let base = slug(label.as_deref().unwrap_or(""));
        let count = names.entry(base.clone()).or_default();
        *count += 1;
        let name = if *count == 1 {
            base
        } else {
            let suffix = format!("_{}", *count);
            let keep = 40usize.saturating_sub(suffix.len());
            format!("{}{}", &base[..base.len().min(keep)], suffix)
        };
        let proposal = NewField {
            field_type: candidate.kind,
            name,
            page,
            rect: candidate.rect,
            options: None,
            default: None,
            required: false,
            tooltip: None,
            tab_order: None,
            export_value: (candidate.kind == FieldType::Checkbox).then(|| "Yes".into()),
            multiline: false,
            max_length: None,
            font_size: 0.0,
            read_only: false,
        };
        if proposal.validate().is_ok() {
            output.push(DetectedField {
                proposal,
                label,
                confidence: candidate.confidence,
                reason: candidate.reason.into(),
            });
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn names_are_portable_and_vietnamese_is_transliterated() {
        assert_eq!(slug("Chữ ký"), "chu_ky");
        assert_eq!(slug("Địa chỉ"), "dia_chi");
        assert_eq!(slug("..."), "field");
        assert!(slug(&"á".repeat(100)).len() <= 40);
    }
    #[test]
    fn intersection_and_iou_are_symmetric() {
        let a = [0.0, 0.0, 10.0, 10.0];
        let b = [5.0, 5.0, 15.0, 15.0];
        assert_eq!(intersection(a, b), 25.0);
        assert_eq!(iou(a, b), iou(b, a));
        assert_eq!(iou(a, a), 1.0);
    }
}
