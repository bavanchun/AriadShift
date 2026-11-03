//! CSV form-data interchange with bounded rows and spreadsheet formula protection.

use super::MAX_DATA_STRING_BYTES;

use std::collections::HashSet;

use ariad_core::tools::params::{
    form_model::{FormValue, MAX_IMPORT_RECORDS},
    forms::EmptyCell,
};
use csv::{ReaderBuilder, Terminator, WriterBuilder};

use super::{FormRecord, check_data_size};
use crate::{PdfError, ToolCtx};

/// Maximum number of CSV columns.
pub const MAX_CSV_COLUMNS: usize = 10_000;

fn invalid(message: &str) -> PdfError {
    PdfError::InvalidParams(message.into())
}

/// Parse CSV records with a required unique header and a bounded record count.
pub fn read_forms(
    bytes: &[u8],
    delimiter: Option<char>,
    empty: EmptyCell,
    max_records: usize,
    ctx: &ToolCtx,
) -> Result<Vec<FormRecord>, PdfError> {
    ctx.check()?;
    check_data_size(bytes)?;
    if max_records == 0 || max_records > MAX_IMPORT_RECORDS {
        return Err(invalid("invalid input size or record limit"));
    }
    let bytes = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
    let delimiter = match delimiter {
        Some(c) if c.is_ascii() && !matches!(c, '\r' | '\n' | '"' | '\0') => c as u8,
        Some(_) => return Err(invalid("invalid CSV delimiter")),
        None => detect_delimiter(bytes),
    };
    let mut reader = ReaderBuilder::new()
        .delimiter(delimiter)
        .flexible(true)
        .from_reader(bytes);
    let header = reader
        .headers()
        .map_err(|_| invalid("invalid CSV header"))?
        .clone();
    if header.is_empty() || header.len() > MAX_CSV_COLUMNS {
        return Err(invalid("invalid CSV column count"));
    }
    let mut names = HashSet::new();
    for name in &header {
        if name.is_empty() || name.len() > MAX_DATA_STRING_BYTES || !names.insert(name) {
            return Err(invalid("CSV header names must be nonempty and unique"));
        }
    }
    let mut records = Vec::new();
    for (index, result) in reader.records().enumerate() {
        ctx.check()?;
        if records.len() == max_records {
            return Err(invalid("too many CSV records"));
        }
        let row = result.map_err(|_| invalid("invalid CSV row"))?;
        if row.len() != header.len() {
            return Err(invalid(&format!(
                "CSV row {} has the wrong number of columns",
                index + 2
            )));
        }
        let mut record = FormRecord::new();
        for (name, cell) in header.iter().zip(row.iter()) {
            if cell.len() > MAX_DATA_STRING_BYTES {
                return Err(invalid("CSV value is too long"));
            }
            let value = if cell.is_empty() {
                match empty {
                    EmptyCell::Skip => continue,
                    EmptyCell::Clear => FormValue::Null(()),
                }
            } else if cell.contains('|') {
                FormValue::Multi(cell.split('|').map(str::to_owned).collect())
            } else {
                FormValue::Text(cell.to_owned())
            };
            record.insert(name.to_owned(), value);
        }
        records.push(record);
    }
    Ok(records)
}

// Count separators in the first logical record, ignoring delimiters inside quoted cells.
pub(super) fn detect_delimiter(bytes: &[u8]) -> u8 {
    let mut counts = [0usize; 3];
    let mut quoted = false;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'"' if quoted && bytes.get(i + 1) == Some(&b'"') => i += 1,
            b'"' => quoted = !quoted,
            b'\r' | b'\n' if !quoted => break,
            c if !quoted => {
                if let Some(index) = [b',', b';', b'\t'].iter().position(|x| *x == c) {
                    counts[index] += 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    let index = (1..3).fold(0, |best, i| if counts[i] > counts[best] { i } else { best });
    [b',', b';', b'\t'][index]
}

/// Serialize one record using CRLF and prefix spreadsheet formulas with an apostrophe.
pub fn write_forms(record: &FormRecord) -> Result<Vec<u8>, PdfError> {
    let mut writer = WriterBuilder::new()
        .terminator(Terminator::CRLF)
        .from_writer(super::BoundedWriter::default());
    let guard = |s: &str| -> String {
        if s.starts_with(['=', '+', '-', '@', '\t', '\r']) {
            format!("'{s}")
        } else {
            s.to_owned()
        }
    };
    writer
        .write_record(record.keys().map(|s| guard(s)))
        .map_err(|_| invalid("cannot encode CSV"))?;
    let cells = record.values().map(|value| {
        let s = match value {
            FormValue::Text(s) => s.clone(),
            FormValue::Bool(value) => value.to_string(),
            FormValue::Multi(values) => values.join("|"),
            FormValue::Null(()) => String::new(),
        };
        guard(&s)
    });
    writer
        .write_record(cells)
        .map_err(|_| invalid("cannot encode CSV"))?;
    writer
        .into_inner()
        .map(super::BoundedWriter::finish)
        .map_err(|_| invalid("cannot encode CSV"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reads_delimited_data_and_rejects_bad_rows() {
        let records = read_forms(
            "name;note\r\nViệt;\"hello\nworld\"\r\n".as_bytes(),
            None,
            EmptyCell::Skip,
            10,
            &ToolCtx::new(),
        )
        .unwrap();
        assert_eq!(records[0]["note"], FormValue::Text("hello\nworld".into()));
        assert!(read_forms(b"x,x\n1,2\n", None, EmptyCell::Skip, 10, &ToolCtx::new()).is_err());
        assert!(read_forms(b"x,y\n1\n", None, EmptyCell::Skip, 10, &ToolCtx::new()).is_err());
    }
    #[test]
    fn protects_formulas_in_headers_and_values() {
        let record = FormRecord::from([("=header".into(), FormValue::Text("+value".into()))]);
        let csv = String::from_utf8(write_forms(&record).unwrap()).unwrap();
        assert_eq!(csv, "'=header\r\n'+value\r\n");
    }
}
