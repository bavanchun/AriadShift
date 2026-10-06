use std::collections::BTreeMap;

use ariad_core::{
    ir::{Alignment as IrAlignment, AssetRef, Block as IrBlock, Inline as IrInline, RawFormat},
    limits::Limits,
    pandoc::{
        MapError,
        ast::{
            Alignment, Block, Citation, CitationMode, ColWidth, Figure, Inline, MetaValue, Pandoc,
            Table, empty_attr,
        },
        to_ir,
    },
    warning::WarningCode,
};

#[test]
fn rejects_unsupported_api_version() {
    let doc_22 = Pandoc {
        pandoc_api_version: vec![1, 22],
        ..Default::default()
    };
    let err = to_ir(&doc_22, &Limits::local()).unwrap_err();
    assert_eq!(
        err,
        MapError::UnsupportedApiVersion {
            version: vec![1, 22]
        }
    );

    let doc_20 = Pandoc {
        pandoc_api_version: vec![2, 0],
        ..Default::default()
    };
    let err = to_ir(&doc_20, &Limits::local()).unwrap_err();
    assert_eq!(
        err,
        MapError::UnsupportedApiVersion {
            version: vec![2, 0]
        }
    );
}

#[test]
fn accepts_standard_1_23_api_versions() {
    let doc_four_part = Pandoc {
        pandoc_api_version: vec![1, 23, 1, 2],
        ..Default::default()
    };
    assert!(to_ir(&doc_four_part, &Limits::local()).is_ok());

    let doc_two_part = Pandoc {
        pandoc_api_version: vec![1, 23],
        ..Default::default()
    };
    assert!(to_ir(&doc_two_part, &Limits::local()).is_ok());
}

#[test]
fn nesting_at_sixty_four_passes_and_at_sixty_five_fails() {
    let mut limits = Limits::local();
    limits.max_nesting_depth = 64;

    // Build blockquote chain with given depth
    fn build_nested_quotes(depth: usize) -> Vec<Block> {
        let mut cur = vec![Block::Para(vec![Inline::Str("innermost".to_owned())])];
        // Each BlockQuote wraps the previous level, increasing nesting by 1
        for _ in 1..depth {
            cur = vec![Block::BlockQuote(cur)];
        }
        cur
    }

    // Depth 64: 1 (top-level) + 63 nested blockquotes = depth 64
    let doc_64 = Pandoc {
        pandoc_api_version: vec![1, 23, 1, 2],
        meta: BTreeMap::new(),
        blocks: build_nested_quotes(64),
    };
    assert!(to_ir(&doc_64, &limits).is_ok());

    // Depth 65: 1 (top-level) + 64 nested blockquotes = depth 65
    let doc_65 = Pandoc {
        pandoc_api_version: vec![1, 23, 1, 2],
        meta: BTreeMap::new(),
        blocks: build_nested_quotes(65),
    };
    let err = to_ir(&doc_65, &limits).unwrap_err();
    assert_eq!(err, MapError::NestingTooDeep { limit: 64 });
}

#[test]
fn enforces_max_blocks_limit() {
    let mut limits = Limits::local();
    limits.max_blocks = 3;

    let doc = Pandoc {
        pandoc_api_version: vec![1, 23, 1, 2],
        meta: BTreeMap::new(),
        blocks: vec![
            Block::Para(vec![Inline::Str("p1".to_owned())]),
            Block::Para(vec![Inline::Str("p2".to_owned())]),
            Block::Para(vec![Inline::Str("p3".to_owned())]),
            Block::Para(vec![Inline::Str("p4".to_owned())]),
        ],
    };
    let err = to_ir(&doc, &limits).unwrap_err();
    assert_eq!(err, MapError::TooManyBlocks { limit: 3 });
}

