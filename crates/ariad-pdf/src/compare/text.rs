//! Text extraction and word-level diffing using `similar` Myers diff.

use ariad_core::{
    ir::{Block, Document as IrDocument, Inline},
    limits::Limits,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use similar::{Algorithm, DiffOp, capture_diff_slices};

/// A word with its text and bounding box in PDF points (bottom-left origin).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WordItem {
    /// Word text.
    pub text: String,
    /// Bounding box `[left, bottom, right, top]` in PDF points.
    pub bbox: [f64; 4],
}

/// The kind of text modification detected between before and after documents.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TextChangeKind {
    /// Text was inserted in the after document.
    Insert,
    /// Text was deleted from the before document.
    Delete,
    /// Text was modified/replaced.
    Replace,
    /// Identical text was moved to a different page position.
    Move,
}

/// A specific text difference on a page.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TextChange {
    /// Kind of change (insert, delete, replace, move).
    pub kind: TextChangeKind,
    /// Prior text in the before document, if applicable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_before: Option<String>,
    /// New text in the after document, if applicable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_after: Option<String>,
    /// Bounding box `[left, bottom, right, top]` in PDF points (bottom-left origin).
    pub bbox: [f64; 4],
}

/// Computes the union bounding box of a collection of rectangles.
#[must_use]
pub fn union_bboxes(boxes: &[[f64; 4]]) -> [f64; 4] {
    if boxes.is_empty() {
        return [0.0, 0.0, 0.0, 0.0];
    }
    let mut left = f64::INFINITY;
    let mut bottom = f64::INFINITY;
    let mut right = f64::NEG_INFINITY;
    let mut top = f64::NEG_INFINITY;
    for b in boxes {
        left = left.min(b[0]);
        bottom = bottom.min(b[1]);
        right = right.max(b[2]);
        top = top.max(b[3]);
    }
    [left, bottom, right, top]
}

/// Extracts text strings from an IR block.
fn block_text(block: &Block) -> String {
    let mut out = String::new();
    match block {
        Block::Heading { content, .. } | Block::Paragraph { content } => {
            extract_inlines(content, &mut out);
        }
        Block::Code { text, .. } => {
            out.push_str(text);
        }
        Block::Math { tex, .. } => {
            out.push_str(tex);
        }
        Block::Quote { blocks } | Block::Footnote { blocks, .. } => {
            for b in blocks {
                let t = block_text(b);
                if !t.is_empty() {
                    if !out.is_empty() {
                        out.push(' ');
                    }
                    out.push_str(&t);
                }
            }
        }
        Block::List { items, .. } => {
            for item in items {
                for b in &item.blocks {
                    let t = block_text(b);
                    if !t.is_empty() {
                        if !out.is_empty() {
                            out.push(' ');
                        }
                        out.push_str(&t);
                    }
                }
            }
        }
        Block::Table {
            caption,
            head,
            body,
            ..
        } => {
            if let Some(cap) = caption {
                extract_inlines(cap, &mut out);
            }
            for row in head.iter().chain(body.iter()) {
                for cell in row {
                    for b in &cell.blocks {
                        let t = block_text(b);
                        if !t.is_empty() {
                            if !out.is_empty() {
                                out.push(' ');
                            }
                            out.push_str(&t);
                        }
                    }
                }
            }
        }
        Block::Figure { caption, .. } => {
            extract_inlines(caption, &mut out);
        }
        Block::Raw { text, .. } => {
            out.push_str(text);
        }
        Block::PageBreak {} => {}
    }
    out
}

fn extract_inlines(inlines: &[Inline], out: &mut String) {
    for inline in inlines {
        match inline {
            Inline::Text { text } | Inline::Code { text } | Inline::Raw { text, .. } => {
                out.push_str(text);
            }
            Inline::Math { tex, .. } => {
                out.push_str(tex);
            }
            Inline::Emph { content }
            | Inline::Strong { content }
            | Inline::Strikeout { content }
            | Inline::Superscript { content }
            | Inline::Subscript { content }
            | Inline::Link { content, .. } => {
                extract_inlines(content, out);
            }
            Inline::SoftBreak {} | Inline::LineBreak {} => {
                out.push(' ');
            }
            Inline::Image { alt, .. } => {
                out.push_str(alt);
            }
            Inline::FootnoteRef { .. } => {}
        }
    }
}

