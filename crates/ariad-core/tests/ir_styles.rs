mod support;

use ariad_core::{
    format::Format,
    ir::*,
    limits::Limits,
    pipeline::{self, WriteOptions},
    reader::markdown,
    writer,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs};

fn paragraph(content: Vec<Inline>) -> Block {
    Block::Paragraph {
        id: BlockId::default(),
        style: None,
        direct: ParagraphStyle::default(),
        content,
    }
}
fn text(value: &str) -> Inline {
    Inline::Text { text: value.into() }
}
fn styled(content: Vec<Inline>) -> Inline {
    Inline::Styled {
        style: None,
        direct: CharacterStyle::default(),
        content,
    }
}
fn read(document: &Document) -> Result<Document, pipeline::PipelineError> {
    pipeline::read_ir_json(&serde_json::to_vec(document).unwrap(), &Limits::local())
}
fn geometry() -> BlockPosition {
    BlockPosition {
        page_no: 1,
        bbox: BoundingBox {
            left: 10.0,
            top: 10.0,
            right: 90.0,
            bottom: 30.0,
        },
    }
}
fn positioned() -> Document {
    let mut doc = markdown::read("same\n\nsame", &Limits::local())
        .unwrap()
        .document;
    doc.furniture.push(paragraph(vec![text("header")]));
    doc.assign_block_ids();
    doc.layout = Some(LayoutIndex {
        pages: vec![PageGeometry {
            page_no: 1,
            width: 600.0,
            height: 800.0,
        }],
        blocks: BTreeMap::from([(doc.body[0].id().clone(), geometry())]),
    });
    doc.source_map
        .insert(doc.body[0].id().clone(), vec![geometry(), geometry()]);
    doc
}

#[test]
fn ir_v2_legacy_read_upgrade_preserves_content() {
    for version in ["ariad-ir/0", "ariad-ir/1"] {
        let bytes = b"image bytes";
        let id = hex::encode(Sha256::digest(bytes));
        let input = json!({"version":version,"body":[{"type":"quote","blocks":[{"type":"paragraph","content":[{"type":"strong","content":[{"type":"image","target":{"type":"asset","id":id},"alt":"image","title":null}]}]}]}],"assets":{id.clone():{"media_type":"image/png","bytes":"aW1hZ2UgYnl0ZXM="}},"layout":{"pages":[{"page_no":1,"width":600.0,"height":800.0}],"blocks":{"body/0/blocks/0":geometry()}}});
        let doc =
            pipeline::read_ir_json(&serde_json::to_vec(&input).unwrap(), &Limits::local()).unwrap();
        assert_eq!(doc.version, IR_VERSION);
        assert_eq!(doc.block_count(), 2);
        assert_eq!(doc.assets[&id].bytes, bytes);
        assert_eq!(doc.layout.as_ref().unwrap().blocks.len(), 1);
        assert!(
            doc.layout
                .as_ref()
                .unwrap()
                .blocks
                .keys()
                .all(|id| id.as_str().starts_with('b'))
        );
        assert_eq!(
            serde_json::to_value(doc.to_v1().unwrap()).unwrap()["body"],
            input["body"]
        );
    }
    for input in [
        json!({}),
        json!({"version":null}),
        json!({"version":2}),
        json!({"version":"ariad-ir/99"}),
        json!({"version":IR_VERSION,"body":[{"type":"paragraph","content":[]}]}),
    ] {
        assert!(
            pipeline::read_ir_json(&serde_json::to_vec(&input).unwrap(), &Limits::local()).is_err()
        );
    }
}

