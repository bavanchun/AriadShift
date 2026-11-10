use ariad_core::{
    ir::{
        Asset, Block, CharacterStyle, Document, FontAsset, FontEmbedding, FontRef, Inline, Margins,
        PageSize, ParagraphAlignment, ParagraphStyle, Section, StyleId,
    },
    warning::WarningCode,
    writer::{
        html, markdown,
        typst::{self, TypstOptions},
    },
};

fn id(value: &str) -> StyleId {
    StyleId::new(value).unwrap()
}
fn text(value: &str) -> Inline {
    Inline::Text { text: value.into() }
}
fn styled_document() -> Document {
    let mut document = Document::default();
    document.styles.defaults.paragraph.size_pt = Some(10.0);
    document.styles.paragraph.insert(
        id("Base"),
        ParagraphStyle {
            font: Some(FontRef {
                family: "Libertinus Serif".into(),
                fallback: vec![],
            }),
            size_pt: Some(14.0),
            color: Some("#123456".to_owned().try_into().unwrap()),
            ..Default::default()
        },
    );
    document.styles.paragraph.insert(
        id("Body"),
        ParagraphStyle {
            based_on: Some(id("Base")),
            align: Some(ParagraphAlignment::Center),
            space_after_pt: Some(8.0),
            ..Default::default()
        },
    );
    document.styles.character.insert(
        id("Accent"),
        CharacterStyle {
            weight: Some(700),
            italic: Some(true),
            underline: Some(true),
            ..Default::default()
        },
    );
    document.body.push(Block::Paragraph {
        id: Default::default(),
        style: Some(id("Body")),
        direct: ParagraphStyle {
            size_pt: Some(16.0),
            space_after_pt: Some(0.0),
            ..Default::default()
        },
        content: vec![Inline::Styled {
            style: Some(id("Accent")),
            direct: Default::default(),
            content: vec![
                text("Tiếng Việt "),
                Inline::Styled {
                    style: None,
                    direct: CharacterStyle {
                        weight: Some(400),
                        italic: Some(false),
                        underline: Some(false),
                        ..Default::default()
                    },
                    content: vec![text("được giữ")],
                },
            ],
        }],
    });
    document = document.finalize_import();
    document.sections.push(Section {
        start: None,
        page_size: PageSize {
            width_pt: 500.0,
            height_pt: 700.0,
        },
        margins: Margins {
            top_pt: 30.0,
            right_pt: 35.0,
            bottom_pt: 40.0,
            left_pt: 45.0,
        },
        columns: 2,
        header: None,
        footer: None,
    });
    document
}

#[test]
fn typst_styles_resolve_defaults_named_styles_and_direct_overrides() {
    let output = typst::write(&styled_document(), &TypstOptions::default());
    for expected in [
        "size: 16pt",
        "weight: 700",
        "weight: 400",
        "style: \"normal\"",
        "rgb(\"#123456\")",
        "width: 500pt",
        "height: 700pt",
        "columns: 2",
        "spacing: 0pt",
    ] {
        assert!(
            output.markup.contains(expected),
            "missing {expected}: {}",
            output.markup
        );
    }
}

#[test]
fn typst_styles_escape_hostile_names_and_keep_vietnamese_text() {
    let mut doc = styled_document();
    doc.styles
        .paragraph
        .get_mut(&id("Base"))
        .unwrap()
        .font
        .as_mut()
        .unwrap()
        .family = "x\"); panic(\"injected\") //".into();
    let output = typst::write(&doc, &TypstOptions::default());
    assert!(!output.markup.contains("panic(\"injected\")"));
    assert!(output.markup.contains("Tiếng Việt"));
    assert!(output.markup.contains("được giữ"));
}

#[test]
fn html_styles_emit_safe_css_and_direct_overrides() {
    let doc = styled_document();
    let output = html::write(&doc);
    for expected in [
        "font-size:16pt",
        "color:#123456",
        "text-align:center",
        "margin-bottom:0pt",
        "font-weight:700",
        "font-weight:400",
        "font-style:normal",
        "data-ariad-style=\"Body\"",
    ] {
        assert!(
            output.content.contains(expected),
            "missing {expected}: {}",
            output.content
        );
    }
    let mut hostile = doc;
    hostile
        .styles
        .paragraph
        .get_mut(&id("Base"))
        .unwrap()
        .font
        .as_mut()
        .unwrap()
        .family = "x\";background:url(https://evil.test);</style><script>bad()</script>".into();
    let output = html::write(&hostile);
    assert!(!output.content.contains("<script>"));
    assert!(!output.content.contains("background:url("));
    assert!(output.content.contains("Tiếng Việt"));
}

