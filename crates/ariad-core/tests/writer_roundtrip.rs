use std::fs;

use ariad_core::{
    ir::{
        Alignment, Asset, AssetRef, Block, ColumnSpec, Document, Inline, ListItem, RawFormat,
        TableCell,
    },
    limits::Limits,
    reader::{html, markdown},
    warning::WarningCode,
    writer,
};

fn normalize_inlines(inlines: &[Inline]) -> Vec<Inline> {
    let mut out: Vec<Inline> = Vec::new();
    for inline in inlines {
        let normalized = match inline {
            Inline::Emph { content } => Inline::Emph {
                content: normalize_inlines(content),
            },
            Inline::Strong { content } => Inline::Strong {
                content: normalize_inlines(content),
            },
            Inline::Strikeout { content } => Inline::Strikeout {
                content: normalize_inlines(content),
            },
            Inline::Superscript { content } => Inline::Superscript {
                content: normalize_inlines(content),
            },
            Inline::Subscript { content } => Inline::Subscript {
                content: normalize_inlines(content),
            },
            Inline::Link {
                url,
                title,
                content,
            } => Inline::Link {
                url: url.clone(),
                title: title.clone(),
                content: normalize_inlines(content),
            },
            Inline::Math { tex, display } => Inline::Math {
                tex: tex.trim().to_owned(),
                display: *display,
            },
            other => other.clone(),
        };

        if let Inline::Text { ref text } = normalized
            && let Some(Inline::Text { text: prev }) = out.last_mut()
        {
            prev.push_str(text);
            continue;
        }
        out.push(normalized);
    }
    out
}

fn normalize_blocks(blocks: &[Block]) -> Vec<Block> {
    blocks
        .iter()
        .map(|block| match block {
            Block::Heading { level, content } => Block::Heading {
                level: *level,
                content: normalize_inlines(content),
            },
            Block::Paragraph { content } => Block::Paragraph {
                content: normalize_inlines(content),
            },
            Block::Quote { blocks } => Block::Quote {
                blocks: normalize_blocks(blocks),
            },
            Block::List {
                ordered,
                start,
                tight,
                items,
            } => Block::List {
                ordered: *ordered,
                start: *start,
                tight: *tight,
                items: items
                    .iter()
                    .map(|item| ListItem {
                        checked: item.checked,
                        blocks: normalize_blocks(&item.blocks),
                    })
                    .collect(),
            },
            Block::Table {
                caption,
                columns,
                head,
                body,
                footnotes,
            } => Block::Table {
                caption: caption.as_ref().map(|c| normalize_inlines(c)),
                columns: columns.clone(),
                head: head
                    .iter()
                    .map(|row| {
                        row.iter()
                            .map(|cell| TableCell {
                                rowspan: cell.rowspan,
                                colspan: cell.colspan,
                                header: cell.header,
                                blocks: normalize_blocks(&cell.blocks),
                            })
                            .collect()
                    })
                    .collect(),
                body: body
                    .iter()
                    .map(|row| {
                        row.iter()
                            .map(|cell| TableCell {
                                rowspan: cell.rowspan,
                                colspan: cell.colspan,
                                header: cell.header,
                                blocks: normalize_blocks(&cell.blocks),
                            })
                            .collect()
                    })
                    .collect(),
                footnotes: normalize_inlines(footnotes),
            },
            Block::Footnote { id, blocks } => Block::Footnote {
                id: id.clone(),
                blocks: normalize_blocks(blocks),
            },
            Block::Figure { asset, caption } => Block::Figure {
                asset: asset.clone(),
                caption: normalize_inlines(caption),
            },
            Block::Math { tex, display } => Block::Math {
                tex: tex.trim().to_owned(),
                display: *display,
            },
            Block::Raw { format, text } => Block::Raw {
                format: *format,
                text: if *format == ariad_core::ir::RawFormat::Html {
                    writer::sanitize::sanitize_raw_html(text)
                        .trim_end_matches('\n')
                        .to_owned()
                } else {
                    text.trim_end_matches('\n').to_owned()
                },
            },
            other => other.clone(),
        })
        .collect()
}

fn normalize_document(doc: &Document) -> Document {
    let mut normalized = doc.clone();
    normalized.meta.source_format = None;
    normalized.body = normalize_blocks(&doc.body);
    normalized.furniture = normalize_blocks(&doc.furniture);
    normalized
}

#[test]
fn all_markdown_fixtures_roundtrip_through_markdown_writer() {
    let mut fixture_count = 0;
    insta::glob!("../../../fixtures", "md/*.md", |path| {
        fixture_count += 1;
        let original_text = fs::read_to_string(path).expect("read Markdown fixture");
        let read1 = markdown::read(&original_text, &Limits::local())
            .unwrap_or_else(|err| panic!("first read of {}: {err}", path.display()));

        let written = writer::markdown::write(&read1.document);
        let is_raw_html_fixture = path.file_name().is_some_and(|n| n == "vi-raw-html.md");
        if is_raw_html_fixture {
            // Per manager decision and fallback, the Markdown writer strips raw HTML tags.
            // Verify that the written content preserves text, strips tags, and emits RawDropped warning.
            assert!(
                written
                    .warnings
                    .iter()
                    .any(|w| w.code == WarningCode::RawDropped)
            );
            assert!(written.content.contains("Lối xuống bến ở phía bên trái."));
            assert!(written.content.contains("Đoạn đường này đang được sửa."));
            assert!(!written.content.contains("<span"));
            assert!(!written.content.contains("<div"));
            return;
        }

        let read2 = markdown::read(&written.content, &Limits::local()).unwrap_or_else(|err| {
            panic!(
                "second read of {}: {err}\nWritten text was:\n{}",
                path.display(),
                written.content
            )
        });

        let norm1 = normalize_document(&read1.document);
        let norm2 = normalize_document(&read2.document);
        assert_eq!(
            norm2,
            norm1,
            "roundtrip mismatch for fixture {}",
            path.display()
        );
    });
    assert!(
        fixture_count >= 20,
        "expected at least 20 fixtures, found {fixture_count}"
    );
}

#[test]
fn longest_backtick_rule_for_fenced_code_blocks() {
    let mut doc = Document::default();
    doc.body.push(Block::Code {
        lang: Some("markdown".to_owned()),
        text: "Here is code with ``` triple backticks and ```` four backticks.\n".to_owned(),
    });

    let written = writer::markdown::write(&doc);
    assert!(written.content.starts_with("`````markdown\n"));
    assert!(written.content.ends_with("`````\n\n"));

    let read_back = markdown::read(&written.content, &Limits::local()).expect("read back");
    assert_eq!(read_back.document.body, doc.body);
}

#[test]
fn inline_code_handles_backticks_and_edge_spaces() {
    let mut doc = Document::default();
    doc.body.push(Block::Paragraph {
        content: vec![
            Inline::Code {
                text: "let x = `single backtick`;".to_owned(),
            },
            Inline::Text {
                text: " and ".to_owned(),
            },
            Inline::Code {
                text: " `leading and trailing` ".to_owned(),
            },
        ],
    });

    let written = writer::markdown::write(&doc);
    let read_back = markdown::read(&written.content, &Limits::local()).expect("read back");
    assert_eq!(read_back.document.body, doc.body);
}

#[test]
fn escaping_markdown_syntax_characters_in_text() {
    let mut doc = Document::default();
    doc.body.push(Block::Paragraph {
        content: vec![Inline::Text {
            text:
                r"Special *stars* _unders_ ~tilde~ `ticks` [link] <tag> $math$ ^carets^ \slashes\"
                    .to_owned(),
        }],
    });
    doc.body.push(Block::Paragraph {
        content: vec![
            Inline::Text {
                text: "# not a heading".to_owned(),
            },
            Inline::SoftBreak {},
            Inline::Text {
                text: "- not a list".to_owned(),
            },
            Inline::SoftBreak {},
            Inline::Text {
                text: "1. not an ordered list".to_owned(),
            },
            Inline::SoftBreak {},
            Inline::Text {
                text: "> not a quote".to_owned(),
            },
        ],
    });

    let written = writer::markdown::write(&doc);
    let read_back = markdown::read(&written.content, &Limits::local()).expect("read back");
    assert_eq!(read_back.document.body, doc.body);
}

#[test]
fn asset_backed_images_are_embedded_as_data_uris() {
    let mut doc = Document::default();
    let image_bytes = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDRfakeimage";
    doc.assets.insert(
        "abc123".to_owned(),
        Asset {
            media_type: "image/png".to_owned(),
            bytes: image_bytes.to_vec(),
        },
    );
    doc.body.push(Block::Paragraph {
        content: vec![Inline::Image {
            target: AssetRef::Asset {
                id: "abc123".to_owned(),
            },
            alt: "diagram".to_owned(),
            title: Some("System architecture".to_owned()),
        }],
    });

    let written = writer::markdown::write(&doc);
    assert!(
        written
            .content
            .contains("![diagram](data:image/png;base64,")
    );
    assert!(written.content.contains(" \"System architecture\")"));

    let read_back = markdown::read(&written.content, &Limits::local()).expect("read back");
    match &read_back.document.body[0] {
        Block::Paragraph { content } => match &content[0] {
            Inline::Image { target, alt, title } => {
                assert_eq!(alt, "diagram");
                assert_eq!(title.as_deref(), Some("System architecture"));
                match target {
                    AssetRef::Url { href } => {
                        assert!(href.starts_with("data:image/png;base64,"));
                    }
                    other => panic!("expected Url target, got {other:?}"),
                }
            }
            other => panic!("expected Image inline, got {other:?}"),
        },
        other => panic!("expected Paragraph block, got {other:?}"),
    }
}

