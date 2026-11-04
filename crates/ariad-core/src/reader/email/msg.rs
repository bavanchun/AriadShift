//! Microsoft Outlook .msg reader producing deterministic Ariad IR.

use std::io::{Cursor, Read};

use crate::{
    format::Format,
    ir::{Block, Document, Inline, ListItem, Metadata},
    limits::{Limits, LimitsError},
    reader::common::{ReadOutput, nfc},
    warning::{Warning, WarningCode},
};

/// Errors that can occur when reading an Outlook MSG file.
#[derive(Debug, thiserror::Error)]
pub enum ReadError {
    #[error(transparent)]
    InvalidLimits(#[from] LimitsError),
    #[error("input exceeds byte limit (actual {actual}, limit {limit})")]
    InputTooLarge { actual: u64, limit: u64 },
    #[error("too many blocks (actual {actual}, limit {limit})")]
    TooManyBlocks { actual: u64, limit: u32 },
    #[error("compound file error: {0}")]
    Cfb(String),
    #[error("email parsing error: {0}")]
    Parse(String),
}

/// Convert UTF-16LE bytes to String, stripping any trailing null byte.
fn decode_utf16le(bytes: &[u8]) -> String {
    let (chunks, _) = bytes.as_chunks::<2>();
    let u16s: Vec<u16> = chunks.iter().map(|c| u16::from_le_bytes(*c)).collect();
    let s = String::from_utf16_lossy(&u16s);
    s.trim_end_matches('\0').to_string()
}

/// Convert String8 / ANSI bytes to String, stripping any trailing null byte.
fn decode_string8(bytes: &[u8]) -> String {
    let s = String::from_utf8_lossy(bytes);
    s.trim_end_matches('\0').to_string()
}

/// Convert Windows FILETIME (100ns units since 1601-01-01) to ISO-8601/RFC 3339 string.
fn format_filetime_rfc3339(filetime: i64) -> Option<String> {
    const FILETIME_TO_UNIX_EPOCH: i64 = 116_444_736_000_000_000;
    if filetime < FILETIME_TO_UNIX_EPOCH {
        return None;
    }
    let unix_secs = (filetime - FILETIME_TO_UNIX_EPOCH) / 10_000_000;
    let sec = (unix_secs % 60) as u32;
    let rem = unix_secs / 60;
    let min = (rem % 60) as u32;
    let rem = rem / 60;
    let hour = (rem % 24) as u32;
    let mut days = rem / 24;

    // Civil day calculation (Howard Hinnant's algorithm)
    days += 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let doe = (days - era * 146_097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = (yoe as i64) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };

    Some(format!("{y:04}-{m:02}-{d:02}T{hour:02}:{min:02}:{sec:02}Z"))
}

/// Decompresses LZFu-compressed RTF data per MS-OXRTFCP specification.
pub fn decompress_rtf(compressed: &[u8]) -> Result<Vec<u8>, String> {
    if compressed.len() < 16 {
        return Err("RTF compressed data too short".to_string());
    }
    let comp_size = u32::from_le_bytes(
        compressed[0..4]
            .try_into()
            .map_err(|_| "invalid header".to_string())?,
    ) as usize;
    let uncomp_size = u32::from_le_bytes(
        compressed[4..8]
            .try_into()
            .map_err(|_| "invalid header".to_string())?,
    ) as usize;
    let magic = u32::from_le_bytes(
        compressed[8..12]
            .try_into()
            .map_err(|_| "invalid header".to_string())?,
    );

    // "MEMA" magic: uncompressed
    if magic == 0x414d_454d {
        let end = (16 + uncomp_size).min(compressed.len());
        return Ok(compressed[16..end].to_vec());
    }

    // "LZFu" magic: compressed
    if magic != 0x7546_5a4c {
        return Err(format!("unknown RTF compression magic: 0x{magic:08x}"));
    }

    let initial_dict = b"{\\rtf1\\ansi\\mac\\deff0\\deftab720{\\fonttbl;}{\\f0\\fnil \\froman \\fswiss \
\\fmodern \\fscript \\fdecor MS Sans SerifSymbolArialTimes New RomanCourier{\\colortbl;\
\\red0\\green0\\blue0;\\red0\\green0\\blue255;\\red0\\green255\\blue255;\\red0\\green255\\blue0;\
\\red255\\green0\\blue255;\\red255\\green0\\blue0;\\red255\\green255\\blue0;\\red255\\green255\\blue255;\
\\red0\\green0\\blue128;\\red0\\green128\\blue128;\\red0\\green128\\blue0;\\red128\\green0\\blue128;\
\\red128\\green0\\blue0;\\red128\\green128\\blue0;\\red128\\green128\\blue128;\\red192\\green192\\green192;\
\\par \\pard\\plain\\f0\\fs20\\b\\i\\u\\tab\\tx";

    let mut dict = [0u8; 4096];
    let init_len = initial_dict.len().min(4096);
    dict[..init_len].copy_from_slice(&initial_dict[..init_len]);
    let mut write_offset = init_len;

    let mut output = Vec::with_capacity(uncomp_size.min(10 * 1024 * 1024));
    let mut in_pos = 16;
    let max_in = (comp_size + 4).min(compressed.len());

    while in_pos < max_in && output.len() < uncomp_size {
        let flag_byte = compressed[in_pos];
        in_pos += 1;

        for bit in 0..8 {
            if in_pos >= max_in || output.len() >= uncomp_size {
                break;
            }
            if (flag_byte & (1 << bit)) == 0 {
                // Literal byte
                let b = compressed[in_pos];
                in_pos += 1;
                output.push(b);
                dict[write_offset] = b;
                write_offset = (write_offset + 1) % 4096;
            } else {
                // 2-byte token: dictionary reference
                if in_pos + 1 >= max_in {
                    break;
                }
                let token = u16::from_be_bytes([compressed[in_pos], compressed[in_pos + 1]]);
                in_pos += 2;

                let offset = (token >> 4) as usize;
                let length = ((token & 0x0F) + 2) as usize;

                if offset == write_offset {
                    // End of stream marker
                    return Ok(output);
                }

                for i in 0..length {
                    if output.len() >= uncomp_size {
                        break;
                    }
                    let b = dict[(offset + i) % 4096];
                    output.push(b);
                    dict[write_offset] = b;
                    write_offset = (write_offset + 1) % 4096;
                }
            }
        }
    }

    Ok(output)
}

/// Strips RTF control words and groups to extract plain text.
pub fn strip_rtf(rtf: &[u8]) -> String {
    let mut text = String::new();
    let mut i = 0;
    let len = rtf.len();
    let mut group_depth: u32 = 0;
    let mut skip_depth: Option<u32> = None;

    while i < len {
        let b = rtf[i];
        if b == b'{' {
            group_depth += 1;
            i += 1;
            if i < len && rtf[i] == b'\\' {
                let mut j = i + 1;
                while j < len && (rtf[j].is_ascii_alphabetic() || rtf[j] == b'*') {
                    j += 1;
                }
                let word = &rtf[i + 1..j];
                if (word == b"*"
                    || word == b"fonttbl"
                    || word == b"colortbl"
                    || word == b"stylesheet"
                    || word == b"info")
                    && skip_depth.is_none()
                {
                    skip_depth = Some(group_depth);
                }
            }
            continue;
        } else if b == b'}' {
            if let Some(sd) = skip_depth
                && group_depth <= sd
            {
                skip_depth = None;
            }
            group_depth = group_depth.saturating_sub(1);
            i += 1;
            continue;
        }

        if skip_depth.is_some() {
            i += 1;
            continue;
        }

        if b == b'\\' {
            i += 1;
            if i >= len {
                break;
            }
            let next_b = rtf[i];
            if next_b == b'{' || next_b == b'}' || next_b == b'\\' {
                text.push(next_b as char);
                i += 1;
            } else if next_b == b'\'' {
                i += 1;
                if i + 1 < len
                    && let Ok(hex_str) = std::str::from_utf8(&rtf[i..i + 2])
                    && let Ok(hex_val) = u8::from_str_radix(hex_str, 16)
                {
                    text.push(hex_val as char);
                    i += 2;
                    continue;
                }
            } else {
                let start = i;
                while i < len && rtf[i].is_ascii_alphabetic() {
                    i += 1;
                }
                let word = &rtf[start..i];
                if i < len && (rtf[i] == b'-' || rtf[i].is_ascii_digit()) {
                    i += 1;
                    while i < len && rtf[i].is_ascii_digit() {
                        i += 1;
                    }
                }
                if i < len && rtf[i] == b' ' {
                    i += 1;
                }

                if word == b"par" || word == b"line" {
                    text.push('\n');
                } else if word == b"tab" {
                    text.push('\t');
                }
            }
        } else if b == b'\r' || b == b'\n' {
            i += 1;
        } else {
            text.push(b as char);
            i += 1;
        }
    }

    text
}

/// Helper to read a stream into a Vec<u8> with size bounds.
fn read_stream_bounded<F: Read + std::io::Seek>(
    comp: &mut cfb::CompoundFile<F>,
    path: &str,
    max_bytes: usize,
) -> Option<Vec<u8>> {
    let mut stream = comp.open_stream(path).ok()?;
    let mut buf = Vec::new();
    let mut take = (&mut stream).take(max_bytes as u64 + 1);
    take.read_to_end(&mut buf).ok()?;
    if buf.len() > max_bytes {
        None
    } else {
        Some(buf)
    }
}

/// Reads string from either UTF-16LE (`*001F`) or String8 (`*001E`) stream.
fn read_string_property<F: Read + std::io::Seek>(
    comp: &mut cfb::CompoundFile<F>,
    parent: &str,
    prop_id: &str,
    max_bytes: usize,
) -> Option<String> {
    let prefix = if parent == "/" {
        format!("/__substg1.0_{prop_id}")
    } else {
        format!("{parent}/__substg1.0_{prop_id}")
    };

    let u16_path = format!("{prefix}001F");
    if let Some(bytes) = read_stream_bounded(comp, &u16_path, max_bytes) {
        let s = decode_utf16le(&bytes);
        if !s.is_empty() {
            return Some(s);
        }
    }

    let a_path = format!("{prefix}001E");
    if let Some(bytes) = read_stream_bounded(comp, &a_path, max_bytes) {
        let s = decode_string8(&bytes);
        if !s.is_empty() {
            return Some(s);
        }
    }

    None
}

/// Parse date from `__properties_version1.0` stream.
fn parse_date_from_properties<F: Read + std::io::Seek>(
    comp: &mut cfb::CompoundFile<F>,
    parent: &str,
) -> Option<String> {
    let prop_path = if parent == "/" {
        "/__properties_version1.0".to_string()
    } else {
        format!("{parent}/__properties_version1.0")
    };

    let bytes = read_stream_bounded(comp, &prop_path, 64 * 1024)?;
    // Root properties stream has 32-byte header
    if bytes.len() < 32 {
        return None;
    }
    let records = &bytes[32..];
    let (chunks, _) = records.as_chunks::<16>();
    for chunk in chunks {
        let prop_type = u16::from_le_bytes([chunk[0], chunk[1]]);
        let prop_id = u16::from_le_bytes([chunk[2], chunk[3]]);
        // 0x0040 = PT_SYSTIME (FILETIME)
        // 0x0039 = PR_CLIENT_SUBMIT_TIME, 0x0E06 = PR_MESSAGE_DELIVERY_TIME
        if prop_type == 0x0040 && (prop_id == 0x0039 || prop_id == 0x0E06) {
            let filetime = i64::from_le_bytes(chunk[8..16].try_into().ok()?);
            if let Some(dt) = format_filetime_rfc3339(filetime) {
                return Some(dt);
            }
        }
    }
    None
}

/// Reads an Outlook MSG file into an IR document under `limits`.
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