#[test]
fn ir_v1_rewrite_keeps_frozen_goldens() {
    let mut checked = 0;
    let mut redacted = 0;
    for entry in fs::read_dir(support::repository_root().join("fixtures/golden")).unwrap() {
        let path = entry.unwrap().path();
        if !path.to_string_lossy().ends_with(".ir.snap") {
            continue;
        }
        let snapshot = fs::read_to_string(&path).unwrap();
        let (_, payload) = snapshot.split_once("\n---\n").unwrap();
        ariad_core::json_depth::prescan_json_depth(
            payload.as_bytes(),
            ariad_core::json_depth::json_depth_budget_for_nesting(
                Limits::local().max_nesting_depth,
            ),
        )
        .unwrap();
        let mut deserializer = serde_json::Deserializer::from_str(payload);
        deserializer.disable_recursion_limit();
        let mut value: serde_json::Value =
            serde::Deserialize::deserialize(&mut deserializer).unwrap();
        deserializer.end().unwrap();
        if value.get("document").is_some() {
            value = value["document"].clone();
        }
        // Docling snapshots deliberately replace embedded bytes with a size marker.
        if value["assets"].as_object().is_some_and(|assets| {
            assets
                .values()
                .any(|a| a["bytes"].as_str().is_some_and(|b| b.starts_with('<')))
        }) {
            redacted += 1;
            continue;
        }
        let frozen: legacy::Document = serde_json::from_value(value.clone()).unwrap();
        let doc =
            pipeline::read_ir_json(&serde_json::to_vec(&value).unwrap(), &Limits::local()).unwrap();
        assert_eq!(
            serde_json::to_vec_pretty(&doc.to_v1().unwrap()).unwrap(),
            serde_json::to_vec_pretty(&frozen).unwrap(),
            "{}",
            path.display()
        );
        assert_eq!(serde_json::to_value(doc.to_v1().unwrap()).unwrap(), value);
        checked += 1;
    }
    eprintln!(
        "Rewrote {checked} real wire goldens; {redacted} redacted display snapshots are checked against real reader outputs by ir_snapshots and reader_golden."
    );
    assert!(
        checked >= 20,
        "exercise the real frozen legacy golden corpus"
    );
}

#[test]
fn ir_v2_schema_validates_styles_sections_and_font_flags() {
    let mut doc = positioned();
    let style_id = StyleId::new("Body").unwrap();
    doc.styles.paragraph.insert(
        style_id.clone(),
        ParagraphStyle {
            font: Some(FontRef {
                family: "Noto Sans".into(),
                fallback: vec!["sans-serif".into()],
            }),
            size_pt: Some(12.0),
            line_height: Some(1.2),
            align: Some(ParagraphAlignment::Justify),
            ..Default::default()
        },
    );
    if let Block::Paragraph { style, .. } = &mut doc.body[0] {
        *style = Some(style_id);
    }
    doc.styles.character.insert(
        StyleId::new("Span").unwrap(),
        CharacterStyle {
            weight: Some(600),
            italic: Some(false),
            color: Some(Colour::new("#12ABef").unwrap()),
            letter_spacing: Some(-0.1),
            ..Default::default()
        },
    );
    doc.styles.table.insert(
        StyleId::new("Grid").unwrap(),
        TableStyle {
            background: Some(Colour::new("#ffffff").unwrap()),
            ..Default::default()
        },
    );
    doc.sections.push(Section {
        start: None,
        page_size: PageSize {
            width_pt: 600.0,
            height_pt: 800.0,
        },
        margins: Margins {
            top_pt: 30.0,
            right_pt: 30.0,
            bottom_pt: 30.0,
            left_pt: 30.0,
        },
        columns: 2,
        header: Some(doc.furniture[0].id().clone()),
        footer: None,
    });
    let bytes = b"font bytes".to_vec();
    doc.assets.insert(
        hex::encode(Sha256::digest(&bytes)),
        Asset {
            media_type: "font/ttf".into(),
            bytes,
            font: Some(FontAsset {
                family: "Noto Sans".into(),
                embedding: FontEmbedding::Allowed,
            }),
        },
    );
    assert_eq!(read(&doc).unwrap(), doc);
    let schema: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(support::ir_schema_path()).unwrap()).unwrap();
    assert!(
        jsonschema::validator_for(&schema)
            .unwrap()
            .is_valid(&serde_json::to_value(&doc).unwrap())
    );
    let mut invalid = serde_json::to_value(&doc).unwrap();
    invalid["body"][0].as_object_mut().unwrap().remove("id");
    assert!(
        !jsonschema::validator_for(&schema)
            .unwrap()
            .is_valid(&invalid)
    );
    let raw = serde_json::to_string(&doc).unwrap();
    assert!(raw.contains("#12abef"));
    assert!(raw.contains("\"italic\":false"));
}