#[test]
fn maps_headings_paragraphs_and_code_blocks() {
    let doc = Pandoc {
        pandoc_api_version: vec![1, 23, 1, 2],
        meta: BTreeMap::new(),
        blocks: vec![
            Block::Header(2, empty_attr(), vec![Inline::Str("Heading 2".to_owned())]),
            Block::Para(vec![
                Inline::Str("Text with ".to_owned()),
                Inline::Code(empty_attr(), "inline code".to_owned()),
            ]),
            Block::CodeBlock(
                (String::new(), vec!["rust".to_owned()], Vec::new()),
                "fn main() {}".to_owned(),
            ),
        ],
    };

    let out = to_ir(&doc, &Limits::local()).unwrap();
    assert_eq!(out.document.body.len(), 3);
    assert_eq!(
        out.document.body[0],
        IrBlock::Heading {
            level: 2,
            content: vec![IrInline::Text {
                text: "Heading 2".to_owned()
            }]
        }
    );
    assert_eq!(
        out.document.body[1],
        IrBlock::Paragraph {
            content: vec![
                IrInline::Text {
                    text: "Text with ".to_owned()
                },
                IrInline::Code {
                    text: "inline code".to_owned()
                }
            ]
        }
    );
    assert_eq!(
        out.document.body[2],
        IrBlock::Code {
            lang: Some("rust".to_owned()),
            text: "fn main() {}".to_owned()
        }
    );
}

#[test]
fn maps_line_block_to_paragraph_with_line_breaks_and_warning() {
    let doc = Pandoc {
        pandoc_api_version: vec![1, 23, 1, 2],
        meta: BTreeMap::new(),
        blocks: vec![Block::LineBlock(vec![
            vec![Inline::Str("Line 1".to_owned())],
            vec![Inline::Str("Line 2".to_owned())],
        ])],
    };

    let out = to_ir(&doc, &Limits::local()).unwrap();
    assert_eq!(out.warnings.len(), 1);
    assert_eq!(out.warnings[0].code, WarningCode::UnsupportedNode);
    assert!(out.warnings[0].message.contains("line block"));

    assert_eq!(
        out.document.body[0],
        IrBlock::Paragraph {
            content: vec![
                IrInline::Text {
                    text: "Line 1".to_owned()
                },
                IrInline::LineBreak {},
                IrInline::Text {
                    text: "Line 2".to_owned()
                },
            ]
        }
    );
}

#[test]
fn maps_definition_list_to_strong_terms_with_warning() {
    let doc = Pandoc {
        pandoc_api_version: vec![1, 23, 1, 2],
        meta: BTreeMap::new(),
        blocks: vec![Block::DefinitionList(vec![(
            vec![Inline::Str("Term".to_owned())],
            vec![vec![Block::Para(vec![Inline::Str(
                "Definition".to_owned(),
            )])]],
        )])],
    };

    let out = to_ir(&doc, &Limits::local()).unwrap();
    assert_eq!(out.warnings.len(), 1);
    assert_eq!(out.warnings[0].code, WarningCode::UnsupportedNode);
    assert!(out.warnings[0].message.contains("definition list"));

    match &out.document.body[0] {
        IrBlock::List {
            ordered,
            tight,
            items,
            ..
        } => {
            assert!(!ordered);
            assert!(!tight);
            assert_eq!(items.len(), 1);
            assert_eq!(items[0].blocks.len(), 2);
            assert_eq!(
                items[0].blocks[0],
                IrBlock::Paragraph {
                    content: vec![IrInline::Strong {
                        content: vec![IrInline::Text {
                            text: "Term".to_owned()
                        }]
                    }]
                }
            );
            assert_eq!(
                items[0].blocks[1],
                IrBlock::Paragraph {
                    content: vec![IrInline::Text {
                        text: "Definition".to_owned()
                    }]
                }
            );
        }
        other => panic!("expected List block, got {other:?}"),
    }
}

#[test]
fn maps_lists_and_preserves_task_checkboxes() {
    let doc = Pandoc {
        pandoc_api_version: vec![1, 23, 1, 2],
        meta: BTreeMap::new(),
        blocks: vec![Block::BulletList(vec![
            vec![Block::Para(vec![
                Inline::Str("☒".to_owned()),
                Inline::Space,
                Inline::Str("Done task".to_owned()),
            ])],
            vec![Block::Para(vec![
                Inline::Str("☐".to_owned()),
                Inline::Space,
                Inline::Str("Todo task".to_owned()),
            ])],
            vec![Block::Para(vec![Inline::Str("Regular item".to_owned())])],
        ])],
    };

    let out = to_ir(&doc, &Limits::local()).unwrap();
    let IrBlock::List { items, .. } = &out.document.body[0] else {
        panic!("expected list");
    };
    assert_eq!(items[0].checked, Some(true));
    assert_eq!(items[1].checked, Some(false));
    assert_eq!(items[2].checked, None);
}

