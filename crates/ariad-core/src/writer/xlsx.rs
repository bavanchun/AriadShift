//! Serializer from document IR to OpenXML spreadsheet (XLSX).

use std::{
    collections::HashSet,
    io::{Cursor, Write as _},
};

use zip::{ZipWriter, write::SimpleFileOptions};

use crate::{
    ir::{Block, Document, TableCell},
    warning::{Warning, WarningCode},
    writer::{
        WriteBinaryOutput,
        csv::{blocks_to_text, find_tables, inlines_to_text},
    },
};

/// Maximum rows allowed per worksheet in Excel OpenXML standard.
pub const MAX_EXCEL_ROWS: usize = 1_048_576;
/// Maximum columns allowed per worksheet in Excel OpenXML standard.
pub const MAX_EXCEL_COLS: usize = 16_384;
/// Maximum characters allowed per cell in Excel OpenXML standard.
pub const MAX_CELL_CHARS: usize = 32_767;
/// Maximum length in Unicode characters for an Excel sheet name.
pub const MAX_SHEET_NAME_LEN: usize = 31;
/// Maximum number of sheets emitted per workbook.
pub const MAX_SHEETS_COUNT: usize = 255;
/// Maximum colspan or rowspan applied per cell to prevent memory thrashing.
pub const MAX_CELL_SPAN: usize = 1024;
/// Maximum clamp/truncation warnings emitted before silencing (matches CSV writer).
pub const MAX_CLAMP_WARNINGS: usize = 64;
/// Maximum total bytes for serialized XLSX binary output (50 MiB).
pub const MAX_OUTPUT_BYTES: usize = 50 * 1024 * 1024;

/// Errors that can occur when serializing an XLSX document.
#[derive(Debug, thiserror::Error)]
pub enum XlsxError {
    /// Packaging the OpenXML ZIP archive failed.
    #[error("failed to package xlsx: {0}")]
    Package(#[from] zip::result::ZipError),
    /// Writing XML to archive failed.
    #[error("io error writing xlsx: {0}")]
    Io(#[from] std::io::Error),
    /// The serialized XLSX output exceeded the total size limit.
    #[error("xlsx output exceeded size limit of {limit} bytes")]
    OutputTooLarge { limit: usize },
}

/// Computes the Excel column letters from a 0-based column index (0 -> "A", 25 -> "Z", 26 -> "AA").
#[must_use]
pub fn col_name(mut col: usize) -> String {
    let mut s = String::new();
    loop {
        let rem = col % 26;
        s.push((b'A' + rem as u8) as char);
        if col < 26 {
            break;
        }
        col = (col / 26) - 1;
    }
    s.chars().rev().collect()
}

fn escape_xml(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\u{FFFE}' | '\u{FFFF}' => {}
            // Filter invalid XML 1.0 control characters
            c if (c < ' ' && c != '\t' && c != '\n' && c != '\r') => {}
            c => out.push(c),
        }
    }
    out
}

fn is_numeric(text: &str) -> Option<f64> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    // Prevent numbers with leading '+' or leading zeros (like zip codes or phone numbers) from losing formatting
    if trimmed.starts_with('+') {
        return None;
    }
    if (trimmed.starts_with('0') && trimmed.len() > 1 && !trimmed.starts_with("0."))
        || (trimmed.starts_with("-0") && trimmed.len() > 2 && !trimmed.starts_with("-0."))
    {
        return None;
    }
    let val: f64 = trimmed.parse().ok()?;
    if val.is_finite() { Some(val) } else { None }
}