    let cursor = Cursor::new(bytes);
    let mut comp = cfb::CompoundFile::open(cursor).map_err(|e| ReadError::Cfb(e.to_string()))?;

    let mut warnings = Vec::new();
    let max_stream_bytes = limits.max_input_bytes.unwrap_or(20 * 1024 * 1024) as usize;

    // 1. Headers: Subject
    let subject =
        read_string_property(&mut comp, "/", "0037", max_stream_bytes).map(|s| nfc(s.trim()));

    // 2. Sender: PR_SENDER_NAME (0x0C1A), PR_SENDER_EMAIL_ADDRESS (0x0C1F), or PR_SENT_REPRESENTING_NAME (0x0042)
    let sender_name = read_string_property(&mut comp, "/", "0C1A", max_stream_bytes)
        .or_else(|| read_string_property(&mut comp, "/", "0042", max_stream_bytes));
    let sender_email = read_string_property(&mut comp, "/", "0C1F", max_stream_bytes)
        .or_else(|| read_string_property(&mut comp, "/", "0065", max_stream_bytes));

    let from = match (sender_name, sender_email) {
        (Some(name), Some(email)) => Some(format!("{name} <{email}>")),
        (Some(name), None) => Some(name),
        (None, Some(email)) => Some(email),
        (None, None) => None,
    };