#[test]
fn maps_table_with_merged_cells_and_alignments() {
    let table: Table = (
        empty_attr(),
        (
            Some(vec![Inline::Str("Table Caption".to_owned())]),
            Vec::new(),
        ),
        vec![
            (Alignment::AlignLeft, ColWidth::ColWidthDefault),
            (Alignment::AlignCenter, ColWidth::ColWidthDefault),
        ],
        (
            empty_attr(),
            vec![(
                empty_attr(),
                vec![
                    (
                        empty_attr(),
                        Alignment::AlignLeft,
                        1,
                        2,
                        vec![Block::Plain(vec![Inline::Str(
                            "Header spanning 2".to_owned(),
                        )])],
                    ),
                    (
                        empty_attr(),
                        Alignment::AlignCenter,
                        1,
                        1,
                        vec![Block::Plain(vec![Inline::Str("H2".to_owned())])],
                    ),
                ],
            )],
        ),
        vec![(
            empty_attr(),
            0,
            Vec::new(),
            vec![(
                empty_attr(),
                vec![(
                    empty_attr(),
                    Alignment::AlignLeft,
                    3,
                    1,
                    vec![Block::Para(vec![Inline::Str("Row spanning 3".to_owned())])],
                )],
            )],
        )],
        (empty_attr(), Vec::new()),
    );

    let doc = Pandoc {
        pandoc_api_version: vec![1, 23, 1, 2],
        meta: BTreeMap::new(),
        blocks: vec![Block::Table(Box::new(table))],
    };

    let out = to_ir(&doc, &Limits::local()).unwrap();
    let IrBlock::Table {
        caption,
        columns,
        head,
        body,
        ..
    } = &out.document.body[0]
    else {
        panic!("expected table");
    };

    assert_eq!(
        caption,
        &Some(vec![IrInline::Text {
            text: "Table Caption".to_owned()
        }])
    );
    assert_eq!(columns[0].align, IrAlignment::Left);
    assert_eq!(columns[1].align, IrAlignment::Center);

    assert_eq!(head[0][0].colspan, 2);
    assert_eq!(head[0][0].rowspan, 1);
    assert_eq!(head[0][0].header, Some(true));

    assert_eq!(body[0][0].colspan, 1);
    assert_eq!(body[0][0].rowspan, 3);
    assert_eq!(body[0][0].header, None);
}

#[test]
fn maps_figure_wrapping_image() {
    let figure: Figure = (
        empty_attr(),
        (
            None,
            vec![Block::Para(vec![Inline::Str("Caption text".to_owned())])],
        ),
        vec![Block::Para(vec![Inline::Image(
            empty_attr(),
            vec![Inline::Str("Alt text".to_owned())],
            ("media/image1.png".to_owned(), String::new()),
        )])],
    );

    let doc = Pandoc {
        pandoc_api_version: vec![1, 23, 1, 2],
        meta: BTreeMap::new(),
        blocks: vec![Block::Figure(figure)],
    };

    let out = to_ir(&doc, &Limits::local()).unwrap();
    assert_eq!(
        out.document.body[0],
        IrBlock::Figure {
            asset: AssetRef::Url {
                href: "media/image1.png".to_owned()
            },
            caption: vec![IrInline::Text {
                text: "Caption text".to_owned()
            }]
        }
    );
}

#[test]
fn flattens_div_and_span() {
    let doc = Pandoc {
        pandoc_api_version: vec![1, 23, 1, 2],
        meta: BTreeMap::new(),
        blocks: vec![Block::Div(
            empty_attr(),
            vec![Block::Para(vec![Inline::Span(
                empty_attr(),
                vec![Inline::Str("Inside span".to_owned())],
            )])],
        )],
    };

    let out = to_ir(&doc, &Limits::local()).unwrap();
    assert_eq!(out.document.body.len(), 1);
    assert_eq!(
        out.document.body[0],
        IrBlock::Paragraph {
            content: vec![IrInline::Text {
                text: "Inside span".to_owned()
            }]
        }
    );
}

