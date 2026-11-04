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
            c if c < ' ' => '_',
            c => c,
        })
        .collect();
    let trimmed = clean.trim();
    let base = if trimmed.is_empty() {
        format!("Table {fallback_index}")
    } else {
        let s: String = trimmed.chars().take(31).collect();
        if s.is_empty() {
            format!("Table {fallback_index}")
        } else {
            s
        }
    };

    let mut candidate = base.clone();
    let mut counter = 2;
    while used.contains(&candidate.to_ascii_lowercase()) {
        let suffix = format!("_{counter}");
        let allowed = 31usize.saturating_sub(suffix.len());
        let truncated: String = base.chars().take(allowed).collect();
        candidate = format!("{truncated}{suffix}");
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

    let all_rows: Vec<&Vec<TableCell>> = head.iter().chain(body.iter()).collect();
    let mut occupied: Vec<Vec<bool>> = Vec::new();
    let mut rows: Vec<RowData> = Vec::with_capacity(all_rows.len());
    let mut merge_cells: Vec<String> = Vec::new();

    for (r, row_cells) in all_rows.iter().enumerate() {
        let row_num = r + 1;
        let mut cells = Vec::new();
        let mut c = 0;

        for cell in *row_cells {
            while is_occupied(&occupied, r, c) {
                c += 1;
            }
            let colspan = (cell.colspan as usize).max(1);
            let rowspan = (cell.rowspan as usize).max(1);
            mark_occupied(&mut occupied, r, c, rowspan, colspan);

            let raw_text = blocks_to_text(&cell.blocks);
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

fn render_worksheet_xml(sheet: &SheetData) -> String {
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
    xml
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
#[must_use]
pub fn write(document: &Document) -> WriteBinaryOutput {
    let tables = find_tables(&document.body);
    let mut warnings = Vec::new();
    let mut sheets = Vec::new();
    let mut used_names = HashSet::new();

    if tables.is_empty() {
        warnings.push(Warning::new(
            WarningCode::NoTablesFound,
            "no tables found in document; generated text dump",
        ));

        let mut rows = Vec::new();
        let mut row_num = 1;
        for block in &document.body {
            let text = blocks_to_text(std::slice::from_ref(block));
            if !text.is_empty() {
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
        for (idx, table_block) in tables.iter().enumerate() {
            sheets.push(build_sheet_from_table(
                idx + 1,
                table_block,
                &mut used_names,
            ));
        }
    }

    let bytes = package_xlsx(&sheets);
    WriteBinaryOutput { bytes, warnings }
}

fn package_xlsx(sheets: &[SheetData]) -> Vec<u8> {
    let mut cursor = Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(&mut cursor);
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

    let _ = zip.start_file("[Content_Types].xml", options);
    let _ = zip.write_all(content_types.as_bytes());

    // 2. _rels/.rels
    let _ = zip.start_file("_rels/.rels", options);
    let _ = zip.write_all(ROOT_RELS_XML.as_bytes());

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

    let _ = zip.start_file("xl/_rels/workbook.xml.rels", options);
    let _ = zip.write_all(wb_rels.as_bytes());

    // 4. xl/styles.xml
    let _ = zip.start_file("xl/styles.xml", options);
    let _ = zip.write_all(MINIMAL_STYLES_XML.as_bytes());

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

    let _ = zip.start_file("xl/workbook.xml", options);
    let _ = zip.write_all(workbook_xml.as_bytes());

    // 6. xl/worksheets/sheet{i}.xml
    for (i, sheet) in sheets.iter().enumerate() {
        let id = i + 1;
        let sheet_xml = render_worksheet_xml(sheet);
        let _ = zip.start_file(format!("xl/worksheets/sheet{id}.xml"), options);
        let _ = zip.write_all(sheet_xml.as_bytes());
    }

    let _ = zip.finish();
    cursor.into_inner()
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
                    caption: Some(vec![Inline::Text {
                        text: "Sales Data".into(),
                    }]),
                    columns: columns.clone(),
                    head: head.clone(),
                    body: body.clone(),
                    footnotes: Vec::new(),
                },
                Block::Table {
                    caption: None,
                    columns,
                    head,
                    body,
                    footnotes: Vec::new(),
                },
            ],
            ..Default::default()
        };

        let output = write(&doc);
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
                content: vec![Inline::Text {
                    text: "Only text here".into(),
                }],
            }],
            ..Default::default()
        };

        let output = write(&doc);
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
}