#[test]
fn ir_v2_ids_survive_insert_reorder_and_furniture_roundtrip() {
    let mut doc = positioned();
    let original = doc.body.iter().map(|b| b.id().clone()).collect::<Vec<_>>();
    assert_ne!(original[0], original[1]);
    assert_eq!(positioned(), doc);
    doc.body.swap(0, 1);
    doc.body.insert(0, paragraph(vec![text("same")]));
    doc.assign_block_ids();
    assert_eq!(doc.body[1].id(), &original[1]);
    assert_eq!(doc.body[2].id(), &original[0]);
    assert_ne!(doc.body[0].id(), &original[0]);
    assert_eq!(doc.source_map[&original[0]].len(), 2);
    assert!(
        doc.layout
            .as_ref()
            .unwrap()
            .blocks
            .contains_key(&original[0])
    );
    assert_eq!(read(&doc).unwrap(), doc);
    assert!(
        doc.to_v1()
            .unwrap()
            .layout
            .unwrap()
            .blocks
            .contains_key("body/2")
    );
}

#[test]
fn ir_v2_footnote_identity_is_distinct_from_block_identity() {
    let mut doc = Document {
        body: vec![
            paragraph(vec![Inline::FootnoteRef {
                id: "note-1".into(),
            }]),
            Block::Footnote {
                id: BlockId::default(),
                style: None,
                direct: ParagraphStyle::default(),
                note_id: "note-1".into(),
                blocks: vec![paragraph(vec![text("note")])],
            },
        ],
        ..Default::default()
    }
    .finalize_import();
    let note_block_id = doc.body[1].id().clone();
    assert_ne!(note_block_id.as_str(), "note-1");
    assert_eq!(read(&doc).unwrap(), doc);
    let old = doc.to_v1().unwrap();
    assert!(matches!(&old.body[1], legacy::Block::Footnote { id, .. } if id == "note-1"));
    doc = pipeline::read_ir_json(&serde_json::to_vec(&old).unwrap(), &Limits::local()).unwrap();
    assert_eq!(doc.body[1].id(), &note_block_id);
    assert!(
        matches!(&doc.body[0], Block::Paragraph { content, .. } if matches!(&content[0], Inline::FootnoteRef { id } if id == "note-1"))
    );
}

#[test]
fn ir_v2_rejects_duplicate_ids_style_cycles_and_invalid_geometry() {
    let base = positioned();
    let mut duplicate = base.clone();
    *duplicate.body[1].id_mut() = duplicate.body[0].id().clone();
    assert!(read(&duplicate).is_err());
    let mut dangling = base.clone();
    dangling
        .source_map
        .insert(BlockId::new("missing").unwrap(), vec![geometry()]);
    assert!(read(&dangling).is_err());
    let mut cycles = base.clone();
    let a = StyleId::new("a").unwrap();
    let b = StyleId::new("b").unwrap();
    cycles.styles.paragraph.insert(
        a.clone(),
        ParagraphStyle {
            based_on: Some(b.clone()),
            ..Default::default()
        },
    );
    cycles.styles.paragraph.insert(
        b,
        ParagraphStyle {
            based_on: Some(a),
            ..Default::default()
        },
    );
    assert!(read(&cycles).is_err());
    let mut missing = base.clone();
    if let Block::Paragraph { style, .. } = &mut missing.body[0] {
        *style = Some(StyleId::new("missing").unwrap());
    }
    assert!(read(&missing).is_err());
    let mut geometry = base.clone();
    geometry
        .layout
        .as_mut()
        .unwrap()
        .blocks
        .values_mut()
        .next()
        .unwrap()
        .bbox
        .left = 200.0;
    assert!(read(&geometry).is_err());
    let mut nonfinite = base.clone();
    nonfinite.styles.defaults.character.size_pt = Some(f64::INFINITY);
    assert!(pipeline::validate_ir_document(&nonfinite, &Limits::local()).is_err());
    let mut bad_section = base.clone();
    bad_section.sections.push(Section {
        start: None,
        page_size: PageSize {
            width_pt: 0.0,
            height_pt: 800.0,
        },
        margins: Margins {
            top_pt: 0.0,
            right_pt: 0.0,
            bottom_pt: 0.0,
            left_pt: 0.0,
        },
        columns: 0,
        header: Some(base.body[0].id().clone()),
        footer: None,
    });
    assert!(read(&bad_section).is_err());
    assert!(BlockId::new("body/0").is_err());
    assert!(StyleId::new("").is_err());
    assert!(Colour::new("red").is_err());
}