#[test]
fn maps_footnotes_to_blocks_and_footnote_refs() {
    let doc = Pandoc {
        pandoc_api_version: vec![1, 23, 1, 2],
        meta: BTreeMap::new(),
        blocks: vec![Block::Para(vec![
            Inline::Str("First note".to_owned()),
            Inline::Note(vec![Block::Para(vec![Inline::Str("Body 1".to_owned())])]),
            Inline::Str(" second note".to_owned()),
            Inline::Note(vec![Block::Para(vec![Inline::Str("Body 2".to_owned())])]),
        ])],
    };

    let out = to_ir(&doc, &Limits::local()).unwrap();
    assert_eq!(out.document.body.len(), 3);

    // Paragraph with FootnoteRefs
    assert_eq!(
        out.document.body[0],
        IrBlock::Paragraph {
            content: vec![
                IrInline::Text {
                    text: "First note".to_owned()
                },
                IrInline::FootnoteRef {
                    id: "fn1".to_owned()
                },
                IrInline::Text {
                    text: " second note".to_owned()
                },
                IrInline::FootnoteRef {
                    id: "fn2".to_owned()
                },
            ]
        }
    );

    // Footnote blocks at the end
    assert_eq!(
        out.document.body[1],
        IrBlock::Footnote {
            id: "fn1".to_owned(),
            blocks: vec![IrBlock::Paragraph {
                content: vec![IrInline::Text {
                    text: "Body 1".to_owned()
                }]
            }]
        }
    );
    assert_eq!(
        out.document.body[2],
        IrBlock::Footnote {
            id: "fn2".to_owned(),
            blocks: vec![IrBlock::Paragraph {
                content: vec![IrInline::Text {
                    text: "Body 2".to_owned()
                }]
            }]
        }
    );
}

#[test]
fn degrades_underline_smallcaps_and_cite_with_warnings() {
    let doc = Pandoc {
        pandoc_api_version: vec![1, 23, 1, 2],
        meta: BTreeMap::new(),
        blocks: vec![Block::Para(vec![
            Inline::Underline(vec![Inline::Str("underlined".to_owned())]),
            Inline::Space,
            Inline::SmallCaps(vec![Inline::Str("small caps".to_owned())]),
            Inline::Space,
            Inline::Cite(
                vec![Citation {
                    citation_id: "ref".to_owned(),
                    citation_prefix: Vec::new(),
                    citation_suffix: Vec::new(),
                    citation_mode: CitationMode::NormalCitation,
                    citation_note_num: 1,
                    citation_hash: 0,
                }],
                vec![Inline::Str("[@ref]".to_owned())],
            ),
        ])],
    };

    let out = to_ir(&doc, &Limits::local()).unwrap();
    assert_eq!(out.warnings.len(), 3);
    assert!(
        out.warnings
            .iter()
            .all(|w| w.code == WarningCode::UnsupportedNode)
    );

    assert_eq!(
        out.document.body[0],
        IrBlock::Paragraph {
            content: vec![
                IrInline::Emph {
                    content: vec![IrInline::Text {
                        text: "underlined".to_owned()
                    }]
                },
                IrInline::Text {
                    text: " ".to_owned()
                },
                IrInline::Text {
                    text: "small caps".to_owned()
                },
                IrInline::Text {
                    text: " ".to_owned()
                },
                IrInline::Text {
                    text: "[@ref]".to_owned()
                },
            ]
        }
    );
}