    // 3. Date
    let date_str = parse_date_from_properties(&mut comp, "/");

    // 4. Discover sub-storages for recipients and attachments
    let mut recip_storages = Vec::new();
    let mut attach_storages = Vec::new();

    for entry in comp.read_root_storage() {
        let name = entry.name().to_string();
        if entry.is_storage() {
            if name.starts_with("__recip_version1.0_#") {
                recip_storages.push(format!("/{}", name));
            } else if name.starts_with("__attach_version1.0_#") {
                attach_storages.push(format!("/{}", name));
            }
        }
    }
    recip_storages.sort();
    attach_storages.sort();

    // Gather recipients
    let mut to_recipients = Vec::new();
    let mut cc_recipients = Vec::new();

    for r_path in &recip_storages {
        let r_name = read_string_property(&mut comp, r_path, "3001", max_stream_bytes);
        let r_email = read_string_property(&mut comp, r_path, "3003", max_stream_bytes);

        // Check recipient type from properties if available
        let mut recip_type = 1u32; // Default MAPI_TO
        let prop_path = format!("{r_path}/__properties_version1.0");
        if let Some(prop_bytes) = read_stream_bounded(&mut comp, &prop_path, 4096)
            && prop_bytes.len() >= 8
        {
            let (chunks, _) = prop_bytes[8..].as_chunks::<16>();
            for chunk in chunks {
                let prop_type_val = u16::from_le_bytes([chunk[0], chunk[1]]);
                let prop_id = u16::from_le_bytes([chunk[2], chunk[3]]);
                // 0x0C15 = PR_RECIPIENT_TYPE (PT_LONG = 0x0003)
                if prop_id == 0x0C15 && prop_type_val == 0x0003 {
                    recip_type = u32::from_le_bytes([chunk[8], chunk[9], chunk[10], chunk[11]]);
                    break;
                }
            }
        }

        let formatted = match (r_name, r_email) {
            (Some(n), Some(e)) => format!("{n} <{e}>"),
            (Some(n), None) => n,
            (None, Some(e)) => e,
            (None, None) => continue,
        };

        if recip_type == 2 {
            cc_recipients.push(formatted);
        } else {
            to_recipients.push(formatted);
        }
    }