#[test]
fn missing_asset_emits_warning_and_reduces_to_alt_text() {
    let mut doc = Document::default();
    doc.body.push(Block::Paragraph {
        content: vec![Inline::Image {
            target: AssetRef::Asset {
                id: "missing".to_owned(),
            },
            alt: "missing photo".to_owned(),
            title: None,
        }],
    });

    let written = writer::markdown::write(&doc);
    assert_eq!(written.warnings.len(), 1);
    assert_eq!(written.warnings[0].code, WarningCode::ImageNotEmbedded);
    assert!(written.content.contains("missing photo"));
    assert!(!written.content.contains("!["));
}

#[test]
fn dangerous_links_and_images_are_dropped_with_warnings() {
    let mut doc = Document::default();
    doc.body.push(Block::Paragraph {
        content: vec![
            Inline::Link {
                url: "javascript:alert(1)".to_owned(),
                title: None,
                content: vec![Inline::Text {
                    text: "click me".to_owned(),
                }],
            },
            Inline::Text {
                text: " and ".to_owned(),
            },
            Inline::Image {
                target: AssetRef::Url {
                    href: "javascript:evil()".to_owned(),
                },
                alt: "dangerous image".to_owned(),
                title: None,
            },
        ],
    });

    let written = writer::markdown::write(&doc);
    assert_eq!(written.warnings.len(), 2);
    assert_eq!(written.warnings[0].code, WarningCode::LinkDropped);
    assert_eq!(written.warnings[1].code, WarningCode::ImageNotEmbedded);
    assert!(!written.content.contains("javascript:"));
    assert!(written.content.contains("click me and dangerous image"));
}

#[test]
fn non_representable_table_emits_warning_and_simplifies_to_pipe_table() {
    let mut doc = Document::default();
    doc.body.push(Block::Table {
        caption: None,
        columns: vec![
            ColumnSpec {
                align: Alignment::Left,
            },
            ColumnSpec {
                align: Alignment::Right,
            },
        ],
        head: vec![vec![
            TableCell {
                rowspan: 1,
                colspan: 2,
                header: Some(true),
                blocks: vec![Block::Paragraph {
                    content: vec![Inline::Text {
                        text: "Spanned Header".to_owned(),
                    }],
                }],
            },
            TableCell {
                rowspan: 1,
                colspan: 1,
                header: Some(true),
                blocks: vec![],
            },
        ]],
        body: vec![vec![
            TableCell {
                rowspan: 2,
                colspan: 1,
                header: None,
                blocks: vec![Block::Paragraph {
                    content: vec![Inline::Text {
                        text: "Spanned Row".to_owned(),
                    }],
                }],
            },
            TableCell {
                rowspan: 1,
                colspan: 1,
                header: None,
                blocks: vec![Block::Paragraph {
                    content: vec![Inline::Text {
                        text: "Cell B".to_owned(),
                    }],
                }],
            },
        ]],
        footnotes: Vec::new(),
    });

    let written = writer::markdown::write(&doc);
    assert_eq!(written.warnings.len(), 1);
    assert_eq!(written.warnings[0].code, WarningCode::UnsupportedNode);
    assert!(written.content.contains("| Spanned Header |"));
}

#[test]
fn ordered_list_honours_custom_start() {
    let mut doc = Document::default();
    doc.body.push(Block::List {
        ordered: true,
        start: Some(7),
        tight: true,
        items: vec![
            ListItem {
                checked: None,
                blocks: vec![Block::Paragraph {
                    content: vec![Inline::Text {
                        text: "Step seven".to_owned(),
                    }],
                }],
            },
            ListItem {
                checked: None,
                blocks: vec![Block::Paragraph {
                    content: vec![Inline::Text {
                        text: "Step eight".to_owned(),
                    }],
                }],
            },
        ],
    });

    let written = writer::markdown::write(&doc);
    assert!(written.content.contains("7. Step seven\n8. Step eight\n"));

    let read_back = markdown::read(&written.content, &Limits::local()).expect("read back");
    assert_eq!(read_back.document.body, doc.body);
}

#[test]
fn math_and_sub_sup_roundtrip() {
    let mut doc = Document::default();
    doc.body.push(Block::Paragraph {
        content: vec![
            Inline::Math {
                tex: "a^2 + b^2 = c^2".to_owned(),
                display: false,
            },
            Inline::Text {
                text: " and ".to_owned(),
            },
            Inline::Superscript {
                content: vec![Inline::Text {
                    text: "up".to_owned(),
                }],
            },
            Inline::Text {
                text: " and ".to_owned(),
            },
            Inline::Subscript {
                content: vec![Inline::Text {
                    text: "down".to_owned(),
                }],
            },
        ],
    });
    doc.body.push(Block::Math {
        tex: r"\int_0^\infty e^{-x} dx = 1".to_owned(),
        display: true,
    });

    let written = writer::markdown::write(&doc);
    assert!(written.content.contains("$a^2 + b^2 = c^2$"));
    assert!(written.content.contains("<sup>up</sup>"));
    assert!(written.content.contains("<sub>down</sub>"));
    assert!(written.content.contains(r"$$\int_0^\infty e^{-x} dx = 1$$"));
}

#[test]
fn html_writer_semantic_roundtrip() {
    use ariad_core::reader::html;

    let mut doc = Document::default();
    doc.meta.title = Some("Test Title".to_owned());
    doc.meta.authors = vec!["Author One".to_owned()];
    doc.meta.language = Some("en".to_owned());

    doc.body.push(Block::Heading {
        level: 1,
        content: vec![Inline::Text {
            text: "Heading Level 1".to_owned(),
        }],
    });
    doc.body.push(Block::Paragraph {
        content: vec![
            Inline::Text {
                text: "Paragraph with ".to_owned(),
            },
            Inline::Strong {
                content: vec![Inline::Text {
                    text: "bold".to_owned(),
                }],
            },
            Inline::Text {
                text: " and ".to_owned(),
            },
            Inline::Emph {
                content: vec![Inline::Text {
                    text: "italic".to_owned(),
                }],
            },
            Inline::Text {
                text: " and ".to_owned(),
            },
            Inline::Code {
                text: "let x = 42;".to_owned(),
            },
            Inline::Text {
                text: " and ".to_owned(),
            },
            Inline::Link {
                url: "https://example.com".to_owned(),
                title: None,
                content: vec![Inline::Text {
                    text: "a link".to_owned(),
                }],
            },
            Inline::FootnoteRef { id: "1".to_owned() },
        ],
    });

    doc.body.push(Block::Code {
        lang: Some("rust".to_owned()),
        text: "fn main() {\n    println!(\"hello\");\n}\n".to_owned(),
    });

    doc.body.push(Block::Table {
        caption: Some(vec![Inline::Text {
            text: "A sample table".to_owned(),
        }]),
        columns: vec![
            ColumnSpec {
                align: Alignment::Left,
            },
            ColumnSpec {
                align: Alignment::Right,
            },
        ],
        head: vec![vec![
            TableCell {
                rowspan: 1,
                colspan: 1,
                header: Some(true),
                blocks: vec![Block::Paragraph {
                    content: vec![Inline::Text {
                        text: "Col 1".to_owned(),
                    }],
                }],
            },
            TableCell {
                rowspan: 1,
                colspan: 1,
                header: Some(true),
                blocks: vec![Block::Paragraph {
                    content: vec![Inline::Text {
                        text: "Col 2".to_owned(),
                    }],
                }],
            },
        ]],
        body: vec![vec![
            TableCell {
                rowspan: 1,
                colspan: 1,
                header: Some(false),
                blocks: vec![Block::Paragraph {
                    content: vec![Inline::Text {
                        text: "Val 1".to_owned(),
                    }],
                }],
            },
            TableCell {
                rowspan: 1,
                colspan: 1,
                header: Some(false),
                blocks: vec![Block::Paragraph {
                    content: vec![Inline::Text {
                        text: "Val 2".to_owned(),
                    }],
                }],
            },
        ]],
        footnotes: Vec::new(),
    });

    doc.body.push(Block::List {
        ordered: false,
        start: None,
        tight: true,
        items: vec![ListItem {
            checked: None,
            blocks: vec![Block::Paragraph {
                content: vec![Inline::Text {
                    text: "Item 1".to_owned(),
                }],
            }],
        }],
    });

    doc.body.push(Block::Footnote {
        id: "1".to_owned(),
        blocks: vec![Block::Paragraph {
            content: vec![Inline::Text {
                text: "Footnote content".to_owned(),
            }],
        }],
    });

    let written = writer::html::write(&doc);
    assert!(written.content.contains("<!DOCTYPE html>"));
    assert!(written.content.contains("<title>Test Title</title>"));
    assert!(written.content.contains("<h1>Heading Level 1</h1>"));
    assert!(written.content.contains("<table"));
    assert!(written.content.contains("role=\"doc-endnotes\""));

    let read_back =
        html::read(written.content.as_bytes(), &Limits::local()).expect("read back html");
    assert_eq!(read_back.document.meta.title, doc.meta.title);
    // Semantic verification: check heading, paragraph, table, code block exist
    assert!(
        read_back
            .document
            .body
            .iter()
            .any(|b| matches!(b, Block::Heading { level: 1, .. }))
    );
    assert!(
        read_back
            .document
            .body
            .iter()
            .any(|b| matches!(b, Block::Paragraph { .. }))
    );
    assert!(
        read_back
            .document
            .body
            .iter()
            .any(|b| matches!(b, Block::Code { .. }))
    );
    assert!(
        read_back
            .document
            .body
            .iter()
            .any(|b| matches!(b, Block::Table { .. }))
    );
    assert!(
        read_back
            .document
            .body
            .iter()
            .any(|b| matches!(b, Block::List { .. }))
    );
    assert!(
        read_back
            .document
            .body
            .iter()
            .any(|b| matches!(b, Block::Footnote { id, .. } if id == "1"))
    );
}

