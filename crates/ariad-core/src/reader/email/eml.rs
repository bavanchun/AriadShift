//! RFC 822 / MIME EML email reader producing deterministic Ariad IR.

use mail_parser::MessageParser;

use crate::{
    format::Format,
    ir::{Block, Document, Inline, ListItem, Metadata},
    limits::{Limits, LimitsError},
    reader::common::{ReadOutput, nfc},
    warning::{Warning, WarningCode},
};

/// Strips Unicode bidirectional control characters.
fn strip_bidi_controls(s: &str) -> String {
    s.chars()
        .filter(|&c| {
            !matches!(
                c,
                '\u{061c}' | '\u{200e}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'
            )
        })
        .collect()
}

/// Errors that can occur when reading an EML file.
#[derive(Debug, thiserror::Error)]
pub enum ReadError {
    #[error(transparent)]
    InvalidLimits(#[from] LimitsError),
    #[error("input exceeds byte limit (actual {actual}, limit {limit})")]
    InputTooLarge { actual: u64, limit: u64 },
    #[error("too many blocks (actual {actual}, limit {limit})")]
    TooManyBlocks { actual: u64, limit: u32 },
    #[error("email parsing error: {0}")]
    Parse(String),
}

fn format_mailbox(mb: &mail_parser::Mailbox<'_>) -> String {
    match (mb.name(), mb.address()) {
        (Some(name), Some(email)) => format!("{name} <{email}>"),
        (None, Some(email)) => email.to_string(),
        (Some(name), None) => name.to_string(),
        (None, None) => String::new(),
    }
}

fn format_address_list(list: &mail_parser::AddressList<'_>) -> String {
    list.mailboxes()
        .map(|mb| format_mailbox(&mb))
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Reads an EML file into an IR document under `limits`.
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

    let message = MessageParser::default()
        .parse(bytes)
        .ok_or_else(|| ReadError::Parse("failed to parse MIME message".to_owned()))?;

    let mut warnings = Vec::new();

    // Extract headers
    let subject = message.subject().map(|s| nfc(s.trim()));
    let from = message.from().map(|list| format_address_list(&list));
    let to = message.to().map(|list| format_address_list(&list));
    let cc = message.cc().map(|list| format_address_list(&list));
    let date_str = message.date().map(|d| d.to_rfc3339());

    let mut blocks = Vec::new();

    // Build header block
    let mut header_inlines = Vec::new();
    if let Some(ref from_val) = from {
        header_inlines.push(Inline::Strong {
            content: vec![Inline::Text {
                text: "From: ".to_owned(),
            }],
        });
        header_inlines.push(Inline::Text {
            text: nfc(from_val),
        });
        header_inlines.push(Inline::LineBreak {});
    }
    if let Some(ref to_val) = to {
        header_inlines.push(Inline::Strong {
            content: vec![Inline::Text {
                text: "To: ".to_owned(),
            }],
        });
        header_inlines.push(Inline::Text { text: nfc(to_val) });
        header_inlines.push(Inline::LineBreak {});
    }
    if let Some(ref cc_val) = cc {
        header_inlines.push(Inline::Strong {
            content: vec![Inline::Text {
                text: "Cc: ".to_owned(),
            }],
        });
        header_inlines.push(Inline::Text { text: nfc(cc_val) });
        header_inlines.push(Inline::LineBreak {});
    }
    if let Some(ref date_val) = date_str {
        header_inlines.push(Inline::Strong {
            content: vec![Inline::Text {
                text: "Date: ".to_owned(),
            }],
        });
        header_inlines.push(Inline::Text {
            text: nfc(date_val),
        });
        header_inlines.push(Inline::LineBreak {});
    }
    if let Some(ref subj_val) = subject {
        header_inlines.push(Inline::Strong {
            content: vec![Inline::Text {
                text: "Subject: ".to_owned(),
            }],
        });
        header_inlines.push(Inline::Text {
            text: nfc(subj_val),
        });
    }

    if !header_inlines.is_empty() {
        blocks.push(Block::Paragraph {
            content: header_inlines,
        });
    }

    let mut assets = crate::ir::AssetStore::default();

    // Extract body: prefer HTML body if available, running it through the sanitizing HTML reader
    if let Some(html_body) = message.body_html(0) {
        match crate::reader::html::read(html_body.as_bytes(), limits) {
            Ok(html_out) => {
                blocks.extend(html_out.document.body);
                for (id, asset) in html_out.document.assets {
                    assets.insert(id, asset);
                }
                warnings.extend(html_out.warnings);
            }
            Err(e) => {
                warnings.push(Warning::new(
                    WarningCode::UnsupportedNode,
                    format!("HTML body parsing failed: {e}; falling back to plain text"),
                ));
                // Fall back to plain text if HTML parsing fails
                if let Some(text_body) = message.body_text(0)
                    && let Ok(text_out) = crate::reader::text::read(&text_body, limits)
                {
                    blocks.extend(text_out.document.body);
                    warnings.extend(text_out.warnings);
                }
            }
        }
    } else if let Some(text_body) = message.body_text(0)
        && let Ok(text_out) = crate::reader::text::read(&text_body, limits)
    {
        blocks.extend(text_out.document.body);
        warnings.extend(text_out.warnings);
    }

    // Attachments: listed by name and size, never opened
    let mut attachment_items = Vec::new();
    for att in message.attachments() {
        let raw_name = att.attachment_name().unwrap_or("unnamed");
        let name = strip_bidi_controls(raw_name);
        let size = att.decoded_len();
        let item_text = format!("{name} ({size} bytes)");
        attachment_items.push(ListItem {
            checked: None,
            blocks: vec![Block::Paragraph {
                content: vec![Inline::Text {
                    text: nfc(&item_text),
                }],
            }],
        });
    }

    let extra_blocks = if !attachment_items.is_empty() { 2 } else { 0 };
    let total_blocks = blocks.len() + extra_blocks;
    if total_blocks > limits.max_blocks as usize {
        return Err(ReadError::TooManyBlocks {
            actual: total_blocks as u64,
            limit: limits.max_blocks,
        });
    }

    if !attachment_items.is_empty() {
        blocks.push(Block::Heading {
            level: 2,
            content: vec![Inline::Text {
                text: "Attachments".to_owned(),
            }],
        });
        blocks.push(Block::List {
            ordered: false,
            start: None,
            tight: true,
            items: attachment_items,
        });
    }

    let document = Document {
        meta: Metadata {
            title: subject,
            authors: from.into_iter().collect(),
            date: date_str,
            source_format: Some(Format::Eml),
            ..Metadata::default()
        },
        body: blocks,
        assets,
        ..Document::default()
    };

    Ok(ReadOutput { document, warnings })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_text_eml_with_headers_and_body() {
        let eml = b"From: Alice <alice@example.com>\r\n\
To: Bob <bob@example.com>\r\n\
Subject: Test Email\r\n\
Date: Mon, 4 Nov 2026 10:00:00 +0000\r\n\
Content-Type: text/plain; charset=utf-8\r\n\
\r\n\
Hello Bob,\r\n\
\r\n\
This is a test email message.\r\n";

        let out = read(eml, &Limits::local()).unwrap();
        assert_eq!(out.document.meta.source_format, Some(Format::Eml));
        assert_eq!(out.document.meta.title.as_deref(), Some("Test Email"));
        assert_eq!(
            out.document.meta.authors,
            vec!["Alice <alice@example.com>".to_string()]
        );
        assert_eq!(
            out.document.meta.date.as_deref(),
            Some("2026-11-04T10:00:00Z")
        );

        // Check that body paragraphs contain the actual body text
        let found_body = out.document.body.iter().any(|b| match b {
            Block::Paragraph { content } => content.iter().any(|inline| match inline {
                Inline::Text { text } => {
                    text.contains("Hello Bob,") || text.contains("This is a test email message.")
                }
                _ => false,
            }),
            _ => false,
        });
        assert!(
            found_body,
            "plain text body must be present in document blocks"
        );
    }

    #[test]
    fn parses_html_eml_and_lists_attachments() {
        let eml = b"From: Alice <alice@example.com>\r\n\
To: Bob <bob@example.com>\r\n\
Subject: HTML with Attachment\r\n\
Content-Type: multipart/mixed; boundary=\"boundary123\"\r\n\
\r\n\
--boundary123\r\n\
Content-Type: text/html; charset=utf-8\r\n\
\r\n\
<p>Hello <b>Bob</b>!</p>\r\n\
--boundary123\r\n\
Content-Type: application/octet-stream; name=\"\x20\x2enotes\xe2\x80\xaetxt\"\r\n\
Content-Disposition: attachment; filename=\"notes\xe2\x80\xaetxt\"\r\n\
Content-Transfer-Encoding: base64\r\n\
\r\n\
U29tZSBzZWNyZXQgYXR0YWNobWVudCBkYXRhCg==\r\n\
--boundary123--\r\n";

        let out = read(eml, &Limits::local()).unwrap();
        assert_eq!(out.document.meta.source_format, Some(Format::Eml));
        assert_eq!(
            out.document.meta.title.as_deref(),
            Some("HTML with Attachment")
        );

        // Check that attachment is listed with stripped bidi controls and decoded size (28 bytes decoded from base64)
        let has_attachment_item = out.document.body.iter().any(|b| {
            if let Block::List { items, .. } = b {
                items.iter().any(|item| {
                    item.blocks.iter().any(|ib| match ib {
                        Block::Paragraph { content } => content.iter().any(|c| match c {
                            Inline::Text { text } => {
                                text.contains("notestxt") && text.contains("28 bytes")
                            }
                            _ => false,
                        }),
                        _ => false,
                    })
                })
            } else {
                false
            }
        });
        assert!(
            has_attachment_item,
            "attachment must have bidi stripped and decoded size"
        );
    }

    #[test]
    fn enforces_max_blocks_limit() {
        let eml = b"From: Alice <alice@example.com>\r\n\
To: Bob <bob@example.com>\r\n\
Subject: Test\r\n\
Content-Type: text/plain; charset=utf-8\r\n\
\r\n\
Para 1\r\n\
\r\n\
Para 2\r\n";
        let mut limits = Limits::local();
        limits.max_blocks = 1;
        let err = read(eml, &limits).unwrap_err();
        assert!(matches!(err, ReadError::TooManyBlocks { .. }));
    }
}