    let to_str = if to_recipients.is_empty() {
        None
    } else {
        Some(to_recipients.join(", "))
    };
    let cc_str = if cc_recipients.is_empty() {
        None
    } else {
        Some(cc_recipients.join(", "))
    };

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
    if let Some(ref to_val) = to_str {
        header_inlines.push(Inline::Strong {
            content: vec![Inline::Text {
                text: "To: ".to_owned(),
            }],
        });
        header_inlines.push(Inline::Text { text: nfc(to_val) });
        header_inlines.push(Inline::LineBreak {});
    }
    if let Some(ref cc_val) = cc_str {
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

    // 5. Body extraction:
    // Order of preference: HTML (0x1013), then Plain (0x1000), then RTF-compressed (0x1009)
    let html_bytes = read_stream_bounded(&mut comp, "/__substg1.0_10130102", max_stream_bytes)
        .or_else(|| {
            read_stream_bounded(&mut comp, "/__substg1.0_1013001F", max_stream_bytes)
                .map(|b| decode_utf16le(&b).into_bytes())
        })
        .or_else(|| read_stream_bounded(&mut comp, "/__substg1.0_1013001E", max_stream_bytes));

    let mut body_read = false;

    if let Some(html_data) = html_bytes {
        match crate::reader::html::read(&html_data, limits) {
            Ok(html_out) => {
                blocks.extend(html_out.document.body);
                for (id, asset) in html_out.document.assets {
                    assets.insert(id, asset);
                }
                warnings.extend(html_out.warnings);
                body_read = true;
            }
            Err(_) => {
                // If HTML reading fails, fall through to text or RTF
            }
        }
    }

    if !body_read {
        // Try Plain text body (0x1000)
        let plain_text = read_string_property(&mut comp, "/", "1000", max_stream_bytes);
        if let Some(text_content) = plain_text
            && let Ok(text_out) = crate::reader::text::read(&text_content, limits)
        {
            blocks.extend(text_out.document.body);
            warnings.extend(text_out.warnings);
            body_read = true;
        }
    }

    if !body_read {
        // Try RTF-compressed (0x1009)
        if let Some(rtf_comp_data) =
            read_stream_bounded(&mut comp, "/__substg1.0_10090102", max_stream_bytes)
            && let Ok(decompressed) = decompress_rtf(&rtf_comp_data)
        {
            let stripped = strip_rtf(&decompressed);
            warnings.push(Warning::new(
                WarningCode::UnsupportedNode,
                "RTF-compressed body converted to plain text",
            ));
            if let Ok(text_out) = crate::reader::text::read(&stripped, limits) {
                blocks.extend(text_out.document.body);
                warnings.extend(text_out.warnings);
            }
        }
    }

    // 6. Attachments: iterate attachment sub-storages, read filename and byte size. Never open content.
    let mut attachment_items = Vec::new();
    for a_path in &attach_storages {
        let name = read_string_property(&mut comp, a_path, "3707", max_stream_bytes)
            .or_else(|| read_string_property(&mut comp, a_path, "3704", max_stream_bytes))
            .or_else(|| read_string_property(&mut comp, a_path, "3001", max_stream_bytes))
            .unwrap_or_else(|| "unnamed".to_string());

        // Get byte size from data bin stream entry length or PR_ATTACH_SIZE property
        let data_stream_path = format!("{a_path}/__substg1.0_37010102");
        let size = if let Ok(entry) = comp.entry(&data_stream_path) {
            entry.len() as usize
        } else {
            // Check PR_ATTACH_SIZE (0x0E20) in attachment properties stream
            let mut prop_size = 0usize;
            let prop_path = format!("{a_path}/__properties_version1.0");
            if let Some(prop_bytes) = read_stream_bounded(&mut comp, &prop_path, 4096)
                && prop_bytes.len() >= 8
            {
                let (chunks, _) = prop_bytes[8..].as_chunks::<16>();
                for chunk in chunks {
                    let prop_type = u16::from_le_bytes([chunk[0], chunk[1]]);
                    let prop_id = u16::from_le_bytes([chunk[2], chunk[3]]);
                    if prop_id == 0x0E20 && prop_type == 0x0003 {
                        prop_size =
                            u32::from_le_bytes([chunk[8], chunk[9], chunk[10], chunk[11]]) as usize;
                        break;
                    }
                }
            }
            prop_size
        };

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
            source_format: Some(Format::Msg),
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

    fn encode_utf16le(s: &str) -> Vec<u8> {
        let mut v = Vec::new();
        for u in s.encode_utf16() {
            v.extend_from_slice(&u.to_le_bytes());
        }
        v.extend_from_slice(&[0, 0]);
        v
    }

    #[test]
    fn parses_msg_with_cfb_structure() {
        use std::io::Write;
        let mut comp = cfb::CompoundFile::create(Cursor::new(Vec::new())).unwrap();

        // Write Subject
        {
            let mut stream = comp.create_stream("/__substg1.0_0037001F").unwrap();
            stream.write_all(&encode_utf16le("Hello Outlook")).unwrap();
        }

        // Write Sender
        {
            let mut stream = comp.create_stream("/__substg1.0_0C1A001F").unwrap();
            stream.write_all(&encode_utf16le("Alice Smith")).unwrap();
        }
        {
            let mut stream = comp.create_stream("/__substg1.0_0C1F001F").unwrap();
            stream
                .write_all(&encode_utf16le("alice@example.com"))
                .unwrap();
        }

        // Write Plain body
        {
            let mut stream = comp.create_stream("/__substg1.0_1000001F").unwrap();
            stream
                .write_all(&encode_utf16le("This is a test message from Outlook."))
                .unwrap();
        }

        // Create attachment sub-storage
        comp.create_storage("/__attach_version1.0_#00000000")
            .unwrap();
        {
            let mut stream = comp
                .create_stream("/__attach_version1.0_#00000000/__substg1.0_3707001F")
                .unwrap();
            stream.write_all(&encode_utf16le("report.pdf")).unwrap();
        }
        {
            let mut stream = comp
                .create_stream("/__attach_version1.0_#00000000/__substg1.0_37010102")
                .unwrap();
            stream.write_all(b"fake pdf content here").unwrap();
        }

        let bytes = comp.into_inner().into_inner();
        let out = read(&bytes, &Limits::local()).unwrap();

        assert_eq!(out.document.meta.source_format, Some(Format::Msg));
        assert_eq!(out.document.meta.title.as_deref(), Some("Hello Outlook"));
        assert_eq!(
            out.document.meta.authors,
            vec!["Alice Smith <alice@example.com>".to_string()]
        );

        // Check attachment listing
        let has_att = out.document.body.iter().any(|b| match b {
            Block::List { items, .. } => items.iter().any(|item| {
                item.blocks.iter().any(|ib| match ib {
                    Block::Paragraph { content } => content.iter().any(|c| match c {
                        Inline::Text { text } => text.contains("report.pdf (21 bytes)"),
                        _ => false,
                    }),
                    _ => false,
                })
            }),
            _ => false,
        });
        assert!(has_att, "attachment should be listed with correct size");
    }

    #[test]
    fn rtf_stripping_works() {
        let rtf = b"{\\rtf1\\ansi\\deff0{\\fonttbl;}\\pard Hello \\b World\\b0!\\par Next line.}";
        let text = strip_rtf(rtf);
        assert!(text.contains("Hello World!"));
        assert!(text.contains("Next line."));
    }

    #[test]
    fn generate_sample_msg_fixtures() {
        use std::io::Write;
        let mut comp = cfb::CompoundFile::create(Cursor::new(Vec::new())).unwrap();

        // Write Subject
        {
            let mut stream = comp.create_stream("/__substg1.0_0037001F").unwrap();
            stream.write_all(&encode_utf16le("Project Update")).unwrap();
        }

        // Write Sender
        {
            let mut stream = comp.create_stream("/__substg1.0_0C1A001F").unwrap();
            stream.write_all(&encode_utf16le("Alice Walker")).unwrap();
        }
        {
            let mut stream = comp.create_stream("/__substg1.0_0C1F001F").unwrap();
            stream
                .write_all(&encode_utf16le("alice@example.com"))
                .unwrap();
        }

        // Recipient storage
        comp.create_storage("/__recip_version1.0_#00000000")
            .unwrap();
        {
            let mut stream = comp
                .create_stream("/__recip_version1.0_#00000000/__substg1.0_3001001F")
                .unwrap();
            stream.write_all(&encode_utf16le("Bob Smith")).unwrap();
        }
        {
            let mut stream = comp
                .create_stream("/__recip_version1.0_#00000000/__substg1.0_3003001F")
                .unwrap();
            stream
                .write_all(&encode_utf16le("bob@example.com"))
                .unwrap();
        }

        // Write Plain body
        {
            let mut stream = comp.create_stream("/__substg1.0_1000001F").unwrap();
            stream
                .write_all(&encode_utf16le(
                    "Hello Bob,\n\nHere is the latest progress report on the document pipeline.\n\nBest regards,\nAlice",
                ))
                .unwrap();
        }

        // Attachment
        comp.create_storage("/__attach_version1.0_#00000000")
            .unwrap();
        {
            let mut stream = comp
                .create_stream("/__attach_version1.0_#00000000/__substg1.0_3707001F")
                .unwrap();
            stream.write_all(&encode_utf16le("report.pdf")).unwrap();
        }
        {
            let mut stream = comp
                .create_stream("/__attach_version1.0_#00000000/__substg1.0_37010102")
                .unwrap();
            stream.write_all(b"%PDF-1.4 dummy pdf attachment").unwrap();
        }

        let bytes = comp.into_inner().into_inner();
        let fixtures_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("fixtures/email");
        let seeds_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("fuzz/seeds/reader_msg");
        std::fs::create_dir_all(&fixtures_dir).unwrap();
        std::fs::create_dir_all(&seeds_dir).unwrap();
        std::fs::write(fixtures_dir.join("sample.msg"), &bytes).unwrap();
        std::fs::write(seeds_dir.join("simple.msg"), &bytes).unwrap();
    }
}
