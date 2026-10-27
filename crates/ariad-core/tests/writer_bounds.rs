use ariad_core::{
    ir::{Alignment, Asset, AssetRef, Block, ColumnSpec, Document, Inline, TableCell},
    limits::Limits,
    reader::markdown,
    warning::WarningCode,
    writer,
};
use std::{
    fs,
    time::{Duration, Instant},
};

fn document_with_repeated_image(asset_bytes: Vec<u8>, references: usize) -> Document {
    let mut doc = Document::default();
    doc.assets.insert(
        "logo".to_owned(),
        Asset {
            media_type: "image/png".to_owned(),
            bytes: asset_bytes,
        },
    );
    for _ in 0..references {
        doc.body.push(Block::Paragraph {
            content: vec![Inline::Image {
                target: AssetRef::Asset {
                    id: "logo".to_owned(),
                },
                alt: "logo".to_owned(),
                title: None,
            }],
        });
    }
    doc
}

#[test]
fn repeated_asset_references_keep_markdown_output_linear() {
    let small = writer::markdown::write(&document_with_repeated_image(vec![0x5a; 4 * 1024], 200));
    let large = writer::markdown::write(&document_with_repeated_image(vec![0x5a; 8 * 1024], 400));
    eprintln!(
        "Markdown image bytes: small={}, large={}",
        small.content.len(),
        large.content.len()
    );
    // Doubling both the asset and the reference count doubles linear output; re-encoding the
    // asset at every reference quadruples it.
    assert!(
        large.content.len() < 3 * small.content.len(),
        "small: {}, large: {}",
        small.content.len(),
        large.content.len()
    );
}

#[test]
fn repeated_asset_references_share_one_markdown_data_uri_and_read_back() {
    let png = fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/md/assets/vietnamese-route.png"),
    )
    .expect("real PNG fixture");
    let written = writer::markdown::write(&document_with_repeated_image(png.clone(), 3));
    assert_eq!(written.content.matches("data:image/png;base64,").count(), 1);

    let read_back = markdown::read(&written.content, &Limits::local()).expect("read back");
    assert_eq!(read_back.document.assets.len(), 1);
    assert_eq!(
        read_back.document.assets.values().next().unwrap().bytes,
        png
    );
    let images = read_back
        .document
        .body
        .iter()
        .filter(|block| {
            matches!(block, Block::Paragraph { content }
                if matches!(content.as_slice(), [Inline::Image { target: AssetRef::Asset { .. }, .. }]))
        })
        .count();
    assert_eq!(images, 3);
}

#[test]
fn repeated_asset_references_keep_html_output_linear() {
    // HTML keeps one embedded image and warns when later references become alt text.
    let small = writer::html::write(&document_with_repeated_image(vec![0x5a; 4 * 1024], 200));
    let large = writer::html::write(&document_with_repeated_image(vec![0x5a; 8 * 1024], 400));
    eprintln!(
        "HTML image bytes: small={}, large={}",
        small.content.len(),
        large.content.len()
    );
    assert!(
        large.content.len() < 3 * small.content.len(),
        "small: {}, large: {}",
        small.content.len(),
        large.content.len()
    );
}

#[test]
fn long_leading_space_runs_are_escaped_in_linear_time() {
    let mut doc = Document::default();
    doc.body.push(Block::Paragraph {
        content: vec![Inline::Text {
            text: format!("{}y", " ".repeat(1_000_000)),
        }],
    });
    let start = Instant::now();
    let written = writer::markdown::write(&doc);
    let elapsed = start.elapsed();
    eprintln!("leading spaces debug timing: {elapsed:?}");
    // A single entity must escape the entire leading run without rescanning it.
    assert!(
        elapsed < Duration::from_secs(10),
        "escaping leading spaces took too long: {elapsed:?}"
    );
    assert!(written.content.starts_with("&#32;"));
    assert_eq!(written.content.matches("&#32;").count(), 1);
    assert_eq!(
        written.content.bytes().filter(|byte| *byte == b' ').count(),
        999_999
    );
    let read_back = markdown::read(&written.content, &Limits::local()).expect("read back");
    assert!(matches!(
        read_back.document.body.as_slice(),
        [Block::Paragraph { .. }]
    ));
}