#[test]
fn every_markdown_fixture_preserves_structure_through_html_writer_and_reader() {
    use ariad_core::reader::html;

    let mut fixture_count = 0;
    insta::glob!("../../../fixtures", "md/*.md", |path| {
        fixture_count += 1;
        let original_text = fs::read_to_string(path).expect("read Markdown fixture");
        let read1 = markdown::read(&original_text, &Limits::local())
            .unwrap_or_else(|err| panic!("markdown read of {}: {err}", path.display()));

        let written = writer::html::write(&read1.document);
        let read2 = html::read(written.content.as_bytes(), &Limits::local())
            .unwrap_or_else(|err| panic!("html read back of {}: {err}", path.display()));

        // 1. Heading structure preservation (levels)
        let h1: Vec<u8> = read1
            .document
            .body
            .iter()
            .filter_map(|b| match b {
                Block::Heading { level, .. } => Some(*level),
                _ => None,
            })
            .collect();
        let h2: Vec<u8> = read2
            .document
            .body
            .iter()
            .filter_map(|b| match b {
                Block::Heading { level, .. } => Some(*level),
                _ => None,
            })
            .collect();
        assert_eq!(
            h1,
            h2,
            "heading levels mismatch for fixture {}",
            path.display()
        );

        // 2. Table structure preservation (dimensions)
        let t1: Vec<(usize, usize)> = read1
            .document
            .body
            .iter()
            .filter_map(|b| match b {
                Block::Table {
                    columns,
                    head,
                    body,
                    ..
                } => Some((columns.len(), head.len() + body.len())),
                _ => None,
            })
            .collect();
        let t2: Vec<(usize, usize)> = read2
            .document
            .body
            .iter()
            .filter_map(|b| match b {
                Block::Table {
                    columns,
                    head,
                    body,
                    ..
                } => Some((columns.len(), head.len() + body.len())),
                _ => None,
            })
            .collect();
        assert_eq!(
            t1,
            t2,
            "table dimensions mismatch for fixture {}",
            path.display()
        );

        // 3. Code blocks count preservation
        let c1 = read1
            .document
            .body
            .iter()
            .filter(|b| matches!(b, Block::Code { .. }))
            .count();
        let c2 = read2
            .document
            .body
            .iter()
            .filter(|b| matches!(b, Block::Code { .. }))
            .count();
        assert_eq!(
            c1,
            c2,
            "code blocks count mismatch for fixture {}",
            path.display()
        );

        // 4. List kinds preservation
        let l1: Vec<bool> = read1
            .document
            .body
            .iter()
            .filter_map(|b| match b {
                Block::List { ordered, .. } => Some(*ordered),
                _ => None,
            })
            .collect();
        let l2: Vec<bool> = read2
            .document
            .body
            .iter()
            .filter_map(|b| match b {
                Block::List { ordered, .. } => Some(*ordered),
                _ => None,
            })
            .collect();
        assert_eq!(
            l1,
            l2,
            "list ordered status mismatch for fixture {}",
            path.display()
        );
    });

    assert!(
        fixture_count >= 20,
        "expected at least 20 fixtures, found {fixture_count}"
    );
}

#[test]
fn html_writer_hostile_input_sanitization() {
    use ariad_core::ir::RawFormat;

    let mut doc = Document::default();
    doc.body.push(Block::Raw {
        format: RawFormat::Html,
        text: "<script>alert('xss')</script><p>Clean paragraph</p><img src=x onerror=\"alert(1)\">"
            .to_owned(),
    });
    doc.body.push(Block::Paragraph {
        content: vec![
            Inline::Raw {
                format: RawFormat::Html,
                text: "<style>body{background:red}</style><span>safe text</span>".to_owned(),
            },
            Inline::Link {
                url: "javascript:evil()".to_owned(),
                title: None,
                content: vec![Inline::Text {
                    text: "malicious link".to_owned(),
                }],
            },
        ],
    });

    let written = writer::html::write(&doc);
    assert!(!written.content.contains("<script"));
    assert!(!written.content.contains("alert"));
    assert!(!written.content.contains("onerror"));
    assert!(!written.content.contains("background:red"));
    assert!(!written.content.contains("javascript:"));
    assert!(written.content.contains("Clean paragraph"));
    assert!(written.content.contains("safe text"));
    assert!(written.content.contains("malicious link"));

    // Verify RawDropped warning was emitted
    assert!(
        written
            .warnings
            .iter()
            .any(|w| w.code == WarningCode::RawDropped)
    );
    assert!(
        written
            .warnings
            .iter()
            .any(|w| w.code == WarningCode::LinkDropped)
    );
}

#[test]
fn markdown_writer_safe_link_and_image_destinations() {
    let mut doc = Document::default();
    let hostile_url = "https://x.example/)<img src=x onerror=alert(1)>\\path";
    let hostile_title = "Quote \" and backslash \\ test";

    doc.body.push(Block::Paragraph {
        content: vec![
            Inline::Link {
                url: hostile_url.to_owned(),
                title: Some(hostile_title.to_owned()),
                content: vec![Inline::Text {
                    text: "Hostile Link".to_owned(),
                }],
            },
            Inline::Text {
                text: " and ".to_owned(),
            },
            Inline::Image {
                target: AssetRef::Url {
                    href: hostile_url.to_owned(),
                },
                alt: "Hostile Image".to_owned(),
                title: Some(hostile_title.to_owned()),
            },
        ],
    });

    let written = writer::markdown::write(&doc);
    assert!(written.content.contains("\\<img src=x onerror=alert(1)\\>"));
    assert!(!written.content.contains("\n<img"));
    assert!(!written.content.contains(" <img"));
    assert!(written.content.contains("\\\""));

    let read_back = markdown::read(&written.content, &Limits::local()).expect("read back");
    match &read_back.document.body[0] {
        Block::Paragraph { content } => {
            match &content[0] {
                Inline::Link {
                    url,
                    title,
                    content,
                } => {
                    assert_eq!(url, hostile_url);
                    assert_eq!(title.as_deref(), Some(hostile_title));
                    assert_eq!(
                        content,
                        &[Inline::Text {
                            text: "Hostile Link".to_owned()
                        }]
                    );
                }
                other => panic!("expected Link, got {other:?}"),
            }
            match &content[2] {
                Inline::Image { target, alt, title } => {
                    match target {
                        AssetRef::Url { href } => assert_eq!(href, hostile_url),
                        other => panic!("expected Url target, got {other:?}"),
                    }
                    assert_eq!(alt, "Hostile Image");
                    assert_eq!(title.as_deref(), Some(hostile_title));
                }
                other => panic!("expected Image, got {other:?}"),
            }
        }
        other => panic!("expected Paragraph, got {other:?}"),
    }
}

#[test]
fn markdown_writer_line_start_and_frontmatter_escaping() {
    let mut doc = Document::default();
    doc.body.push(Block::Paragraph {
        content: vec![Inline::Text {
            text: "---".to_owned(),
        }],
    });
    doc.body.push(Block::Paragraph {
        content: vec![Inline::Text {
            text: "title: Pwned\n---".to_owned(),
        }],
    });
    doc.body.push(Block::Paragraph {
        content: vec![
            Inline::Text {
                text: "=== Not setext".to_owned(),
            },
            Inline::SoftBreak {},
            Inline::Text {
                text: "+++ Not setext".to_owned(),
            },
            Inline::SoftBreak {},
            Inline::Text {
                text: "--- Not thematic break".to_owned(),
            },
        ],
    });

    let written = writer::markdown::write(&doc);
    assert!(written.content.starts_with("\\---\n\n"));

    let read_back = markdown::read(&written.content, &Limits::local()).expect("read back");
    assert!(read_back.document.meta.title.is_none());
    assert_eq!(read_back.document.body.len(), 3);
    match &read_back.document.body[0] {
        Block::Paragraph { content } => {
            assert_eq!(
                content,
                &[Inline::Text {
                    text: "---".to_owned()
                }]
            );
        }
        other => panic!("expected Paragraph, got {other:?}"),
    }
}

#[test]
fn markdown_writer_list_indentation_and_loose_items() {
    let mut doc = Document::default();
    doc.body.push(Block::List {
        ordered: true,
        start: Some(10),
        tight: false,
        items: vec![
            ListItem {
                checked: None,
                blocks: vec![
                    Block::Paragraph {
                        content: vec![Inline::Text {
                            text: "First para in item 10".to_owned(),
                        }],
                    },
                    Block::Paragraph {
                        content: vec![Inline::Text {
                            text: "Second para in item 10".to_owned(),
                        }],
                    },
                    Block::Code {
                        lang: Some("rust".to_owned()),
                        text: "let x = 10;\n".to_owned(),
                    },
                ],
            },
            ListItem {
                checked: None,
                blocks: vec![Block::Paragraph {
                    content: vec![Inline::Text {
                        text: "Item 11".to_owned(),
                    }],
                }],
            },
        ],
    });

    let written = writer::markdown::write(&doc);
    assert!(written.content.contains("10. First para in item 10\n\n    Second para in item 10\n\n    ```rust\n    let x = 10;\n    ```\n\n11. Item 11\n"));

    let read_back = markdown::read(&written.content, &Limits::local()).expect("read back");
    assert_eq!(
        normalize_document(&read_back.document),
        normalize_document(&doc)
    );
}