#[test]
fn markdown_styles_emit_one_loss_warning_and_keep_text() {
    let mut doc = styled_document();
    doc.body.push(doc.body[0].clone());
    let output = markdown::write(&doc);
    assert_eq!(
        output
            .warnings
            .iter()
            .filter(|w| w.code == WarningCode::StylesDropped)
            .count(),
        1
    );
    assert!(output.content.contains("Tiếng Việt được giữ"));
    assert!(!output.content.contains("<span"));
    let plain = ariad_core::reader::markdown::read("plain **text**", &Default::default()).unwrap();
    assert!(
        !markdown::write(&plain.document)
            .warnings
            .iter()
            .any(|w| w.code == WarningCode::StylesDropped)
    );
    let mut unused = plain.document;
    unused.styles.character.insert(
        id("Unused"),
        CharacterStyle {
            weight: Some(700),
            ..Default::default()
        },
    );
    assert!(
        !markdown::write(&unused)
            .warnings
            .iter()
            .any(|w| w.code == WarningCode::StylesDropped)
    );
}

#[test]
fn writers_never_reembed_restricted_font_assets() {
    for embedding in [FontEmbedding::Restricted, FontEmbedding::Unknown] {
        let mut doc = styled_document();
        doc.assets.insert(
            "a".repeat(64),
            Asset {
                media_type: "font/ttf".into(),
                bytes: b"PRIVATE_FONT_BYTES".to_vec(),
                font: Some(FontAsset {
                    family: "Private".into(),
                    embedding,
                }),
            },
        );
        let html = html::write(&doc);
        let typst = typst::write(&doc, &TypstOptions::default());
        let pandoc = ariad_core::pandoc::from_ir(&doc);
        assert!(!html.content.contains("UFJJVkFURV9GT05UX0JZVEVT"));
        assert!(typst.files.values().all(|v| v != b"PRIVATE_FONT_BYTES"));
        for warnings in [&html.warnings, &typst.warnings, &pandoc.warnings] {
            assert!(
                warnings
                    .iter()
                    .any(|w| w.code == WarningCode::FontEmbeddingRestricted)
            );
        }
    }
}

#[test]
fn character_defaults_and_semantic_emphasis_obey_inner_resets() {
    let mut doc = ariad_core::reader::markdown::read("**strong**", &Default::default())
        .unwrap()
        .document;
    doc.styles.defaults.character.italic = Some(true);
    let Block::Paragraph { content, .. } = &mut doc.body[0] else {
        panic!()
    };
    content.push(Inline::Strong {
        content: vec![Inline::Styled {
            style: None,
            direct: CharacterStyle {
                weight: Some(400),
                italic: Some(false),
                ..Default::default()
            },
            content: vec![text("normal")],
        }],
    });
    let html = html::write(&doc).content;
    assert!(html.contains("font-weight:700;"));
    assert!(html.contains("font-weight:400;font-style:normal;"));
    let typst = typst::write(&doc, &TypstOptions::default()).markup;
    assert!(typst.contains("weight: 700"));
    assert!(typst.contains("weight: 400"));
    let mapped = ariad_core::pandoc::MapOutput::for_docx(&doc);
    let ast = serde_json::to_string(&mapped.pandoc).unwrap();
    assert!(
        !ast.contains("\"Strong\""),
        "style-aware semantic runs must not override an inner explicit reset"
    );
    assert!(ast.contains("custom-style"));
}

#[test]
fn writers_report_invalid_style_references_and_preserve_text() {
    let mut doc = styled_document();
    doc.styles.paragraph.clear();
    let html = html::write(&doc);
    let typst = typst::write(&doc, &TypstOptions::default());
    let pandoc = ariad_core::pandoc::from_ir(&doc);
    for warnings in [&html.warnings, &typst.warnings, &pandoc.warnings] {
        assert!(
            warnings
                .iter()
                .any(|w| w.message.contains("dangling paragraph style reference"))
        );
    }
    assert!(html.content.contains("Tiếng Việt"));
    assert!(typst.markup.contains("Tiếng Việt"));
    assert!(StyleId::new("bad\" onclick=\"injected").is_err());
}

