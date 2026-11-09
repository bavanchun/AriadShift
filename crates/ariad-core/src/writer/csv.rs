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

pub use super::xlsx::{
    MAX_CELL_CHARS, MAX_CELL_SPAN, MAX_EXCEL_COLS, MAX_EXCEL_ROWS, MAX_OUTPUT_BYTES,
    MAX_SHEETS_COUNT,
};

/// Maximum number of per-cell clamp warnings emitted in a single conversion.
pub const MAX_CLAMP_WARNINGS: usize = 64;

/// Converts a single IR table into CSV text with formula-injection guarding,
/// collecting warnings for row/column truncation, span clamping, or text cutoffs.
#[must_use]
pub fn table_to_csv_with_warnings(
    columns: &[ColumnSpec],
    head: &[Vec<TableCell>],
    body: &[Vec<TableCell>],
    warnings: &mut Vec<Warning>,
) -> String {
    let total_rows = head.len() + body.len();
    if total_rows > MAX_EXCEL_ROWS {
        warnings.push(Warning::new(
            WarningCode::UnsupportedNode,
            format!("table has {total_rows} rows; truncated to maximum {MAX_EXCEL_ROWS}"),
        ));
    }

    let all_rows: Vec<&Vec<TableCell>> = head
        .iter()
        .chain(body.iter())
        .take(MAX_EXCEL_ROWS)
        .collect();
    if all_rows.is_empty() {
        warnings.push(Warning::new(
            WarningCode::UnsupportedNode,
            "table has no rows; produced empty CSV",
        ));
        return String::new();
    }

    let mut occupied: Vec<Vec<bool>> = Vec::new();
    let mut grid: Vec<Vec<Option<String>>> = Vec::new();
    let mut row_col_exceeded = false;
    let mut clamp_warnings_count = 0;
    let mut char_trunc_warnings_count = 0;

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
                if !row_col_exceeded {
                    warnings.push(Warning::new(
                        WarningCode::UnsupportedNode,
                        format!(
                            "table columns exceed maximum {MAX_EXCEL_COLS}; excess columns dropped"
                        ),
                    ));
                    row_col_exceeded = true;
                }
                break;
            }

            let raw_colspan = (cell.colspan as usize).max(1);
            let raw_rowspan = (cell.rowspan as usize).max(1);
            let max_remaining_rows = all_rows.len().saturating_sub(r).max(1);
            let colspan = raw_colspan.min(MAX_EXCEL_COLS - c).min(MAX_CELL_SPAN);
            let rowspan = raw_rowspan.min(max_remaining_rows).min(MAX_CELL_SPAN);

            if raw_colspan > colspan && clamp_warnings_count < MAX_CLAMP_WARNINGS {
                clamp_warnings_count += 1;
                warnings.push(Warning::new(
                    WarningCode::UnsupportedNode,
                    format!("cell colspan {raw_colspan} clamped to {colspan}"),
                ));
            }
            if raw_rowspan > rowspan && clamp_warnings_count < MAX_CLAMP_WARNINGS {
                clamp_warnings_count += 1;
                warnings.push(Warning::new(
                    WarningCode::UnsupportedNode,
                    format!("cell rowspan {raw_rowspan} clamped to {rowspan}"),
                ));
            }

            mark_occupied(&mut occupied, r, c, rowspan, colspan);

            let mut raw_text = blocks_to_text(&cell.blocks);
            let char_count = raw_text.chars().count();
            if char_count > MAX_CELL_CHARS {
                if char_trunc_warnings_count < MAX_CLAMP_WARNINGS {
                    char_trunc_warnings_count += 1;
                    warnings.push(Warning::new(
                        WarningCode::UnsupportedNode,
                        format!(
                            "cell content of {char_count} characters truncated to {MAX_CELL_CHARS} characters"
                        ),
                    ));
                }
                raw_text = raw_text.chars().take(MAX_CELL_CHARS).collect();
            }
            let guarded = guard_formula(&raw_text);

            if grid.len() <= r {
                grid.resize_with(r + 1, Vec::new);
            }
            let grid_row = &mut grid[r];
            if grid_row.len() <= c {
                grid_row.resize_with(c + 1, || None);
            }
            grid_row[c] = Some(guarded);

            c += colspan;
        }
    }

    let declared_col_count = columns.len();
    let mut out = String::new();
    for row in grid {
        let effective_cols = if declared_col_count > 0 {
            declared_col_count.max(row.len())
        } else {
            row.len()
        };
        let mut row_fields = Vec::with_capacity(effective_cols);
        for col_idx in 0..effective_cols {
            let field_text = row
                .get(col_idx)
                .and_then(|opt| opt.as_deref())
                .unwrap_or("");
            row_fields.push(escape_csv_field(field_text));
        }
        let line = row_fields.join(",");
        if out.len() + line.len() + 2 > MAX_OUTPUT_BYTES {
            warnings.push(Warning::new(
                WarningCode::OutputTruncated,
                format!(
                    "CSV output exceeded maximum size limit of {MAX_OUTPUT_BYTES} bytes; content truncated at record boundary"
                ),
            ));
            return out;
        }
        out.push_str(&line);
        out.push_str("\r\n");
    }

    out
}