fn wide_table_with_empty_rows(columns: usize, rows: usize) -> Document {
    let mut doc = Document::default();
    doc.body.push(Block::Table {
        caption: None,
        columns: vec![
            ColumnSpec {
                align: Alignment::Default
            };
            columns
        ],
        head: vec![],
        body: vec![Vec::new(); rows],
        footnotes: vec![],
    });
    doc
}

#[test]
fn empty_rows_of_wide_tables_keep_markdown_output_linear() {
    let small = writer::markdown::write(&wide_table_with_empty_rows(500, 500))
        .content
        .len();
    let large = writer::markdown::write(&wide_table_with_empty_rows(1_000, 1_000))
        .content
        .len();
    eprintln!("Empty table bytes: small={small}, large={large}");
    // Padding every empty row to the column count grows with columns × rows, so doubling
    // both quadruples the output.
    assert!(large < 3 * small, "small: {small}, large: {large}");
}

#[test]
fn many_image_references_emit_one_asset_copy() {
    let doc = document_with_repeated_image(vec![0x5a; 1024], 10_000);
    for output in [writer::markdown::write(&doc), writer::html::write(&doc)] {
        assert_eq!(output.content.matches("data:image/png;base64,").count(), 1);
        assert!(output.content.len() < 1_000_000);
    }
}

#[test]
fn footnote_ids_cannot_inject_html() {
    let id = "<img\tsrc=x\tonerror=alert(1)>%é";
    let doc = Document {
        body: vec![
            Block::Paragraph {
                content: vec![Inline::FootnoteRef { id: id.into() }],
            },
            Block::Footnote {
                id: id.into(),
                blocks: vec![],
            },
        ],
        ..Document::default()
    };
    let output = writer::markdown::write(&doc);
    assert!(!output.content.contains("<img"));
    assert!(output.content.contains("%3Cimg%09src"));
    assert!(output.content.contains("%25%C3%A9"));
    assert_eq!(output.content.matches("%3Cimg%09src").count(), 2);
}

fn cell(text: &str) -> TableCell {
    TableCell {
        rowspan: 1,
        colspan: 1,
        header: None,
        blocks: vec![Block::Paragraph {
            content: vec![Inline::Text { text: text.into() }],
        }],
    }
}

#[test]
fn table_annotations_are_preserved_and_header_loss_warns() {
    let doc = Document {
        body: vec![Block::Table {
            caption: Some(vec![Inline::Text {
                text: "Table caption".into(),
            }]),
            columns: vec![ColumnSpec {
                align: Alignment::Default,
            }],
            head: vec![vec![cell("First header")], vec![cell("Second header")]],
            body: vec![vec![cell("Body cell")]],
            footnotes: vec![Inline::Text {
                text: "Table note".into(),
            }],
        }],
        ..Document::default()
    };
    for output in [writer::markdown::write(&doc), writer::html::write(&doc)] {
        assert!(output.content.contains("Table caption"));
        assert!(output.content.contains("Table note"));
        assert!(
            output.content.contains("Second header")
                || output
                    .warnings
                    .iter()
                    .any(|w| w.code == WarningCode::UnsupportedNode)
        );
    }
}

#[test]
fn very_wide_empty_tables_are_bounded_and_warn() {
    let output = writer::markdown::write(&wide_table_with_empty_rows(100_000, 1000));
    assert!(output.content.len() < 100_000);
    assert!(
        output
            .warnings
            .iter()
            .any(|w| w.code == WarningCode::UnsupportedNode)
    );
}

