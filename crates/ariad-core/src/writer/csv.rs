//! Serializer from document IR to CSV.

use crate::{
    ir::{Block, ColumnSpec, Document, Inline, ListItem, TableCell},
    warning::{Warning, WarningCode},
    writer::WriteOutput,
};

/// Escapes a CSV field per RFC 4180.
///
/// If the field contains comma, double-quote, carriage return, or newline,
/// it is wrapped in double quotes and inner quotes are doubled.
#[must_use]
pub fn escape_csv_field(s: &str) -> String {
    if s.contains([',', '"', '\r', '\n']) {
        let mut out = String::with_capacity(s.len() + 2);
        out.push('"');
        for c in s.chars() {
            if c == '"' {
                out.push('"');
                out.push('"');
            } else {
                out.push(c);
            }
        }
        out.push('"');
        out
    } else {
        s.to_owned()
    }
}

/// Protects against spreadsheet formula injection by prefixing triggering characters
/// (`=`, `+`, `-`, `@`, `\t`, `\r`) with an apostrophe `'`, identical to forms export.
#[must_use]
pub fn guard_formula(s: &str) -> String {
    if s.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        format!("'{s}")
    } else {
        s.to_owned()
    }
}

/// Extracts plain text from a sequence of inline nodes.
#[must_use]
pub fn inlines_to_text(inlines: &[Inline]) -> String {
    let mut out = String::new();
    append_inlines(inlines, &mut out);
    out
}

fn append_inlines(inlines: &[Inline], out: &mut String) {
    for inline in inlines {
        match inline {
            Inline::Text { text } => out.push_str(text),
            Inline::Emph { content }
            | Inline::Strong { content }
            | Inline::Strikeout { content }
            | Inline::Superscript { content }
            | Inline::Subscript { content } => append_inlines(content, out),
            Inline::Code { text } => out.push_str(text),
            Inline::Link { content, .. } => append_inlines(content, out),
            Inline::Image { alt, .. } => out.push_str(alt),
            Inline::SoftBreak {} => out.push(' '),
            Inline::LineBreak {} => out.push('\n'),
            Inline::Math { tex, .. } => out.push_str(tex),
            Inline::FootnoteRef { .. } => {}
            Inline::Raw { text, .. } => out.push_str(text),
        }
    }
}

/// Extracts plain text from a sequence of blocks.
#[must_use]
pub fn blocks_to_text(blocks: &[Block]) -> String {
    let mut out = String::new();
    append_blocks(blocks, &mut out);
    out
}

fn append_blocks(blocks: &[Block], out: &mut String) {
    for (i, block) in blocks.iter().enumerate() {
        if i > 0 && !out.is_empty() {
            out.push('\n');
        }
        match block {
            Block::Paragraph { content } | Block::Heading { content, .. } => {
                append_inlines(content, out);
            }
            Block::Code { text, .. } => out.push_str(text),
            Block::Math { tex, .. } => out.push_str(tex),
            Block::Quote { blocks } => append_blocks(blocks, out),
            Block::List { items, .. } => append_list_items(items, out),
            Block::Table { .. } => {}
            Block::Figure { caption, .. } => append_inlines(caption, out),
            Block::Footnote { blocks, .. } => append_blocks(blocks, out),
            Block::Raw { text, .. } => out.push_str(text),
            Block::PageBreak {} => {}
        }
    }
}

fn append_list_items(items: &[ListItem], out: &mut String) {
    for (i, item) in items.iter().enumerate() {
        if i > 0 && !out.is_empty() {
            out.push('\n');
        }
        for (j, b) in item.blocks.iter().enumerate() {
            if j > 0 && !out.is_empty() {
                out.push(' ');
            }
            append_blocks(std::slice::from_ref(b), out);
        }
    }
}

/// Maximum rows allowed per CSV table.
pub const MAX_EXCEL_ROWS: usize = 1_048_576;
/// Maximum columns allowed per CSV table.
pub const MAX_EXCEL_COLS: usize = 16_384;
/// Maximum characters allowed per CSV cell.
pub const MAX_CELL_CHARS: usize = 32_767;
/// Maximum colspan or rowspan applied per cell to prevent memory thrashing.
pub const MAX_CELL_SPAN: usize = 1024;

