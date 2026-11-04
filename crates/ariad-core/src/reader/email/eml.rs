//! RFC 822 / MIME EML email reader producing deterministic Ariad IR.

use mail_parser::MessageParser;

use crate::{
    format::Format,
    ir::{Block, Document, Inline, ListItem, Metadata},
    limits::{Limits, LimitsError},
    reader::common::{ReadOutput, nfc},
};

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
            Err(_) => {
                // Fall back to plain text if HTML parsing fails
                if let Some(text_body) = message.body_text(0)
                    && let Ok(text_out) = crate::reader::text::read(&text_body, limits)
                {
                    blocks.extend(text_out.document.body);
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
        let name = att.attachment_name().unwrap_or("unnamed");
        let size = att.raw_body().len();
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
        assert!(!out.document.body.is_empty());
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
Content-Type: text/plain; name=\"notes.txt\"\r\n\
Content-Disposition: attachment; filename=\"notes.txt\"\r\n\
\r\n\
Some secret attachment data\r\n\
--boundary123--\r\n";

        let out = read(eml, &Limits::local()).unwrap();
        assert_eq!(out.document.meta.source_format, Some(Format::Eml));
        assert_eq!(
            out.document.meta.title.as_deref(),
            Some("HTML with Attachment")
        );

        // Check that attachment is listed
        let has_attachments = out.document.body.iter().any(|b| {
            if let Block::Heading { content, .. } = b {
                content.iter().any(|i| match i {
                    Inline::Text { text } => text == "Attachments",
                    _ => false,
                })
            } else {
                false
            }
        });
        assert!(has_attachments, "attachment heading must be present");
    }
}
