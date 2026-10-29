use ariad_core::{
    format::Format,
    ir::{Alignment, Asset, Block, ColumnSpec, Document, Inline, TableCell},
    limits::Limits,
    pipeline::{self, PipelineError, WriteOptions},
};
use sha2::{Digest, Sha256};

fn json(document: &Document) -> Vec<u8> {
    serde_json::to_vec(document).unwrap()
}

fn read_document(document: &Document, limits: &Limits) -> Result<Document, PipelineError> {
    pipeline::read_ir_json(&json(document), limits)
}

fn rejected_limit(bytes: &[u8], limits: &Limits, expected: &str) {
    assert_eq!(
        pipeline::read_ir_json(bytes, limits).unwrap_err(),
        PipelineError::LimitExceeded {
            limit: expected.into()
        }
    );
}

fn rejected_document_limit(document: &Document, limits: &Limits, expected: &str) {
    rejected_limit(&json(document), limits, expected);
}

fn rejected_invalid_ir(document: &Document) {
    assert!(matches!(
        read_document(document, &Limits::local()),
        Err(PipelineError::InvalidIr { .. })
    ));
}

fn nested_quotes(depth: usize) -> String {
    format!(
        "{{\"version\":\"ariad-ir/1\",\"body\":[{}{{\"type\":\"paragraph\",\"content\":[]}}{}]}}",
        "{\"type\":\"quote\",\"blocks\":[".repeat(depth),
        "]}".repeat(depth),
    )
}

#[test]
fn deeply_nested_quotes_are_refused_at_the_json_prescan_boundary() {
    let limits = Limits::local();
    let bytes = nested_quotes(286);
    assert!(
        ariad_core::json_depth::prescan_json_depth(
            bytes.as_bytes(),
            ariad_core::json_depth::json_depth_budget_for_nesting(limits.max_nesting_depth)
        )
        .is_ok()
    );
    rejected_limit(bytes.as_bytes(), &limits, "max_nesting_depth");
    rejected_limit(nested_quotes(287).as_bytes(), &limits, "max_nesting_depth");
}

#[test]
fn quote_depth_boundary_is_enforced() {
    for max_nesting_depth in [3, 99] {
        let limits = Limits {
            max_nesting_depth,
            ..Limits::local()
        };
        pipeline::read_ir_json(
            nested_quotes(usize::from(max_nesting_depth)).as_bytes(),
            &limits,
        )
        .unwrap();
        rejected_limit(
            nested_quotes(usize::from(max_nesting_depth) + 1).as_bytes(),
            &limits,
            "max_nesting_depth",
        );
    }
}

fn emphasized(depth: usize) -> Inline {
    let mut inline = Inline::Text {
        text: "text".into(),
    };
    for _ in 0..depth {
        inline = Inline::Emph {
            content: vec![inline],
        };
    }
    inline
}

#[test]
fn blocks_and_inlines_share_one_depth_budget() {
    let limits = Limits {
        max_nesting_depth: 3,
        ..Limits::local()
    };
    for inline_depth in [2, 3] {
        let document = Document {
            body: vec![Block::Quote {
                blocks: vec![Block::Paragraph {
                    content: vec![emphasized(inline_depth)],
                }],
            }],
            ..Document::default()
        };
        if inline_depth == 2 {
            read_document(&document, &limits).unwrap();
        } else {
            rejected_document_limit(&document, &limits, "max_nesting_depth");
        }
    }
}

#[test]
fn inline_depth_in_furniture_figures_and_table_annotations_is_checked() {
    let limits = Limits {
        max_nesting_depth: 2,
        ..Limits::local()
    };
    let inline = emphasized(3);
    let cases = [
        Block::Paragraph {
            content: vec![inline.clone()],
        },
        Block::Figure {
            asset: ariad_core::ir::AssetRef::Url {
                href: "https://example.test/image.png".into(),
            },
            caption: vec![inline.clone()],
        },
        Block::Table {
            caption: Some(vec![inline.clone()]),
            columns: vec![],
            head: vec![],
            body: vec![],
            footnotes: vec![],
        },
        Block::Table {
            caption: None,
            columns: vec![],
            head: vec![],
            body: vec![],
            footnotes: vec![inline],
        },
    ];
    for block in cases {
        let document = Document {
            furniture: vec![block],
            ..Document::default()
        };
        rejected_document_limit(&document, &limits, "max_nesting_depth");
    }
}