/// Converts a single IR table into CSV text with formula-injection guarding.
#[must_use]
pub fn table_to_csv(
    columns: &[ColumnSpec],
    head: &[Vec<TableCell>],
    body: &[Vec<TableCell>],
) -> String {
    let all_rows: Vec<&Vec<TableCell>> = head
        .iter()
        .chain(body.iter())
        .take(MAX_EXCEL_ROWS)
        .collect();
    if all_rows.is_empty() {
        return String::new();
    }

    let mut occupied: Vec<Vec<bool>> = Vec::new();
    let mut grid: Vec<Vec<Option<String>>> = Vec::new();

    for (r, row_cells) in all_rows.iter().enumerate() {
        if r >= MAX_EXCEL_ROWS {
            break;
        }
        let mut c = 0;
        for cell in *row_cells {
            while c < MAX_EXCEL_COLS && is_occupied(&occupied, r, c) {
                c += 1;
            }
            if c >= MAX_EXCEL_COLS {
                break;
            }

            let raw_colspan = (cell.colspan as usize).max(1);
            let raw_rowspan = (cell.rowspan as usize).max(1);
            let max_remaining_rows = all_rows.len().saturating_sub(r).max(1);
            let colspan = raw_colspan.min(MAX_EXCEL_COLS - c).min(MAX_CELL_SPAN);
            let rowspan = raw_rowspan.min(max_remaining_rows).min(MAX_CELL_SPAN);

            mark_occupied(&mut occupied, r, c, rowspan, colspan);

            let mut raw_text = blocks_to_text(&cell.blocks);
            if raw_text.chars().count() > MAX_CELL_CHARS {
                raw_text = raw_text.chars().take(MAX_CELL_CHARS).collect();
            }
            let guarded = guard_formula(&raw_text);

            ensure_grid_size(&mut grid, r + 1, c + 1);
            grid[r][c] = Some(guarded);

            c += colspan;
        }
    }

    let col_count = columns
        .len()
        .max(grid.iter().map(Vec::len).max().unwrap_or(0));

    let mut out = String::new();
    for row in grid {
        let mut row_fields = Vec::with_capacity(col_count);
        for col_idx in 0..col_count {
            let field_text = row
                .get(col_idx)
                .and_then(|opt| opt.as_deref())
                .unwrap_or("");
            row_fields.push(escape_csv_field(field_text));
        }
        out.push_str(&row_fields.join(","));
        out.push_str("\r\n");
    }

    out
}

fn is_occupied(occupied: &[Vec<bool>], r: usize, c: usize) -> bool {
    occupied
        .get(r)
        .and_then(|row| row.get(c))
        .copied()
        .unwrap_or(false)
}

fn mark_occupied(
    occupied: &mut Vec<Vec<bool>>,
    r: usize,
    c: usize,
    rowspan: usize,
    colspan: usize,
) {
    if occupied.len() < r + rowspan {
        occupied.resize_with(r + rowspan, Vec::new);
    }
    for row in occupied.iter_mut().skip(r).take(rowspan) {
        if row.len() < c + colspan {
            row.resize(c + colspan, false);
        }
        for cell in row.iter_mut().skip(c).take(colspan) {
            *cell = true;
        }
    }
}

fn ensure_grid_size(grid: &mut Vec<Vec<Option<String>>>, rows: usize, cols: usize) {
    if grid.len() < rows {
        grid.resize_with(rows, Vec::new);
    }
    for row in grid.iter_mut() {
        if row.len() < cols {
            row.resize_with(cols, || None);
        }
    }
}

/// Recursively collects all [`Block::Table`] references from a slice of blocks.
pub fn find_tables(blocks: &[Block]) -> Vec<&Block> {
    let mut tables = Vec::new();
    collect_tables(blocks, &mut tables);
    tables
}

fn collect_tables<'a>(blocks: &'a [Block], out: &mut Vec<&'a Block>) {
    for block in blocks {
        match block {
            b @ Block::Table { .. } => out.push(b),
            Block::Quote { blocks } => collect_tables(blocks, out),
            Block::List { items, .. } => {
                for item in items {
                    collect_tables(&item.blocks, out);
                }
            }
            Block::Footnote { blocks, .. } => collect_tables(blocks, out),
            _ => {}
        }
    }
}