fn sanitize_sheet_name(raw: &str, fallback_index: usize, used: &mut HashSet<String>) -> String {
    let clean: String = raw
        .chars()
        .map(|c| match c {
            '\\' | '/' | '?' | '*' | ':' | '[' | ']' => '_',
            '\u{FFFE}' | '\u{FFFF}' => '_',
            c if c < ' ' => '_',
            c => c,
        })
        .collect();
    // Excel rule: sheet name cannot begin or end with an apostrophe.
    let trimmed = clean.trim().trim_matches('\'').trim();
    let base = if trimmed.is_empty() {
        format!("Table {fallback_index}")
    } else {
        let s: String = trimmed.chars().take(MAX_SHEET_NAME_LEN).collect();
        let s_trimmed = s.trim_end_matches('\'').trim();
        if s_trimmed.is_empty() {
            format!("Table {fallback_index}")
        } else {
            s_trimmed.to_owned()
        }
    };

    let mut candidate = base.clone();
    let mut counter = 2;
    while used.contains(&candidate.to_ascii_lowercase()) {
        let suffix = format!("_{counter}");
        let allowed = MAX_SHEET_NAME_LEN.saturating_sub(suffix.len());
        let truncated: String = base.chars().take(allowed).collect();
        let mut truncated_clean = truncated.trim_end_matches('\'').to_owned();
        if truncated_clean.is_empty() {
            truncated_clean = format!("Table_{counter}");
        }
        candidate = format!("{truncated_clean}{suffix}");
        counter += 1;
    }
    used.insert(candidate.to_ascii_lowercase());
    candidate
}

struct SheetData {
    name: String,
    rows: Vec<RowData>,
    merge_cells: Vec<String>,
}

struct RowData {
    row_num: usize,
    cells: Vec<CellData>,
}

enum CellValue {
    Number(String),
    String(String),
}

struct CellData {
    col: usize,
    value: CellValue,
}

fn build_sheet_from_table(
    table_idx: usize,
    table_block: &Block,
    used_names: &mut HashSet<String>,
    warnings: &mut Vec<Warning>,
) -> SheetData {
    let (caption, head, body) = match table_block {
        Block::Table {
            caption,
            head,
            body,
            ..
        } => (caption.as_deref(), head, body),
        _ => {
            return SheetData {
                name: format!("Sheet {table_idx}"),
                rows: Vec::new(),
                merge_cells: Vec::new(),
            };
        }
    };

    let raw_name = caption.map(inlines_to_text).unwrap_or_default();
    let name = sanitize_sheet_name(&raw_name, table_idx, used_names);

    let total_rows = head.len() + body.len();
    if total_rows == 0 {
        warnings.push(Warning::new(
            WarningCode::UnsupportedNode,
            "table has no rows; produced empty sheet",
        ));
    } else if total_rows > MAX_EXCEL_ROWS {
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
    let mut occupied: Vec<Vec<bool>> = Vec::new();
    let mut rows: Vec<RowData> = Vec::with_capacity(all_rows.len().min(1000));
    let mut merge_cells: Vec<String> = Vec::new();
    let mut row_col_exceeded = false;
    let mut clamp_warnings_count = 0;
    let mut char_trunc_warnings_count = 0;

    for (r, row_cells) in all_rows.iter().enumerate() {
        if r >= MAX_EXCEL_ROWS {
            break;
        }
        let row_num = r + 1;
        let mut cells = Vec::new();
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

            let value = if is_numeric(&raw_text).is_some() {
                CellValue::Number(raw_text.trim().to_owned())
            } else {
                CellValue::String(raw_text)
            };

            cells.push(CellData { col: c, value });

            if colspan > 1 || rowspan > 1 {
                let from_ref = format!("{}{}", col_name(c), row_num);
                let to_ref = format!("{}{}", col_name(c + colspan - 1), row_num + rowspan - 1);
                merge_cells.push(format!("{from_ref}:{to_ref}"));
            }

            c += colspan;
        }

        if !cells.is_empty() {
            rows.push(RowData { row_num, cells });
        }
    }

    SheetData {
        name,
        rows,
        merge_cells,
    }
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

fn render_worksheet_xml(sheet: &SheetData) -> Result<String, XlsxError> {
    render_worksheet_xml_with_limit(sheet, MAX_UNCOMPRESSED_SHEET_BYTES)
}

fn render_worksheet_xml_with_limit(
    sheet: &SheetData,
    max_bytes: usize,
) -> Result<String, XlsxError> {
    let mut xml = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\">\n<sheetData>\n",
    );

    for row in &sheet.rows {
        xml.push_str(&format!("<row r=\"{}\">\n", row.row_num));
        for cell in &row.cells {
            let cell_ref = format!("{}{}", col_name(cell.col), row.row_num);
            match &cell.value {
                CellValue::Number(num) => {
                    xml.push_str(&format!("<c r=\"{cell_ref}\"><v>{num}</v></c>\n"));
                }
                CellValue::String(s) => {
                    if s.is_empty() {
                        xml.push_str(&format!("<c r=\"{cell_ref}\"/>\n"));
                    } else {
                        let escaped = escape_xml(s);
                        xml.push_str(&format!(
                            "<c r=\"{cell_ref}\" t=\"inlineStr\"><is><t xml:space=\"preserve\">{escaped}</t></is></c>\n"
                        ));
                    }
                }
            }
        }
        xml.push_str("</row>\n");
        if xml.len() > max_bytes {
            return Err(XlsxError::OutputTooLarge { limit: max_bytes });
        }
    }

    xml.push_str("</sheetData>\n");

    if !sheet.merge_cells.is_empty() {
        xml.push_str(&format!(
            "<mergeCells count=\"{}\">\n",
            sheet.merge_cells.len()
        ));
        for range in &sheet.merge_cells {
            xml.push_str(&format!("<mergeCell ref=\"{range}\"/>\n"));
        }
        xml.push_str("</mergeCells>\n");
    }

    xml.push_str("</worksheet>\n");
    if xml.len() > max_bytes {
        return Err(XlsxError::OutputTooLarge { limit: max_bytes });
    }
    Ok(xml)
}