#[test]
fn markdown_writer_m1_escaping_edge_cases() {
    let mut doc = Document::default();
    doc.meta.title = Some("- item like title".to_owned());

    doc.body.push(Block::Heading {
        level: 2,
        content: vec![Inline::Text {
            text: "Heading with trailing hashes ###".to_owned(),
        }],
    });
    doc.body.push(Block::Heading {
        level: 3,
        content: vec![
            Inline::Text {
                text: "Heading with softbreak".to_owned(),
            },
            Inline::SoftBreak {},
            Inline::Text {
                text: "and newline".to_owned(),
            },
        ],
    });

    doc.body.push(Block::Table {
        caption: None,
        columns: vec![ColumnSpec {
            align: Alignment::Default,
        }],
        head: vec![vec![TableCell {
            rowspan: 1,
            colspan: 1,
            header: Some(true),
            blocks: vec![Block::Paragraph {
                content: vec![Inline::Text {
                    text: "Header".to_owned(),
                }],
            }],
        }]],
        body: vec![vec![TableCell {
            rowspan: 1,
            colspan: 1,
            header: Some(false),
            blocks: vec![Block::Paragraph {
                content: vec![Inline::Code {
                    text: "a | b".to_owned(),
                }],
            }],
        }]],
        footnotes: Vec::new(),
    });

    doc.body.push(Block::Paragraph {
        content: vec![
            Inline::Text {
                text: "    four leading spaces".to_owned(),
            },
            Inline::SoftBreak {},
            Inline::Text {
                text: "Text with $math$ symbols".to_owned(),
            },
        ],
    });

    doc.body.push(Block::Footnote {
        id: "fn [bracket]".to_owned(),
        blocks: vec![Block::Paragraph {
            content: vec![Inline::Text {
                text: "Footnote content".to_owned(),
            }],
        }],
    });

    let written = writer::markdown::write(&doc);
    assert!(written.content.contains("title: \"- item like title\""));
    assert!(
        written
            .content
            .contains("## Heading with trailing hashes \\#\\#\\#\n\n")
    );
    assert!(
        written
            .content
            .contains("### Heading with softbreak and newline\n\n")
    );
    assert!(written.content.contains("`a \\| b`"));
    assert!(written.content.contains("&#32;   four leading spaces"));
    assert!(written.content.contains("\\$math\\$"));
    assert!(written.content.contains("[^fn%20%5Bbracket%5D]:"));

    let read_back = markdown::read(&written.content, &Limits::local()).expect("read back");
    assert_eq!(read_back.document.meta.title, doc.meta.title);
}

#[test]
fn shared_sanitizer_removals_and_safe_markup() {
    let safe_html = "<p><kbd>Ctrl</kbd>+<kbd>C</kbd> and <span lang=\"vi\">Xin chào</span> and <a href=\"#section-1\">jump</a></p>";
    let cleaned_safe = writer::sanitize::sanitize_raw_html(safe_html);
    assert!(cleaned_safe.contains("<kbd>Ctrl</kbd>"));
    assert!(cleaned_safe.contains("lang=\"vi\""));
    assert!(cleaned_safe.contains("href=\"#section-1\""));

    let unsafe_html = "<script>alert(1)</script><div onclick=\"evil()\">test</div><a href=\"relative/page.html\">link</a>";
    let cleaned_unsafe = writer::sanitize::sanitize_raw_html(unsafe_html);
    assert!(!cleaned_unsafe.contains("<script"));
    assert!(!cleaned_unsafe.contains("onclick"));
    assert!(!cleaned_unsafe.contains("relative/page.html"));
    assert!(cleaned_unsafe.contains("test"));
}

#[test]
fn html_writer_coherent_raw_html_runs_and_warning_conditions() {
    use ariad_core::ir::RawFormat;

    let mut doc_safe = Document::default();
    doc_safe.body.push(Block::Paragraph {
        content: vec![
            Inline::Raw {
                format: RawFormat::Html,
                text: "<kbd>".to_owned(),
            },
            Inline::Text {
                text: "Ctrl".to_owned(),
            },
            Inline::Raw {
                format: RawFormat::Html,
                text: "</kbd>".to_owned(),
            },
            Inline::Text {
                text: "+".to_owned(),
            },
            Inline::Raw {
                format: RawFormat::Html,
                text: "<kbd>".to_owned(),
            },
            Inline::Text {
                text: "C".to_owned(),
            },
            Inline::Raw {
                format: RawFormat::Html,
                text: "</kbd>".to_owned(),
            },
        ],
    });

    let written_safe = writer::html::write(&doc_safe);
    assert!(written_safe.content.contains("<p>Ctrl+C</p>"));
    assert!(!written_safe.content.contains("<kbd>"));
    assert_eq!(
        written_safe
            .warnings
            .iter()
            .filter(|w| w.code == WarningCode::RawDropped)
            .count(),
        1
    );

    let mut doc_unsafe = Document::default();
    doc_unsafe.body.push(Block::Paragraph {
        content: vec![
            Inline::Raw {
                format: RawFormat::Html,
                text: "<span onclick=\"alert(1)\">".to_owned(),
            },
            Inline::Text {
                text: "clickable".to_owned(),
            },
            Inline::Raw {
                format: RawFormat::Html,
                text: "</span>".to_owned(),
            },
        ],
    });

    let written_unsafe = writer::html::write(&doc_unsafe);
    assert!(written_unsafe.content.contains("<p>clickable</p>"));
    assert!(!written_unsafe.content.contains("onclick"));
    assert_eq!(
        written_unsafe
            .warnings
            .iter()
            .filter(|w| w.code == WarningCode::RawDropped)
            .count(),
        1
    );
}

#[test]
fn html_writer_math_renders_visible_presentation_before_annotation() {
    let mut doc = Document::default();
    doc.body.push(Block::Paragraph {
        content: vec![Inline::Math {
            tex: "E = mc^2".to_owned(),
            display: false,
        }],
    });
    doc.body.push(Block::Math {
        tex: r"\int_0^1 x dx".to_owned(),
        display: true,
    });

    let written = writer::html::write(&doc);
    assert!(
        written
            .content
            .contains("<math><semantics><mtext>E = mc^2</mtext><annotation encoding=\"application/x-tex\">E = mc^2</annotation></semantics></math>")
    );
    assert!(
        written
            .content
            .contains("<math display=\"block\"><semantics><mtext>\\int_0^1 x dx</mtext><annotation encoding=\"application/x-tex\">\\int_0^1 x dx</annotation></semantics></math>")
    );
}

#[test]
fn html_writer_prose_only_nfc_normalization() {
    let nfd_prose = "Ho\u{0323}c"; // Decomposed 'Học'
    let nfd_code = "let x\u{0323} = 1;";

    let mut doc = Document::default();
    doc.body.push(Block::Paragraph {
        content: vec![
            Inline::Text {
                text: nfd_prose.to_owned(),
            },
            Inline::Code {
                text: nfd_code.to_owned(),
            },
        ],
    });
    doc.body.push(Block::Code {
        lang: None,
        text: nfd_code.to_owned(),
    });

    let written = writer::html::write(&doc);
    // Prose paragraph text should be NFC normalized to 'Học'
    assert!(written.content.contains("Học"));
    // Code block and inline code must NOT be NFC normalized
    assert!(written.content.contains("let x\u{0323} = 1;"));
}

#[test]
fn html_writer_title_fallback_handling() {
    let mut doc_empty = Document::default();
    doc_empty.meta.title = Some("   \t\n  ".to_owned());
    let written_empty = writer::html::write(&doc_empty);
    assert!(!written_empty.content.contains("<title>"));

    let mut doc_none = Document::default();
    doc_none.meta.title = None;
    let written_none = writer::html::write(&doc_none);
    assert!(!written_none.content.contains("<title>"));

    let mut doc_valid = Document::default();
    doc_valid.meta.title = Some("Document Title".to_owned());
    let written_valid = writer::html::write(&doc_valid);
    assert!(
        written_valid
            .content
            .contains("<title>Document Title</title>")
    );
}

#[test]
fn html_writer_does_not_resanitize_own_markup_and_handles_unclosed_runs() {
    let mut doc = Document::default();
    doc.body.push(Block::Paragraph {
        content: vec![
            Inline::Text {
                text: "Press ".to_owned(),
            },
            Inline::Raw {
                format: RawFormat::Html,
                text: "<span>".to_owned(),
            },
            Inline::Text {
                text: "see ".to_owned(),
            },
            Inline::Image {
                target: AssetRef::Url {
                    href: "real.png".to_owned(),
                },
                alt: "p".to_owned(),
                title: None,
            },
            Inline::Text {
                text: " and ".to_owned(),
            },
            Inline::Math {
                tex: "x^2".to_owned(),
                display: false,
            },
            Inline::Text {
                text: " ok".to_owned(),
            },
            Inline::Raw {
                format: RawFormat::Html,
                text: "</span>".to_owned(),
            },
            Inline::Text {
                text: ".".to_owned(),
            },
        ],
    });

    let written = writer::html::write(&doc);
    assert!(written.content.contains("<img src=\"real.png\" alt=\"p\">"));
    assert!(
        written
            .content
            .contains("<math><semantics><mtext>x^2</mtext>")
    );
    assert!(
        written
            .content
            .contains("Press see <img src=\"real.png\" alt=\"p\"> and <math>")
    );
    assert!(written.content.contains("ok."));
    assert!(!written.content.contains("<span>"));

    let mut doc_unclosed = Document::default();
    doc_unclosed.body.push(Block::Paragraph {
        content: vec![
            Inline::Raw {
                format: RawFormat::Html,
                text: "<a href=\"https://evil.example/login\">".to_owned(),
            },
            Inline::Text {
                text: "see ".to_owned(),
            },
            Inline::Math {
                tex: "y^2".to_owned(),
                display: false,
            },
        ],
    });
    doc_unclosed.body.push(Block::Paragraph {
        content: vec![Inline::Text {
            text: "Rest of document".to_owned(),
        }],
    });

    let written_unclosed = writer::html::write(&doc_unclosed);
    // Prevents unclosed inline raw HTML tags from leaking and hijacking later paragraphs
    assert!(!written_unclosed.content.contains("<a href="));
    assert!(written_unclosed.content.contains("<p>Rest of document</p>"));
    assert!(
        written_unclosed
            .content
            .contains("<math><semantics><mtext>y^2</mtext>")
    );
}