/// Converts a single IR table into CSV text with formula-injection guarding.
#[must_use]
pub fn table_to_csv(
    columns: &[ColumnSpec],
    head: &[Vec<TableCell>],
    body: &[Vec<TableCell>],
) -> String {
    let mut warnings = Vec::new();
    table_to_csv_with_warnings(columns, head, body, &mut warnings)
}

fn is_occupied(occupied: &[Vec<bool>], r: usize, c: usize) -> bool {
    if occupied.is_empty() {
        return false;
    }
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
    if rowspan <= 1 && colspan <= 1 && occupied.is_empty() {
        return;
    }
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
    let mut warnings = Vec::new();
    let mut content = String::new();

    if document.body.is_empty() {
        warnings.push(Warning::new(
            WarningCode::UnsupportedNode,
            "document has no content; produced empty CSV",
        ));
        return WriteOutput { content, warnings };
    }

    let tables = find_tables(&document.body);

    if tables.is_empty() {
        warnings.push(Warning::new(
            WarningCode::NoTablesFound,
            "no tables found in document; generated text dump",
        ));

        if document.body.len() > MAX_EXCEL_ROWS {
            warnings.push(Warning::new(
                WarningCode::UnsupportedNode,
                format!(
                    "document has {} blocks; text dump truncated to maximum {} rows",
                    document.body.len(),
                    MAX_EXCEL_ROWS
                ),
            ));
        }

        let mut char_trunc_warnings_count = 0;
        for (r, block) in document.body.iter().enumerate() {
            if r >= MAX_EXCEL_ROWS {
                break;
            }
            let mut text = blocks_to_text(std::slice::from_ref(block));
            if !text.is_empty() {
                let char_count = text.chars().count();
                if char_count > MAX_CELL_CHARS {
                    if char_trunc_warnings_count < MAX_CLAMP_WARNINGS {
                        char_trunc_warnings_count += 1;
                        warnings.push(Warning::new(
                            WarningCode::UnsupportedNode,
                            format!(
                                "cell content of {char_count} characters truncated to {MAX_CELL_CHARS} characters"
                            ),
                        ));
                    }
                    text = text.chars().take(MAX_CELL_CHARS).collect();
                }
                let guarded = guard_formula(&text);
                let field = escape_csv_field(&guarded);
                if content.len() + field.len() + 2 > MAX_OUTPUT_BYTES {
                    warnings.push(Warning::new(
                        WarningCode::UnsupportedNode,
                        format!(
                            "CSV output exceeded maximum size limit of {MAX_OUTPUT_BYTES} bytes; content truncated"
                        ),
                    ));
                    let remaining = MAX_OUTPUT_BYTES.saturating_sub(content.len());
                    let mut end = remaining.min(field.len());
                    while !field.is_char_boundary(end) {
                        end = end.saturating_sub(1);
                    }
                    content.push_str(&field[..end]);
                    break;
                }
                content.push_str(&field);
                content.push_str("\r\n");
            }
        }
    } else {
        let max_tables = MAX_SHEETS_COUNT;
        let tables_to_process = if tables.len() > max_tables {
            warnings.push(Warning::new(
                WarningCode::UnsupportedNode,
                format!(
                    "document has {} tables; capped at maximum {} tables",
                    tables.len(),
                    max_tables
                ),
            ));
            &tables[..max_tables]
        } else {
            &tables[..]
        };

        for (idx, table_block) in tables_to_process.iter().enumerate() {
            if content.len() >= MAX_OUTPUT_BYTES {
                break;
            }
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
                let table_csv = table_to_csv_with_warnings(columns, head, body, &mut warnings);
                if content.len() + table_csv.len() > MAX_OUTPUT_BYTES {
                    warnings.push(Warning::new(
                        WarningCode::UnsupportedNode,
                        format!(
                            "CSV output exceeded maximum size limit of {MAX_OUTPUT_BYTES} bytes; content truncated"
                        ),
                    ));
                    let remaining = MAX_OUTPUT_BYTES.saturating_sub(content.len());
                    let mut end = remaining.min(table_csv.len());
                    while !table_csv.is_char_boundary(end) {
                        end = end.saturating_sub(1);
                    }
                    content.push_str(&table_csv[..end]);
                    break;
                }
                content.push_str(&table_csv);
            }
        }
    }

    if content.len() > MAX_OUTPUT_BYTES {
        warnings.push(Warning::new(
            WarningCode::UnsupportedNode,
            format!(
                "CSV output exceeded maximum size limit of {MAX_OUTPUT_BYTES} bytes; content truncated"
            ),
        ));
        let mut end = MAX_OUTPUT_BYTES;
        while !content.is_char_boundary(end) {
            end = end.saturating_sub(1);
        }
        content.truncate(end);
    }

    WriteOutput { content, warnings }
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
        assert!(
            output
                .warnings
                .iter()
                .any(|w| w.message.contains("clamped"))
        );
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
        assert!(
            output
                .warnings
                .iter()
                .any(|w| w.message.contains("truncated"))
        );
        assert!(output.content.len() >= MAX_CELL_CHARS);
        // Ensure it doesn't contain the full 40,000 characters
        assert!(!output.content.contains(&"B".repeat(MAX_CELL_CHARS + 1)));
    }

    #[test]
    fn col_cap_emits_warning_when_columns_exceed_limit() {
        let wide_row: Vec<TableCell> = (0..MAX_EXCEL_COLS + 5)
            .map(|_| make_cell("c", 1, 1))
            .collect();
        let doc = Document {
            body: vec![Block::Table {
                caption: None,
                columns: vec![],
                head: vec![],
                body: vec![wide_row],
                footnotes: Vec::new(),
            }],
            ..Default::default()
        };
        let output = write(&doc);
        assert!(output.warnings.iter().any(|w| {
            w.code == WarningCode::UnsupportedNode
                && w.message
                    .contains("table columns exceed maximum 16384; excess columns dropped")
        }));
    }

    #[test]
    fn row_cap_emits_warning_when_rows_exceed_limit() {
        let single_row = vec![make_cell("r", 1, 1)];
        let body = vec![single_row; MAX_EXCEL_ROWS + 2];
        let doc = Document {
            body: vec![Block::Table {
                caption: None,
                columns: vec![],
                head: vec![],
                body,
                footnotes: Vec::new(),
            }],
            ..Default::default()
        };
        let output = write(&doc);
        assert!(output.warnings.iter().any(|w| {
            w.code == WarningCode::UnsupportedNode
                && w.message
                    .contains("table has 1048578 rows; truncated to maximum 1048576")
        }));
    }

    #[test]
    fn table_cap_emits_warning_when_tables_exceed_limit() {
        let tables: Vec<Block> = (0..260)
            .map(|i| Block::Table {
                caption: Some(vec![Inline::Text {
                    text: format!("Table {i}"),
                }]),
                columns: vec![ColumnSpec {
                    align: Alignment::Default,
                }],
                head: vec![],
                body: vec![vec![make_cell("data", 1, 1)]],
                footnotes: Vec::new(),
            })
            .collect();
        let doc = Document {
            body: tables,
            ..Default::default()
        };

        let output = write(&doc);
        assert!(output.warnings.iter().any(|w| {
            w.code == WarningCode::UnsupportedNode
                && w.message
                    .contains("document has 260 tables; capped at maximum 255 tables")
        }));
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

    #[test]
    fn empty_table_emits_warning() {
        let mut warnings = Vec::new();
        let res = table_to_csv_with_warnings(&[], &[], &[], &mut warnings);
        assert!(res.is_empty());
        assert!(warnings.iter().any(|w| {
            w.code == WarningCode::UnsupportedNode
                && w.message.contains("table has no rows; produced empty CSV")
        }));
    }

    #[test]
    fn empty_document_emits_warning() {
        let doc = Document::default();
        let output = write(&doc);
        assert!(output.content.is_empty());
        assert!(output.warnings.iter().any(|w| {
            w.code == WarningCode::UnsupportedNode
                && w.message
                    .contains("document has no content; produced empty CSV")
        }));
    }

    #[test]
    fn clamp_warnings_capped_at_64() {
        let rows: Vec<Vec<TableCell>> = (0..70).map(|_| vec![make_cell("cell", 1, 2000)]).collect();
        let doc = Document {
            body: vec![Block::Table {
                caption: None,
                columns: vec![],
                head: vec![],
                body: rows,
                footnotes: Vec::new(),
            }],
            ..Default::default()
        };
        let output = write(&doc);
        let clamp_count = output
            .warnings
            .iter()
            .filter(|w| w.message.contains("clamped"))
            .count();
        assert_eq!(clamp_count, MAX_CLAMP_WARNINGS);
    }

    #[test]
    fn csv_output_truncated_when_exceeding_max_bytes() {
        let chunk = "A".repeat(30_000);
        let rows: Vec<Vec<TableCell>> = (0..1800).map(|_| vec![make_cell(&chunk, 1, 1)]).collect();
        let doc = Document {
            body: vec![Block::Table {
                caption: None,
                columns: vec![],
                head: vec![],
                body: rows,
                footnotes: Vec::new(),
            }],
            ..Default::default()
        };
        let output = write(&doc);
        assert!(output.content.len() <= MAX_OUTPUT_BYTES);
        assert!(output.content.ends_with("\r\n"));
        assert!(output.warnings.iter().any(|w| {
            w.code == WarningCode::OutputTruncated
                && w.message.contains("CSV output exceeded maximum size limit")
        }));
    }
}