const MINIMAL_STYLES_XML: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
  <fonts count="1"><font><sz val="11"/><name val="Calibri"/></font></fonts>
  <fills count="2"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill></fills>
  <borders count="1"><border><left/><right/><top/><bottom/><diagonal/></border></borders>
  <cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs>
  <cellXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/></cellXfs>
  <cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0"/></cellStyles>
</styleSheet>
"#;

const ROOT_RELS_XML: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/>
</Relationships>
"#;

/// Serializes an IR [`Document`] to an OpenXML spreadsheet (XLSX).
///
/// Each IR table becomes its own sheet. If no tables are found, document blocks are dumped
/// row by row into a single sheet, and a `no_tables_found` warning is issued.
#[must_use = "document serialization returns a Result with output bytes and warnings"]
pub fn write(document: &Document) -> Result<WriteBinaryOutput, XlsxError> {
    let tables = find_tables(&document.body);
    let mut warnings = Vec::new();
    let mut sheets = Vec::new();
    let mut used_names = HashSet::new();

    if tables.is_empty() {
        if document.body.is_empty() {
            warnings.push(Warning::new(
                WarningCode::UnsupportedNode,
                "document has no content; produced empty sheet",
            ));
        } else {
            warnings.push(Warning::new(
                WarningCode::NoTablesFound,
                "no tables found in document; generated text dump",
            ));
        }

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

        let mut rows = Vec::new();
        let mut row_num = 1;
        let mut char_trunc_warnings_count = 0;
        for block in &document.body {
            if row_num > MAX_EXCEL_ROWS {
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
                rows.push(RowData {
                    row_num,
                    cells: vec![CellData {
                        col: 0,
                        value: CellValue::String(text),
                    }],
                });
                row_num += 1;
            }
        }

        sheets.push(SheetData {
            name: "Sheet1".to_owned(),
            rows,
            merge_cells: Vec::new(),
        });
    } else {
        let max_sheets = MAX_SHEETS_COUNT;
        let tables_to_process = if tables.len() > max_sheets {
            warnings.push(Warning::new(
                WarningCode::UnsupportedNode,
                format!(
                    "document has {} tables; capped at maximum {} sheets",
                    tables.len(),
                    max_sheets
                ),
            ));
            &tables[..max_sheets]
        } else {
            &tables[..]
        };

        for (idx, table_block) in tables_to_process.iter().enumerate() {
            sheets.push(build_sheet_from_table(
                idx + 1,
                table_block,
                &mut used_names,
                &mut warnings,
            ));
        }
    }

    let bytes = package_xlsx(&sheets)?;
    Ok(WriteBinaryOutput { bytes, warnings })
}