/// Extracts reading-order words and calculated bounding boxes from an `IrDocument` for `page_no`.
#[must_use]
pub fn extract_words_from_reader(
    doc: &IrDocument,
    page_no: u32,
    _limits: &Limits,
) -> Vec<WordItem> {
    let layout = doc.layout.as_ref();
    let page_height = layout
        .and_then(|l| l.pages.iter().find(|p| p.page_no == page_no))
        .map(|p| p.height)
        .unwrap_or(792.0);

    let mut words = Vec::new();

    let Some(layout) = layout else {
        return words;
    };

    // Iterate through blocks associated with `page_no` in reading order
    for (idx, block) in doc.body.iter().enumerate() {
        let path = format!("body/{idx}");
        let Some(pos) = layout.blocks.get(&path) else {
            continue;
        };
        if pos.page_no != page_no {
            continue;
        }

        let text = block_text(block);
        let raw_words: Vec<&str> = text.split_whitespace().collect();
        if raw_words.is_empty() {
            continue;
        }

        // Convert block top-left coordinates to PDF bottom-left points
        let block_left = pos.bbox.left;
        let block_right = pos.bbox.right.max(pos.bbox.left + 1.0);
        let block_bottom = (page_height - pos.bbox.bottom).max(0.0);
        let block_top = (page_height - pos.bbox.top).max(block_bottom + 1.0);

        let total_chars: usize = raw_words.iter().map(|w| w.chars().count()).sum();
        let total_chars = total_chars.max(1) as f64;
        let mut curr_char = 0usize;

        for w in raw_words {
            let char_len = w.chars().count();
            let start_frac = (curr_char as f64) / total_chars;
            let end_frac = ((curr_char + char_len) as f64) / total_chars;
            curr_char += char_len;

            let w_left = block_left + (block_right - block_left) * start_frac;
            let w_right = block_left + (block_right - block_left) * end_frac;

            words.push(WordItem {
                text: w.to_string(),
                bbox: [w_left, block_bottom, w_right, block_top],
            });
        }
    }

    words
}