#[test]
fn markdown_writer_char_boundary_safe_slicing_and_multibyte_whitespace() {
    let nbsp = "\u{00A0}";
    let ideographic = "\u{3000}";
    let zwj_family = "👨\u{200D}👩\u{200D}👧\u{200D}👦";
    let combining_vietnamese = "nghi\u{0309}";

    let doc = Document {
        body: vec![Block::Paragraph {
            content: vec![
                Inline::Text {
                    text: "Giờ ".to_owned(),
                },
                Inline::Emph {
                    content: vec![Inline::Text {
                        text: format!("{nbsp}nghi"),
                    }],
                },
                Inline::Text {
                    text: " ngơi ".to_owned(),
                },
                Inline::Strong {
                    content: vec![Inline::Text {
                        text: format!("một{ideographic}"),
                    }],
                },
                Inline::Text {
                    text: "x ".to_owned(),
                },
                Inline::Strikeout {
                    content: vec![Inline::Text {
                        text: format!("{zwj_family} {combining_vietnamese}"),
                    }],
                },
                Inline::Math {
                    tex: format!("{nbsp}x + y{ideographic}"),
                    display: false,
                },
            ],
        }],
        ..Default::default()
    };

    let written = writer::markdown::write(&doc);
    assert!(!written.content.is_empty());

    let read_back = markdown::read(&written.content, &Limits::local()).expect("must parse");
    assert_eq!(read_back.document.body.len(), 1);

    // Reviewer repro CLI snippet: <p>Giờ <em>&#160;nghi</em> ngơi <strong>một&#12288;</strong>x</p>
    let html_input = b"<p>Gi\xc3\xb2 <em>&#160;nghi</em> ng\xc6\xa1i <strong>m\xe1\xbb\x99t&#12288;</strong>x</p>";
    let html_ir = html::read(html_input, &Limits::local()).expect("parse html");
    let md_out = writer::markdown::write(&html_ir.document);
    let md_roundtrip = markdown::read(&md_out.content, &Limits::local()).expect("parse md");
    assert_eq!(md_roundtrip.document.body.len(), 1);
}

#[test]
fn seeded_table_driven_whole_document_generator_roundtrip() {
    let test_documents: Vec<(&'static str, Document)> = vec![
        // 1. Text hostile strings
        ("text_html_script", Document {
            body: vec![Block::Paragraph {
                content: vec![Inline::Text { text: "<script>alert('xss')</script>".to_owned() }],
            }],
            ..Default::default()
        }),
        ("text_js_link", Document {
            body: vec![Block::Paragraph {
                content: vec![Inline::Text { text: "[click](javascript:alert(1))".to_owned() }],
            }],
            ..Default::default()
        }),
        ("text_heading_markers", Document {
            body: vec![Block::Paragraph {
                content: vec![Inline::Text { text: "### Injected Heading".to_owned() }],
            }],
            ..Default::default()
        }),
        ("text_list_marker", Document {
            body: vec![Block::Paragraph {
                content: vec![Inline::Text { text: "- [x] task item".to_owned() }],
            }],
            ..Default::default()
        }),
        ("text_table_syntax", Document {
            body: vec![Block::Paragraph {
                content: vec![Inline::Text { text: "| a | b |\n|---|---|\n| 1 | 2 |".to_owned() }],
            }],
            ..Default::default()
        }),
        ("text_thematic_break", Document {
            body: vec![Block::Paragraph {
                content: vec![Inline::Text { text: "---".to_owned() }],
            }],
            ..Default::default()
        }),

        // 2. Inline Emph, Strong, Strikeout with hostile text and N1 multibyte whitespace
        ("inlines_formatting_multibyte_n1", Document {
            body: vec![Block::Paragraph {
                content: vec![
                    Inline::Emph {
                        content: vec![Inline::Text { text: "\u{00A0}[a](javascript:alert(1))".to_owned() }],
                    },
                    Inline::Strong {
                        content: vec![Inline::Text { text: "\u{3000}<script>bad()</script>".to_owned() }],
                    },
                    Inline::Strikeout {
                        content: vec![Inline::Text { text: "# Heading in strike".to_owned() }],
                    },
                ],
            }],
            ..Default::default()
        }),

        // 3. Code spans with newlines and backticks (H2)
        ("code_spans_h2_newlines_and_ticks", Document {
            body: vec![Block::Paragraph {
                content: vec![
                    Inline::Code { text: "a\n# Injected\n[x](javascript:alert(1))".to_owned() },
                    Inline::Code { text: "back`tick and ``` triple".to_owned() },
                    Inline::Code { text: "`starts and ends with tick`".to_owned() },
                    Inline::Code { text: " space padded ".to_owned() },
                ],
            }],
            ..Default::default()
        }),

        // 4. Math with $, $$, newlines and backticks (H1)
        ("math_inline_h1_breakouts", Document {
            body: vec![Block::Paragraph {
                content: vec![
                    Inline::Math {
                        tex: "x$ [a](javascript:alert(1)) <img src=x onerror=alert(2)> $y".to_owned(),
                        display: false,
                    },
                    Inline::Math {
                        tex: "multi\nline\r\nmath".to_owned(),
                        display: false,
                    },
                    Inline::Math {
                        tex: "with `backtick` inside".to_owned(),
                        display: false,
                    },
                ],
            }],
            ..Default::default()
        }),
        ("math_block_h1_breakouts", Document {
            body: vec![
                Block::Math {
                    tex: "E = mc^2\n$$\n# Injected Heading\n$$\nx = y".to_owned(),
                    display: true,
                },
            ],
            ..Default::default()
        }),

        // 5. Raw TeX inline and block (H3)
        ("raw_tex_h3_breakouts", Document {
            body: vec![
                Block::Paragraph {
                    content: vec![
                        Inline::Raw {
                            format: RawFormat::Tex,
                            text: "\\x{} [e](javascript:alert(6)) <img src=x onerror=alert(7)>".to_owned(),
                        },
                    ],
                },
                Block::Raw {
                    format: RawFormat::Tex,
                    text: "\\begin{equation}\n# Heading\n<script>alert(8)</script>\n\\end{equation}".to_owned(),
                },
            ],
            ..Default::default()
        }),

        // 6. Raw HTML inlines and blocks (Fallback + N2)
        ("raw_html_fallback_and_n2", Document {
            body: vec![
                Block::Paragraph {
                    content: vec![
                        Inline::Raw {
                            format: RawFormat::Html,
                            text: "<button>[b](javascript:alert(1))</button>".to_owned(),
                        },
                        Inline::Raw {
                            format: RawFormat::Html,
                            text: "<video src=\"https://evil.com/video.mp4\"></video>".to_owned(),
                        },
                        Inline::Raw {
                            format: RawFormat::Html,
                            text: "<span>hello</span>".to_owned(),
                        },
                    ],
                },
                Block::Raw {
                    format: RawFormat::Html,
                    text: "<div class=\"dangerous\"><script>alert(9)</script>Safe Text</div>".to_owned(),
                },
            ],
            ..Default::default()
        }),

        // 7. Literal placeholders and attributes
        ("placeholder_attribute_preservation", Document {
            body: vec![Block::Paragraph {
                content: vec![
                    Inline::Text { text: "Hi ".to_owned() },
                    Inline::Raw {
                        format: RawFormat::Html,
                        text: "<span title=\"XARIADPH0X\">".to_owned(),
                    },
                    Inline::Link {
                        url: "https://x.example/ onmouseover=alert(1) b".to_owned(),
                        title: None,
                        content: vec![Inline::Text { text: "a".to_owned() }],
                    },
                    Inline::Raw {
                        format: RawFormat::Html,
                        text: "</span>".to_owned(),
                    },
                    Inline::Text { text: " and literal XARIADPH1X in text".to_owned() },
                ],
            }],
            ..Default::default()
        }),

        // 8. Whole document with Metadata, Headings, Lists, Tables, Quotes, Code
        ("whole_document_complex", Document {
            meta: ariad_core::ir::Metadata {
                title: Some("Complex Document & <script>alert(1)</script>".to_owned()),
                authors: vec!["Author <One>".to_owned(), "Author 2".to_owned()],
                date: Some("2026-10-07".to_owned()),
                keywords: vec!["test".to_owned(), "security".to_owned()],
                ..Default::default()
            },
            body: vec![
                Block::Heading {
                    level: 2,
                    content: vec![Inline::Text { text: "Heading with # and [link](javascript:alert(1))".to_owned() }],
                },
                Block::Quote {
                    blocks: vec![Block::Paragraph {
                        content: vec![Inline::Text { text: "Quoted hostile: <img src=x onerror=alert(1)>".to_owned() }],
                    }],
                },
                Block::List {
                    ordered: true,
                    start: Some(1),
                    tight: true,
                    items: vec![
                        ListItem {
                            checked: Some(true),
                            blocks: vec![Block::Paragraph {
                                content: vec![Inline::Text { text: "Task 1".to_owned() }],
                            }],
                        },
                        ListItem {
                            checked: None,
                            blocks: vec![Block::Paragraph {
                                content: vec![Inline::Text { text: "Normal 2".to_owned() }],
                            }],
                        },
                    ],
                },
                Block::Table {
                    caption: Some(vec![Inline::Text { text: "Table Caption | with pipe".to_owned() }]),
                    columns: vec![
                        ColumnSpec { align: Alignment::Left },
                        ColumnSpec { align: Alignment::Right },
                    ],
                    head: vec![vec![
                        TableCell {
                            colspan: 1,
                            rowspan: 1,
                            header: Some(true),
                            blocks: vec![Block::Paragraph {
                                content: vec![Inline::Text { text: "Col | 1".to_owned() }],
                            }],
                        },
                        TableCell {
                            colspan: 1,
                            rowspan: 1,
                            header: Some(true),
                            blocks: vec![Block::Paragraph {
                                content: vec![Inline::Text { text: "Col | 2".to_owned() }],
                            }],
                        },
                    ]],
                    body: vec![vec![
                        TableCell {
                            colspan: 1,
                            rowspan: 1,
                            header: Some(false),
                            blocks: vec![Block::Paragraph {
                                content: vec![Inline::Code { text: "cell|code".to_owned() }],
                            }],
                        },
                        TableCell {
                            colspan: 1,
                            rowspan: 1,
                            header: Some(false),
                            blocks: vec![Block::Paragraph {
                                content: vec![Inline::Text { text: "data [x](javascript:alert(1))".to_owned() }],
                            }],
                        },
                    ]],
                    footnotes: Vec::new(),
                },
            ],
            ..Default::default()
        }),
    ];

    for (name, doc) in test_documents {
        let written = writer::markdown::write(&doc);
        assert!(
            !written.content.is_empty(),
            "output for {name} must not be empty"
        );

        if written.content.contains("<script>") {
            assert!(
                written.content.contains("\\<script>")
                    || written.content.contains("```")
                    || written.content.contains('`'),
                "{name}: written markdown contains unescaped <script> tag:\n{}",
                written.content
            );
        }
        if written.content.contains("onerror=") {
            assert!(
                written.content.contains('`') || written.content.contains("\\onerror="),
                "{name}: written markdown contains raw unescaped onerror=:\n{}",
                written.content
            );
        }

        let read_back = markdown::read(&written.content, &Limits::local())
            .unwrap_or_else(|e| panic!("{name}: reader must not fail on written markdown: {e:?}"));

        // Verify read_back does not contain dangerous links or scripts
        for block in &read_back.document.body {
            if let Block::Paragraph { content } = block {
                for inline in content {
                    if let Inline::Link { url, .. } = inline {
                        assert!(
                            !url.to_ascii_lowercase().starts_with("javascript:"),
                            "{name}: live javascript: link in parsed IR: {url}"
                        );
                    }
                    if let Inline::Raw {
                        format: RawFormat::Html,
                        text,
                    } = inline
                    {
                        assert!(
                            !text.contains("<script>") && !text.contains("onerror="),
                            "{name}: dangerous raw HTML in parsed IR: {text}"
                        );
                    }
                }
            }
        }

        // Specific test invariants:
        if name == "code_spans_h2_newlines_and_ticks"
            || name == "math_block_h1_breakouts"
            || name == "raw_tex_h3_breakouts"
        {
            assert!(
                !read_back
                    .document
                    .body
                    .iter()
                    .any(|b| matches!(b, Block::Heading { .. })),
                "{name}: injected heading found in parsed IR"
            );
        }

        // Roundtrip md -> IR -> md
        let re_written = writer::markdown::write(&read_back.document);
        let _re_read = markdown::read(&re_written.content, &Limits::local()).unwrap_or_else(|e| {
            panic!("{name}: re-reading rewritten markdown must not fail: {e:?}")
        });
    }
}