/// Serializes an IR [`Document`] to CSV format.
///
/// If tables are present, each table is emitted (separated by blank lines if multiple).
/// If no tables are found, document blocks are emitted row by row as a text dump,
/// and a `no_tables_found` warning is issued.
#[must_use]
pub fn write(document: &Document) -> WriteOutput {
    let tables = find_tables(&document.body);
    if tables.is_empty() {
        let mut content = String::new();
        for block in &document.body {
            let text = blocks_to_text(std::slice::from_ref(block));
            if !text.is_empty() {
                let guarded = guard_formula(&text);
                content.push_str(&escape_csv_field(&guarded));
                content.push_str("\r\n");
            }
        }
        return WriteOutput {
            content,
            warnings: vec![Warning::new(
                WarningCode::NoTablesFound,
                "no tables found in document; generated text dump",
            )],
        };
    }

    let mut content = String::new();
    for (idx, table_block) in tables.iter().enumerate() {
        if idx > 0 {
            content.push_str("\r\n");
        }
        if let Block::Table {
            columns,
            head,
            body,
            ..
        } = table_block
        {
            content.push_str(&table_to_csv(columns, head, body));
        }
    }

    WriteOutput {
        content,
        warnings: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{Alignment, ColumnSpec, TableCell};

    fn make_cell(text: &str, rowspan: u32, colspan: u32) -> TableCell {
        TableCell {
            rowspan,
            colspan,
            header: None,
            blocks: vec![Block::Paragraph {
                content: vec![Inline::Text {
                    text: text.to_owned(),
                }],
            }],
        }
    }

    #[test]
    fn escapes_commas_quotes_and_newlines() {
        assert_eq!(escape_csv_field("simple"), "simple");
        assert_eq!(escape_csv_field("with,comma"), "\"with,comma\"");
        assert_eq!(escape_csv_field("with\"quote"), "\"with\"\"quote\"");
        assert_eq!(escape_csv_field("with\nnewline"), "\"with\nnewline\"");
    }

    #[test]
    fn guards_formula_injection() {
        assert_eq!(guard_formula("=1+1"), "'=1+1");
        assert_eq!(guard_formula("+CMD"), "'+CMD");
        assert_eq!(guard_formula("-5"), "'-5");
        assert_eq!(guard_formula("@SUM(A1)"), "'@SUM(A1)");
        assert_eq!(guard_formula("\tTAB"), "'\tTAB");
        assert_eq!(guard_formula("\rCR"), "'\rCR");
        assert_eq!(guard_formula("Normal Text"), "Normal Text");
    }

    #[test]
    fn writes_table_with_spans() {
        let columns = vec![
            ColumnSpec {
                align: Alignment::Default,
            },
            ColumnSpec {
                align: Alignment::Default,
            },
        ];
        let head = vec![vec![
            make_cell("Header A", 1, 1),
            make_cell("=Formula", 1, 1),
        ]];
        let body = vec![vec![
            make_cell("Spanning 2 cols", 1, 2),
            // Covered cell omitted in IR row
        ]];
        let doc = Document {
            body: vec![Block::Table {
                caption: None,
                columns,
                head,
                body,
                footnotes: Vec::new(),
            }],
            ..Default::default()
        };
        let output = write(&doc);
        assert!(output.warnings.is_empty());
        assert_eq!(output.content, "Header A,'=Formula\r\nSpanning 2 cols,\r\n");
    }

    #[test]
    fn text_dump_emits_warning_when_no_tables() {
        let doc = Document {
            body: vec![
                Block::Heading {
                    level: 1,
                    content: vec![Inline::Text {
                        text: "Title".into(),
                    }],
                },
                Block::Paragraph {
                    content: vec![Inline::Text {
                        text: "=Formula Para".into(),
                    }],
                },
            ],
            ..Default::default()
        };
        let output = write(&doc);
        assert_eq!(output.warnings.len(), 1);
        assert_eq!(output.warnings[0].code, WarningCode::NoTablesFound);
        assert_eq!(output.content, "Title\r\n'=Formula Para\r\n");
    }

    #[test]
    fn bounded_colspan_and_rowspan_prevent_allocation_overflow() {
        let doc = Document {
            body: vec![Block::Table {
                caption: None,
                columns: vec![ColumnSpec {
                    align: Alignment::Default,
                }],
                head: vec![],
                body: vec![vec![
                    make_cell("Safe Huge Cell", u32::MAX, u32::MAX),
                    make_cell("Next", 1, 1),
                ]],
                footnotes: Vec::new(),
            }],
            ..Default::default()
        };

        let output = write(&doc);
        assert!(output.warnings.is_empty());
        assert!(output.content.contains("Safe Huge Cell"));
    }

    #[test]
    fn overlong_cell_text_truncated() {
        let long_text = "B".repeat(40_000);
        let doc = Document {
            body: vec![Block::Table {
                caption: None,
                columns: vec![ColumnSpec {
                    align: Alignment::Default,
                }],
                head: vec![],
                body: vec![vec![make_cell(&long_text, 1, 1)]],
                footnotes: Vec::new(),
            }],
            ..Default::default()
        };

        let output = write(&doc);
        assert!(output.content.len() >= MAX_CELL_CHARS);
        // Ensure it doesn't contain the full 40,000 characters
        assert!(!output.content.contains(&"B".repeat(MAX_CELL_CHARS + 1)));
    }

    #[test]
    fn formula_guard_matches_interchange_characters() {
        // Cross-check: guard prefixes with an apostrophe for all standard injection prefixes
        for prefix in ['=', '+', '-', '@', '\t', '\r'] {
            let input = format!("{prefix}cmd|'/C calc'!A0");
            let guarded = guard_formula(&input);
            assert!(
                guarded.starts_with('\''),
                "prefix {prefix:?} was not guarded"
            );
            assert_eq!(&guarded[1..], &input);
        }
        // Negative numbers are also guarded (fidelity cost noted in F5)
        assert_eq!(guard_formula("-5"), "'-5");
        assert_eq!(guard_formula("5"), "5");
    }
}