/// Diffs word sequences from before and after pages using `similar 3.2.1` Myers algorithm.
#[must_use]
pub fn diff_words(before_words: &[WordItem], after_words: &[WordItem]) -> Vec<TextChange> {
    let before_texts: Vec<&str> = before_words.iter().map(|w| w.text.as_str()).collect();
    let after_texts: Vec<&str> = after_words.iter().map(|w| w.text.as_str()).collect();

    let ops = capture_diff_slices(Algorithm::Myers, &before_texts, &after_texts);
    let mut changes = Vec::new();

    for op in ops {
        match op {
            DiffOp::Equal {
                old_index,
                new_index,
                len,
            } => {
                let mut moved_before = Vec::new();
                let mut moved_after = Vec::new();

                for i in 0..len {
                    let bw = &before_words[old_index + i];
                    let aw = &after_words[new_index + i];

                    let dx = (bw.bbox[0] - aw.bbox[0]).abs();
                    let dy = (bw.bbox[1] - aw.bbox[1]).abs();

                    if dx > 15.0 || dy > 15.0 {
                        moved_before.push(bw);
                        moved_after.push(aw);
                    } else if !moved_after.is_empty() {
                        let text = moved_after
                            .iter()
                            .map(|w| w.text.as_str())
                            .collect::<Vec<_>>()
                            .join(" ");
                        let bboxes: Vec<[f64; 4]> = moved_after.iter().map(|w| w.bbox).collect();
                        changes.push(TextChange {
                            kind: TextChangeKind::Move,
                            text_before: Some(text.clone()),
                            text_after: Some(text),
                            bbox: union_bboxes(&bboxes),
                        });
                        moved_before.clear();
                        moved_after.clear();
                    }
                }
                if !moved_after.is_empty() {
                    let text = moved_after
                        .iter()
                        .map(|w| w.text.as_str())
                        .collect::<Vec<_>>()
                        .join(" ");
                    let bboxes: Vec<[f64; 4]> = moved_after.iter().map(|w| w.bbox).collect();
                    changes.push(TextChange {
                        kind: TextChangeKind::Move,
                        text_before: Some(text.clone()),
                        text_after: Some(text),
                        bbox: union_bboxes(&bboxes),
                    });
                }
            }
            DiffOp::Delete {
                old_index, old_len, ..
            } => {
                let deleted = &before_words[old_index..old_index + old_len];
                let text = deleted
                    .iter()
                    .map(|w| w.text.as_str())
                    .collect::<Vec<_>>()
                    .join(" ");
                let bboxes: Vec<[f64; 4]> = deleted.iter().map(|w| w.bbox).collect();
                changes.push(TextChange {
                    kind: TextChangeKind::Delete,
                    text_before: Some(text),
                    text_after: None,
                    bbox: union_bboxes(&bboxes),
                });
            }
            DiffOp::Insert {
                new_index, new_len, ..
            } => {
                let inserted = &after_words[new_index..new_index + new_len];
                let text = inserted
                    .iter()
                    .map(|w| w.text.as_str())
                    .collect::<Vec<_>>()
                    .join(" ");
                let bboxes: Vec<[f64; 4]> = inserted.iter().map(|w| w.bbox).collect();
                changes.push(TextChange {
                    kind: TextChangeKind::Insert,
                    text_before: None,
                    text_after: Some(text),
                    bbox: union_bboxes(&bboxes),
                });
            }
            DiffOp::Replace {
                old_index,
                old_len,
                new_index,
                new_len,
            } => {
                let old_slice = &before_words[old_index..old_index + old_len];
                let new_slice = &after_words[new_index..new_index + new_len];
                let before_text = old_slice
                    .iter()
                    .map(|w| w.text.as_str())
                    .collect::<Vec<_>>()
                    .join(" ");
                let after_text = new_slice
                    .iter()
                    .map(|w| w.text.as_str())
                    .collect::<Vec<_>>()
                    .join(" ");
                let bboxes: Vec<[f64; 4]> = new_slice.iter().map(|w| w.bbox).collect();
                changes.push(TextChange {
                    kind: TextChangeKind::Replace,
                    text_before: Some(before_text),
                    text_after: Some(after_text),
                    bbox: union_bboxes(&bboxes),
                });
            }
        }
    }

    changes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_detects_insert_delete_replace_and_move() {
        let before = vec![
            WordItem {
                text: "Hello".into(),
                bbox: [50.0, 700.0, 90.0, 712.0],
            },
            WordItem {
                text: "original".into(),
                bbox: [50.0, 680.0, 100.0, 692.0],
            },
            WordItem {
                text: "AnchorOne".into(),
                bbox: [50.0, 660.0, 120.0, 672.0],
            },
            WordItem {
                text: "deleted".into(),
                bbox: [50.0, 650.0, 95.0, 662.0],
            },
            WordItem {
                text: "AnchorTwo".into(),
                bbox: [50.0, 630.0, 120.0, 642.0],
            },
            WordItem {
                text: "moved".into(),
                bbox: [50.0, 800.0, 90.0, 812.0],
            },
        ];

        let after = vec![
            WordItem {
                text: "Hello".into(),
                bbox: [50.0, 700.0, 90.0, 712.0],
            },
            WordItem {
                text: "modified".into(),
                bbox: [50.0, 680.0, 105.0, 692.0],
            },
            WordItem {
                text: "AnchorOne".into(),
                bbox: [50.0, 660.0, 120.0, 672.0],
            },
            // "deleted" is absent
            WordItem {
                text: "AnchorTwo".into(),
                bbox: [50.0, 630.0, 120.0, 642.0],
            },
            WordItem {
                text: "inserted".into(),
                bbox: [50.0, 620.0, 100.0, 632.0],
            },
            WordItem {
                text: "moved".into(),
                bbox: [50.0, 600.0, 90.0, 612.0], // moved down by 200pt
            },
        ];

        let changes = diff_words(&before, &after);
        assert_eq!(changes.len(), 4);

        assert_eq!(changes[0].kind, TextChangeKind::Replace);
        assert_eq!(changes[0].text_before.as_deref(), Some("original"));
        assert_eq!(changes[0].text_after.as_deref(), Some("modified"));

        assert_eq!(changes[1].kind, TextChangeKind::Delete);
        assert_eq!(changes[1].text_before.as_deref(), Some("deleted"));
        assert_eq!(changes[1].text_after, None);

        assert_eq!(changes[2].kind, TextChangeKind::Insert);
        assert_eq!(changes[2].text_before, None);
        assert_eq!(changes[2].text_after.as_deref(), Some("inserted"));

        assert_eq!(changes[3].kind, TextChangeKind::Move);
        assert_eq!(changes[3].text_before.as_deref(), Some("moved"));
        assert_eq!(changes[3].text_after.as_deref(), Some("moved"));
    }

    #[test]
    fn identical_pages_report_zero_changes() {
        let words = vec![
            WordItem {
                text: "Unchanged".into(),
                bbox: [50.0, 700.0, 120.0, 712.0],
            },
            WordItem {
                text: "Page".into(),
                bbox: [125.0, 700.0, 160.0, 712.0],
            },
        ];
        let changes = diff_words(&words, &words);
        assert!(changes.is_empty());
    }
}