#[test]
fn handles_raw_blocks_and_inlines() {
    let doc = Pandoc {
        pandoc_api_version: vec![1, 23, 1, 2],
        meta: BTreeMap::new(),
        blocks: vec![
            Block::RawBlock("html".to_owned(), "<div>html</div>".to_owned()),
            Block::RawBlock("tex".to_owned(), "\\latex".to_owned()),
            Block::RawBlock("openxml".to_owned(), "<w:p/>".to_owned()),
            Block::Para(vec![
                Inline::RawInline("html".to_owned(), "<span>inline</span>".to_owned()),
                Inline::RawInline("openxml".to_owned(), "<w:r/>".to_owned()),
            ]),
        ],
    };

    let out = to_ir(&doc, &Limits::local()).unwrap();
    assert_eq!(out.warnings.len(), 2);
    assert!(
        out.warnings
            .iter()
            .all(|w| w.code == WarningCode::RawDropped)
    );

    assert_eq!(
        out.document.body[0],
        IrBlock::Raw {
            format: RawFormat::Html,
            text: "<div>html</div>".to_owned()
        }
    );
    assert_eq!(
        out.document.body[1],
        IrBlock::Raw {
            format: RawFormat::Tex,
            text: "\\latex".to_owned()
        }
    );
    assert_eq!(
        out.document.body[2],
        IrBlock::Paragraph {
            content: vec![IrInline::Raw {
                format: RawFormat::Html,
                text: "<span>inline</span>".to_owned()
            }]
        }
    );
}

#[test]
fn enforces_link_scheme_allow_list() {
    let doc = Pandoc {
        pandoc_api_version: vec![1, 23, 1, 2],
        meta: BTreeMap::new(),
        blocks: vec![Block::Para(vec![
            Inline::Link(
                empty_attr(),
                vec![Inline::Str("safe link".to_owned())],
                ("https://example.com".to_owned(), String::new()),
            ),
            Inline::Space,
            Inline::Link(
                empty_attr(),
                vec![Inline::Str("unsafe link".to_owned())],
                ("javascript:alert(1)".to_owned(), String::new()),
            ),
        ])],
    };

    let out = to_ir(&doc, &Limits::local()).unwrap();
    assert_eq!(out.warnings.len(), 1);
    assert_eq!(out.warnings[0].code, WarningCode::LinkDropped);

    assert_eq!(
        out.document.body[0],
        IrBlock::Paragraph {
            content: vec![
                IrInline::Link {
                    url: "https://example.com".to_owned(),
                    title: None,
                    content: vec![IrInline::Text {
                        text: "safe link".to_owned()
                    }],
                },
                IrInline::Text {
                    text: " ".to_owned()
                },
                IrInline::Text {
                    text: "unsafe link".to_owned()
                },
            ]
        }
    );
}

#[test]
fn normalizes_vietnamese_prose_to_nfc_and_drops_presentation_metadata() {
    // "Tiếng Việt" in NFD: 'e' + combining circumflex + combining acute
    let decomposed_title = "Ti\u{0065}\u{0302}\u{0301}ng Vi\u{0065}\u{0302}\u{0323}t";
    let decomposed_body = "Khu v\u{0075}\u{031b}\u{006f}\u{031b}\u{0300}n \u{0111}\u{006f}\u{0323}c s\u{0061}\u{0301}ch";

    let mut meta = BTreeMap::new();
    meta.insert(
        "title".to_owned(),
        MetaValue::MetaString(decomposed_title.to_owned()),
    );
    meta.insert(
        "author".to_owned(),
        MetaValue::MetaList(vec![MetaValue::MetaString("AriadShift".to_owned())]),
    );
    meta.insert("lang".to_owned(), MetaValue::MetaString("vi".to_owned()));
    meta.insert(
        "generator".to_owned(),
        MetaValue::MetaString("pandoc".to_owned()),
    );
    meta.insert(
        "viewport".to_owned(),
        MetaValue::MetaString("width=device-width".to_owned()),
    );

    let doc = Pandoc {
        pandoc_api_version: vec![1, 23, 1, 2],
        meta,
        blocks: vec![Block::Para(vec![Inline::Str(decomposed_body.to_owned())])],
    };

    let out = to_ir(&doc, &Limits::local()).unwrap();

    // Check NFC normalized title
    assert_eq!(out.document.meta.title.as_deref(), Some("Tiếng Việt"));
    assert_eq!(out.document.meta.authors, vec!["AriadShift"]);
    assert_eq!(out.document.meta.language.as_deref(), Some("vi"));

    // Check NFC normalized body
    let IrBlock::Paragraph { content } = &out.document.body[0] else {
        panic!("expected paragraph");
    };
    let IrInline::Text { text } = &content[0] else {
        panic!("expected text");
    };
    assert_eq!(text, "Khu vườn đọc sách");
}