#[test]
fn empty_containers_and_inline_wrappers_use_the_depth_budget() {
    let wrappers = [
        Inline::Emph { content: vec![] },
        Inline::Strong { content: vec![] },
        Inline::Strikeout { content: vec![] },
        Inline::Superscript { content: vec![] },
        Inline::Subscript { content: vec![] },
        Inline::Link {
            url: "https://example.test".into(),
            title: None,
            content: vec![],
        },
    ];
    let limits = Limits {
        max_nesting_depth: 0,
        ..Limits::local()
    };
    for inline in wrappers {
        let document = Document {
            body: vec![Block::Paragraph {
                content: vec![inline],
            }],
            ..Document::default()
        };
        rejected_document_limit(&document, &limits, "max_nesting_depth");
    }
    let blocks = [
        Block::Quote { blocks: vec![] },
        Block::Footnote {
            id: "note".into(),
            blocks: vec![],
        },
        Block::List {
            ordered: false,
            start: None,
            tight: false,
            items: vec![],
        },
        Block::Table {
            caption: None,
            columns: vec![],
            head: vec![],
            body: vec![],
            footnotes: vec![],
        },
    ];
    for block in blocks {
        let document = Document {
            body: vec![block],
            ..Document::default()
        };
        rejected_document_limit(&document, &limits, "max_nesting_depth");
    }
}

#[test]
fn list_items_and_table_cells_carry_the_parent_depth() {
    let paragraph = Block::Paragraph {
        content: vec![emphasized(2)],
    };
    let cell = TableCell {
        rowspan: 1,
        colspan: 1,
        header: None,
        blocks: vec![paragraph.clone()],
    };
    let blocks = [
        Block::List {
            ordered: false,
            start: None,
            tight: false,
            items: vec![ariad_core::ir::ListItem {
                checked: None,
                blocks: vec![paragraph],
            }],
        },
        Block::Table {
            caption: None,
            columns: vec![ColumnSpec {
                align: Alignment::Default,
            }],
            head: vec![vec![cell.clone()]],
            body: vec![vec![cell]],
            footnotes: vec![],
        },
    ];
    let limits = Limits {
        max_nesting_depth: 2,
        ..Limits::local()
    };
    for block in blocks {
        let document = Document {
            body: vec![block],
            ..Document::default()
        };
        rejected_document_limit(&document, &limits, "max_nesting_depth");
    }
}

#[test]
fn asset_bytes_are_bounded_even_when_unreferenced() {
    let bytes = vec![1, 2, 3, 4];
    let id = hex::encode(Sha256::digest(&bytes));
    let mut document = Document::default();
    document.assets.insert(
        id,
        Asset {
            media_type: "image/png".into(),
            bytes,
        },
    );
    let mut limits = Limits {
        max_asset_bytes: Some(4),
        ..Limits::local()
    };
    read_document(&document, &limits).unwrap();
    limits.max_asset_bytes = Some(3);
    rejected_document_limit(&document, &limits, "max_asset_bytes");
}

#[test]
fn asset_ids_must_be_lowercase_sha256_of_their_bytes() {
    for id in [
        "wrong".to_owned(),
        hex::encode(Sha256::digest(b"data")).to_uppercase(),
    ] {
        let mut document = Document::default();
        document.assets.insert(
            id,
            Asset {
                media_type: "image/png".into(),
                bytes: b"data".to_vec(),
            },
        );
        rejected_invalid_ir(&document);
    }
}

#[test]
fn invalid_heading_levels_and_duplicate_nested_footnotes_are_refused() {
    for level in [0, 7, 255] {
        let document = Document {
            body: vec![Block::Heading {
                level,
                content: vec![],
            }],
            ..Document::default()
        };
        rejected_invalid_ir(&document);
    }
    let footnote = Block::Footnote {
        id: "same".into(),
        blocks: vec![],
    };
    let document = Document {
        body: vec![footnote.clone()],
        furniture: vec![Block::Quote {
            blocks: vec![footnote],
        }],
        ..Document::default()
    };
    rejected_invalid_ir(&document);
}

