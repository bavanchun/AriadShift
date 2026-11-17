//! Ruled-table detection on real and generated PDFs.

use ariad_core::writer::csv::find_tables;
use ariad_core::{
    ir::{Block, Document, Inline, TableCell},
    limits::Limits,
    reader::pdf::{self, ReadError, ReadOptions},
    writer::{csv, xlsx},
};

const DETECT: ReadOptions = ReadOptions {
    detect_tables: true,
};

fn fixture(name: &str) -> Vec<u8> {
    let path = format!("{}/../../fixtures/pdf/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|error| panic!("{path}: {error}"))
}

fn inline_text(inlines: &[Inline], out: &mut String) {
    for inline in inlines {
        match inline {
            Inline::Text { text } | Inline::Code { text } => out.push_str(text),
            Inline::Styled { content, .. }
            | Inline::Emph { content }
            | Inline::Strong { content }
            | Inline::Strikeout { content }
            | Inline::Superscript { content }
            | Inline::Subscript { content }
            | Inline::Link { content, .. } => inline_text(content, out),
            _ => {}
        }
    }
}

fn cell_text(cell: &TableCell) -> String {
    let mut text = String::new();
    for block in &cell.blocks {
        if let Block::Paragraph { content, .. } = block {
            if !text.is_empty() {
                text.push(' ');
            }
            inline_text(content, &mut text);
        }
    }
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Cell text of every table in reading order: `[table][row][cell]`, head rows first.
fn tables(name: &str, options: &ReadOptions) -> Vec<Vec<Vec<String>>> {
    let output = pdf::read_with_options(&fixture(name), &Limits::local(), options).unwrap();
    find_tables(&output.document.body)
        .into_iter()
        .map(|table| {
            let Block::Table { head, body, .. } = table else {
                unreachable!()
            };
            head.iter()
                .chain(body)
                .map(|row| row.iter().map(cell_text).collect())
                .collect()
        })
        .collect()
}

fn read(name: &str, options: &ReadOptions) -> Document {
    pdf::read_with_options(&fixture(name), &Limits::local(), options)
        .unwrap()
        .document
}

fn rows(rows: &[&[&str]]) -> Vec<Vec<String>> {
    rows.iter()
        .map(|row| row.iter().map(|cell| (*cell).to_owned()).collect())
        .collect()
}

fn paragraph_text(block: &Block) -> Option<String> {
    let Block::Paragraph { content, .. } = block else {
        return None;
    };
    let mut text = String::new();
    inline_text(content, &mut text);
    Some(text)
}

const PROSE: [&str; 5] = [
    "pd-en-gao-08-35-highlights.pdf",
    "pd-vi-decree-39-2022.pdf",
    "pd-vi-ip-law-155-2025.pdf",
    "pd-vi-qd-1922-2026.pdf",
    "arxiv-multilingual-open-text-2201-05609-v2.pdf",
];

#[test]
fn detection_is_off_by_default() {
    for name in ["ruled-tables.pdf", "en-map-room.pdf", "vi-garden-route.pdf"] {
        let default = pdf::read(&fixture(name), &Limits::local()).unwrap();
        assert!(find_tables(&default.document.body).is_empty(), "{name}");
        let explicit =
            pdf::read_with_options(&fixture(name), &Limits::local(), &ReadOptions::default())
                .unwrap();
        assert_eq!(default.document, explicit.document, "{name}");
        assert_eq!(default.warnings, explicit.warnings, "{name}");
    }
}

#[test]
fn ruled_tables_become_table_blocks_with_merged_cells_and_a_head() {
    let document = read("ruled-tables.pdf", &DETECT);
    let found = tables("ruled-tables.pdf", &DETECT);
    assert_eq!(
        found,
        vec![
            rows(&[
                &["Region", "Units", "Revenue"],
                &["North", "120", "4,500"],
                &["Central region", "95", "3,210"],
                &["East", "143", "5,008"],
            ]),
            rows(&[&["Plan", "Actual"], &["Team A", "10", "12"], &["11", "13"]]),
        ]
    );

    let tables = find_tables(&document.body);
    let Block::Table {
        columns,
        head,
        body,
        ..
    } = tables[0]
    else {
        unreachable!()
    };
    assert_eq!(columns.len(), 3);
    // The heavier rule under the first row makes it the head.
    assert_eq!(head.len(), 1);
    assert_eq!(body.len(), 3);
    assert!(head[0].iter().all(|cell| cell.header == Some(true)));
    assert!(body.iter().flatten().all(|cell| cell.header.is_none()));

    let Block::Table {
        columns,
        head,
        body,
        ..
    } = tables[1]
    else {
        unreachable!()
    };
    assert_eq!(columns.len(), 3);
    // Bold first row against regular body text.
    assert_eq!(head.len(), 1);
    let spans = |cell: &TableCell| (cell.rowspan, cell.colspan);
    assert_eq!(
        head[0].iter().map(spans).collect::<Vec<_>>(),
        [(1, 2), (1, 1)]
    );
    assert_eq!(
        body[0].iter().map(spans).collect::<Vec<_>>(),
        [(2, 1), (1, 1), (1, 1)]
    );
    assert_eq!(
        body[1].iter().map(spans).collect::<Vec<_>>(),
        [(1, 1), (1, 1)]
    );
}

#[test]
fn tables_sit_between_the_paragraphs_around_them() {
    let document = read("ruled-tables.pdf", &DETECT);
    let kinds: Vec<String> = document
        .body
        .iter()
        .map(|block| match block {
            Block::Table { .. } => "table".to_owned(),
            other => paragraph_text(other).unwrap_or_else(|| "other".to_owned()),
        })
        .collect();
    assert_eq!(
        kinds[0],
        "Quarterly figures are listed in the tables below. Each table is ruled on every side."
    );
    assert_eq!(kinds[1], "table");
    assert_eq!(kinds[2], "Merged cells follow.");
    assert_eq!(kinds[3], "table");
    // Page 2: nothing was detected, so every line of its shapes stays text.
    assert_eq!(kinds[4], "Shapes that are not tables.");
    let rest = kinds[5..].join("\n");
    for expected in ["L00", "L12", "Note: figures are provisional."] {
        assert!(rest.contains(expected), "{expected} missing from {rest:?}");
    }
    assert!(kinds[5..].iter().all(|kind| kind != "table"));
}

#[test]
fn table_blocks_are_positioned_in_the_layout_index() {
    let document = read("ruled-tables.pdf", &DETECT);
    let layout = document.layout.as_ref().expect("layout index");
    let positions: Vec<_> = document
        .body
        .iter()
        .filter(|block| matches!(block, Block::Table { .. }))
        .map(|block| layout.blocks.get(block.id()).expect("table position"))
        .collect();
    assert_eq!(positions.len(), 2);
    assert!(positions.iter().all(|position| position.page_no == 1));
    // Table A spans x 72..432 and y 92..188 from the top of a 792 pt page.
    let first = &positions[0].bbox;
    assert!((first.left - 72.0).abs() < 1.0 && (first.right - 432.0).abs() < 1.0);
    assert!((first.top - 92.0).abs() < 1.0 && (first.bottom - 188.0).abs() < 1.0);
}

#[test]
fn csv_and_xlsx_writers_take_the_detected_tables_unchanged() {
    let document = read("ruled-tables.pdf", &DETECT);
    let output = csv::write(&document);
    assert_eq!(
        output.content,
        "Region,Units,Revenue\r\nNorth,120,\"4,500\"\r\nCentral region,95,\"3,210\"\r\n\
         East,143,\"5,008\"\r\n\r\nPlan,,Actual\r\nTeam A,10,12\r\n,11,13\r\n"
    );
    assert!(output.warnings.is_empty(), "{:?}", output.warnings);
    let workbook = xlsx::write(&document).unwrap();
    assert!(workbook.bytes.starts_with(b"PK"));
}

#[test]
fn tables_of_real_documents_match_the_page() {
    // Fixtures generated by Typst: one ruled table each.
    assert_eq!(
        tables("en-map-room.pdf", &DETECT),
        vec![rows(&[
            &["Archive item", "Recorded detail"],
            &["Street plan", "Survey date"],
            &["Overlay", "Transparent sheet"],
            &["Map weight", "Brass"],
        ])]
    );
    assert_eq!(
        tables("vi-garden-route.pdf", &DETECT),
        vec![rows(&[
            &["Điểm dừng", "Phút đi bộ"],
            &["Bến sông", "8"],
            &["Vườn đọc", "14"],
            &["Ghi chú", "Ước lượng trong ngày nắng"],
        ])]
    );
}

#[test]
fn tables_of_a_research_paper_match_the_rendered_page() {
    let found = tables("arxiv-tableformer-2203-01017-v1.pdf", &DETECT);
    // Figure 1c, the two tables of Figure 5b and the structure grid of Figure 6. The two tables
    // of Figure 5b share a page in separate columns, and the left one is read first.
    assert_eq!(found.len(), 4);
    assert_eq!(
        found[2],
        rows(&[
            &[
                "",
                "Shares (in millions)",
                "Weighted Average Grant Date Fair Value"
            ],
            &["", "RSUs", "PSUs", "RSUs", "PSUs"],
            &["Nonvested on January 1", "1.1", "0.3", "90.10 $", "$ 91.19"],
            &["Granted", "0.5", "0.1", "117.44", "122.41"],
            &["Vested", "(0.5)", "(0.1)", "87.08", "81.14"],
            &["Canceled or forfeited", "(0.1)", "—", "102.01", "92.18"],
            &[
                "Nonvested on December 31",
                "1.0",
                "0.3",
                "104.85 $",
                "$ 104.51"
            ],
        ])
    );
    assert_eq!(found[1][0], ["", "", "論文ファイル", "参考文献"]);
    assert_eq!(found[1].len(), 10);
}

#[test]
fn prose_documents_read_the_same_with_detection_on() {
    for name in PROSE {
        let off = pdf::read(&fixture(name), &Limits::local()).unwrap();
        let on = pdf::read_with_options(&fixture(name), &Limits::local(), &DETECT).unwrap();
        assert!(find_tables(&on.document.body).is_empty(), "{name}");
        assert_eq!(off.document, on.document, "{name}");
        assert_eq!(off.warnings, on.warnings, "{name}");
    }
}

/// One-based page numbers that hold a detected table, one entry per table.
fn table_pages(name: &str) -> Vec<u32> {
    let document = read(name, &DETECT);
    let layout = document.layout.as_ref().expect("layout index");
    document
        .body
        .iter()
        .filter(|block| matches!(block, Block::Table { .. }))
        .map(|block| {
            layout
                .blocks
                .get(block.id())
                .expect("table position")
                .page_no
        })
        .collect()
}

/// Text of every top-level paragraph of the document, joined by newlines.
fn body_text(document: &Document) -> String {
    let paragraphs: Vec<String> = document.body.iter().filter_map(paragraph_text).collect();
    paragraphs.join("\n")
}

#[test]
fn page_frames_around_columns_are_not_tables() {
    // Page 1 draws the frame as segments, page 2 as one stroked rectangle.
    let pages = table_pages("ruled-table-traps.pdf");
    assert!(!pages.contains(&1), "page 1 gave a table");
    assert!(!pages.contains(&2), "page 2 gave a table");
    let text = body_text(&read("ruled-table-traps.pdf", &DETECT));
    for expected in [
        "The harbour committee met",
        "The treasurer reported",
        "mooring rings",
    ] {
        assert!(text.contains(expected), "{expected} missing from {text:?}");
    }
}

#[test]
fn a_sideways_table_is_not_read_as_a_table() {
    // Page 3 draws a 3 by 3 grid under a quarter-turn matrix. Read as a table it would come out
    // transposed and reversed, so its text stays text.
    let pages = table_pages("ruled-table-traps.pdf");
    assert!(!pages.contains(&3), "page 3 gave a table");
    let text = body_text(&read("ruled-table-traps.pdf", &DETECT));
    assert!(text.contains("The table on this page is drawn sideways."));
    for cell in ["A1", "B2", "C3"] {
        assert!(text.contains(cell), "{cell} missing from {text:?}");
    }
}

#[test]
fn a_table_in_the_right_column_follows_the_left_column() {
    // Page 4: a title across both columns, prose on the left, and on the right a table with
    // prose under it.
    let document = read("ruled-table-traps.pdf", &DETECT);
    let layout = document.layout.as_ref().expect("layout index");
    let kinds: Vec<String> = document
        .body
        .iter()
        .filter(|block| {
            layout
                .blocks
                .get(block.id())
                .is_some_and(|p| p.page_no == 4)
        })
        .map(|block| match block {
            Block::Table { .. } => "table".to_owned(),
            Block::Heading { .. } => "heading".to_owned(),
            other => paragraph_text(other).unwrap_or_else(|| "other".to_owned()),
        })
        .collect();
    let start = |text: &str| text.chars().take(24).collect::<String>();
    let order: Vec<String> = kinds.iter().map(|kind| start(kind)).collect();
    let table = order
        .iter()
        .position(|kind| kind == "table")
        .expect("a table");
    let after = |needle: &str| order.iter().position(|kind| kind.starts_with(needle));
    assert_eq!(order[0], "heading");
    // Every left-column paragraph comes before the table, the right-column prose after it.
    assert!(after("The harbour committee").unwrap() < table, "{order:?}");
    assert!(after("A survey of the lower").unwrap() < table, "{order:?}");
    assert!(
        after("The treasurer reported").unwrap() > table,
        "{order:?}"
    );
}

#[test]
fn the_block_limit_still_applies_to_detected_tables() {
    let limits = Limits {
        max_blocks: 3,
        ..Limits::local()
    };
    let result = pdf::read_with_options(&fixture("ruled-tables.pdf"), &limits, &DETECT);
    assert!(matches!(result, Err(ReadError::TooManyBlocks { limit: 3 })));
}

/// Prints every detected table of the fixtures named in `PDF_FIXTURES` (comma separated); run
/// with `--ignored --nocapture`. Unreadable files are reported, not failed.
#[test]
#[ignore = "measurement dump"]
fn dump_tables() {
    let names = std::env::var("PDF_FIXTURES").unwrap_or_default();
    for name in names.split(',').filter(|name| !name.is_empty()) {
        let output = pdf::read_with_options(&fixture(name), &Limits::local(), &DETECT);
        let Ok(output) = output else {
            println!("== {name}: unreadable");
            continue;
        };
        let found = find_tables(&output.document.body);
        println!("== {name}: {} tables", found.len());
        for (index, table) in tables(name, &DETECT).iter().enumerate() {
            let cells: usize = table.iter().map(Vec::len).sum();
            println!("-- table {index}: {} rows, {cells} cells", table.len());
            for row in table {
                println!("   {}", row.join(" | "));
            }
        }
    }
}