#[test]
fn ir_v2_styled_content_obeys_limits_and_asset_walks() {
    let doc = Document {
        body: vec![paragraph(vec![styled(vec![styled(vec![
            text("visible words"),
            Inline::Image {
                target: AssetRef::Url {
                    href: "https://example.test/image.png".into(),
                },
                alt: "image".into(),
                title: None,
            },
        ])])])],
        ..Default::default()
    }
    .finalize_import();
    assert_eq!(ariad_core::inspect::count_document(&doc).images, 1);
    assert_eq!(ariad_core::inspect::count_document(&doc).words, 2);
    let read = pipeline::read(
        Format::AriadIrJson,
        &serde_json::to_vec(&doc).unwrap(),
        &Limits::local(),
    )
    .unwrap();
    assert_eq!(read.document, doc);
    assert!(
        pipeline::validate_ir_document(
            &doc,
            &Limits {
                max_nesting_depth: 1,
                ..Limits::local()
            }
        )
        .is_err()
    );
    assert!(writer::html::write(&doc).content.contains("visible words"));
    assert!(
        writer::markdown::write(&doc)
            .content
            .contains("visible words")
    );
    let mut too_deep = text("limit");
    for _ in 0..300 {
        too_deep = styled(vec![too_deep]);
    }
    let deep = Document {
        body: vec![paragraph(vec![too_deep])],
        ..Default::default()
    };
    assert!(pipeline::write(Format::AriadIrJson, &deep, WriteOptions::default()).is_err());
    assert!(
        writer::markdown::write(&deep)
            .warnings
            .iter()
            .any(
                |w| w.code == ariad_core::warning::WarningCode::UnsupportedNode
                    && w.message.contains("nesting limit")
            )
    );
}

#[test]
fn style_cascade_preserves_false_and_nested_character_overrides() {
    let mut sheet = StyleSheet::default();
    sheet.defaults.paragraph.size_pt = Some(10.0);
    let parent = StyleId::new("Parent").unwrap();
    let child = StyleId::new("Child").unwrap();
    sheet.paragraph.insert(
        parent.clone(),
        ParagraphStyle {
            size_pt: Some(12.0),
            keep_with_next: Some(true),
            ..Default::default()
        },
    );
    sheet.paragraph.insert(
        child.clone(),
        ParagraphStyle {
            based_on: Some(parent),
            align: Some(ParagraphAlignment::Center),
            ..Default::default()
        },
    );
    let resolved = sheet
        .resolve_paragraph(
            Some(&child),
            &ParagraphStyle {
                size_pt: Some(14.0),
                keep_with_next: Some(false),
                ..Default::default()
            },
            64,
        )
        .unwrap();
    assert_eq!(resolved.size_pt, Some(14.0));
    assert_eq!(resolved.keep_with_next, Some(false));
    assert_eq!(resolved.align, Some(ParagraphAlignment::Center));
    let outer = CharacterStyle {
        italic: Some(true),
        weight: Some(700),
        ..Default::default()
    };
    let inner = sheet
        .resolve_character(
            None,
            &CharacterStyle {
                italic: Some(false),
                ..Default::default()
            },
            Some(&outer),
        )
        .unwrap();
    assert_eq!(inner.italic, Some(false));
    assert_eq!(inner.weight, Some(700));
    assert!(
        sheet
            .resolve_paragraph(Some(&child), &ParagraphStyle::default(), 1)
            .is_err()
    );
}