#[test]
fn table_colspans_cannot_exceed_the_declared_columns() {
    for spans in [vec![2, 1], vec![u32::MAX, 1]] {
        let document = Document {
            body: vec![Block::Table {
                caption: None,
                columns: vec![
                    ColumnSpec {
                        align: Alignment::Default
                    };
                    2
                ],
                head: vec![],
                body: vec![
                    spans
                        .into_iter()
                        .map(|colspan| TableCell {
                            rowspan: 1,
                            colspan,
                            header: None,
                            blocks: vec![],
                        })
                        .collect(),
                ],
                footnotes: vec![],
            }],
            ..Document::default()
        };
        rejected_invalid_ir(&document);
    }
}

#[test]
fn valid_invariants_and_per_asset_boundaries_are_preserved() {
    let mut document = Document {
        body: vec![
            Block::Heading {
                level: 1,
                content: vec![],
            },
            Block::Heading {
                level: 6,
                content: vec![],
            },
            Block::Footnote {
                id: "one".into(),
                blocks: vec![],
            },
            Block::Table {
                caption: None,
                columns: vec![
                    ColumnSpec {
                        align: Alignment::Default
                    };
                    2
                ],
                head: vec![vec![TableCell {
                    rowspan: 1,
                    colspan: 2,
                    header: None,
                    blocks: vec![],
                }]],
                body: vec![],
                footnotes: vec![],
            },
        ],
        furniture: vec![Block::Footnote {
            id: "two".into(),
            blocks: vec![],
        }],
        ..Document::default()
    };
    for bytes in [vec![1, 2, 3], vec![4, 5, 6]] {
        document.assets.insert(
            hex::encode(Sha256::digest(&bytes)),
            Asset {
                media_type: "image/png".into(),
                bytes,
            },
        );
    }
    let limits = Limits {
        max_asset_bytes: Some(3),
        ..Limits::local()
    };
    assert_eq!(read_document(&document, &limits).unwrap(), document);
    rejected_document_limit(
        &document,
        &Limits {
            max_blocks: 4,
            ..limits
        },
        "max_blocks",
    );
}

#[test]
fn existing_fixture_ir_and_text_outputs_roundtrip_unchanged() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
    let mut count = 0;
    for dir in ["md", "html"] {
        for entry in std::fs::read_dir(root.join(dir)).unwrap() {
            let path = entry.unwrap().path();
            let format = match path.extension().and_then(|ext| ext.to_str()) {
                Some("md") => Format::Markdown,
                Some("html") => Format::Html,
                _ => continue,
            };
            let original = pipeline::read(format, &std::fs::read(&path).unwrap(), &Limits::local())
                .unwrap()
                .document;
            let encoded = json(&original);
            let validated = pipeline::read_ir_json(&encoded, &Limits::local())
                .unwrap_or_else(|err| panic!("{}: {err}", path.display()));
            assert_eq!(json(&validated), encoded, "{}", path.display());
            for target in [Format::Markdown, Format::Html] {
                assert_eq!(
                    pipeline::write(target, &original, WriteOptions::default()).unwrap(),
                    pipeline::write(target, &validated, WriteOptions::default()).unwrap()
                );
            }
            count += 1;
        }
    }
    assert!(count >= 26);
}

#[test]
fn amplification_is_refused_before_the_internal_output_budget() {
    assert_eq!(Limits::MAX_TEXT_OUTPUT_BYTES, 512 * 1024 * 1024);
    let document = Document {
        body: vec![Block::Paragraph {
            content: vec![Inline::Text {
                text: "\"".repeat(45 * 1024 * 1024),
            }],
        }],
        ..Document::default()
    };
    for format in [Format::Markdown, Format::Html] {
        let output = pipeline::write(format, &document, WriteOptions::default()).unwrap();
        assert!(output.bytes.is_empty());
        assert!(
            output
                .warnings
                .iter()
                .any(|w| w.message.contains("output ceiling"))
        );
    }
}