pub const MAX_UNCOMPRESSED_SHEET_BYTES: usize = 200 * 1024 * 1024;

fn package_xlsx(sheets: &[SheetData]) -> Result<Vec<u8>, XlsxError> {
    package_xlsx_with_limit(sheets, MAX_OUTPUT_BYTES)
}

fn package_xlsx_with_limit(sheets: &[SheetData], limit: usize) -> Result<Vec<u8>, XlsxError> {
    let cursor = Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(cursor);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    // 1. [Content_Types].xml
    let mut content_types = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\n<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\n<Default Extension=\"xml\" ContentType=\"application/xml\"/>\n<Override PartName=\"/xl/workbook.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml\"/>\n<Override PartName=\"/xl/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml\"/>\n",
    );
    for i in 1..=sheets.len() {
        content_types.push_str(&format!(
            "<Override PartName=\"/xl/worksheets/sheet{i}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml\"/>\n"
        ));
    }
    content_types.push_str("</Types>\n");

    zip.start_file("[Content_Types].xml", options)?;
    zip.write_all(content_types.as_bytes())?;

    // 2. _rels/.rels
    zip.start_file("_rels/.rels", options)?;
    zip.write_all(ROOT_RELS_XML.as_bytes())?;

    // 3. xl/_rels/workbook.xml.rels
    let mut wb_rels = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\n<Relationship Id=\"rIdStyles\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles\" Target=\"styles.xml\"/>\n",
    );
    for i in 1..=sheets.len() {
        wb_rels.push_str(&format!(
            "<Relationship Id=\"rId{i}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet\" Target=\"worksheets/sheet{i}.xml\"/>\n"
        ));
    }
    wb_rels.push_str("</Relationships>\n");

    zip.start_file("xl/_rels/workbook.xml.rels", options)?;
    zip.write_all(wb_rels.as_bytes())?;

    // 4. xl/styles.xml
    zip.start_file("xl/styles.xml", options)?;
    zip.write_all(MINIMAL_STYLES_XML.as_bytes())?;

    // 5. xl/workbook.xml
    let mut workbook_xml = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<workbook xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\">\n<sheets>\n",
    );
    for (i, sheet) in sheets.iter().enumerate() {
        let id = i + 1;
        let escaped_name = escape_xml(&sheet.name);
        workbook_xml.push_str(&format!(
            "<sheet name=\"{escaped_name}\" sheetId=\"{id}\" r:id=\"rId{id}\"/>\n"
        ));
    }
    workbook_xml.push_str("</sheets>\n</workbook>\n");

    zip.start_file("xl/workbook.xml", options)?;
    zip.write_all(workbook_xml.as_bytes())?;

    // 6. xl/worksheets/sheet{i}.xml
    for (i, sheet) in sheets.iter().enumerate() {
        let id = i + 1;
        let sheet_xml = render_worksheet_xml(sheet)?;
        zip.start_file(format!("xl/worksheets/sheet{id}.xml"), options)?;
        zip.write_all(sheet_xml.as_bytes())?;
        if zip.get_ref().map(|c| c.get_ref().len()).unwrap_or(0) > limit {
            return Err(XlsxError::OutputTooLarge { limit });
        }
    }

    let cursor = zip.finish()?;
    let bytes = cursor.into_inner();
    if bytes.len() > limit {
        return Err(XlsxError::OutputTooLarge { limit });
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{Alignment, ColumnSpec, Inline, TableCell};
    use zip::ZipArchive;

    fn make_cell(text: &str, rowspan: u32, colspan: u32) -> TableCell {
        TableCell {
            rowspan,
            colspan,
            header: None,
            blocks: vec![Block::Paragraph {
                id: Default::default(),
                style: None,
                direct: Default::default(),
                content: vec![Inline::Text {
                    text: text.to_owned(),
                }],
            }],
        }
    }

    #[test]
    fn column_names_follow_excel_convention() {
        assert_eq!(col_name(0), "A");
        assert_eq!(col_name(1), "B");
        assert_eq!(col_name(25), "Z");
        assert_eq!(col_name(26), "AA");
        assert_eq!(col_name(27), "AB");
        assert_eq!(col_name(51), "AZ");
        assert_eq!(col_name(52), "BA");
        assert_eq!(col_name(701), "ZZ");
        assert_eq!(col_name(702), "AAA");
    }

    #[test]
    fn numbers_detected_properly() {
        assert_eq!(is_numeric("123"), Some(123.0));
        assert_eq!(is_numeric(" -45.67 "), Some(-45.67));
        assert_eq!(is_numeric("0"), Some(0.0));
        assert_eq!(is_numeric("0.5"), Some(0.5));
        // Phone numbers or leading zero strings are kept as text
        assert_eq!(is_numeric("0123"), None);
        assert_eq!(is_numeric("+12345"), None);
        assert_eq!(is_numeric("text"), None);
    }

    #[test]
    fn writes_xlsx_package_with_sheets_and_merged_cells() {
        let columns = vec![
            ColumnSpec {
                align: Alignment::Default,
            },
            ColumnSpec {
                align: Alignment::Default,
            },
        ];
        let head = vec![vec![make_cell("Header A", 1, 1), make_cell("Count", 1, 1)]];
        let body = vec![
            vec![make_cell("Merged Item", 2, 1), make_cell("100", 1, 1)],
            vec![make_cell("200", 1, 1)],
        ];

        let doc = Document {
            body: vec![
                Block::Table {
                    id: Default::default(),
                    style: None,
                    direct: Default::default(),
                    caption: Some(vec![Inline::Text {
                        text: "Sales Data".into(),
                    }]),
                    columns: columns.clone(),
                    head: head.clone(),
                    body: body.clone(),
                    footnotes: Vec::new(),
                },
                Block::Table {
                    id: Default::default(),
                    style: None,
                    direct: Default::default(),
                    caption: None,
                    columns,
                    head,
                    body,
                    footnotes: Vec::new(),
                },
            ],
            ..Default::default()
        };

        let output = write(&doc).unwrap();
        assert!(output.warnings.is_empty());
        assert!(!output.bytes.is_empty());

        let mut archive = ZipArchive::new(Cursor::new(output.bytes)).unwrap();
        assert!(archive.by_name("[Content_Types].xml").is_ok());
        assert!(archive.by_name("xl/workbook.xml").is_ok());
        assert!(archive.by_name("xl/worksheets/sheet1.xml").is_ok());
        assert!(archive.by_name("xl/worksheets/sheet2.xml").is_ok());

        // Verify merge cell in sheet1
        let sheet1_xml = {
            let mut sheet1_file = archive.by_name("xl/worksheets/sheet1.xml").unwrap();
            let mut s = String::new();
            std::io::Read::read_to_string(&mut sheet1_file, &mut s).unwrap();
            s
        };
        assert!(sheet1_xml.contains("<mergeCell ref=\"A2:A3\"/>"));
        assert!(sheet1_xml.contains("<v>100</v>"));
        assert!(sheet1_xml.contains("<v>200</v>"));
        assert!(sheet1_xml.contains("Header A"));

        // Verify sheet names in workbook.xml
        let wb_xml = {
            let mut wb_file = archive.by_name("xl/workbook.xml").unwrap();
            let mut s = String::new();
            std::io::Read::read_to_string(&mut wb_file, &mut s).unwrap();
            s
        };
        assert!(wb_xml.contains("name=\"Sales Data\""));
        assert!(wb_xml.contains("name=\"Table 2\""));
    }

    #[test]
    fn text_dump_emits_no_tables_found_warning() {
        let doc = Document {
            body: vec![Block::Paragraph {
                id: Default::default(),
                style: None,
                direct: Default::default(),
                content: vec![Inline::Text {
                    text: "Only text here".into(),
                }],
            }],
            ..Default::default()
        };

        let output = write(&doc).unwrap();
        assert_eq!(output.warnings.len(), 1);
        assert_eq!(output.warnings[0].code, WarningCode::NoTablesFound);

        let mut archive = ZipArchive::new(Cursor::new(output.bytes)).unwrap();
        let sheet1_xml = {
            let mut sheet1_file = archive.by_name("xl/worksheets/sheet1.xml").unwrap();
            let mut s = String::new();
            std::io::Read::read_to_string(&mut sheet1_file, &mut s).unwrap();
            s
        };
        assert!(sheet1_xml.contains("Only text here"));
    }

    #[test]
    fn bounded_colspan_and_rowspan_clamp_safely() {
        // Colspan of u32::MAX must not allocate terabytes or abort
        let doc = Document {
            body: vec![Block::Table {
                id: Default::default(),
                style: None,
                direct: Default::default(),
                caption: None,
                columns: vec![ColumnSpec {
                    align: Alignment::Default,
                }],
                head: vec![],
                body: vec![vec![
                    make_cell("Safe Cell", u32::MAX, u32::MAX),
                    make_cell("Next Cell", 1, 1),
                ]],
                footnotes: Vec::new(),
            }],
            ..Default::default()
        };

        let output = write(&doc).unwrap();
        assert!(!output.bytes.is_empty());
        assert!(
            output
                .warnings
                .iter()
                .any(|w| w.message.contains("clamped"))
        );
        let mut archive = ZipArchive::new(Cursor::new(output.bytes)).unwrap();
        let sheet1_xml = {
            let mut sheet1_file = archive.by_name("xl/worksheets/sheet1.xml").unwrap();
            let mut s = String::new();
            std::io::Read::read_to_string(&mut sheet1_file, &mut s).unwrap();
            s
        };
        assert!(sheet1_xml.contains("Safe Cell"));
        // Range must be bounded to MAX_CELL_SPAN and table rows
        assert!(sheet1_xml.contains("<mergeCell ref=\"A1:AMJ1\"/>"));
    }

    #[test]
    fn overlong_cell_text_truncated_to_max_chars() {
        let long_text = "A".repeat(40_000);
        let doc = Document {
            body: vec![Block::Table {
                id: Default::default(),
                style: None,
                direct: Default::default(),
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

        let output = write(&doc).unwrap();
        assert!(
            output
                .warnings
                .iter()
                .any(|w| w.message.contains("truncated"))
        );
        let mut archive = ZipArchive::new(Cursor::new(output.bytes)).unwrap();
        let sheet1_xml = {
            let mut sheet1_file = archive.by_name("xl/worksheets/sheet1.xml").unwrap();
            let mut s = String::new();
            std::io::Read::read_to_string(&mut sheet1_file, &mut s).unwrap();
            s
        };
        // The cell text must be truncated to MAX_CELL_CHARS (32,767)
        let expected = "A".repeat(MAX_CELL_CHARS);
        assert!(sheet1_xml.contains(&expected));
        assert!(!sheet1_xml.contains(&format!("{expected}A")));
    }

    #[test]
    fn sheet_name_sanitization_rules_and_deduplication() {
        let mut used = HashSet::new();

        // 1. Apostrophes at either end are stripped
        let name1 = sanitize_sheet_name("'Finance Report'", 1, &mut used);
        assert_eq!(name1, "Finance Report");
        assert!(!name1.starts_with('\'') && !name1.ends_with('\''));

        // 2. Overlong names (> 31 chars) truncated
        let name2 = sanitize_sheet_name(
            "A Very Very Very Long Sheet Name That Exceeds Limit",
            2,
            &mut used,
        );
        assert!(name2.chars().count() <= 31);

        // 3. Characters forbidden in Excel replaced
        let name3 = sanitize_sheet_name("Sales:2026/Q1[Final]*?\\", 3, &mut used);
        assert_eq!(name3, "Sales_2026_Q1_Final____");

        // 4. Case-insensitive deduplication
        let mut used_dups = HashSet::new();
        let d1 = sanitize_sheet_name("Summary", 1, &mut used_dups);
        let d2 = sanitize_sheet_name("SUMMARY", 2, &mut used_dups);
        let d3 = sanitize_sheet_name("summary", 3, &mut used_dups);
        assert_eq!(d1, "Summary");
        assert_eq!(d2, "SUMMARY_2");
        assert_eq!(d3, "summary_3");

        // 5. Special chars & < > " in workbook.xml properly escaped
        let doc = Document {
            body: vec![Block::Table {
                id: Default::default(),
                style: None,
                direct: Default::default(),
                caption: Some(vec![Inline::Text {
                    text: "Tom & Jerry <Fun>".into(),
                }]),
                columns: vec![ColumnSpec {
                    align: Alignment::Default,
                }],
                head: vec![],
                body: vec![vec![make_cell("data", 1, 1)]],
                footnotes: Vec::new(),
            }],
            ..Default::default()
        };
        let output = write(&doc).unwrap();
        let mut archive = ZipArchive::new(Cursor::new(output.bytes)).unwrap();
        let wb_xml = {
            let mut wb_file = archive.by_name("xl/workbook.xml").unwrap();
            let mut s = String::new();
            std::io::Read::read_to_string(&mut wb_file, &mut s).unwrap();
            s
        };
        assert!(wb_xml.contains("name=\"Tom &amp; Jerry &lt;Fun&gt;\""));
    }

    #[test]
    fn text_dump_mode_extracts_nested_blocks() {
        let doc = Document {
            body: vec![
                Block::Quote {
                    id: Default::default(),
                    style: None,
                    direct: Default::default(),
                    blocks: vec![Block::Paragraph {
                        id: Default::default(),
                        style: None,
                        direct: Default::default(),
                        content: vec![Inline::Text {
                            text: "Quote text".into(),
                        }],
                    }],
                },
                Block::List {
                    id: Default::default(),
                    style: None,
                    direct: Default::default(),
                    ordered: false,
                    start: None,
                    tight: true,
                    items: vec![crate::ir::ListItem {
                        checked: None,
                        blocks: vec![Block::Paragraph {
                            id: Default::default(),
                            style: None,
                            direct: Default::default(),
                            content: vec![Inline::Text {
                                text: "List item text".into(),
                            }],
                        }],
                    }],
                },
            ],
            ..Default::default()
        };

        let output = write(&doc).unwrap();
        assert_eq!(output.warnings[0].code, WarningCode::NoTablesFound);
        let mut archive = ZipArchive::new(Cursor::new(output.bytes)).unwrap();
        let sheet1_xml = {
            let mut sheet1_file = archive.by_name("xl/worksheets/sheet1.xml").unwrap();
            let mut s = String::new();
            std::io::Read::read_to_string(&mut sheet1_file, &mut s).unwrap();
            s
        };
        assert!(sheet1_xml.contains("Quote text"));
        assert!(sheet1_xml.contains("List item text"));
    }

    #[test]
    fn sheet_cap_truncates_to_255_and_emits_warning() {
        let tables: Vec<Block> = (0..260)
            .map(|i| Block::Table {
                id: Default::default(),
                style: None,
                direct: Default::default(),
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

        let output = write(&doc).unwrap();
        assert!(output.warnings.iter().any(|w| {
            w.code == WarningCode::UnsupportedNode
                && w.message
                    .contains("document has 260 tables; capped at maximum 255 sheets")
        }));

        let mut archive = ZipArchive::new(Cursor::new(output.bytes)).unwrap();
        assert!(archive.by_name("xl/worksheets/sheet255.xml").is_ok());
        assert!(archive.by_name("xl/worksheets/sheet256.xml").is_err());
    }

    #[test]
    fn col_cap_emits_warning_when_columns_exceed_limit() {
        let wide_row: Vec<TableCell> = (0..MAX_EXCEL_COLS + 5)
            .map(|_| make_cell("c", 1, 1))
            .collect();
        let doc = Document {
            body: vec![Block::Table {
                id: Default::default(),
                style: None,
                direct: Default::default(),
                caption: None,
                columns: vec![],
                head: vec![],
                body: vec![wide_row],
                footnotes: Vec::new(),
            }],
            ..Default::default()
        };
        let output = write(&doc).unwrap();
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
                id: Default::default(),
                style: None,
                direct: Default::default(),
                caption: None,
                columns: vec![],
                head: vec![],
                body,
                footnotes: Vec::new(),
            }],
            ..Default::default()
        };
        let output = write(&doc).unwrap();
        assert!(output.warnings.iter().any(|w| {
            w.code == WarningCode::UnsupportedNode
                && w.message
                    .contains("table has 1048578 rows; truncated to maximum 1048576")
        }));
    }

    #[test]
    fn output_too_large_error_returned_when_exceeding_max_bytes() {
        let sheet = SheetData {
            name: "Sheet1".to_string(),
            rows: vec![RowData {
                row_num: 1,
                cells: vec![CellData {
                    col: 0,
                    value: CellValue::String("A".repeat(500)),
                }],
            }],
            merge_cells: vec![],
        };
        // Passing limit = 100 bytes will fail as minimum XLSX zip is > 1KB
        let err = package_xlsx_with_limit(&[sheet], 100).unwrap_err();
        assert!(matches!(err, XlsxError::OutputTooLarge { limit: 100 }));
    }

    #[test]
    fn clamp_warnings_capped_at_64() {
        let rows: Vec<Vec<TableCell>> = (0..70).map(|_| vec![make_cell("cell", 1, 2000)]).collect();
        let doc = Document {
            body: vec![Block::Table {
                id: Default::default(),
                style: None,
                direct: Default::default(),
                caption: None,
                columns: vec![],
                head: vec![],
                body: rows,
                footnotes: Vec::new(),
            }],
            ..Default::default()
        };
        let output = write(&doc).unwrap();
        let clamp_count = output
            .warnings
            .iter()
            .filter(|w| w.message.contains("clamped"))
            .count();
        assert_eq!(clamp_count, MAX_CLAMP_WARNINGS);
    }

    #[test]
    fn empty_table_emits_warning() {
        let doc = Document {
            body: vec![Block::Table {
                id: Default::default(),
                style: None,
                direct: Default::default(),
                caption: None,
                columns: vec![],
                head: vec![],
                body: vec![],
                footnotes: Vec::new(),
            }],
            ..Default::default()
        };
        let output = write(&doc).unwrap();
        assert!(output.warnings.iter().any(|w| {
            w.code == WarningCode::UnsupportedNode
                && w.message
                    .contains("table has no rows; produced empty sheet")
        }));
    }

    #[test]
    fn empty_document_emits_warning() {
        let doc = Document::default();
        let output = write(&doc).unwrap();
        assert!(output.warnings.iter().any(|w| {
            w.code == WarningCode::UnsupportedNode
                && w.message
                    .contains("document has no content; produced empty sheet")
        }));
    }

    #[test]
    fn worksheet_xml_bounded_while_building() {
        let sheet = SheetData {
            name: "Sheet1".to_string(),
            rows: vec![RowData {
                row_num: 1,
                cells: vec![CellData {
                    col: 0,
                    value: CellValue::String("A".repeat(500)),
                }],
            }],
            merge_cells: vec![],
        };
        // Passing max_bytes = 100 will fail during row rendering and report limit 100
        let err = render_worksheet_xml_with_limit(&sheet, 100).unwrap_err();
        assert!(matches!(err, XlsxError::OutputTooLarge { limit: 100 }));
    }
}