#[test]
fn explicit_legacy_output_warns_and_prohibited_fonts_are_never_exported() {
    let mut doc = positioned();
    for (bytes, embedding) in [
        (b"allowed".to_vec(), FontEmbedding::Allowed),
        (b"restricted".to_vec(), FontEmbedding::Restricted),
        (b"unknown".to_vec(), FontEmbedding::Unknown),
    ] {
        doc.assets.insert(
            hex::encode(Sha256::digest(&bytes)),
            Asset {
                media_type: "font/ttf".into(),
                bytes,
                font: Some(FontAsset {
                    family: "Font".into(),
                    embedding,
                }),
            },
        );
    }
    let bytes = b"font without permission metadata".to_vec();
    doc.assets.insert(
        hex::encode(Sha256::digest(&bytes)),
        Asset {
            media_type: "font/ttf".into(),
            bytes,
            font: None,
        },
    );
    let bytes = b"font with parameterized media type".to_vec();
    doc.assets.insert(
        hex::encode(Sha256::digest(&bytes)),
        Asset {
            media_type: "Font/TTF; version=1".into(),
            bytes,
            font: None,
        },
    );
    for version in [1, 2] {
        let written = pipeline::write(
            Format::AriadIrJson,
            &doc,
            WriteOptions {
                ir_version: Some(version),
                ..Default::default()
            },
        )
        .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&written.bytes).unwrap();
        assert_eq!(value["assets"].as_object().unwrap().len(), 1);
        assert!(
            written
                .warnings
                .iter()
                .any(|w| w.code == "font_embedding_restricted")
        );
        assert_eq!(
            written.warnings.iter().any(|w| w.code == "ir_version_loss"),
            version == 1
        );
    }
    for format in [Format::Markdown, Format::Html, Format::Txt, Format::Xlsx] {
        let written = pipeline::write(format, &doc, WriteOptions::default()).unwrap();
        assert!(
            written
                .warnings
                .iter()
                .any(|warning| warning.code == "font_embedding_restricted")
        );
    }
    assert!(
        pipeline::write(
            Format::Html,
            &doc,
            WriteOptions {
                ir_version: Some(1),
                ..Default::default()
            }
        )
        .is_err()
    );
    assert!(
        pipeline::write(
            Format::AriadIrJson,
            &doc,
            WriteOptions {
                ir_version: Some(3),
                ..Default::default()
            }
        )
        .is_err()
    );
}

#[test]
fn styled_normalization_preserves_opaque_bytes_and_identities() {
    let doc = Document {
        body: vec![paragraph(vec![styled(vec![
            text("a\r\nb"),
            Inline::Code {
                text: "code\r\nbytes".into(),
            },
            Inline::Math {
                tex: "math\r\nbytes".into(),
                display: false,
            },
            Inline::Link {
                url: "https://example.test/a\r\nb".into(),
                title: None,
                content: vec![text("link")],
            },
        ])])],
        ..Default::default()
    }
    .finalize_import();
    // JSON is identity preserving; the HTML writer's prose normalizer sees nested Styled.
    assert_eq!(read(&doc).unwrap(), doc);
    let html = writer::html::write(&doc).content;
    assert!(html.contains("a\nb"));
    assert!(html.contains("code\r\nbytes"));
}

#[test]
fn nested_container_ids_and_legacy_table_cell_paths_are_preserved() {
    let path = "body/0/blocks/0/items/0/blocks/0/head/0/cells/0/blocks/0";
    let value = json!({"version":"ariad-ir/1","body":[{"type":"quote","blocks":[{"type":"list","ordered":false,"start":null,"tight":false,"items":[{"checked":null,"blocks":[{"type":"table","caption":null,"columns":[{"align":"default"}],"head":[[{"rowspan":1,"colspan":1,"blocks":[{"type":"paragraph","content":[{"type":"text","text":"cell"}]}]}]],"body":[]}]}]}]}],"furniture":[{"type":"footnote","id":"furniture-note","blocks":[{"type":"paragraph","content":[]}]}],"layout":{"pages":[{"page_no":1,"width":600.0,"height":800.0}],"blocks":{path:geometry()}}});
    let doc =
        pipeline::read_ir_json(&serde_json::to_vec(&value).unwrap(), &Limits::local()).unwrap();
    assert_eq!(doc.block_count(), 6);
    let blocks = blocks_with_paths(&doc);
    let (_, cell) = blocks.iter().find(|(p, _)| p == path).unwrap();
    assert_eq!(doc.layout.as_ref().unwrap().blocks[cell.id()], geometry());
    let unique: std::collections::BTreeSet<_> =
        blocks.iter().map(|(_, block)| block.id()).collect();
    assert_eq!(unique.len(), 6);
    assert_eq!(
        serde_json::to_value(doc.to_v1().unwrap()).unwrap()["layout"],
        value["layout"]
    );
}

#[test]
fn source_maps_work_without_layout_and_legacy_boxes_may_cross_page_edges() {
    let mut doc = positioned();
    doc.layout
        .as_mut()
        .unwrap()
        .blocks
        .values_mut()
        .next()
        .unwrap()
        .bbox
        .left = -10.0;
    assert_eq!(read(&doc).unwrap(), doc);
    doc.layout = None;
    assert_eq!(read(&doc).unwrap(), doc);
}
