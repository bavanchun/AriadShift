use ariad_core::{
    ir::{
        Block, CharacterStyle, ColumnSpec, Document, FontRef, Inline, ParagraphStyle, StyleId,
        TableCell, TableStyle,
    },
    limits::Limits,
    protocol::{Event, Input, Output, PROTOCOL, Request},
};
use ariad_host::{engines::pandoc::serve, pandoc_bin, workspace::Workspace};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::{Cursor, Read},
    process::{Command, ExitCode},
};

fn id(value: &str) -> StyleId {
    StyleId::new(value).unwrap()
}
fn text(value: &str) -> Inline {
    Inline::Text { text: value.into() }
}
fn paragraph() -> Block {
    Block::Paragraph {
        id: Default::default(),
        style: Some(id("Body")),
        direct: ParagraphStyle {
            size_pt: Some(18.0),
            space_after_pt: Some(0.0),
            ..Default::default()
        },
        content: vec![Inline::Styled {
            style: Some(id("Accent")),
            direct: Default::default(),
            content: vec![
                text("Tiếng Việt"),
                Inline::Styled {
                    style: None,
                    direct: CharacterStyle {
                        weight: Some(400),
                        italic: Some(false),
                        underline: Some(false),
                        ..Default::default()
                    },
                    content: vec![text(" reset")],
                },
            ],
        }],
    }
}
fn document() -> Document {
    let mut doc = Document::default();
    doc.styles.paragraph.insert(
        id("Base"),
        ParagraphStyle {
            font: Some(FontRef {
                family: "Libertinus Serif".into(),
                fallback: vec![],
            }),
            size_pt: Some(12.0),
            color: Some("#123456".to_owned().try_into().unwrap()),
            ..Default::default()
        },
    );
    doc.styles.paragraph.insert(
        id("Body"),
        ParagraphStyle {
            based_on: Some(id("Base")),
            ..Default::default()
        },
    );
    doc.styles.character.insert(
        id("Accent"),
        CharacterStyle {
            weight: Some(700),
            italic: Some(true),
            underline: Some(true),
            ..Default::default()
        },
    );
    doc.styles.table.insert(
        id("Grid"),
        TableStyle {
            background: Some("#ddeeff".to_owned().try_into().unwrap()),
            ..Default::default()
        },
    );
    doc.body = vec![
        paragraph(),
        Block::Table {
            id: Default::default(),
            style: Some(id("Grid")),
            direct: Default::default(),
            caption: None,
            columns: vec![ColumnSpec {
                align: ariad_core::ir::Alignment::Default,
            }],
            head: vec![],
            body: vec![vec![TableCell {
                rowspan: 1,
                colspan: 1,
                header: None,
                blocks: vec![paragraph()],
            }]],
            footnotes: vec![],
        },
    ];
    doc.finalize_import()
}
fn convert(doc: &Document) -> (Workspace, Vec<u8>) {
    pandoc_bin::locate().expect("real Pandoc >= 3.12 must be available; run just pandoc");
    let workspace = Workspace::new().unwrap();
    let input = workspace.input_dir().join("styles.json");
    fs::write(&input, serde_json::to_vec(doc).unwrap()).unwrap();
    let request = Request::Convert {
        protocol: PROTOCOL.into(),
        job: "styled-docx".into(),
        input: Input {
            path: input.to_string_lossy().into(),
            format: "ariad-ir+json".into(),
        },
        output: Output {
            dir: workspace.output_dir().to_string_lossy().into(),
            format: "docx".into(),
        },
        work_dir: workspace.work_dir().to_string_lossy().into(),
        options: BTreeMap::new(),
        limits: Limits {
            timeout_s: Some(60),
            max_memory_mb: Some(1024),
            ..Limits::local()
        },
        session_id: None,
    };
    let mut events = Vec::new();
    let exit = serve(
        Cursor::new(serde_json::to_vec(&request).unwrap()),
        &mut events,
    );
    assert_eq!(
        exit,
        ExitCode::SUCCESS,
        "{}",
        String::from_utf8_lossy(&events)
    );
    let events: Vec<Event> = String::from_utf8(events)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let path = events
        .iter()
        .find_map(|e| {
            if let Event::Artifact { path, .. } = e {
                Some(path)
            } else {
                None
            }
        })
        .unwrap();
    let bytes = fs::read(path).unwrap();
    (workspace, bytes)
}
fn member(bytes: &[u8], name: &str) -> String {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut text = String::new();
    archive
        .by_name(name)
        .unwrap()
        .read_to_string(&mut text)
        .unwrap();
    text
}
fn assert_xml_valid(xml: &str) {
    let mut reader = quick_xml::Reader::from_str(xml);
    while !matches!(reader.read_event().unwrap(), quick_xml::events::Event::Eof) {}
}
#[test]
fn docx_styles_emit_definitions_and_custom_style_references() {
    let (_workspace, bytes) = convert(&document());
    let styles = member(&bytes, "word/styles.xml");
    let content = member(&bytes, "word/document.xml");
    assert_xml_valid(&styles);
    assert_xml_valid(&content);
    for expected in [
        "AriadPStyleBase",
        "AriadPStyleBody",
        "AriadCStyleAccent",
        "AriadTStyleGrid",
        "w:basedOn w:val=\"AriadPStyleBase\"",
        "w:sz w:val=\"36\"",
        "w:color w:val=\"123456\"",
        "w:rFonts w:ascii=\"Libertinus Serif\"",
        "w:b w:val=\"0\"",
        "w:i w:val=\"0\"",
        "w:u w:val=\"none\"",
        "ddeeff",
    ] {
        assert!(styles.contains(expected), "missing {expected} in {styles}");
    }
    assert!(
        content.contains("<w:b w:val=\"0\"/>"),
        "bold reset must be direct run formatting: {content}"
    );
    assert!(
        content.contains("<w:i w:val=\"0\"/>"),
        "italic reset must be direct run formatting: {content}"
    );
    assert!(
        content.contains("<w:u w:val=\"none\"/>"),
        "underline reset must be direct run formatting: {content}"
    );
    for expected in [
        "w:pStyle w:val=\"AriadPDirect",
        "w:rStyle w:val=\"AriadCDirect",
        "w:tblStyle w:val=\"AriadTDirect",
        "Tiếng Việt",
        "reset",
    ] {
        assert!(
            content.contains(expected),
            "missing {expected} in {content}"
        );
    }
}
#[test]
fn docx_styles_roundtrip_through_real_pandoc() {
    let (workspace, bytes) = convert(&document());
    let path = workspace.work_dir().join("read-back.docx");
    fs::write(&path, &bytes).unwrap();
    let pandoc = pandoc_bin::locate().unwrap();
    let output = Command::new(pandoc.path)
        .args([
            "+RTS",
            "-M1024M",
            "-RTS",
            "--sandbox",
            "-f",
            "docx+styles",
            "-t",
            "json",
        ])
        .arg(path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let ast: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let json = serde_json::to_string(&ast).unwrap();
    for expected in [
        "custom-style",
        "AriadPDirect",
        "AriadCDirect",
        "Tiếng",
        "Việt",
        "reset",
    ] {
        assert!(json.contains(expected), "missing {expected}: {json}");
    }
    // Package inspection establishes actual style values independently of AST attributes.
    let styles = member(&bytes, "word/styles.xml");
    for expected in [
        "Libertinus Serif",
        "w:sz w:val=\"36\"",
        "w:b w:val=\"1\"",
        "w:color w:val=\"123456\"",
    ] {
        assert!(styles.contains(expected));
    }
}

#[test]
fn docx_font_names_cannot_inject_xml_or_reembed_restricted_bytes() {
    use ariad_core::ir::{Asset, FontAsset, FontEmbedding};
    let mut doc = document();
    doc.styles
        .paragraph
        .get_mut(&id("Base"))
        .unwrap()
        .font
        .as_mut()
        .unwrap()
        .family = "X\"/><w:rFonts w:ascii=\"Injected".into();
    for (index, embedding) in [FontEmbedding::Restricted, FontEmbedding::Unknown]
        .into_iter()
        .enumerate()
    {
        let bytes = format!("PRIVATE_FONT_BYTES_{index}").into_bytes();
        let id = hex::encode(Sha256::digest(&bytes));
        doc.assets.insert(
            id,
            Asset {
                media_type: "font/ttf".into(),
                bytes,
                font: Some(FontAsset {
                    family: "Private".into(),
                    embedding,
                }),
            },
        );
    }
    let (_workspace, bytes) = convert(&doc);
    let styles = member(&bytes, "word/styles.xml");
    assert_xml_valid(&styles);
    assert!(!styles.contains("<w:rFonts w:ascii=\"Injected\""));
    assert!(styles.contains("&quot;"));
    assert!(styles.contains("&lt;w:rFonts"));
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    for index in 0..archive.len() {
        let mut member = archive.by_index(index).unwrap();
        let mut bytes = Vec::new();
        member.read_to_end(&mut bytes).unwrap();
        assert!(
            !bytes
                .windows(b"PRIVATE_FONT_BYTES".len())
                .any(|window| window == b"PRIVATE_FONT_BYTES")
        );
    }
}

#[test]
fn docx_custom_heading_style_keeps_its_outline_level() {
    let mut doc = document();
    doc.body = vec![Block::Heading {
        id: Default::default(),
        style: Some(id("Body")),
        direct: ParagraphStyle {
            size_pt: Some(24.0),
            ..Default::default()
        },
        level: 2,
        content: vec![text("Styled heading")],
    }];
    doc.assign_block_ids();
    let (_workspace, bytes) = convert(&doc);
    let styles = member(&bytes, "word/styles.xml");
    assert!(styles.contains("w:outlineLvl w:val=\"1\""));
    assert!(styles.contains("w:sz w:val=\"48\""));
    assert!(member(&bytes, "word/document.xml").contains("w:pStyle w:val=\"AriadPDirect"));
}
