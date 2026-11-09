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

#[derive(Debug, Default)]
struct XmlNode {
    name: String,
    attrs: BTreeMap<String, String>,
    children: Vec<XmlNode>,
    text: String,
}
impl XmlNode {
    fn child(&self, name: &str) -> &Self {
        self.children
            .iter()
            .find(|node| node.name == name)
            .unwrap_or_else(|| panic!("missing {name} in {self:?}"))
    }
    fn attr(&self, name: &str) -> &str {
        self.attrs
            .get(name)
            .unwrap_or_else(|| panic!("missing {name} in {self:?}"))
    }
}
fn parse_xml(xml: &str) -> XmlNode {
    use quick_xml::{Reader, XmlVersion, events::Event};
    let mut reader = Reader::from_str(xml);
    let mut stack = vec![XmlNode::default()];
    loop {
        match reader.read_event().unwrap() {
            event @ (Event::Start(_) | Event::Empty(_)) => {
                let (Event::Start(e) | Event::Empty(e)) = &event else {
                    unreachable!()
                };
                let node = XmlNode {
                    name: e.local_name().as_ref().to_owned(),
                    attrs: e
                        .attributes()
                        .map(|attr| {
                            let attr = attr.unwrap();
                            (
                                attr.key.as_ref().to_owned(),
                                attr.normalized_value(XmlVersion::Implicit1_0)
                                    .unwrap()
                                    .into_owned(),
                            )
                        })
                        .collect(),
                    ..Default::default()
                };
                if matches!(event, Event::Start(_)) {
                    stack.push(node);
                } else {
                    stack.last_mut().unwrap().children.push(node);
                }
            }
            Event::End(_) => {
                let node = stack.pop().unwrap();
                stack.last_mut().unwrap().children.push(node);
            }
            Event::Text(e) => stack
                .last_mut()
                .unwrap()
                .text
                .push_str(&e.xml_content(XmlVersion::Implicit1_0)),
            Event::Eof => break,
            _ => {}
        }
    }
    assert_eq!(stack.len(), 1);
    stack.pop().unwrap().children.pop().unwrap()
}
fn assert_property_order(node: &XmlNode) {
    let order: &[&str] = match node.name.as_str() {
        "pPr" => &[
            "pStyle",
            "keepNext",
            "keepLines",
            "pageBreakBefore",
            "widowControl",
            "numPr",
            "pBdr",
            "shd",
            "tabs",
            "spacing",
            "ind",
            "jc",
            "outlineLvl",
            "rPr",
            "sectPr",
            "pPrChange",
        ],
        "rPr" => &[
            "rStyle",
            "rFonts",
            "b",
            "bCs",
            "i",
            "iCs",
            "caps",
            "smallCaps",
            "strike",
            "dstrike",
            "outline",
            "shadow",
            "emboss",
            "imprint",
            "noProof",
            "snapToGrid",
            "vanish",
            "webHidden",
            "color",
            "spacing",
            "w",
            "kern",
            "position",
            "sz",
            "szCs",
            "highlight",
            "u",
            "effect",
            "bdr",
            "shd",
            "fitText",
            "vertAlign",
            "rtl",
            "cs",
            "em",
            "lang",
            "eastAsianLayout",
            "specVanish",
            "oMath",
            "rPrChange",
        ],
        _ => &[],
    };
    if !order.is_empty() {
        let actual: Vec<_> = node
            .children
            .iter()
            .map(|child| child.name.as_str())
            .collect();
        let expected: Vec<_> = order
            .iter()
            .filter(|name| actual.contains(name))
            .copied()
            .collect();
        assert_eq!(actual, expected, "{}", node.name);
    }
    for child in &node.children {
        assert_property_order(child);
    }
}
fn pandoc_text(node: &serde_json::Value) -> String {
    if let Some(nodes) = node.as_array() {
        return nodes.iter().map(pandoc_text).collect();
    }
    match node["t"].as_str() {
        Some("Str") => node["c"].as_str().unwrap().into(),
        Some("Space") => " ".into(),
        Some(_) => pandoc_text(&node["c"]),
        None => String::new(),
    }
}
fn custom_style(attr: &serde_json::Value) -> &str {
    attr[2]
        .as_array()
        .unwrap()
        .iter()
        .find(|pair| pair[0] == "custom-style")
        .unwrap()[1]
        .as_str()
        .unwrap()
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
    let styles = parse_xml(&member(&bytes, "word/styles.xml"));
    let content = parse_xml(&member(&bytes, "word/document.xml"));
    assert_property_order(&content);
    let paragraph = content.child("body").child("p");
    let paragraph_id = paragraph.child("pPr").child("pStyle").attr("w:val");
    assert!(paragraph_id.starts_with("AriadPDirect"));
    let definition = |style_id: &str| {
        styles
            .children
            .iter()
            .find(|style| {
                style
                    .attrs
                    .get("w:styleId")
                    .is_some_and(|id| id == style_id)
            })
            .unwrap()
    };
    let assert_font = |properties: &XmlNode| {
        let fonts = properties.child("rFonts");
        for attribute in ["w:ascii", "w:hAnsi", "w:eastAsia", "w:cs"] {
            assert_eq!(fonts.attr(attribute), "Libertinus Serif");
        }
        assert_eq!(properties.child("sz").attr("w:val"), "36");
        assert_eq!(properties.child("color").attr("w:val"), "123456");
    };
    let paragraph_style = definition(paragraph_id);
    assert_eq!(paragraph_style.attr("w:type"), "paragraph");
    assert_font(paragraph_style.child("rPr"));
    let div = &ast["blocks"][0];
    assert_eq!(div["t"], "Div");
    assert_eq!(
        custom_style(&div["c"][0]),
        paragraph_style.child("name").attr("w:val")
    );
    assert_eq!(pandoc_text(&div["c"][1]), "Tiếng Việt reset");
    let para = &div["c"][1][0];
    assert_eq!(para["t"], "Para");
    for (target, weight) in [("Tiếng Việt", "1"), ("reset", "0")] {
        let run = paragraph
            .children
            .iter()
            .filter(|node| node.name == "r")
            .find(|run| run.child("t").text.trim() == target)
            .unwrap();
        let run_id = run.child("rPr").child("rStyle").attr("w:val");
        assert!(run_id.starts_with("AriadCDirect"));
        let style = definition(run_id);
        assert_eq!(style.attr("w:type"), "character");
        assert_font(style.child("rPr"));
        assert_eq!(style.child("rPr").child("b").attr("w:val"), weight);
        assert_eq!(style.child("rPr").child("bCs").attr("w:val"), weight);
        assert_font(run.child("rPr"));
        assert_eq!(run.child("rPr").child("b").attr("w:val"), weight);
        let span = para["c"]
            .as_array()
            .unwrap()
            .iter()
            .find(|node| node["t"] == "Span" && pandoc_text(&node["c"][1]) == target)
            .unwrap();
        assert_eq!(
            custom_style(&span["c"][0]),
            style.child("name").attr("w:val")
        );
    }
}

#[test]
fn docx_materialized_run_properties_follow_ooxml_sequence_order() {
    let mut doc = document();
    let character = doc.styles.character.get_mut(&id("Accent")).unwrap();
    character.small_caps = Some(true);
    character.strike = Some(true);
    character.letter_spacing = Some(1.0);
    character.baseline_shift = Some(2.0);
    character.highlight = Some("#ddeeff".to_owned().try_into().unwrap());
    let Block::Paragraph { content, .. } = &mut doc.body[0] else {
        panic!()
    };
    *content = vec![Inline::Superscript {
        content: std::mem::take(content),
    }];
    doc.assign_block_ids();
    let (_workspace, bytes) = convert(&doc);
    let content = parse_xml(&member(&bytes, "word/document.xml"));
    assert_property_order(&content);
    let run = content.child("body").child("p").child("r");
    let names: Vec<_> = run
        .child("rPr")
        .children
        .iter()
        .map(|child| child.name.as_str())
        .collect();
    assert_eq!(
        names,
        [
            "rStyle",
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
            "vertAlign"
        ]
    );
    assert_eq!(
        run.child("rPr").child("vertAlign").attr("w:val"),
        "superscript"
    );
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
