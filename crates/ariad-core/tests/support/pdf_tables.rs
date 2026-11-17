//! Helpers shared by the PDF table detection tests.
#![allow(dead_code)]

use ariad_core::writer::csv::find_tables;
use ariad_core::{
    ir::{Block, Document, Inline, TableCell},
    limits::Limits,
    reader::pdf::{self, ReadOptions},
};

pub const DETECT: ReadOptions = ReadOptions {
    detect_tables: true,
};

pub fn fixture(name: &str) -> Vec<u8> {
    let path = format!("{}/../../fixtures/pdf/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|error| panic!("{path}: {error}"))
}

pub fn inline_text(inlines: &[Inline], out: &mut String) {
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

pub fn cell_text(cell: &TableCell) -> String {
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
pub fn tables(name: &str, options: &ReadOptions) -> Vec<Vec<Vec<String>>> {
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

pub fn read(name: &str, options: &ReadOptions) -> Document {
    pdf::read_with_options(&fixture(name), &Limits::local(), options)
        .unwrap()
        .document
}

pub fn rows(rows: &[&[&str]]) -> Vec<Vec<String>> {
    rows.iter()
        .map(|row| row.iter().map(|cell| (*cell).to_owned()).collect())
        .collect()
}

pub fn paragraph_text(block: &Block) -> Option<String> {
    let Block::Paragraph { content, .. } = block else {
        return None;
    };
    let mut text = String::new();
    inline_text(content, &mut text);
    Some(text)
}

/// Page number of every table block of the document, in reading order.
pub fn table_pages(document: &Document) -> Vec<u32> {
    let layout = document.layout.as_ref().expect("layout index");
    find_tables(&document.body)
        .into_iter()
        .map(|table| {
            layout
                .blocks
                .get(table.id())
                .expect("table position")
                .page_no
        })
        .collect()
}
