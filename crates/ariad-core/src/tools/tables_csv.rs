//! Table extraction to CSV shared by every runtime of `pdf.tables_to_csv`.
//!
//! The function takes a document IR, finds its tables (also inside quotes and lists), and
//! serializes each one to RFC 4180 CSV with formula-injection guarding. Hosts and the browser
//! only differ in how they obtain the IR and where they store the files.

use crate::{
    ir::{Block, Document},
    warning::{Warning, WarningCode},
    writer::csv::{find_tables, table_to_csv_with_warnings},
};

/// Maximum number of tables extracted into CSV files by `pdf.tables_to_csv`.
pub const MAX_TABLES_COUNT: usize = 255;
/// Maximum cumulative CSV bytes emitted by `pdf.tables_to_csv` (50 MiB).
pub const MAX_TOTAL_CSV_BYTES: usize = 50 * 1024 * 1024;

/// Message of the `engine_failure` for a document without any table. It is a fixed text with no
/// document content, so every runtime may show it to the caller.
pub const NO_TABLES_MESSAGE: &str = "no tables found in document";

/// Code of the warning that says the Docling engine pack did not read the tables.
pub const DOCLING_FALLBACK_CODE: &str = "docling_fallback";

/// Text of the [`DOCLING_FALLBACK_CODE`] warning when the built-in reader found the tables. The
/// reader detects tables drawn with ruling lines and tables whose columns are aligned with
/// whitespace; other layouts stay text.
pub const RULED_ONLY_MESSAGE: &str = "The Docling engine pack is not installed; tables were detected from ruling lines and from columns aligned with whitespace, and other layouts stay text. Install Docling for layout-aware table detection.";

/// One extracted table.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CsvTable {
    /// One-based position among the extracted tables. Tables without content are skipped and do
    /// not take a number, so the numbers are consecutive.
    pub number: usize,
    /// CSV text of the table.
    pub csv: String,
}

/// Why no CSV files were produced.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TablesCsvError {
    /// The document has no table with content.
    NoTables,
    /// The tables exceed the byte cap or one table was truncated.
    LimitExceeded,
}

/// Tables extracted from a document, in reading order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TablesCsv {
    pub tables: Vec<CsvTable>,
    /// Warnings raised while serializing the tables.
    pub warnings: Vec<Warning>,
    /// The number of tables found when it passed [`MAX_TABLES_COUNT`] and the rest were dropped.
    pub capped_from: Option<usize>,
}

impl TablesCsv {
    /// The notice for a capped extraction, or `None` when every table was extracted.
    #[must_use]
    pub fn cap_message(&self) -> Option<String> {
        self.capped_from.map(|found| {
            format!("document has {found} tables; capped at maximum {MAX_TABLES_COUNT} tables")
        })
    }
}

/// Whether a warning code from reading the PDF still matters for table extraction.
///
/// CSV files carry no styling, so the reader's `styles_dropped` notice is only noise here.
#[must_use]
pub fn keeps_read_warning_code(code: &str) -> bool {
    code != "styles_dropped"
}

/// Name of the CSV file for table `number` (`table-001.csv`).
#[must_use]
pub fn csv_file_name(number: usize) -> String {
    format!("table-{number:03}.csv")
}

/// Extracts every table of `document` into CSV text.
///
/// # Errors
///
/// [`TablesCsvError::NoTables`] when no table has content and
/// [`TablesCsvError::LimitExceeded`] when the output passes [`MAX_TOTAL_CSV_BYTES`] or a table
/// was truncated.
pub fn extract(document: &Document) -> Result<TablesCsv, TablesCsvError> {
    let tables = find_tables(&document.body);
    if tables.is_empty() {
        return Err(TablesCsvError::NoTables);
    }

    let mut warnings = Vec::new();
    let capped_from = (tables.len() > MAX_TABLES_COUNT).then_some(tables.len());
    let capped = &tables[..tables.len().min(MAX_TABLES_COUNT)];

    let mut extracted = Vec::with_capacity(capped.len());
    let mut total_bytes: usize = 0;
    for (index, block) in capped.iter().enumerate() {
        if total_bytes >= MAX_TOTAL_CSV_BYTES {
            return Err(TablesCsvError::LimitExceeded);
        }
        let Block::Table {
            columns,
            head,
            body,
            ..
        } = block
        else {
            continue;
        };
        let mut table_warnings = Vec::new();
        let csv = table_to_csv_with_warnings(columns, head, body, &mut table_warnings);
        let truncated = table_warnings
            .iter()
            .any(|warning| warning.code == WarningCode::OutputTruncated);
        warnings.extend(table_warnings);
        if csv.is_empty() {
            warnings.push(Warning::new(
                WarningCode::UnsupportedNode,
                format!("table {} has no content; skipped", index + 1),
            ));
            continue;
        }
        total_bytes = total_bytes.saturating_add(csv.len());
        if total_bytes > MAX_TOTAL_CSV_BYTES || truncated {
            return Err(TablesCsvError::LimitExceeded);
        }
        extracted.push(CsvTable {
            number: extracted.len() + 1,
            csv,
        });
    }

    if extracted.is_empty() {
        return Err(TablesCsvError::NoTables);
    }
    Ok(TablesCsv {
        tables: extracted,
        warnings,
        capped_from,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{Alignment, ColumnSpec, Inline, TableCell};

    fn cell(text: &str) -> TableCell {
        TableCell {
            rowspan: 1,
            colspan: 1,
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

    fn table(rows: &[&str]) -> Block {
        Block::Table {
            id: Default::default(),
            style: None,
            direct: Default::default(),
            caption: None,
            columns: vec![ColumnSpec {
                align: Alignment::Default,
            }],
            head: Vec::new(),
            body: rows.iter().map(|text| vec![cell(text)]).collect(),
            footnotes: Vec::new(),
        }
    }

    #[test]
    fn tables_are_numbered_consecutively_after_empty_tables_are_skipped() {
        let document = Document {
            body: vec![table(&["first"]), table(&[]), table(&["third"])],
            ..Default::default()
        }
        .finalize_import();
        let extracted = extract(&document).unwrap();
        let numbered: Vec<(usize, &str)> = extracted
            .tables
            .iter()
            .map(|table| (table.number, table.csv.trim_end()))
            .collect();
        assert_eq!(numbered, [(1, "first"), (2, "third")]);
        assert_eq!(csv_file_name(extracted.tables[1].number), "table-002.csv");
        assert!(
            extracted
                .warnings
                .iter()
                .any(|warning| warning.message == "table 2 has no content; skipped"),
            "{:?}",
            extracted.warnings
        );
    }

    #[test]
    fn a_document_of_empty_tables_has_no_tables() {
        let document = Document {
            body: vec![table(&[])],
            ..Default::default()
        }
        .finalize_import();
        assert_eq!(extract(&document), Err(TablesCsvError::NoTables));
    }
}