#[test]
fn table_character_styles_survive_paragraph_defaults_and_allow_direct_overrides() {
    use ariad_core::ir::{Alignment, ColumnSpec, TableCell, TableStyle};
    let mut doc = Document::default();
    doc.styles.defaults.paragraph.size_pt = Some(10.0);
    doc.styles.table.insert(
        id("Grid"),
        TableStyle {
            character: CharacterStyle {
                size_pt: Some(18.0),
                color: Some("#005533".to_owned().try_into().unwrap()),
                ..Default::default()
            },
            ..Default::default()
        },
    );
    let mut cells = Vec::new();
    for (identifier, size) in [("Inherited", None), ("Direct", Some(20.0))] {
        cells.push(TableCell {
            rowspan: 1,
            colspan: 1,
            header: None,
            blocks: vec![Block::Paragraph {
                id: Default::default(),
                style: None,
                direct: ParagraphStyle {
                    size_pt: size,
                    ..Default::default()
                },
                content: vec![text(identifier)],
            }],
        });
    }
    doc.body.push(Block::Table {
        id: Default::default(),
        style: Some(id("Grid")),
        direct: Default::default(),
        caption: None,
        columns: vec![
            ColumnSpec {
                align: Alignment::Default
            };
            2
        ],
        head: vec![],
        body: vec![cells],
        footnotes: vec![],
    });
    let html = html::write(&doc).content;
    assert!(
        html.contains("font-size:18pt;color:#005533;\">Inherited"),
        "{html}"
    );
    assert!(
        html.contains("font-size:20pt;color:#005533;\">Direct"),
        "{html}"
    );
    let typst = typst::write(&doc, &TypstOptions::default()).markup;
    assert!(typst.contains("size: 18pt"));
    assert!(typst.contains("size: 20pt"));
    let mapped = ariad_core::pandoc::from_ir(&doc);
    let template = test_reference_template();
    let bytes = mapped
        .docx_styles
        .reference_doc(&template, &Default::default())
        .unwrap();
    use std::io::{Cursor, Read};
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut xml = String::new();
    zip.by_name("word/styles.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    assert!(xml.contains("w:sz w:val=\"36\""));
    assert!(xml.contains("w:sz w:val=\"40\""));
}