#[test]
fn oversized_writer_output_is_refused_with_a_warning() {
    // Escaping expands each byte into six output bytes in HTML.
    let doc = Document {
        body: vec![Block::Paragraph {
            content: vec![Inline::Text {
                text: "\"".repeat(45 * 1024 * 1024),
            }],
        }],
        ..Document::default()
    };
    for output in [writer::markdown::write(&doc), writer::html::write(&doc)] {
        assert!(
            output.content.is_empty(),
            "oversized output must be refused"
        );
        assert!(
            output
                .warnings
                .iter()
                .any(|w| w.message.contains("output ceiling"))
        );
    }
}

#[test]
fn shared_markdown_images_keep_their_common_title() {
    let mut doc = document_with_repeated_image(vec![0x5a; 32], 3);
    for block in &mut doc.body {
        if let Block::Paragraph { content } = block
            && let Inline::Image { title, .. } = &mut content[0]
        {
            *title = Some("Common title".into());
        }
    }
    let output = writer::markdown::write(&doc);
    assert!(output.content.contains("\"Common title\""));
    assert!(output.warnings.is_empty());
    let parsed = markdown::read(&output.content, &Limits::local()).unwrap();
    assert_eq!(parsed.document.body.len(), 3);
    for block in parsed.document.body {
        assert!(
            matches!(block, Block::Paragraph { content } if matches!(&content[0], Inline::Image { title: Some(title), .. } if title == "Common title"))
        );
    }
}

#[test]
fn shared_markdown_images_warn_and_keep_the_first_conflicting_title() {
    assert_shared_image_titles(Some("first"), Some("second"));
}

#[test]
fn shared_markdown_images_warn_when_a_title_is_missing() {
    assert_shared_image_titles(None, Some("second"));
    assert_shared_image_titles(Some("first"), None);
}

fn assert_shared_image_titles(first: Option<&str>, second: Option<&str>) {
    let mut doc = document_with_repeated_image(b"\x89PNG\r\n\x1a\n".to_vec(), 2);
    for (index, title) in [first, second].into_iter().enumerate() {
        if let Block::Paragraph { content } = &mut doc.body[index]
            && let Inline::Image {
                alt,
                title: image_title,
                ..
            } = &mut content[0]
        {
            *alt = format!("alt {index}");
            *image_title = title.map(str::to_owned);
        }
    }
    let output = writer::markdown::write(&doc);
    let title = first
        .map(|title| format!(" \"{title}\""))
        .unwrap_or_default();
    assert_eq!(
        output.content,
        format!(
            "![alt 0][ariad-asset-0]\n\n![alt 1][ariad-asset-0]\n\n[ariad-asset-0]: data:image/png;base64,iVBORw0KGgo={title}\n"
        )
    );
    assert_eq!(output.warnings.len(), 1);
    assert_eq!(output.warnings[0].code, WarningCode::UnsupportedNode);
    assert_eq!(
        output.warnings[0].message,
        "different titles on repeated embedded images were reduced to the first title"
    );
    let parsed = markdown::read(&output.content, &Limits::local()).unwrap();
    assert_eq!(parsed.document.assets.len(), 1);
    assert_eq!(parsed.document.body.len(), 2);
    for (index, block) in parsed.document.body.iter().enumerate() {
        assert!(matches!(block, Block::Paragraph { content }
            if matches!(content.as_slice(), [Inline::Image { target: AssetRef::Asset { .. }, alt, title }]
                if alt == &format!("alt {index}") && title.as_deref() == first)));
    }
}

#[test]
fn html_repeated_assets_keep_escaped_alt_text_and_warn_once() {
    let mut doc = document_with_repeated_image(vec![0x5a; 32], 3);
    if let Block::Paragraph { content } = &mut doc.body[1]
        && let Inline::Image { alt, .. } = &mut content[0]
    {
        *alt = "<img src=x onerror=alert(1)>".into();
    }
    let output = writer::html::write(&doc);
    assert_eq!(
        output
            .warnings
            .iter()
            .filter(|w| w.code == WarningCode::ImageNotEmbedded)
            .count(),
        1
    );
    assert!(
        output
            .content
            .contains("&lt;img src=x onerror=alert(1)&gt;")
    );
    assert!(!output.content.contains("<img src=x"));
}