#[test]
fn html_writer_literal_placeholders_preserved_and_video_stripped() {
    let mut doc = Document::default();
    doc.body.push(Block::Raw {
        format: RawFormat::Html,
        text: "<div title=\"XARIADPH0X\"><video src=\"https://example.com/video.mp4\" controls></video><p>Literal XARIADPH1X in text and <em>emphasized</em></p></div>".to_owned(),
    });

    let written = writer::html::write(&doc);
    assert!(
        written
            .content
            .contains("Literal XARIADPH1X in text and <em>emphasized</em>"),
        "literal XARIADPH1X must be preserved: {}",
        written.content
    );
    assert!(
        written.content.contains("<div title=\"XARIADPH0X\">"),
        "literal XARIADPH0X in title attribute must be preserved: {}",
        written.content
    );
    assert!(
        !written.content.contains("<video"),
        "<video> tag must be stripped: {}",
        written.content
    );
    assert!(
        written
            .warnings
            .iter()
            .any(|w| w.code == WarningCode::RawDropped),
        "RawDropped warning must be emitted for stripped video"
    );
}

#[test]
fn test_delimiter_calculation_helper_runs_and_padding() {
    // 1..5 ticks
    for count in 1..=5 {
        let ticks = "`".repeat(count);
        let (span_delim, span_content) =
            writer::markdown::format_delimiter_and_content(&ticks, false);
        assert_eq!(span_delim.len(), count + 1);
        assert_eq!(span_content, format!(" {ticks} "));

        let (fence_delim, fence_content) =
            writer::markdown::format_delimiter_and_content(&ticks, true);
        assert!(fence_delim.len() >= 3);
        assert_eq!(fence_delim.len(), std::cmp::max(3, count + 1));
        assert_eq!(fence_content, ticks);
    }

    // 1..5 tildes in block fence
    for count in 1..=5 {
        let tildes = "~".repeat(count);
        let (fence_delim, fence_content) =
            writer::markdown::format_delimiter_and_content(&tildes, true);
        assert!(fence_delim.len() >= 3);
        assert_eq!(fence_delim.len(), std::cmp::max(3, count + 1));
        assert_eq!(fence_content, tildes);
    }

    // Empty content
    let (empty_span_delim, empty_span_content) =
        writer::markdown::format_delimiter_and_content("", false);
    assert_eq!(empty_span_delim, "`");
    assert_eq!(empty_span_content, "");

    let (empty_fence_delim, empty_fence_content) =
        writer::markdown::format_delimiter_and_content("", true);
    assert_eq!(empty_fence_delim, "```");
    assert_eq!(empty_fence_content, "");

    // Leading and trailing spaces and backticks
    let cases = [
        (" code", "  code "),
        ("code ", " code  "),
        ("`code", " `code "),
        ("code`", " code` "),
        ("`code`", " `code` "),
        (" code ", "  code  "),
        ("simple", "simple"),
    ];
    for (input, expected) in cases {
        let (_, content) = writer::markdown::format_delimiter_and_content(input, false);
        assert_eq!(content, expected, "padding rule for {input:?}");
    }
}

#[test]
fn test_strip_html_tags_bare_lt_and_script_style_discard() {
    assert_eq!(
        writer::strip_html_tags("a < b and c > d"),
        "a < b and c > d"
    );
    assert_eq!(writer::strip_html_tags("1 < 2 < 3"), "1 < 2 < 3");
    assert_eq!(
        writer::strip_html_tags("<b>bold</b> and a < b"),
        "bold and a < b"
    );
    assert_eq!(
        writer::strip_html_tags("<script>alert(1)</script>hello"),
        "hello"
    );
    assert_eq!(
        writer::strip_html_tags("<style>body{color:red;}</style>world"),
        "world"
    );
    assert_eq!(
        writer::strip_html_tags("<script src=\"foo.js\">alert(1);</script>clean"),
        "clean"
    );
    assert_eq!(
        writer::strip_html_tags("<STYLE type=\"text/css\">p { margin: 0; }</STYLE>text"),
        "text"
    );
}

#[test]
fn test_lone_cr_in_containers_and_raw_tex() {
    // Invariant: lone CR in pre tags inside blockquotes must not break out
    let raw_html = b"<blockquote><pre><code>a&#13;[x](javascript:alert(1))&#13;# Injected&#13;<img src=x onerror=alert(2)></code></pre></blockquote>";
    let parsed_html = html::read(raw_html, &Limits::local()).expect("read html with CR in pre");
    let md_out = writer::markdown::write(&parsed_html.document);

    // Invariant: no lone CR or any CR in output
    assert!(
        !md_out.content.contains('\r'),
        "markdown output must not contain CR"
    );
    assert!(!md_out.content.contains('\u{2028}'));
    assert!(!md_out.content.contains('\u{0085}'));

    // When parsed back, the code must remain inside the quote/code and NOT break out into heading, link, or image
    let re_read = markdown::read(&md_out.content, &Limits::local()).expect("parse markdown");
    for block in &re_read.document.body {
        assert!(
            !matches!(block, Block::Heading { .. }),
            "CR must not cause heading injection"
        );
    }

    // 2. Raw TeX with lone CR in a list item
    let mut doc_list = Document::default();
    doc_list.body.push(Block::List {
        ordered: false,
        start: None,
        tight: true,
        items: vec![ListItem {
            checked: None,
            blocks: vec![Block::Raw {
                format: RawFormat::Tex,
                text:
                    "a\r# Injected Heading\r[x](javascript:alert(1))\r<img src=x onerror=alert(2)>"
                        .to_owned(),
            }],
        }],
    });
    let list_md = writer::markdown::write(&doc_list);
    assert!(
        !list_md.content.contains('\r'),
        "list markdown output must not contain CR"
    );
    let re_read_list =
        markdown::read(&list_md.content, &Limits::local()).expect("parse list markdown");
    for block in &re_read_list.document.body {
        assert!(
            !matches!(block, Block::Heading { .. }),
            "TeX CR in list must not cause heading injection"
        );
    }
}

fn recursive_has_headings(blocks: &[Block]) -> bool {
    blocks.iter().any(|b| match b {
        Block::Heading { .. } => true,
        Block::Quote { blocks } => recursive_has_headings(blocks),
        Block::List { items, .. } => items.iter().any(|it| recursive_has_headings(&it.blocks)),
        Block::Table { head, body, .. } => head
            .iter()
            .chain(body.iter())
            .flatten()
            .any(|cell| recursive_has_headings(&cell.blocks)),
        Block::Footnote { blocks, .. } => recursive_has_headings(blocks),
        _ => false,
    })
}

fn recursive_has_lists(blocks: &[Block]) -> bool {
    blocks.iter().any(|b| match b {
        Block::List { .. } => true,
        Block::Quote { blocks } => recursive_has_lists(blocks),
        Block::Table { head, body, .. } => head
            .iter()
            .chain(body.iter())
            .flatten()
            .any(|cell| recursive_has_lists(&cell.blocks)),
        Block::Footnote { blocks, .. } => recursive_has_lists(blocks),
        _ => false,
    })
}

fn inlines_has_links(inlines: &[Inline]) -> bool {
    inlines.iter().any(|i| match i {
        Inline::Link { .. } => true,
        Inline::Emph { content }
        | Inline::Strong { content }
        | Inline::Strikeout { content }
        | Inline::Superscript { content }
        | Inline::Subscript { content } => inlines_has_links(content),
        _ => false,
    })
}

