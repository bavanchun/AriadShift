use ariad_core::ir::{Block, Document, Inline, Metadata};
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
        Block::Quote { blocks, .. } => 1 + blocks_depth(blocks),
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
        Block::Code { .. } | Block::Math { .. } | Block::Raw { .. } | Block::PageBreak { .. } => 0,
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
            Inline::Styled { content, .. }
            | Inline::Emph { content }
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
            Block::Heading { content, .. } | Block::Paragraph { content, .. } => {
                assert_inlines_nfc(content);
            }
            Block::Quote { blocks, .. } => assert_blocks_nfc(blocks, depth + 1, max_depth),
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
            Block::Footnote {
                note_id: _, blocks, ..
            } => {
                // Footnote IDs are identifiers; FootnoteRef IDs are kept verbatim to maintain
                // referential integrity without altering identifier tokens, so Block::Footnote.note_id
                // is preserved verbatim as well and excluded from the NFC normalization check.
                assert_blocks_nfc(blocks, depth + 1, max_depth);
            }
            Block::Code { .. }
            | Block::Math { .. }
            | Block::Raw { .. }
            | Block::PageBreak { .. } => {}
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

/// Asserts that a deserialized `ariad-ir/1` document matches the original document under Decision D1.
///
/// Under Decision D1 (`crates/ariad-core/src/ir/mod.rs`), `IrFormat::from_format` freezes
/// the 21 format variants supported in `ir.v1.schema.json`. Formats added after `ir.v1`
/// (including Phase 10 formats: `Txt`, `Csv`, `Eml`, `Msg`, `Svg`, etc.) intentionally map to `None`
/// when serialized to `ariad-ir/1` JSON metadata to preserve schema stability.
///
/// This helper asserts that:
/// 1. `deserialized.meta.source_format` matches the expected D1 projection of `original.meta.source_format`.
/// 2. All other fields of `Document` and `Metadata` match exactly via explicit destructuring.
pub fn assert_ir_v1_round_trip(original: &Document, deserialized: &Document) {
    let expected_source_format = original
        .meta
        .source_format
        .and_then(ariad_core::ir::IrFormat::from_format)
        .map(ariad_core::ir::IrFormat::to_format);
    assert_eq!(
        deserialized.meta.source_format, expected_source_format,
        "deserialized source_format must match D1 IrFormat projection"
    );

    let Document {
        version: orig_version,
        meta:
            Metadata {
                title: orig_title,
                authors: orig_authors,
                language: orig_language,
                date: orig_date,
                subject: orig_subject,
                keywords: orig_keywords,
                source_format: _,
            },
        body: orig_body,
        assets: orig_assets,
        layout: orig_layout,
        provenance: orig_provenance,
        furniture: orig_furniture,
        styles: orig_styles,
        sections: orig_sections,
        source_map: orig_source_map,
    } = original;
    let Document {
        version: de_version,
        meta:
            Metadata {
                title: de_title,
                authors: de_authors,
                language: de_language,
                date: de_date,
                subject: de_subject,
                keywords: de_keywords,
                source_format: _,
            },
        body: de_body,
        assets: de_assets,
        layout: de_layout,
        provenance: de_provenance,
        furniture: de_furniture,
        styles: de_styles,
        sections: de_sections,
        source_map: de_source_map,
    } = deserialized;

    assert_eq!(orig_version, de_version);
    assert_eq!(orig_title, de_title);
    assert_eq!(orig_authors, de_authors);
    assert_eq!(orig_language, de_language);
    assert_eq!(orig_date, de_date);
    assert_eq!(orig_subject, de_subject);
    assert_eq!(orig_keywords, de_keywords);
    assert_eq!(orig_body, de_body);
    assert_eq!(orig_assets, de_assets);
    assert_eq!(orig_layout, de_layout);
    assert_eq!(orig_provenance, de_provenance);
    assert_eq!(orig_furniture, de_furniture);
    assert_eq!(orig_styles, de_styles);
    assert_eq!(orig_sections, de_sections);
    assert_eq!(orig_source_map, de_source_map);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ariad_core::format::Format;
    use serde::Deserialize;

    fn sample_doc_with_format(format: Format) -> Document {
        Document {
            meta: Metadata {
                title: Some("Sample Title".to_string()),
                authors: vec!["Author One".to_string()],
                language: Some("en".to_string()),
                date: Some("2026-11-10".to_string()),
                subject: Some("Testing".to_string()),
                keywords: vec!["fuzz".to_string(), "ir".to_string()],
                source_format: Some(format),
            },
            body: vec![Block::Paragraph {
                id: ariad_core::ir::BlockId::new("b1").unwrap(),
                style: None,
                direct: Default::default(),
                content: vec![Inline::Text {
                    text: "Paragraph content".to_string(),
                }],
            }],
            ..Document::default()
        }
    }

    fn round_trip_json(doc: &Document) -> Document {
        let serialized = serde_json::to_string(doc).expect("serialize doc");
        let mut de = serde_json::Deserializer::from_str(&serialized);
        de.disable_recursion_limit();
        Document::deserialize(&mut de).expect("deserialize doc")
    }

    #[test]
    fn phase_10_formats_round_trip_with_none_source_format() {
        let phase_10_formats = [
            Format::Txt,
            Format::Csv,
            Format::Eml,
            Format::Msg,
            Format::Svg,
        ];
        for fmt in phase_10_formats {
            let original = sample_doc_with_format(fmt);
            let deserialized = round_trip_json(&original);
            assert!(
                deserialized.meta.source_format.is_none(),
                "format {fmt:?} must serialize to null per D1"
            );
            assert_ir_v1_round_trip(&original, &deserialized);
        }
    }

    #[test]
    fn ir_v1_format_round_trip_preserves_source_format() {
        let original = sample_doc_with_format(Format::Markdown);
        let deserialized = round_trip_json(&original);
        assert_eq!(
            deserialized.meta.source_format,
            Some(Format::Markdown),
            "Format::Markdown must be preserved across JSON round-trip"
        );
        assert_ir_v1_round_trip(&original, &deserialized);
    }

    #[test]
    #[should_panic(expected = "deserialized source_format must match D1 IrFormat projection")]
    fn ir_v1_format_panics_if_deserialized_is_none() {
        let original = sample_doc_with_format(Format::Markdown);
        let mut deserialized = original.clone();
        deserialized.meta.source_format = None;
        assert_ir_v1_round_trip(&original, &deserialized);
    }

    #[test]
    #[should_panic]
    fn panics_on_title_difference() {
        let original = sample_doc_with_format(Format::Txt);
        let mut deserialized = round_trip_json(&original);
        deserialized.meta.title = Some("Different Title".to_string());
        assert_ir_v1_round_trip(&original, &deserialized);
    }

    #[test]
    #[should_panic]
    fn panics_on_body_difference() {
        let original = sample_doc_with_format(Format::Txt);
        let mut deserialized = round_trip_json(&original);
        deserialized.body.push(Block::Paragraph {
            id: ariad_core::ir::BlockId::new("b2").unwrap(),
            style: None,
            direct: Default::default(),
            content: vec![Inline::Text {
                text: "Extra block".to_string(),
            }],
        });
        assert_ir_v1_round_trip(&original, &deserialized);
    }

    #[test]
    #[should_panic]
    fn panics_on_authors_difference() {
        let original = sample_doc_with_format(Format::Txt);
        let mut deserialized = round_trip_json(&original);
        deserialized.meta.authors = vec!["Another Author".to_string()];
        assert_ir_v1_round_trip(&original, &deserialized);
    }

    #[test]
    #[should_panic]
    fn panics_on_language_difference() {
        let original = sample_doc_with_format(Format::Txt);
        let mut deserialized = round_trip_json(&original);
        deserialized.meta.language = Some("fr".to_string());
        assert_ir_v1_round_trip(&original, &deserialized);
    }

    #[test]
    #[should_panic]
    fn panics_on_date_difference() {
        let original = sample_doc_with_format(Format::Txt);
        let mut deserialized = round_trip_json(&original);
        deserialized.meta.date = Some("1999-01-01".to_string());
        assert_ir_v1_round_trip(&original, &deserialized);
    }

    #[test]
    #[should_panic]
    fn panics_on_subject_difference() {
        let original = sample_doc_with_format(Format::Txt);
        let mut deserialized = round_trip_json(&original);
        deserialized.meta.subject = Some("Different Subject".to_string());
        assert_ir_v1_round_trip(&original, &deserialized);
    }

    #[test]
    #[should_panic]
    fn panics_on_keywords_difference() {
        let original = sample_doc_with_format(Format::Txt);
        let mut deserialized = round_trip_json(&original);
        deserialized.meta.keywords.clear();
        assert_ir_v1_round_trip(&original, &deserialized);
    }

    #[test]
    #[should_panic]
    fn panics_on_version_difference() {
        let original = sample_doc_with_format(Format::Txt);
        let mut deserialized = round_trip_json(&original);
        deserialized.version = "ariad-ir/999".to_string();
        assert_ir_v1_round_trip(&original, &deserialized);
    }

    #[test]
    #[should_panic]
    fn panics_on_furniture_difference() {
        let original = sample_doc_with_format(Format::Txt);
        let mut deserialized = round_trip_json(&original);
        deserialized.furniture.push(Block::Paragraph {
            id: ariad_core::ir::BlockId::new("f1").unwrap(),
            style: None,
            direct: Default::default(),
            content: vec![Inline::Text {
                text: "Furniture text".to_string(),
            }],
        });
        assert_ir_v1_round_trip(&original, &deserialized);
    }
}
