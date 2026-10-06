use ariad_core::ir::{Block, Document, Inline};
use unicode_normalization::UnicodeNormalization;

/// Normalizes a UTF-8 string to Unicode Normalization Form C (NFC).
#[must_use]
pub fn nfc(s: &str) -> String {
    s.nfc().collect()
}

/// Recursively computes the maximum block nesting depth of a single block.
#[must_use]
pub fn block_depth(block: &Block) -> usize {
    match block {
        Block::Heading { .. } | Block::Paragraph { .. } => 0,
        Block::Quote { blocks } => 1 + blocks_depth(blocks),
        Block::List { items, .. } => {
            1 + items
                .iter()
                .map(|item| blocks_depth(&item.blocks))
                .max()
                .unwrap_or(0)
        }
        Block::Table { head, body, .. } => {
            1 + head
                .iter()
                .chain(body.iter())
                .flat_map(|row| row.iter())
                .map(|cell| blocks_depth(&cell.blocks))
                .max()
                .unwrap_or(0)
        }
        Block::Figure { .. } => 0,
        Block::Footnote { blocks, .. } => 1 + blocks_depth(blocks),
        Block::Code { .. } | Block::Math { .. } | Block::Raw { .. } | Block::PageBreak {} => 0,
    }
}

/// Recursively computes the maximum block nesting depth in a slice of blocks.
#[must_use]
pub fn blocks_depth(blocks: &[Block]) -> usize {
    blocks.iter().map(block_depth).max().unwrap_or(0)
}

/// Asserts that block nesting depth never exceeds `max_depth`.
pub fn assert_blocks_depth(blocks: &[Block], max_depth: usize) {
    let depth = blocks_depth(blocks);
    assert!(
        depth <= max_depth,
        "block nesting depth {depth} exceeds max_depth {max_depth}"
    );
}

/// Recursively asserts that all human-readable text in inlines is NFC-normalized.
pub fn assert_inlines_nfc(inlines: &[Inline]) {
    for inline in inlines {
        match inline {
            Inline::Text { text } => assert_eq!(text, &nfc(text), "Inline text must be NFC"),
            Inline::Emph { content }
            | Inline::Strong { content }
            | Inline::Strikeout { content }
            | Inline::Superscript { content }
            | Inline::Subscript { content } => assert_inlines_nfc(content),
            Inline::Link { title, content, .. } => {
                if let Some(t) = title {
                    assert_eq!(t, &nfc(t), "Link title must be NFC");
                }
                assert_inlines_nfc(content);
            }
            Inline::Image { alt, title, .. } => {
                assert_eq!(alt, &nfc(alt), "Image alt must be NFC");
                if let Some(t) = title {
                    assert_eq!(t, &nfc(t), "Image title must be NFC");
                }
            }
            Inline::FootnoteRef { .. }
            | Inline::Code { .. }
            | Inline::Math { .. }
            | Inline::Raw { .. }
            | Inline::LineBreak {}
            | Inline::SoftBreak {} => {}
        }
    }
}

/// Recursively asserts that all block content is NFC-normalized and depth <= `max_depth`.
pub fn assert_blocks_nfc(blocks: &[Block], depth: usize, max_depth: usize) {
    assert!(
        depth <= max_depth,
        "block nesting depth {depth} exceeds limit {max_depth}"
    );
    for block in blocks {
        match block {
            Block::Heading { content, .. } | Block::Paragraph { content } => {
                assert_inlines_nfc(content);
            }
            Block::Quote { blocks } => assert_blocks_nfc(blocks, depth + 1, max_depth),
            Block::List { items, .. } => {
                for item in items {
                    assert_blocks_nfc(&item.blocks, depth + 1, max_depth);
                }
            }
            Block::Table {
                caption,
                head,
                body,
                footnotes,
                ..
            } => {
                if let Some(cap) = caption {
                    assert_inlines_nfc(cap);
                }
                for row in head.iter().chain(body.iter()) {
                    for cell in row {
                        assert_blocks_nfc(&cell.blocks, depth + 1, max_depth);
                    }
                }
                assert_inlines_nfc(footnotes);
            }
            Block::Figure { caption, .. } => {
                assert_inlines_nfc(caption);
            }
            Block::Footnote { id: _, blocks } => {
                // Footnote IDs are identifiers; FootnoteRef IDs are kept verbatim to maintain
                // referential integrity without altering identifier tokens, so Block::Footnote.id
                // is preserved verbatim as well and excluded from the NFC normalization check.
                assert_blocks_nfc(blocks, depth + 1, max_depth);
            }
            Block::Code { .. } | Block::Math { .. } | Block::Raw { .. } | Block::PageBreak {} => {}
        }
    }
}

/// Asserts that document metadata and all blocks satisfy NFC normalization and nesting limits.
pub fn assert_document_nfc(doc: &Document, max_depth: usize) {
    if let Some(ref title) = doc.meta.title {
        assert_eq!(title, &nfc(title), "Metadata title must be NFC");
    }
    for author in &doc.meta.authors {
        assert_eq!(author, &nfc(author), "Metadata author must be NFC");
    }
    if let Some(ref subject) = doc.meta.subject {
        assert_eq!(subject, &nfc(subject), "Metadata subject must be NFC");
    }
    if let Some(ref date) = doc.meta.date {
        assert_eq!(date, &nfc(date), "Metadata date must be NFC");
    }
    for kw in &doc.meta.keywords {
        assert_eq!(kw, &nfc(kw), "Metadata keyword must be NFC");
    }
    assert_blocks_nfc(&doc.body, 0, max_depth);
    assert_blocks_nfc(&doc.furniture, 0, max_depth);
}
