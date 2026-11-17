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

/// One extracted table.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CsvTable {
    /// One-based position of the table in the document. Tables without content are skipped, so
    /// consecutive entries can skip a number.
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
            number: index + 1,
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