fn recursive_has_links(blocks: &[Block]) -> bool {
    blocks.iter().any(|b| match b {
        Block::Paragraph { content } | Block::Heading { content, .. } => inlines_has_links(content),
        Block::Quote { blocks } => recursive_has_links(blocks),
        Block::List { items, .. } => items.iter().any(|it| recursive_has_links(&it.blocks)),
        Block::Table {
            head,
            body,
            caption,
            ..
        } => {
            caption.as_ref().is_some_and(|c| inlines_has_links(c))
                || head
                    .iter()
                    .chain(body.iter())
                    .flatten()
                    .any(|cell| recursive_has_links(&cell.blocks))
        }
        Block::Footnote { blocks, .. } => recursive_has_links(blocks),
        _ => false,
    })
}

struct XorShift64(u64);

impl XorShift64 {
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn next_range(&mut self, upper: usize) -> usize {
        (self.next_u64() as usize) % upper
    }

    fn choice<'a>(&mut self, slice: &'a [&'a str]) -> &'a str {
        slice[self.next_range(slice.len())]
    }
}

#[test]
fn seeded_deterministic_generator_whole_document_invariants() {
    let hostile_vocab: &[&str] = &[
        "$",
        "$$",
        "\\",
        "\r",
        "\r\n",
        "\n",
        "\u{2028}",
        "\u{0085}",
        "`",
        "```",
        "````",
        "~",
        "~~~",
        "<",
        ">",
        "|",
        "[",
        "]",
        "!",
        "\t",
        "5 apples",
        "\u{00A0}",
        "\u{3000}",
        "[x](javascript:alert(1))",
        "<script>alert(1)</script>",
        "<img src=x onerror=alert(2)>",
        "a\r[x](javascript:alert(1))\r# Injected\r<img src=x onerror=alert(2)>",
        "z\n# Injected\n\n[c](javascript:alert(1))\n<img src=x onerror=alert(2)>",
        "[d](javascript:alert(3)) \\",
        " [x](javascript:alert(8)) ",
        "<a href=\"https://evil.example/login\">",
        "a < b and c > d",
        "<!-- comment -->",
        "\\sum_{i=1}^n x_i",
        "regular text",
        "- ",
        "* ",
        "+ ",
        "1. ",
        "1) ",
        "---",
        "===",
        "#",
        "https://x",
        "www.x",
        "a@b.c",
    ];

    let mut rng = XorShift64(0xDEAD_BEEF_CAFE_BABE);

    for iter in 0..70 {
        let v1 = rng.choice(hostile_vocab).to_owned();
        let v2 = rng.choice(hostile_vocab).to_owned();
        let v3 = rng.choice(hostile_vocab).to_owned();

        let mut doc = Document::default();
        doc.meta.title = Some(format!("Title {v1}"));
        doc.meta.authors = vec![format!("Author {v2}")];
        doc.meta.keywords = vec![format!("Keyword {v3}")];

        match iter % 7 {
            0 => {
                doc.body.push(Block::Paragraph {
                    content: vec![
                        Inline::Text { text: v1.clone() },
                        Inline::Code { text: v2.clone() },
                        Inline::Math {
                            tex: v3.clone(),
                            display: false,
                        },
                    ],
                });
            }
            1 => {
                doc.body.push(Block::Math {
                    tex: v1.clone(),
                    display: true,
                });
                doc.body.push(Block::Paragraph {
                    content: vec![Inline::Text {
                        text: "Followup".to_owned(),
                    }],
                });
            }
            2 => {
                doc.body.push(Block::Code {
                    lang: Some("rust".to_owned()),
                    text: format!("{v1}\n{v2}\n{v3}"),
                });
            }
            3 => {
                doc.body.push(Block::Quote {
                    blocks: vec![
                        Block::Paragraph {
                            content: vec![Inline::Text { text: v1.clone() }],
                        },
                        Block::Code {
                            lang: None,
                            text: v2.clone(),
                        },
                    ],
                });
            }
            4 => {
                doc.body.push(Block::Raw {
                    format: RawFormat::Tex,
                    text: v1.clone(),
                });
                doc.body.push(Block::Raw {
                    format: RawFormat::Html,
                    text: format!("<div class=\"test\">{v2}</div>"),
                });
            }
            5 => {
                doc.body.push(Block::Table {
                    caption: None,
                    columns: vec![ColumnSpec {
                        align: Alignment::Default,
                    }],
                    head: vec![vec![TableCell {
                        colspan: 1,
                        rowspan: 1,
                        header: Some(true),
                        blocks: vec![Block::Paragraph {
                            content: vec![Inline::Text { text: v1.clone() }],
                        }],
                    }]],
                    body: vec![vec![TableCell {
                        colspan: 1,
                        rowspan: 1,
                        header: Some(false),
                        blocks: vec![Block::Paragraph {
                            content: vec![Inline::Code { text: v2.clone() }],
                        }],
                    }]],
                    footnotes: Vec::new(),
                });
            }
            _ => {
                doc.body.push(Block::List {
                    ordered: iter % 2 == 0,
                    start: Some(1),
                    tight: true,
                    items: vec![ListItem {
                        checked: Some(true),
                        blocks: vec![match iter % 3 {
                            0 => Block::Code {
                                lang: None,
                                text: format!("{v1}\n{v2}"),
                            },
                            1 => Block::Math {
                                tex: format!("{v1}\n# Injected"),
                                display: true,
                            },
                            _ => Block::Raw {
                                format: RawFormat::Tex,
                                text: format!("{v1}\n# Injected"),
                            },
                        }],
                    }],
                });
            }
        }

        let md_written = writer::markdown::write(&doc);
        assert!(
            !md_written.content.contains('\r'),
            "iter {iter}: markdown content must not contain CR"
        );
        assert!(
            !md_written.content.contains('\u{2028}'),
            "iter {iter}: markdown content must not contain U+2028"
        );
        assert!(
            !md_written.content.contains('\u{0085}'),
            "iter {iter}: markdown content must not contain U+0085"
        );

        let md_parsed = markdown::read(&md_written.content, &Limits::local())
            .unwrap_or_else(|e| panic!("iter {iter}: failed reading written markdown: {e:?}"));

        for block in &md_parsed.document.body {
            if let Block::Paragraph { content } = block {
                for inline in content {
                    if let Inline::Link { url, .. } = inline {
                        assert!(
                            !url.to_ascii_lowercase().starts_with("javascript:"),
                            "iter {iter}: javascript link found: {url}"
                        );
                    }
                }
            }
        }

        let orig_had_headings = recursive_has_headings(&doc.body);
        let orig_had_lists = recursive_has_lists(&doc.body);
        let orig_had_tables = doc.body.iter().any(|b| matches!(b, Block::Table { .. }));
        let orig_had_links = recursive_has_links(&doc.body);

        let md_has_headings = recursive_has_headings(&md_parsed.document.body);
        if !orig_had_headings {
            assert!(
                !md_has_headings,
                "iter {iter}: injected heading in markdown when original had none"
            );
        }
        let md_has_lists = recursive_has_lists(&md_parsed.document.body);
        if !orig_had_lists {
            assert!(
                !md_has_lists,
                "iter {iter}: injected list in markdown when original had none"
            );
        }
        let md_has_tables = md_parsed
            .document
            .body
            .iter()
            .any(|b| matches!(b, Block::Table { .. }));
        if !orig_had_tables {
            assert!(
                !md_has_tables,
                "iter {iter}: injected table in markdown when original had none"
            );
        }
        let md_has_links = recursive_has_links(&md_parsed.document.body);
        if !orig_had_links {
            assert!(
                !md_has_links,
                "iter {iter}: injected link in markdown when original had none"
            );
        }

        let html_written = writer::html::write(&doc);
        assert!(
            !html_written.content.contains('\r'),
            "iter {iter}: html content must not contain CR"
        );
        assert!(
            !html_written.content.contains('\u{2028}'),
            "iter {iter}: html content must not contain U+2028"
        );
        assert!(
            !html_written.content.contains('\u{0085}'),
            "iter {iter}: html content must not contain U+0085"
        );
        assert!(
            !html_written
                .content
                .to_ascii_lowercase()
                .contains("<script"),
            "iter {iter}: html output must not contain <script"
        );

        let html_parsed = html::read(html_written.content.as_bytes(), &Limits::local())
            .unwrap_or_else(|e| panic!("iter {iter}: failed reading written html: {e:?}"));

        let html_has_headings = html_parsed
            .document
            .body
            .iter()
            .any(|b| matches!(b, Block::Heading { .. }));
        if !orig_had_headings {
            assert!(
                !html_has_headings,
                "iter {iter}: injected heading in html when original had none"
            );
        }
        for block in &html_parsed.document.body {
            if let Block::Paragraph { content } = block {
                for inline in content {
                    if let Inline::Link { url, .. } = inline {
                        assert!(
                            !url.to_ascii_lowercase().starts_with("javascript:"),
                            "iter {iter}: javascript link found in html: {url}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn test_display_math_single_line_and_block_marker_safety() {
    let markers = ["- ", "* ", "+ ", "1. ", "---", "==="];

    for marker in markers {
        // 1. Block::Math
        let doc_block = Document {
            body: vec![Block::Math {
                tex: format!("{marker}safe"),
                display: true,
            }],
            ..Default::default()
        };
        let out_block = writer::markdown::write(&doc_block);
        assert_eq!(out_block.content, format!("$${marker}safe$$\n\n"));
        let parsed_block =
            markdown::read(&out_block.content, &Limits::local()).expect("read back Block::Math");
        assert!(
            !recursive_has_lists(&parsed_block.document.body),
            "marker {marker} in Block::Math must not produce a list"
        );
        assert!(
            !recursive_has_headings(&parsed_block.document.body),
            "marker {marker} in Block::Math must not produce a heading"
        );

        // 2. Inline::Math display
        let doc_inline = Document {
            body: vec![Block::Paragraph {
                content: vec![Inline::Math {
                    tex: format!("{marker}safe"),
                    display: true,
                }],
            }],
            ..Default::default()
        };
        let out_inline = writer::markdown::write(&doc_inline);
        assert!(out_inline.content.contains(&format!("$${marker}safe$$")));
        let parsed_inline = markdown::read(&out_inline.content, &Limits::local())
            .expect("read back inline display math");
        assert!(
            !recursive_has_lists(&parsed_inline.document.body),
            "marker {marker} in inline display math must not produce a list"
        );
        assert!(
            !recursive_has_headings(&parsed_inline.document.body),
            "marker {marker} in inline display math must not produce a heading"
        );
    }

    // 3. TeX containing URLs or emails is treated as outside safe class
    let hostile_tex_samples = [
        "- https://evil.example/login",
        "1. http://evil.example/path",
        "+ www.evil.example",
        "user@evil.example",
    ];
    for hostile in hostile_tex_samples {
        let doc = Document {
            body: vec![Block::Math {
                tex: hostile.to_owned(),
                display: true,
            }],
            ..Default::default()
        };
        let out = writer::markdown::write(&doc);
        assert!(
            out.warnings
                .iter()
                .any(|w| w.code == WarningCode::UnsupportedNode),
            "hostile tex '{hostile}' must emit UnsupportedNode warning"
        );
        let parsed = markdown::read(&out.content, &Limits::local())
            .expect("read back math with hostile tex");
        assert!(
            !recursive_has_links(&parsed.document.body),
            "hostile tex '{hostile}' must not produce live links on read-back"
        );
        assert!(
            !recursive_has_lists(&parsed.document.body),
            "hostile tex '{hostile}' must not produce a list on read-back"
        );
    }

    // 4. Reproducer from review: dm.html
    let dm_html = b"<p>Before</p><math display=\"block\"><semantics><mi>x</mi><annotation encoding=\"application/x-tex\">- https://evil.example/login</annotation></semantics></math><p>Mid</p><math display=\"block\"><semantics><mi>x</mi><annotation encoding=\"application/x-tex\">---</annotation></semantics></math>";
    let parsed_dm = html::read(dm_html, &Limits::local()).expect("read dm.html");
    let orig_had_lists = recursive_has_lists(&parsed_dm.document.body);
    let orig_had_headings = recursive_has_headings(&parsed_dm.document.body);
    let orig_had_links = recursive_has_links(&parsed_dm.document.body);
    assert!(!orig_had_lists);
    assert!(!orig_had_headings);
    assert!(!orig_had_links);

    let md_out = writer::markdown::write(&parsed_dm.document);
    let re_read = markdown::read(&md_out.content, &Limits::local()).expect("re-read dm markdown");
    assert!(
        !recursive_has_lists(&re_read.document.body),
        "dm reproducer must not produce a list on read-back"
    );
    assert!(
        !recursive_has_headings(&re_read.document.body),
        "dm reproducer must not produce a heading on read-back"
    );
    assert!(
        !recursive_has_links(&re_read.document.body),
        "dm reproducer must not produce a link on read-back"
    );
}

#[test]
fn test_task_list_non_paragraph_first_block_safety() {
    // 1. Reproducer from review: task.html
    let task_html = b"<ul><li><input type=\"checkbox\" checked><pre><code>[x](javascript:alert(1))\n<img src=x onerror=alert(2)>\n# Injected</code></pre></li></ul>";
    let parsed_task = html::read(task_html, &Limits::local()).expect("read task.html");
    let md_out = writer::markdown::write(&parsed_task.document);

    // Warning emitted for dropped checkbox
    assert!(
        md_out
            .warnings
            .iter()
            .any(|w| w.code == WarningCode::UnsupportedNode),
        "dropping checkbox must emit UnsupportedNode warning"
    );

    // Read back: code fence must have remained a code fence
    let re_read = markdown::read(&md_out.content, &Limits::local()).expect("re-read task markdown");
    assert!(
        !recursive_has_headings(&re_read.document.body),
        "task code fence must not break out into heading"
    );
    assert!(
        !recursive_has_links(&re_read.document.body),
        "task code fence must not break out into link"
    );

    // 2. Nested probe: ordered item > quote > task item > display math with newline and `# H`
    let nested_doc = Document {
        body: vec![Block::List {
            ordered: true,
            start: Some(1),
            tight: false,
            items: vec![ListItem {
                checked: None,
                blocks: vec![Block::Quote {
                    blocks: vec![Block::List {
                        ordered: false,
                        start: None,
                        tight: false,
                        items: vec![ListItem {
                            checked: Some(true),
                            blocks: vec![Block::Math {
                                tex: "x = 1\n# Injected Heading".to_owned(),
                                display: true,
                            }],
                        }],
                    }],
                }],
            }],
        }],
        ..Default::default()
    };
    let nested_out = writer::markdown::write(&nested_doc);
    let nested_re_read =
        markdown::read(&nested_out.content, &Limits::local()).expect("re-read nested markdown");
    assert!(
        !recursive_has_headings(&nested_re_read.document.body),
        "nested task math block must not produce heading"
    );
}

#[test]
fn test_table_cell_link_destination_and_title_pipe_escaping() {
    let doc = Document {
        body: vec![Block::Table {
            caption: None,
            columns: vec![ColumnSpec {
                align: Alignment::Default,
            }],
            head: vec![vec![TableCell {
                rowspan: 1,
                colspan: 1,
                header: Some(true),
                blocks: vec![Block::Paragraph {
                    content: vec![Inline::Text {
                        text: "Header".to_owned(),
                    }],
                }],
            }]],
            body: vec![vec![TableCell {
                rowspan: 1,
                colspan: 1,
                header: None,
                blocks: vec![Block::Paragraph {
                    content: vec![Inline::Link {
                        url: "https://e.com/a|b".to_owned(),
                        title: Some("t|x".to_owned()),
                        content: vec![Inline::Text {
                            text: "l".to_owned(),
                        }],
                    }],
                }],
            }]],
            footnotes: Vec::new(),
        }],
        ..Default::default()
    };

    let md_out = writer::markdown::write(&doc);
    assert!(
        md_out.content.contains("https://e.com/a%7Cb"),
        "pipe in link destination inside table must be percent-encoded: {}",
        md_out.content
    );
    assert!(
        md_out.content.contains(r#"t\|x"#),
        r#"pipe in link title inside table must be backslash-escaped: {}"#,
        md_out.content
    );

    let re_read =
        markdown::read(&md_out.content, &Limits::local()).expect("re-read table with pipe link");
    if let Some(Block::Table { columns, body, .. }) = re_read.document.body.first() {
        assert_eq!(columns.len(), 1, "table must have exactly 1 column");
        assert_eq!(body.len(), 1, "table must have 1 body row");
        assert_eq!(body[0].len(), 1, "row must have exactly 1 cell");
    } else {
        panic!("expected table on read-back");
    }
}

#[test]
fn test_adjacent_code_spans_do_not_merge() {
    let doc = Document {
        body: vec![Block::Paragraph {
            content: vec![
                Inline::Code {
                    text: "a|b".to_owned(),
                },
                Inline::Code {
                    text: "c|d".to_owned(),
                },
            ],
        }],
        ..Default::default()
    };

    let md_out = writer::markdown::write(&doc);
    assert!(
        md_out.content.contains("<!---->"),
        "adjacent code spans must be separated: {}",
        md_out.content
    );

    let re_read =
        markdown::read(&md_out.content, &Limits::local()).expect("re-read adjacent code spans");
    if let Some(Block::Paragraph { content }) = re_read.document.body.first() {
        let code_count = content
            .iter()
            .filter(|i| matches!(i, Inline::Code { .. }))
            .count();
        assert_eq!(
            code_count, 2,
            "must parse as two separate code spans: {content:?}"
        );
    } else {
        panic!("expected paragraph on read-back");
    }

    // Math fallback code span followed by Inline::Code
    let doc2 = Document {
        body: vec![Block::Paragraph {
            content: vec![
                Inline::Math {
                    tex: "unsafe$math".to_owned(),
                    display: false,
                },
                Inline::Code {
                    text: "tail".to_owned(),
                },
            ],
        }],
        ..Default::default()
    };
    let md_out2 = writer::markdown::write(&doc2);
    assert!(
        md_out2.content.contains("<!---->"),
        "fallback math code span adjacent to code must be separated: {}",
        md_out2.content
    );
    let re_read2 = markdown::read(&md_out2.content, &Limits::local())
        .expect("re-read math fallback and code spans");
    if let Some(Block::Paragraph { content }) = re_read2.document.body.first() {
        let code_count = content
            .iter()
            .filter(|i| matches!(i, Inline::Code { .. }))
            .count();
        assert_eq!(
            code_count, 2,
            "must parse as two separate code spans: {content:?}"
        );
    } else {
        panic!("expected paragraph on read-back");
    }
}

#[test]
fn test_footnote_id_ending_in_backslash_percent_encoded() {
    let doc = Document {
        body: vec![
            Block::Paragraph {
                content: vec![
                    Inline::Text {
                        text: "Ref ".to_owned(),
                    },
                    Inline::FootnoteRef {
                        id: "fn\\".to_owned(),
                    },
                ],
            },
            Block::Footnote {
                id: "fn\\".to_owned(),
                blocks: vec![Block::Paragraph {
                    content: vec![Inline::Text {
                        text: "Note content".to_owned(),
                    }],
                }],
            },
        ],
        ..Default::default()
    };

    let md_out = writer::markdown::write(&doc);
    assert!(
        md_out.content.contains("[^fn%5C]"),
        "trailing backslash in footnote id must be percent-encoded: {}",
        md_out.content
    );
    assert!(
        !md_out.content.contains(r#"[^fn\]"#),
        "must not escape closing bracket"
    );

    let re_read = markdown::read(&md_out.content, &Limits::local())
        .expect("re-read footnote with encoded backslash");
    assert!(
        re_read.document.body.iter().any(|b| matches!(b, Block::Paragraph { content } if content.iter().any(|i| matches!(i, Inline::FootnoteRef { .. })))),
        "footnote ref must be preserved on read-back"
    );
}
