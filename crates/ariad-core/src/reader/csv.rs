//! CSV table reader producing deterministic Ariad IR.

use crate::{
    format::Format,
    ir::{Alignment, Block, ColumnSpec, Document, Inline, Metadata, TableCell},
    limits::{Limits, LimitsError},
    reader::common::{ReadOutput, nfc},
};

/// Errors that can occur when reading CSV into IR.
#[derive(Debug, thiserror::Error)]
pub enum ReadError {
    #[error(transparent)]
    InvalidLimits(#[from] LimitsError),
    #[error("input exceeds byte limit (actual {actual}, limit {limit})")]
    InputTooLarge { actual: u64, limit: u64 },
    #[error("too many blocks (actual {actual}, limit {limit})")]
    TooManyBlocks { actual: u64, limit: u32 },
    #[error("CSV parsing error: {0}")]
    Csv(String),
}

/// Detects delimiter (comma, semicolon, or tab) in the first non-empty line.
fn detect_delimiter(bytes: &[u8]) -> u8 {
    let mut counts = [0usize; 3];
    let mut quoted = false;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'"' if quoted && bytes.get(i + 1) == Some(&b'"') => i += 1,
            b'"' => quoted = !quoted,
            b'\r' | b'\n' if !quoted => {
                if counts.iter().any(|&c| c > 0) {
                    break;
                }
            }
            c if !quoted => {
                if let Some(index) = b",;\t".iter().position(|x| *x == c) {
                    counts[index] += 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    let index = (1..3).fold(0, |best, i| if counts[i] > counts[best] { i } else { best });
    b",;\t"[index]
}

/// Un-guards spreadsheet formula prefix (leading single quote before =, +, -, @, \t, \r).
fn unguard(value: &str) -> &str {
    match value.strip_prefix('\'') {
        Some(rest) if rest.starts_with(['=', '+', '-', '@', '\t', '\r']) => rest,
        _ => value,
    }
}

/// Reads CSV data into an IR document containing a [`Block::Table`].
pub fn read(bytes: &[u8], limits: &Limits) -> Result<ReadOutput, ReadError> {
    limits.validate()?;
    let byte_len = bytes.len() as u64;
    if let Some(max_bytes) = limits.max_input_bytes
        && byte_len > max_bytes
    {
        return Err(ReadError::InputTooLarge {
            actual: byte_len,
            limit: max_bytes,
        });
    }

    if bytes.is_empty() {
        return Ok(ReadOutput {
            document: Document {
                meta: Metadata {
                    source_format: Some(Format::Csv),
                    ..Metadata::default()
                },
                body: Vec::new(),
                ..Document::default()
            },
            warnings: Vec::new(),
        });
    }

    let delimiter = detect_delimiter(bytes);
    let mut reader = csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .has_headers(false)
        .flexible(true)
        .from_reader(bytes);

    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut max_cols = 0;

    for result in reader.records() {
        let record = result.map_err(|e| ReadError::Csv(e.to_string()))?;
        let mut row = Vec::with_capacity(record.len());
        for field in &record {
            let clean = nfc(unguard(field));
            row.push(clean);
        }
        max_cols = max_cols.max(row.len());
        rows.push(row);
    }

    if rows.is_empty() {
        return Ok(ReadOutput {
            document: Document {
                meta: Metadata {
                    source_format: Some(Format::Csv),
                    ..Metadata::default()
                },
                body: Vec::new(),
                ..Document::default()
            },
            warnings: Vec::new(),
        });
    }

    if limits.max_blocks < 1 {
        return Err(ReadError::TooManyBlocks {
            actual: 1,
            limit: limits.max_blocks,
        });
    }

    let make_cell = |text: &str, is_header: bool| -> TableCell {
        TableCell {
            rowspan: 1,
            colspan: 1,
            header: if is_header { Some(true) } else { None },
            blocks: vec![Block::Paragraph {
                content: vec![Inline::Text {
                    text: text.to_owned(),
                }],
            }],
        }
    };

    let columns = vec![
        ColumnSpec {
            align: Alignment::Default,
        };
        max_cols
    ];

    let mut row_iter = rows.into_iter();
    let head = if let Some(mut first_row) = row_iter.next() {
        while first_row.len() < max_cols {
            first_row.push(String::new());
        }
        vec![first_row.into_iter().map(|f| make_cell(&f, true)).collect()]
    } else {
        Vec::new()
    };

    let mut body = Vec::new();
    for mut row in row_iter {
        while row.len() < max_cols {
            row.push(String::new());
        }
        body.push(row.into_iter().map(|f| make_cell(&f, false)).collect());
    }

    let table = Block::Table {
        caption: None,
        columns,
        head,
        body,
        footnotes: Vec::new(),
    };

    let document = Document {
        meta: Metadata {
            source_format: Some(Format::Csv),
            ..Metadata::default()
        },
        body: vec![table],
        ..Document::default()
    };

    Ok(ReadOutput {
        document,
        warnings: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_csv_table() {
        let data = b"Name,Age,City\nAlice,30,Hanoi\nBob,25,Da Nang\n";
        let out = read(data, &Limits::local()).unwrap();
        assert_eq!(out.document.meta.source_format, Some(Format::Csv));
        assert_eq!(out.document.body.len(), 1);

        if let Block::Table {
            columns,
            head,
            body,
            ..
        } = &out.document.body[0]
        {
            assert_eq!(columns.len(), 3);
            assert_eq!(head.len(), 1);
            assert_eq!(head[0].len(), 3);
            assert_eq!(body.len(), 2);
            assert_eq!(body[0].len(), 3);
            assert_eq!(body[1].len(), 3);
        } else {
            panic!("expected Table block");
        }
    }

    #[test]
    fn handles_semicolon_and_tab_delimiters() {
        let semicolon = b"Col1;Col2\nVal1;Val2\n";
        let out = read(semicolon, &Limits::local()).unwrap();
        if let Block::Table { columns, .. } = &out.document.body[0] {
            assert_eq!(columns.len(), 2);
        }

        let tab = b"Col1\tCol2\nVal1\tVal2\n";
        let out = read(tab, &Limits::local()).unwrap();
        if let Block::Table { columns, .. } = &out.document.body[0] {
            assert_eq!(columns.len(), 2);
        }
    }

    #[test]
    fn unguards_formula_prefixes() {
        let data = b"Header\n'=1+2\n'@SUM(A1)\n";
        let out = read(data, &Limits::local()).unwrap();
        if let Block::Table { body, .. } = &out.document.body[0] {
            if let Block::Paragraph { content } = &body[0][0].blocks[0] {
                assert_eq!(
                    content[0],
                    Inline::Text {
                        text: "=1+2".to_owned()
                    }
                );
            }
            if let Block::Paragraph { content } = &body[1][0].blocks[0] {
                assert_eq!(
                    content[0],
                    Inline::Text {
                        text: "@SUM(A1)".to_owned()
                    }
                );
            }
        }
    }

    #[test]
    fn pads_jagged_rows() {
        let data = b"A,B,C\n1,2\n3,4,5,6\n";
        let out = read(data, &Limits::local()).unwrap();
        if let Block::Table {
            columns,
            head,
            body,
            ..
        } = &out.document.body[0]
        {
            assert_eq!(columns.len(), 4);
            assert_eq!(head[0].len(), 4);
            assert_eq!(body[0].len(), 4);
            assert_eq!(body[1].len(), 4);
        }
    }

    #[test]
    fn enforces_limits() {
        let limits = Limits {
            max_input_bytes: Some(10),
            ..Limits::local()
        };
        let err = read(b"A,B,C\n1,2,3,4,5\n", &limits).unwrap_err();
        assert!(matches!(err, ReadError::InputTooLarge { .. }));
    }
}
