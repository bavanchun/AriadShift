use ariad_core::{
    docling,
    ir::{Block, CharacterStyle, Inline, ParagraphAlignment},
    limits::Limits,
    pandoc::{ast, to_ir},
    pipeline::validate_ir_document,
    reader::html,
};
use ariad_core::{
    ir::{StyleId, StyleSheet},
    pandoc::to_ir::{read_docx_styles, to_ir_with_styles},
};
use serde_json::json;
use std::io::{Cursor, Write};

const WORD_NS: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";

fn docx(parts: &[(&str, &str)]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (path, xml) in parts {
        zip.start_file(
            *path,
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated),
        )
        .unwrap();
        zip.write_all(xml.as_bytes()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

fn styles_package(styles: &str) -> Vec<u8> {
    docx(&[(
        "word/styles.xml",
        &format!(r#"<w:styles xmlns:w="{WORD_NS}">{styles}</w:styles>"#),
    )])
}

fn id(value: &str) -> StyleId {
    StyleId::new(value).unwrap()
}

#[test]
fn docx_styles_xml_resolves_inherited_paragraph_character_and_table_styles() {
    let bytes = styles_package(
        r#"
        <w:docDefaults><w:rPrDefault><w:rPr><w:sz w:val="20"/></w:rPr></w:rPrDefault></w:docDefaults>
        <w:style w:type="paragraph" w:styleId="base"><w:name w:val="Body"/>
            <w:pPr><w:spacing w:before="120" w:after="80" w:line="360"/><w:jc w:val="both"/></w:pPr>
            <w:rPr><w:rFonts w:ascii="Example Sans"/><w:sz w:val="24"/><w:color w:val="AbCDEF"/><w:b/></w:rPr>
        </w:style>
        <w:style w:type="paragraph" w:styleId="derived"><w:name w:val="BodyCustom"/><w:basedOn w:val="base"/>
            <w:pPr><w:ind w:left="240" w:hanging="60"/><w:keepNext w:val="0"/></w:pPr>
            <w:rPr><w:i/><w:b w:val="0"/></w:rPr>
        </w:style>
        <w:style w:type="character" w:styleId="char-base"><w:name w:val="Emphasis"/><w:rPr><w:i/><w:u/><w:color w:val="123456"/></w:rPr></w:style>
        <w:style w:type="character" w:styleId="char"><w:name w:val="Accent"/><w:basedOn w:val="char-base"/><w:rPr><w:i w:val="false"/><w:sz w:val="28"/><w:spacing w:val="20"/></w:rPr></w:style>
        <w:style w:type="table" w:styleId="table-base"><w:name w:val="Grid"/><w:tblPr><w:shd w:fill="ABCDEF"/></w:tblPr><w:rPr><w:b/></w:rPr></w:style>
        <w:style w:type="table" w:styleId="table"><w:name w:val="GridCustom"/><w:basedOn w:val="table-base"/><w:pPr><w:jc w:val="center"/></w:pPr><w:rPr><w:i/></w:rPr></w:style>
    "#,
    );
    let (sheet, warnings) = read_docx_styles(&bytes, &Limits::local()).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let paragraph = sheet
        .resolve_paragraph(Some(&id("BodyCustom")), &Default::default(), 64)
        .unwrap();
    assert_eq!(paragraph.font.as_ref().unwrap().family, "Example Sans");
    assert_eq!(paragraph.size_pt, Some(12.0));
    assert_eq!(paragraph.line_height, Some(1.5));
    assert_eq!(paragraph.space_before_pt, Some(6.0));
    assert_eq!(paragraph.space_after_pt, Some(4.0));
    assert_eq!(paragraph.indent_left_pt, Some(12.0));
    assert_eq!(paragraph.indent_first_pt, Some(-3.0));
    assert_eq!(paragraph.keep_with_next, Some(false));
    assert_eq!(
        sheet.paragraph[&id("BodyCustom")].based_on,
        Some(id("Body"))
    );
    let character = &sheet.character[&id("Accent")];
    assert_eq!(character.italic, Some(false));
    assert_eq!(character.underline, Some(true));
    assert_eq!(character.size_pt, Some(14.0));
    assert_eq!(character.letter_spacing, Some(1.0));
    assert!(
        sheet
            .character
            .values()
            .any(|style| style.weight == Some(400) && style.italic == Some(true))
    );
    let table = sheet
        .resolve_table(Some(&id("GridCustom")), &Default::default(), 64)
        .unwrap();
    assert_eq!(table.background.unwrap().as_str(), "#abcdef");
    assert_eq!(table.paragraph.align, Some(ParagraphAlignment::Center));
    assert_eq!(table.character.weight, Some(700));
    assert_eq!(table.character.italic, Some(true));
    let attr = (
        String::new(),
        vec![],
        vec![("custom-style".into(), "BodyCustom".into())],
    );
    let source = ast::Pandoc {
        blocks: vec![ast::Block::Div(
            attr,
            vec![ast::Block::Para(vec![ast::Inline::Span(
                (
                    String::new(),
                    vec![],
                    vec![("custom-style".into(), "Accent".into())],
                ),
                vec![ast::Inline::Underline(vec![ast::Inline::Str(
                    "text".into(),
                )])],
            )])],
        )],
        ..ast::Pandoc::default()
    };
    let out = to_ir_with_styles(&source, sheet, &Limits::local()).unwrap();
    validate_ir_document(&out.document, &Limits::local()).unwrap();
    let content = paragraph_content(&out.document.body[0]);
    let [
        Inline::Styled {
            style: Some(companion),
            content,
            ..
        },
    ] = content
    else {
        panic!("paragraph run properties lost")
    };
    assert_ne!(companion, &id("BodyCustom"));
    assert_eq!(out.document.styles.character[companion].weight, Some(400));
    assert!(
        matches!(&content[0], Inline::Styled { style: Some(style), .. } if style == &id("Accent"))
    );
}

fn paragraph_content(block: &Block) -> &[Inline] {
    paragraph(block)
}

#[test]
fn docx_styles_xml_accepts_implicit_table_normal_base() {
    let bytes = styles_package(
        r#"<w:style w:default="1" w:styleId="Table" w:type="table"><w:name w:val="Table"/><w:basedOn w:val="TableNormal"/></w:style>"#,
    );
    let (sheet, warnings) = read_docx_styles(&bytes, &Limits::local()).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(sheet.table.len(), 1);
    assert_eq!(sheet.table[&id("Table")], Default::default());
    assert_eq!(sheet.defaults.table, Default::default());
}

#[test]
fn docx_styles_xml_rejects_table_normal_base_for_non_table_styles() {
    for kind in ["paragraph", "character"] {
        for (base, error) in [
            ("", "dangling DOCX basedOn reference"),
            (
                r#"<w:style w:type="table" w:styleId="TableNormal"><w:name w:val="Normal Table"/></w:style>"#,
                "DOCX basedOn has a different style type",
            ),
        ] {
            let bytes = styles_package(&format!(
                r#"{base}<w:style w:type="{kind}" w:styleId="A"><w:basedOn w:val="TableNormal"/></w:style>"#
            ));
            assert_eq!(
                read_docx_styles(&bytes, &Limits::local()).unwrap_err(),
                error,
                "{kind} style with base {base:?}"
            );
        }
    }
}

#[test]
fn docx_styles_xml_rejects_cycles_duplicate_ids_and_expansion_limits() {
    for styles in [
        r#"<w:style w:type="paragraph" w:styleId="A"><w:basedOn w:val="B"/></w:style><w:style w:type="paragraph" w:styleId="B"><w:basedOn w:val="A"/></w:style>"#,
        r#"<w:style w:type="character" w:styleId="A"><w:basedOn w:val="A"/></w:style>"#,
        r#"<w:style w:type="table" w:styleId="A"><w:basedOn w:val="A"/></w:style>"#,
        r#"<w:style w:type="paragraph" w:styleId="A"/><w:style w:type="paragraph" w:styleId="A"/>"#,
        r#"<w:style w:type="paragraph" w:styleId="A"><w:name w:val="Same"/></w:style><w:style w:type="paragraph" w:styleId="B"><w:name w:val="Same"/></w:style>"#,
        r#"<w:style w:type="paragraph" w:styleId="A"><w:basedOn w:val="Missing"/></w:style>"#,
        r#"<w:style w:type="table" w:styleId="A"><w:basedOn w:val="Missing"/></w:style>"#,
        r#"<w:style w:type="paragraph" w:styleId="A"><w:rPr><w:sz w:val="NaN"/></w:rPr></w:style>"#,
    ] {
        assert!(
            read_docx_styles(&styles_package(styles), &Limits::local()).is_err(),
            "{styles}"
        );
    }
    let bytes = styles_package(r#"<w:style w:type="paragraph" w:styleId="A"/>"#);
    for limits in [
        Limits {
            max_decompressed_bytes: 8,
            ..Limits::local()
        },
        Limits {
            max_ir_json_bytes: 8,
            ..Limits::local()
        },
        Limits {
            max_nesting_depth: 1,
            ..Limits::local()
        },
        Limits {
            max_blocks: 0,
            ..Limits::local()
        },
        Limits {
            max_input_bytes: Some(8),
            ..Limits::local()
        },
    ] {
        assert!(read_docx_styles(&bytes, &limits).is_err());
    }
    let two = docx(&[("word/styles.xml", "<styles/>"), ("extra", "text")]);
    assert!(
        read_docx_styles(
            &two,
            &Limits {
                max_archive_entries: 1,
                ..Limits::local()
            }
        )
        .is_err()
    );
    for xml in [
        format!(r#"<!DOCTYPE w:styles [<!ENTITY x "boom">]><w:styles xmlns:w="{WORD_NS}"/>"#),
        format!(r#"<w:styles xmlns:w="{WORD_NS}"><w:style"#),
        format!(r#"<w:styles xmlns:w="{WORD_NS}"/><w:styles xmlns:w="{WORD_NS}"/>"#),
        r#"<w:styles xmlns:w="urn:unrelated"/>"#.into(),
    ] {
        assert!(
            read_docx_styles(&docx(&[("word/styles.xml", &xml)]), &Limits::local()).is_err(),
            "{xml}"
        );
    }
    assert!(read_docx_styles(&docx(&[("../word/styles.xml", "bad")]), &Limits::local()).is_err());
}

#[test]
fn html_inline_css_rejects_external_resources_and_invalid_values() {
    let out = html::read(br#"<p style="color:url(https://example.test/x);font-family:expression(alert(1));font-size:NaNpt;line-height:inf;text-indent:1e999pt;--x:@import;background-color:#xyz;unknown:10px">safe <span style="color:url(file:///etc/passwd);unknown:2">content</span><a href="javascript:alert(1)">link</a></p>"#, &Limits::local()).unwrap();
    assert!(out.document.body[0].direct().is_empty());
    let serialized = serde_json::to_string(&out.document).unwrap();
    assert!(!serialized.contains("url("));
    assert!(!serialized.contains("expression("));
    assert!(!serialized.contains("javascript:"));
    let text: String = paragraph(&out.document.body[0])
        .iter()
        .map(|inline| match inline {
            Inline::Text { text } => text.as_str(),
            _ => panic!("expected preserved plain text"),
        })
        .collect();
    assert_eq!(text, "safe contentlink");
    assert_eq!(
        out.warnings
            .iter()
            .filter(|warning| warning.message.contains("CSS `color`"))
            .count(),
        1
    );
    assert_eq!(
        out.warnings
            .iter()
            .filter(|warning| warning.message.contains("CSS `unknown`"))
            .count(),
        1
    );
    validate_ir_document(&out.document, &Limits::local()).unwrap();
}

#[test]
fn styled_readers_preserve_spans_footnotes_captions_and_furniture() {
    let source = json!({
        "schema_name":"DoclingDocument", "version":"1.10.0",
        "body":{"children":[{"$ref":"#/tables/0"}]},
        "furniture":{"children":[{"$ref":"#/texts/2"}]},
        "texts":[
            {"label":"caption","text":"caption","formatting":{"underline":true}},
            {"label":"footnote","text":"note","formatting":{"italic":true}},
            {"label":"page_header","text":"header","formatting":{"bold":true}}
        ],
        "tables":[{"captions":[{"$ref":"#/texts/0"}],"footnotes":[{"$ref":"#/texts/1"}],
            "prov":[{"page_no":1,"bbox":{"l":10,"t":20,"r":200,"b":100,"coord_origin":"TOPLEFT"}}],
            "data":{"table_cells":[
                {"row_span":2,"col_span":2,"start_row_offset_idx":0,"start_col_offset_idx":0,"text":"cell","column_header":true},
                {"row_span":1,"col_span":1,"start_row_offset_idx":0,"start_col_offset_idx":2,"text":"right","column_header":true},
                {"row_span":1,"col_span":1,"start_row_offset_idx":1,"start_col_offset_idx":2,"text":"lower","column_header":true},
                {"row_span":1,"col_span":1,"start_row_offset_idx":2,"start_col_offset_idx":1,"text":"body"}
            ]}}],
        "pages":{"1":{"page_no":1,"size":{"width":600,"height":800}}}
    });
    let out = docling::read(&serde_json::to_vec(&source).unwrap(), &Limits::local()).unwrap();
    let Block::Table {
        caption: Some(caption),
        head,
        footnotes,
        ..
    } = &out.document.body[0]
    else {
        panic!("table lost")
    };
    assert_eq!(head[0][0].rowspan, 2);
    assert_eq!(head[0][0].colspan, 2);
    assert_eq!(styled(caption).0.underline, Some(true));
    assert!(!footnotes.is_empty());
    assert_eq!(out.document.furniture.len(), 1);
    assert!(
        out.document
            .layout
            .as_ref()
            .unwrap()
            .blocks
            .contains_key(out.document.body[0].id())
    );
    assert_ne!(out.document.furniture[0].id(), out.document.body[0].id());
    validate_ir_document(&out.document, &Limits::local()).unwrap();
    let out = html::read(br##"<table style="font-size:12pt"><caption style="color:red">caption</caption><tr><th rowspan="2" colspan="2"><span style="font-weight:bold">cell</span></th></tr></table><aside role="doc-footnote" id="n"><p style="font-style:italic">note</p></aside><p><a role="doc-noteref" href="#n">1</a> <code style="color:blue">  e&#x301;  </code></p>"##, &Limits::local()).unwrap();
    assert!(
        matches!(&out.document.body[0], Block::Table { caption: Some(_), head, .. } if head[0][0].rowspan == 2 && head[0][0].colspan == 2)
    );
    assert!(matches!(&out.document.body[1], Block::Footnote { note_id, .. } if note_id == "n"));
    assert!(
        serde_json::to_string(&out.document)
            .unwrap()
            .contains("  é  ")
    );
    validate_ir_document(&out.document, &Limits::local()).unwrap();
}

#[test]
fn styled_figure_captions_preserve_structure_and_independent_alt_text() {
    for alt in ["caption", "independent alt"] {
        let source = format!(
            "<figure style=\"font-size:12pt\">\n<img src=\"chart.png\" alt=\"{alt}\">\n<figcaption><span style=\"color:red\">caption</span></figcaption>\n</figure>"
        );
        let out = html::read(source.as_bytes(), &Limits::local()).unwrap();
        if alt == "caption" {
            let [Block::Figure { caption, .. }] = out.document.body.as_slice() else {
                panic!(
                    "styled caption changed figure structure: {:?}",
                    out.document.body
                );
            };
            assert_eq!(
                styled(caption).0.color.as_ref().unwrap().as_str(),
                "#ff0000"
            );
        } else {
            assert_eq!(out.document.body.len(), 2);
            let (direct, content) = styled(paragraph(&out.document.body[0]));
            assert_eq!(direct.size_pt, Some(12.0));
            assert!(matches!(content, [Inline::Image { alt: actual, .. }] if actual == alt));
        }
        let expected = if alt == "caption" {
            "caption".to_owned()
        } else {
            format!("{alt}\ncaption")
        };
        assert_eq!(
            ariad_core::fidelity::observe(&out.document).unwrap().text,
            expected
        );
        validate_ir_document(&out.document, &Limits::local()).unwrap();
    }
}

#[test]
fn docx_missing_styles_and_unsupported_properties_warn_without_fabricated_metrics() {
    let (sheet, warnings) = read_docx_styles(
        &docx(&[("word/document.xml", "<document/>")]),
        &Limits::local(),
    )
    .unwrap();
    assert_eq!(sheet, StyleSheet::default());
    assert_eq!(warnings.len(), 1);
    let bytes = styles_package(
        r#"<w:style w:type="character" w:styleId="A"><w:rPr><w:rFonts w:asciiTheme="minorHAnsi"/><w:caps/><w:caps/></w:rPr></w:style>"#,
    );
    let (sheet, warnings) = read_docx_styles(&bytes, &Limits::local()).unwrap();
    assert!(sheet.character[&id("A")].font.is_none());
    assert_eq!(warnings.len(), 2);
}

fn paragraph(block: &Block) -> &[Inline] {
    match block {
        Block::Paragraph { content, .. } | Block::Heading { content, .. } => content,
        other => panic!("expected text block, got {other:?}"),
    }
}

fn styled(content: &[Inline]) -> (&CharacterStyle, &[Inline]) {
    match content {
        [
            Inline::Styled {
                direct, content, ..
            },
        ] => (direct, content),
        other => panic!("expected styled content, got {other:?}"),
    }
}

fn runs(content: &[Inline], parent: &CharacterStyle, result: &mut Vec<(String, CharacterStyle)>) {
    for inline in content {
        match inline {
            Inline::Text { text } => result.push((text.clone(), parent.clone())),
            Inline::Styled {
                direct, content, ..
            } => {
                let mut style = parent.clone();
                style.overlay(direct);
                runs(content, &style, result);
            }
            _ => panic!("unexpected run {inline:?}"),
        }
    }
}

#[test]
fn docling_styles_map_available_formatting_and_preserve_unknowns() {
    let source = json!({
        "schema_name": "DoclingDocument", "version": "1.10.0",
        "body": {"children": [{"$ref":"#/texts/0"}, {"$ref":"#/texts/1"}]},
        "texts": [
            {"label":"text", "text":"underlined", "formatting":{
                "bold":false, "italic":false, "underline":true,
                "strikethrough":false, "script":"baseline"
            }},
            {"label":"text", "text":"unknown metadata", "future_field":{"family":"invented"}}
        ]
    });
    let out = docling::read(&serde_json::to_vec(&source).unwrap(), &Limits::local()).unwrap();
    let (direct, content) = styled(paragraph(&out.document.body[0]));
    assert_eq!(direct.underline, Some(true));
    assert_eq!(direct.weight, Some(400));
    assert_eq!(direct.italic, Some(false));
    assert_eq!(direct.strike, Some(false));
    assert_eq!(direct.font, None);
    assert_eq!(direct.size_pt, None);
    assert_eq!(direct.color, None);
    assert!(matches!(content, [Inline::Text { text }] if text == "underlined"));
    assert!(matches!(
        paragraph(&out.document.body[1]),
        [Inline::Text { .. }]
    ));
    assert!(out.warnings.is_empty());
    validate_ir_document(&out.document, &Limits::local()).unwrap();
}

#[test]
fn pandoc_custom_style_div_and_span_keep_named_styles() {
    let attr = |name: &str| {
        (
            String::new(),
            vec![],
            vec![("custom-style".into(), name.into())],
        )
    };
    let source = ast::Pandoc {
        blocks: vec![ast::Block::Div(
            attr("Body Text"),
            vec![
                ast::Block::Header(
                    2,
                    attr("Heading Custom"),
                    vec![ast::Inline::Str("Heading".into())],
                ),
                ast::Block::Para(vec![ast::Inline::Span(
                    attr("Accent"),
                    vec![
                        ast::Inline::Str("styled".into()),
                        ast::Inline::Note(vec![ast::Block::Para(vec![ast::Inline::Str(
                            "note".into(),
                        )])]),
                    ],
                )]),
            ],
        )],
        ..ast::Pandoc::default()
    };
    let out = to_ir(&source, &Limits::local()).unwrap();
    let Block::Heading {
        style: Some(heading),
        ..
    } = &out.document.body[0]
    else {
        panic!("heading style lost")
    };
    let Block::Paragraph {
        style: Some(body),
        content,
        ..
    } = &out.document.body[1]
    else {
        panic!("paragraph style lost")
    };
    assert_ne!(heading, body);
    assert!(out.document.styles.paragraph.contains_key(heading));
    assert!(out.document.styles.paragraph.contains_key(body));
    let [
        Inline::Styled {
            style: Some(accent),
            content,
            ..
        },
    ] = content.as_slice()
    else {
        panic!("character style lost")
    };
    assert!(out.document.styles.character.contains_key(accent));
    assert!(
        matches!(content.as_slice(), [Inline::Text { text }, Inline::FootnoteRef { .. }] if text == "styled")
    );
    assert!(matches!(&out.document.body[2], Block::Footnote { blocks, .. } if blocks.len() == 1));
    validate_ir_document(&out.document, &Limits::local()).unwrap();
}

#[test]
fn html_inline_css_maps_supported_properties_and_inheritance() {
    let out = html::read(br#"<div style="font-family: 'Example Sans', serif; color: #AbC; font-size:16px; text-align:center"><p style="margin-top:6pt;margin-bottom:8px;text-indent:-2pt;padding-left:4px;padding-right:3pt;line-height:1.5"><span style="font-weight:700;font-style:italic;text-decoration:underline line-through;background-color:#123456">outer <span style="font-weight:normal;font-style:normal;text-decoration:none">reset</span></span></p></div>"#, &Limits::local()).unwrap();
    let Block::Paragraph {
        direct, content, ..
    } = &out.document.body[0]
    else {
        panic!("paragraph")
    };
    assert_eq!(direct.align, Some(ParagraphAlignment::Center));
    assert_eq!(direct.size_pt, Some(12.0));
    assert_eq!(direct.font.as_ref().unwrap().family, "Example Sans");
    assert_eq!(direct.font.as_ref().unwrap().fallback, ["serif"]);
    assert_eq!(direct.space_before_pt, Some(6.0));
    assert_eq!(direct.space_after_pt, Some(6.0));
    assert_eq!(direct.indent_first_pt, Some(-2.0));
    assert_eq!(direct.indent_left_pt, Some(3.0));
    assert_eq!(direct.indent_right_pt, Some(3.0));
    assert_eq!(direct.line_height, Some(1.5));
    let serialized = serde_json::to_value(content).unwrap();
    assert!(serialized.to_string().contains("#aabbcc"));
    assert!(serialized.to_string().contains("\"underline\":false"));
    assert!(serialized.to_string().contains("\"weight\":400"));
    let mut text_runs = Vec::new();
    runs(content, &CharacterStyle::default(), &mut text_runs);
    assert_eq!(text_runs.len(), 2);
    assert_eq!(text_runs[0].0, "outer ");
    assert_eq!(text_runs[0].1.weight, Some(700));
    assert_eq!(text_runs[0].1.italic, Some(true));
    assert_eq!(text_runs[0].1.underline, Some(true));
    assert_eq!(text_runs[0].1.strike, Some(true));
    assert_eq!(text_runs[1].0, "reset");
    assert_eq!(text_runs[1].1.weight, Some(400));
    assert_eq!(text_runs[1].1.italic, Some(false));
    assert_eq!(text_runs[1].1.underline, Some(false));
    assert_eq!(text_runs[1].1.strike, Some(false));
    assert_eq!(
        text_runs[1].1.highlight.as_ref().unwrap().as_str(),
        "#123456"
    );
    assert!(out.warnings.is_empty(), "{:?}", out.warnings);
    validate_ir_document(&out.document, &Limits::local()).unwrap();
}

#[test]
fn docx_style_inheritance_expansion_and_depth_are_bounded() {
    let mut xml = format!(
        r#"<w:style w:type="character" w:styleId="Base"><w:rPr><w:rFonts w:ascii="{}"/></w:rPr></w:style>"#,
        "Family".repeat(100)
    );
    for index in 0..100 {
        xml.push_str(&format!(r#"<w:style w:type="character" w:styleId="Run{index}"><w:basedOn w:val="Base"/></w:style>"#));
    }
    let bytes = styles_package(&xml);
    assert!(read_docx_styles(&bytes, &Limits::local()).is_ok());
    let limits = Limits {
        max_ir_json_bytes: xml.len() as u64 + 1024,
        ..Limits::local()
    };
    let error = read_docx_styles(&bytes, &limits).unwrap_err();
    assert!(error.contains("resolved DOCX styles"), "{error}");
    let mut xml = String::new();
    for index in 0..65 {
        let based = if index == 0 {
            String::new()
        } else {
            format!(r#"<w:basedOn w:val="Run{}"/>"#, index - 1)
        };
        xml.push_str(&format!(
            r#"<w:style w:type="character" w:styleId="Run{index}">{based}</w:style>"#
        ));
    }
    let error = read_docx_styles(&styles_package(&xml), &Limits::local()).unwrap_err();
    assert!(error.contains("inheritance exceeds depth"), "{error}");
}

#[test]
fn html_styles_respect_computed_units_and_nesting_limits() {
    let out = html::read(br#"<p style="line-height:24px;font-size:12pt">one <span style="font-size:24pt">two</span></p>"#, &Limits::local()).unwrap();
    assert_eq!(out.document.body[0].direct().line_height, Some(1.5));
    let mut text_runs = Vec::new();
    runs(
        paragraph(&out.document.body[0]),
        &CharacterStyle::default(),
        &mut text_runs,
    );
    assert_eq!(text_runs[0].1.size_pt, Some(12.0));
    assert_eq!(text_runs[1].1.size_pt, Some(24.0));
    let source = format!(
        "<p>{}text{}</p>",
        "<strong>".repeat(64),
        "</strong>".repeat(64)
    );
    assert!(html::read(source.as_bytes(), &Limits::local()).is_ok());
    let source = source.replace("<p>", "<p style=\"color:red\">");
    assert!(matches!(
        html::read(source.as_bytes(), &Limits::local()),
        Err(html::ReadError::NestingTooDeep { limit: 64 })
    ));
}

#[test]
fn html_css_declarations_do_not_escape_quoted_values_and_inherit_resets_prior_values() {
    let out = html::read(br#"<div style="font-weight:400;color:blue"><p style="font-family:'bad; color:red';font-weight:700;font-weight:inherit">text</p></div>"#, &Limits::local()).unwrap();
    let (direct, _) = styled(paragraph(&out.document.body[0]));
    assert_eq!(direct.weight, Some(400));
    assert_eq!(direct.color.as_ref().unwrap().as_str(), "#0000ff");
    assert_eq!(out.warnings.len(), 1);
    assert!(out.warnings[0].message.contains("font-family"));
}

#[test]
fn html_box_properties_stay_local_and_invalid_line_height_keeps_prior_value() {
    let out = html::read(br#"<blockquote style="margin-top:8pt;padding-left:6pt;background-color:red"><p style="line-height:1.5;line-height:bogus">text</p></blockquote>"#, &Limits::local()).unwrap();
    let Block::Quote { direct, blocks, .. } = &out.document.body[0] else {
        panic!("quote")
    };
    assert_eq!(direct.space_before_pt, Some(8.0));
    assert_eq!(direct.indent_left_pt, Some(6.0));
    assert!(direct.background.is_some());
    assert_eq!(blocks[0].direct().space_before_pt, None);
    assert_eq!(blocks[0].direct().indent_left_pt, None);
    assert_eq!(blocks[0].direct().background, None);
    assert_eq!(blocks[0].direct().line_height, Some(1.5));
    assert_eq!(out.warnings.len(), 1);
}

#[test]
fn pandoc_same_paragraph_and_character_name_does_not_style_unrelated_runs() {
    let attr = || {
        (
            String::new(),
            vec![],
            vec![("custom-style".into(), "Shared".into())],
        )
    };
    let source = ast::Pandoc {
        blocks: vec![ast::Block::Div(
            attr(),
            vec![ast::Block::Para(vec![
                ast::Inline::Str("plain ".into()),
                ast::Inline::Span(attr(), vec![ast::Inline::Str("styled".into())]),
            ])],
        )],
        ..ast::Pandoc::default()
    };
    let out = to_ir(&source, &Limits::local()).unwrap();
    assert!(
        matches!(paragraph(&out.document.body[0]), [Inline::Text { text }, Inline::Styled { style: Some(style), .. }] if text == "plain " && style == &id("Shared"))
    );
    let (sheet, _) = read_docx_styles(&styles_package(r#"<w:style w:type="numbering" w:styleId="Ignored"><w:basedOn w:val="Missing"/><w:rPr><w:sz w:val="NaN"/></w:rPr></w:style>"#), &Limits::local()).unwrap();
    assert!(sheet.is_empty());
}

#[test]
fn generated_style_wrappers_share_the_block_and_inline_nesting_budget() {
    let source = json!({"schema_name":"DoclingDocument", "version":"1.10.0",
        "body":{"children":[{"$ref":"#/texts/0"}]},
        "texts":[{"label":"text","text":"bold","formatting":{"bold":true}}]});
    let bytes = serde_json::to_vec(&source).unwrap();
    let limits = Limits {
        max_nesting_depth: 1,
        ..Limits::local()
    };
    assert!(matches!(
        docling::read(&bytes, &limits),
        Err(docling::ReadError::NestingTooDeep { limit: 1 })
    ));
    assert!(
        docling::read(
            &bytes,
            &Limits {
                max_nesting_depth: 2,
                ..Limits::local()
            }
        )
        .is_ok()
    );
    let source = format!(
        "<table><caption style=\"color:red\">{}caption{}</caption><tr><td>cell</td></tr></table>",
        "<strong>".repeat(63),
        "</strong>".repeat(63)
    );
    assert!(matches!(
        html::read(source.as_bytes(), &Limits::local()),
        Err(html::ReadError::NestingTooDeep { limit: 64 })
    ));
}

#[test]
fn docx_document_properties_recover_direct_runs_and_reject_hostile_xml() {
    use ariad_core::pandoc::to_ir::to_ir_docx;
    let styles = format!(
        r#"<w:styles xmlns:w="{WORD_NS}"><w:style w:type="paragraph" w:styleId="body"><w:name w:val="Body"/><w:rPr><w:sz w:val="22"/></w:rPr></w:style></w:styles>"#
    );
    let xml = format!(
        r#"<w:document xmlns:w="{WORD_NS}"><w:body><w:p/><w:p><w:pPr><w:pStyle w:val="body"/><w:jc w:val="center"/></w:pPr><w:r><w:rPr><w:sz w:val="32"/><w:b w:val="0"/><w:color w:val="123456"/><w:rPrChange><w:rPr><w:sz w:val="64"/><w:b/></w:rPr></w:rPrChange></w:rPr><w:t>A &amp; B</w:t></w:r></w:p></w:body></w:document>"#
    );
    let ast: ast::Pandoc = serde_json::from_value(json!({"pandoc-api-version":[1,23], "meta":{}, "blocks":[{"t":"Para","c":[{"t":"Str","c":"A"},{"t":"Space"},{"t":"Str","c":"&"},{"t":"Space"},{"t":"Str","c":"B"}]}]})).unwrap();
    let bytes = docx(&[("word/styles.xml", &styles), ("word/document.xml", &xml)]);
    let mapped = to_ir_docx(&ast, &bytes, &Limits::local()).unwrap();
    assert_eq!(
        mapped.document.body[0].style().as_ref().unwrap().as_str(),
        "Body"
    );
    assert_eq!(
        mapped.document.body[0].direct().align,
        Some(ParagraphAlignment::Center)
    );
    let observations = ariad_core::fidelity::observe(&mapped.document).unwrap();
    assert!(
        observations
            .runs
            .iter()
            .any(|run| run.style.get("size_pt") == Some(&json!(16.0))
                && run.style.get("weight") == Some(&json!(400)))
    );
    for hostile in [
        format!(
            r#"<!DOCTYPE document [<!ENTITY x SYSTEM "file:///etc/passwd">]><w:document xmlns:w="{WORD_NS}"><w:body/></w:document>"#
        ),
        format!(
            r#"<w:document xmlns:w="{WORD_NS}"><w:body><w:p><w:r><w:r/></w:r></w:p></w:body></w:document>"#
        ),
        format!(r#"<w:document xmlns:w="{WORD_NS}"><w:body><w:p/><w:p/></w:body></w:document>"#),
    ] {
        let bytes = docx(&[
            ("word/styles.xml", &styles),
            ("word/document.xml", &hostile),
        ]);
        let limits = Limits {
            max_blocks: 1,
            ..Limits::local()
        };
        assert!(to_ir_docx(&ast, &bytes, &limits).is_err());
    }
}