#[test]
fn sparse_tables_keep_cells_in_later_wider_rows() {
    let doc = Document {
        body: vec![Block::Table {
            caption: None,
            columns: vec![],
            head: vec![],
            body: vec![vec![cell("First")], vec![cell("Second"), cell("Last cell")]],
            footnotes: vec![],
        }],
        ..Document::default()
    };
    assert!(writer::markdown::write(&doc).content.contains("Last cell"));
}

#[test]
fn images_in_furniture_and_nested_blocks_share_the_definition() {
    let mut doc = document_with_repeated_image(vec![0x5a; 32], 3);
    let block = doc.body.pop().unwrap();
    doc.furniture.push(Block::Quote {
        blocks: vec![block],
    });
    let block = doc.body.pop().unwrap();
    doc.body.push(Block::Table {
        caption: None,
        columns: vec![],
        head: vec![],
        body: vec![vec![TableCell {
            rowspan: 1,
            colspan: 1,
            header: None,
            blocks: vec![block],
        }]],
        footnotes: vec![],
    });
    for output in [writer::markdown::write(&doc), writer::html::write(&doc)] {
        assert_eq!(output.content.matches("data:image/png;base64,").count(), 1);
    }
}

#[test]
fn writer_ceiling_does_not_reject_large_plain_text() {
    let doc = Document {
        body: vec![Block::Paragraph {
            content: vec![Inline::Text {
                text: "a".repeat(33 * 1024 * 1024),
            }],
        }],
        ..Document::default()
    };
    for output in [writer::markdown::write(&doc), writer::html::write(&doc)] {
        assert!(output.content.len() > 33 * 1024 * 1024);
        assert!(output.warnings.is_empty());
    }
}

#[test]
fn markdown_warns_when_row_headers_become_plain_cells() {
    let mut header = cell("Row header");
    header.header = Some(true);
    let doc = Document {
        body: vec![Block::Table {
            caption: None,
            columns: vec![],
            head: vec![],
            body: vec![vec![header]],
            footnotes: vec![],
        }],
        ..Document::default()
    };
    let output = writer::markdown::write(&doc);
    assert!(output.content.contains("Row header"));
    assert!(
        output
            .warnings
            .iter()
            .any(|w| w.code == WarningCode::UnsupportedNode)
    );
    let output = writer::html::write(&doc);
    assert!(output.content.contains("<th>Row header</th>"));
}

#[test]
fn supported_deep_inline_content_stays_writable() {
    let mut inline = Inline::Text {
        text: "Deep text".into(),
    };
    for _ in 0..99 {
        inline = Inline::Emph {
            content: vec![inline],
        };
    }
    let doc = Document {
        body: vec![Block::Paragraph {
            content: vec![inline],
        }],
        ..Document::default()
    };
    for output in [writer::markdown::write(&doc), writer::html::write(&doc)] {
        assert!(output.content.contains("Deep text"));
    }
}

#[test]
fn omitted_header_rows_do_not_emit_unused_asset_definitions() {
    let mut doc = document_with_repeated_image(vec![0x5a; 32], 2);
    let blocks = std::mem::take(&mut doc.body);
    doc.body.push(Block::Table {
        caption: None,
        columns: vec![],
        head: vec![
            vec![cell("Header")],
            vec![TableCell {
                rowspan: 1,
                colspan: 1,
                header: None,
                blocks,
            }],
        ],
        body: vec![],
        footnotes: vec![],
    });
    let output = writer::markdown::write(&doc);
    assert!(!output.content.contains("data:image/png"));
    assert!(
        output
            .warnings
            .iter()
            .any(|w| w.code == WarningCode::UnsupportedNode)
    );
}
