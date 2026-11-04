//! Plain text reader producing deterministic Ariad IR.

use crate::{
    format::Format,
    ir::{Block, Document, Inline, Metadata},
    limits::{Limits, LimitsError},
    reader::common::{ReadOutput, nfc},
    warning::Warning,
};

/// Errors that can occur when reading plain text into IR.
#[derive(Debug, thiserror::Error)]
pub enum ReadError {
    #[error(transparent)]
    InvalidLimits(#[from] LimitsError),
    #[error("input exceeds byte limit (actual {actual}, limit {limit})")]
    InputTooLarge { actual: u64, limit: u64 },
    #[error("too many blocks (actual {actual}, limit {limit})")]
    TooManyBlocks { actual: u64, limit: u32 },
    #[error("input is not valid UTF-8: {0}")]
    InvalidUtf8(String),
}

/// Reads UTF-8 plain text into an IR document under `limits`.
pub fn read(text: &str, limits: &Limits) -> Result<ReadOutput, ReadError> {
    limits.validate()?;
    let byte_len = text.len() as u64;
    if let Some(max_bytes) = limits.max_input_bytes
        && byte_len > max_bytes
    {
        return Err(ReadError::InputTooLarge {
            actual: byte_len,
            limit: max_bytes,
        });
    }

    let text_no_bom = text.strip_prefix('\u{feff}').unwrap_or(text);
    let normalized = nfc(&text_no_bom.replace("\r\n", "\n").replace('\r', "\n"));

    let mut blocks = Vec::new();

    // Split on double newlines (paragraphs)
    for paragraph_str in normalized.split("\n\n").map(str::trim) {
        if paragraph_str.is_empty() {
            continue;
        }

        if blocks.len() >= limits.max_blocks as usize {
            return Err(ReadError::TooManyBlocks {
                actual: (blocks.len() + 1) as u64,
                limit: limits.max_blocks,
            });
        }

        let mut inlines = Vec::new();
        let lines: Vec<&str> = paragraph_str.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            let line_trimmed = line.trim_end();
            if !line_trimmed.is_empty() {
                inlines.push(Inline::Text {
                    text: line_trimmed.to_owned(),
                });
            }
            if i + 1 < lines.len() {
                inlines.push(Inline::SoftBreak {});
            }
        }

        if !inlines.is_empty() {
            blocks.push(Block::Paragraph { content: inlines });
        }
    }

    let document = Document {
        meta: Metadata {
            source_format: Some(Format::Txt),
            ..Metadata::default()
        },
        body: blocks,
        ..Document::default()
    };

    Ok(ReadOutput {
        document,
        warnings: Vec::<Warning>::new(),
    })
}

/// Reads raw bytes as plain text into an IR document under `limits`.
pub fn read_bytes(bytes: &[u8], limits: &Limits) -> Result<ReadOutput, ReadError> {
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

    let text = std::str::from_utf8(bytes).map_err(|e| ReadError::InvalidUtf8(e.to_string()))?;
    read(text, limits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_text_paragraphs() {
        let input = "First paragraph line 1\nline 2\n\nSecond paragraph.";
        let out = read(input, &Limits::local()).unwrap();
        assert_eq!(out.document.body.len(), 2);
        assert_eq!(out.document.meta.source_format, Some(Format::Txt));

        if let Block::Paragraph { content } = &out.document.body[0] {
            assert_eq!(content.len(), 3);
            assert_eq!(
                content[0],
                Inline::Text {
                    text: "First paragraph line 1".to_owned()
                }
            );
            assert_eq!(content[1], Inline::SoftBreak {});
            assert_eq!(
                content[2],
                Inline::Text {
                    text: "line 2".to_owned()
                }
            );
        } else {
            panic!("expected paragraph");
        }
    }

    #[test]
    fn normalizes_unicode_nfc() {
        // e + combining acute accent -> é
        let decomposed = "caf\u{0065}\u{0301}";
        let out = read(decomposed, &Limits::local()).unwrap();
        let Block::Paragraph { content } = &out.document.body[0] else {
            panic!("expected paragraph");
        };
        assert_eq!(
            content[0],
            Inline::Text {
                text: "café".to_owned()
            }
        );
    }

    #[test]
    fn strips_bom_and_splits_crlf_paragraphs() {
        let input = "\u{feff}First paragraph.\r\n\r\nSecond paragraph line 1\r\nline 2.\r\n\r\nThird paragraph.";
        let out = read(input, &Limits::local()).unwrap();
        assert_eq!(out.document.body.len(), 3);
        let Block::Paragraph { content } = &out.document.body[0] else {
            panic!("expected paragraph 1");
        };
        assert_eq!(
            content[0],
            Inline::Text {
                text: "First paragraph.".to_owned()
            }
        );

        let Block::Paragraph { content } = &out.document.body[1] else {
            panic!("expected paragraph 2");
        };
        assert_eq!(
            content[0],
            Inline::Text {
                text: "Second paragraph line 1".to_owned()
            }
        );
        assert_eq!(content[1], Inline::SoftBreak {});
        assert_eq!(
            content[2],
            Inline::Text {
                text: "line 2.".to_owned()
            }
        );

        let Block::Paragraph { content } = &out.document.body[2] else {
            panic!("expected paragraph 3");
        };
        assert_eq!(
            content[0],
            Inline::Text {
                text: "Third paragraph.".to_owned()
            }
        );
    }

    #[test]
    fn enforces_input_byte_limit() {
        let limits = Limits {
            max_input_bytes: Some(10),
            ..Limits::local()
        };
        let input = "This is definitely longer than 10 bytes.";
        let err = read(input, &limits).unwrap_err();
        assert!(matches!(err, ReadError::InputTooLarge { .. }));
    }

    #[test]
    fn enforces_max_blocks_limit() {
        let limits = Limits {
            max_blocks: 2,
            ..Limits::local()
        };
        let input = "Para 1\n\nPara 2\n\nPara 3";
        let err = read(input, &limits).unwrap_err();
        assert!(matches!(err, ReadError::TooManyBlocks { .. }));
    }

    #[test]
    fn rejects_invalid_utf8_in_read_bytes() {
        let invalid = b"Hello \xff\xfe world";
        let err = read_bytes(invalid, &Limits::local()).unwrap_err();
        assert!(matches!(err, ReadError::InvalidUtf8(_)));
    }
}