fn test_reference_template() -> Vec<u8> {
    use std::io::{Cursor, Write};
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file("word/styles.xml", zip::write::SimpleFileOptions::default())
        .unwrap();
    zip.write_all(b"<w:styles xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"></w:styles>").unwrap();
    zip.finish().unwrap().into_inner()
}

#[test]
fn docx_style_children_follow_ooxml_sequence_order() {
    use ariad_core::ir::TableStyle;
    use quick_xml::{Reader, events::Event};
    use std::{
        collections::BTreeMap,
        io::{Cursor, Read},
    };

    let mut doc = styled_document();
    let paragraph = doc.styles.paragraph.get_mut(&id("Body")).unwrap();
    paragraph.keep_with_next = Some(true);
    paragraph.page_break_before = Some(true);
    paragraph.background = Some("#ddeeff".to_owned().try_into().unwrap());
    paragraph.indent_left_pt = Some(12.0);
    let paragraph = paragraph.clone();
    let character = doc.styles.character.get_mut(&id("Accent")).unwrap();
    character.font = Some(FontRef {
        family: "Libertinus Serif".into(),
        fallback: vec![],
    });
    character.size_pt = Some(16.0);
    character.small_caps = Some(true);
    character.strike = Some(true);
    character.color = Some("#123456".to_owned().try_into().unwrap());
    character.letter_spacing = Some(1.0);
    character.baseline_shift = Some(2.0);
    character.highlight = Some("#ddeeff".to_owned().try_into().unwrap());
    let character = character.clone();
    doc.styles.table.insert(
        id("Grid"),
        TableStyle {
            paragraph,
            character,
            background: Some("#ddeeff".to_owned().try_into().unwrap()),
        },
    );
    doc.body.push(Block::Heading {
        id: Default::default(),
        style: Some(id("Body")),
        direct: Default::default(),
        level: 2,
        content: vec![text("Styled heading")],
    });
    let mapped = ariad_core::pandoc::MapOutput::for_docx(&doc);
    let bytes = mapped
        .docx_styles
        .reference_doc(&test_reference_template(), &Default::default())
        .unwrap();
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut xml = String::new();
    archive
        .by_name("word/styles.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    let mut reader = Reader::from_str(&xml);
    let mut path: Vec<String> = Vec::new();
    let mut style = String::new();
    let mut sequences: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
    loop {
        match reader.read_event().unwrap() {
            event @ (Event::Start(_) | Event::Empty(_)) => {
                let (Event::Start(e) | Event::Empty(e)) = &event else {
                    unreachable!()
                };
                let name = e.local_name().as_ref().to_owned();
                if name == "style" {
                    style = e
                        .try_get_attribute("w:styleId")
                        .unwrap()
                        .unwrap()
                        .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                        .unwrap()
                        .into_owned();
                }
                if let Some(parent) = path.last()
                    && matches!(parent.as_str(), "style" | "pPr" | "rPr")
                {
                    sequences
                        .entry((style.clone(), parent.clone()))
                        .or_default()
                        .push(name.clone());
                }
                if matches!(event, Event::Start(_)) {
                    path.push(name);
                }
            }
            Event::End(_) => {
                path.pop();
            }
            Event::Eof => break,
            _ => {}
        }
    }
    let style_order = ["name", "basedOn", "pPr", "rPr", "tblPr"];
    let paragraph_order = [
        "keepNext",
        "pageBreakBefore",
        "shd",
        "spacing",
        "ind",
        "jc",
        "outlineLvl",
    ];
    let run_order = [
        "rFonts",
        "b",
        "bCs",
        "i",
        "smallCaps",
        "strike",
        "color",
        "spacing",
        "position",
        "sz",
        "szCs",
        "u",
        "shd",
    ];
    for ((style, parent), children) in &sequences {
        let order: &[&str] = match parent.as_str() {
            "style" => &style_order,
            "pPr" => &paragraph_order,
            "rPr" => &run_order,
            _ => unreachable!(),
        };
        let expected: Vec<_> = order
            .iter()
            .filter(|name| children.iter().any(|child| child == **name))
            .copied()
            .collect();
        assert_eq!(children, &expected, "{style}/{parent}");
    }
    for (style, parent, expected) in [
        (
            "AriadPStyleBody",
            "style",
            vec!["name", "basedOn", "pPr", "rPr"],
        ),
        ("AriadPStyleBody", "pPr", paragraph_order[..6].to_vec()),
        ("AriadCStyleAccent", "rPr", run_order.to_vec()),
        (
            "AriadTStyleGrid",
            "style",
            style_order
                .iter()
                .copied()
                .filter(|name| *name != "basedOn")
                .collect(),
        ),
    ] {
        assert_eq!(
            sequences[&(style.into(), parent.into())],
            expected,
            "{style}/{parent}"
        );
    }
    assert!(
        sequences
            .iter()
            .any(|((_, parent), children)| parent == "pPr"
                && children.last().is_some_and(|name| name == "outlineLvl"))
    );
}

#[test]
fn generated_reference_styles_enforce_the_supplied_archive_budget() {
    let mapped = ariad_core::pandoc::from_ir(&styled_document());
    let limits = ariad_core::limits::Limits {
        max_archive_entries: 1,
        max_decompressed_bytes: 32,
        ..Default::default()
    };
    assert!(
        mapped
            .docx_styles
            .reference_doc(&test_reference_template(), &limits)
            .is_err()
    );
}

#[test]
fn unstyled_documents_keep_flat_typst_markup() {
    let document = ariad_core::reader::markdown::read(
        "# Heading\n\nPlain **strong** and *emphasized* text.\n\n> Quoted text\n\n- An item\n",
        &Default::default(),
    )
    .unwrap()
    .document;
    let mut document = document;
    document.body.push(Block::Paragraph {
        id: Default::default(),
        style: None,
        direct: Default::default(),
        content: vec![Inline::Strong {
            content: vec![Inline::Styled {
                style: None,
                direct: Default::default(),
                content: vec![text("Transparent span")],
            }],
        }],
    });
    let output = typst::write(&document, &TypstOptions::default());
    assert!(!output.markup.contains("#block("), "{}", output.markup);
    assert!(!output.markup.contains("#text("), "{}", output.markup);
    assert!(output.markup.contains("#strong["));
    assert!(output.markup.contains("#emph["));
}

#[test]
fn typst_sections_render_nested_furniture_and_switch_page_setup() {
    let mut doc = styled_document();
    doc.furniture.push(Block::Quote {
        id: Default::default(),
        style: None,
        direct: Default::default(),
        blocks: vec![Block::Paragraph {
            id: Default::default(),
            style: None,
            direct: Default::default(),
            content: vec![text("Nested header")],
        }],
    });
    doc.body.push(Block::Paragraph {
        id: Default::default(),
        style: None,
        direct: Default::default(),
        content: vec![text("Second section")],
    });
    doc.assign_block_ids();
    let Block::Quote { blocks, .. } = &doc.furniture[0] else {
        panic!()
    };
    doc.sections[0].header = Some(blocks[0].id().clone());
    let mut second = doc.sections[0].clone();
    second.start = Some(doc.body[1].id().clone());
    second.header = None;
    second.page_size = PageSize {
        width_pt: 600.0,
        height_pt: 800.0,
    };
    second.columns = 1;
    doc.sections.push(second);
    let output = typst::write(&doc, &TypstOptions::default());
    assert!(output.markup.contains("Nested header"));
    assert!(output.markup.contains("width: 600pt, height: 800pt"));
    assert!(output.markup.contains("columns: 1"));
    assert!(output.markup.contains("header: none"));
}

#[test]
fn named_empty_styles_keep_their_pandoc_references() {
    use ariad_core::ir::{Alignment, ColumnSpec, TableCell, TableStyle};
    let mut doc = Document::default();
    doc.styles
        .paragraph
        .insert(id("EmptyP"), ParagraphStyle::default());
    doc.styles
        .character
        .insert(id("EmptyC"), CharacterStyle::default());
    doc.styles.table.insert(id("EmptyT"), TableStyle::default());
    doc.body.push(Block::Paragraph {
        id: Default::default(),
        style: Some(id("EmptyP")),
        direct: Default::default(),
        content: vec![Inline::Styled {
            style: Some(id("EmptyC")),
            direct: Default::default(),
            content: vec![text("Named text")],
        }],
    });
    doc.body.push(Block::Table {
        id: Default::default(),
        style: Some(id("EmptyT")),
        direct: Default::default(),
        caption: None,
        columns: vec![ColumnSpec {
            align: Alignment::Default,
        }],
        head: vec![],
        body: vec![vec![TableCell {
            rowspan: 1,
            colspan: 1,
            header: None,
            blocks: vec![],
        }]],
        footnotes: vec![],
    });
    let mapped = ariad_core::pandoc::from_ir(&doc);
    let json = serde_json::to_string(&mapped.pandoc).unwrap();
    for name in [
        "AriadPStyleEmptyP",
        "AriadCStyleEmptyC",
        "AriadTStyleEmptyT",
    ] {
        assert!(json.contains(name), "missing {name}: {json}");
    }
}

#[test]
fn target_neutral_pandoc_preserves_styled_headings_and_semantic_emphasis() {
    let mut doc = styled_document();
    doc.body = vec![Block::Heading {
        id: Default::default(),
        style: Some(id("Body")),
        direct: Default::default(),
        level: 2,
        content: vec![Inline::Strong {
            content: vec![text("Styled heading")],
        }],
    }];
    doc.assign_block_ids();
    let mapped = ariad_core::pandoc::from_ir(&doc);
    let ast = serde_json::to_string(&mapped.pandoc).unwrap();
    assert!(ast.contains("\"Header\""), "{ast}");
    assert!(ast.contains("\"Strong\""), "{ast}");
    let mapped = ariad_core::pandoc::MapOutput::for_docx(&doc);
    let ast = serde_json::to_string(&mapped.pandoc).unwrap();
    assert!(!ast.contains("\"Header\""), "{ast}");
    assert!(!ast.contains("\"Strong\""), "{ast}");
    assert!(ast.contains("custom-style"));
}
