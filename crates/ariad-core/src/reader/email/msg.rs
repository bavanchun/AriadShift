//! Microsoft Outlook .msg reader producing deterministic Ariad IR.

use std::io::{Cursor, Read};

use crate::{
    format::Format,
    ir::{Block, Document, Inline, ListItem, Metadata},
    limits::{Limits, LimitsError},
    reader::{
        common::{ReadOutput, nfc},
        email::strip_bidi_controls,
    },
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

/// CRC lookup table of MS-OXRTFCP 2.1.2.2.1 (reflected polynomial 0xEDB88320).
const RTF_CRC_TABLE: [u32; 256] = {
    let mut table = [0u32; 256];
    let mut n = 0;
    while n < 256 {
        let mut value = n as u32;
        let mut bit = 0;
        while bit < 8 {
            value = if value & 1 == 1 {
                (value >> 1) ^ 0xEDB8_8320
            } else {
                value >> 1
            };
            bit += 1;
        }
        table[n] = value;
        n += 1;
    }
    table
};

/// CRC of MS-OXRTFCP 2.1.3.2: initial value 0, no final inversion.
fn rtf_crc(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0, |crc, &byte| {
        RTF_CRC_TABLE[((crc ^ u32::from(byte)) & 0xff) as usize] ^ (crc >> 8)
    })
}

/// Error text for a compressed RTF stream whose CRC does not match.
const RTF_CRC_MISMATCH: &str = "RTF CRC mismatch";

/// Decompresses LZFu-compressed RTF data per MS-OXRTFCP specification.
///
/// For compressed input the CRC of every CONTENTS byte (MS-OXRTFCP
/// 2.2.3.2) must match the header, or the input is treated as corrupt.
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

    // "MEMA" or "MELA" magic: uncompressed
    if magic == 0x414d_454d || magic == 0x414c_454d {
        let end = 16usize.saturating_add(uncomp_size).min(compressed.len());
        return Ok(compressed[16..end].to_vec());
    }

    // "LZFu" magic: compressed
    if magic != 0x7546_5a4c {
        return Err(format!("unknown RTF compression magic: 0x{magic:08x}"));
    }

    let max_in = 4usize.saturating_add(comp_size).min(compressed.len());
    let stored_crc = u32::from_le_bytes(
        compressed[12..16]
            .try_into()
            .map_err(|_| "invalid header".to_string())?,
    );
    if rtf_crc(&compressed[16..max_in.max(16)]) != stored_crc {
        return Err(RTF_CRC_MISMATCH.to_string());
    }

    const RTF_PREBUF: &[u8; 207] = b"{\\rtf1\\ansi\\mac\\deff0\\deftab720{\\fonttbl;}{\\f0\\fnil \\froman \\fswiss \\fmodern \\fscript \\fdecor MS Sans SerifSymbolArialTimes New RomanCourier{\\colortbl\\red0\\green0\\blue0\r\n\\par \\pard\\plain\\f0\\fs20\\b\\i\\u\\tab\\tx";

    let mut dict = [b' '; 4096];
    dict[..RTF_PREBUF.len()].copy_from_slice(RTF_PREBUF);
    let mut write_offset = RTF_PREBUF.len();

    let mut output = Vec::with_capacity(uncomp_size.min(10 * 1024 * 1024));
    let mut in_pos = 16;

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

                let mut offset = (token >> 4) as usize;
                let length = ((token & 0x0F) + 2) as usize;

                if offset == write_offset {
                    // End of stream marker
                    return Ok(output);
                }

                for _ in 0..length {
                    if output.len() >= uncomp_size {
                        break;
                    }
                    let b = dict[offset];
                    output.push(b);
                    dict[write_offset] = b;
                    write_offset = (write_offset + 1) % 4096;
                    offset = (offset + 1) % 4096;
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

/// A stream that exists but is larger than the reader's cap.
struct StreamTooLarge;

/// Reads a stream into a `Vec<u8>`, reading at most `max_bytes + 1` bytes.
///
/// Returns `Ok(None)` when the stream is missing or unreadable.
fn read_stream_checked<F: Read + std::io::Seek>(
    comp: &mut cfb::CompoundFile<F>,
    path: &str,
    max_bytes: usize,
) -> Result<Option<Vec<u8>>, StreamTooLarge> {
    let Ok(mut stream) = comp.open_stream(path) else {
        return Ok(None);
    };
    let mut buf = Vec::new();
    let mut take = (&mut stream).take(max_bytes as u64 + 1);
    if take.read_to_end(&mut buf).is_err() {
        return Ok(None);
    }
    if buf.len() > max_bytes {
        Err(StreamTooLarge)
    } else {
        Ok(Some(buf))
    }
}

/// Helper to read a stream into a Vec<u8> with size bounds.
fn read_stream_bounded<F: Read + std::io::Seek>(
    comp: &mut cfb::CompoundFile<F>,
    path: &str,
    max_bytes: usize,
) -> Option<Vec<u8>> {
    read_stream_checked(comp, path, max_bytes).ok().flatten()
}

/// Reads string from either UTF-16LE (`*001F`) or String8 (`*001E`) stream,
/// reporting a stream that exceeds `max_bytes` when no variant is usable.
fn read_string_property_checked<F: Read + std::io::Seek>(
    comp: &mut cfb::CompoundFile<F>,
    parent: &str,
    prop_id: &str,
    max_bytes: usize,
) -> Result<Option<String>, StreamTooLarge> {
    let prefix = if parent == "/" {
        format!("/__substg1.0_{prop_id}")
    } else {
        format!("{parent}/__substg1.0_{prop_id}")
    };

    let mut too_large = false;
    for (suffix, decode) in [
        ("001F", decode_utf16le as fn(&[u8]) -> String),
        ("001E", decode_string8),
    ] {
        match read_stream_checked(comp, &format!("{prefix}{suffix}"), max_bytes) {
            Ok(Some(bytes)) => {
                let s = decode(&bytes);
                if !s.is_empty() {
                    return Ok(Some(s));
                }
            }
            Ok(None) => {}
            Err(StreamTooLarge) => too_large = true,
        }
    }

    if too_large {
        Err(StreamTooLarge)
    } else {
        Ok(None)
    }
}

/// Reads string from either UTF-16LE (`*001F`) or String8 (`*001E`) stream.
fn read_string_property<F: Read + std::io::Seek>(
    comp: &mut cfb::CompoundFile<F>,
    parent: &str,
    prop_id: &str,
    max_bytes: usize,
) -> Option<String> {
    read_string_property_checked(comp, parent, prop_id, max_bytes)
        .ok()
        .flatten()
}

/// Maximum size of a recipient or attachment `__properties_version1.0` stream.
const MAX_SUB_PROPERTIES_BYTES: usize = 4096;

/// Header label for recipients whose To, Cc or Bcc type cannot be trusted.
const UNKNOWN_RECIPIENTS_LABEL: &str = "Recipients (type unknown): ";

/// Header bucket of one MSG recipient.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RecipientType {
    To,
    Cc,
    Bcc,
    /// The properties stream exceeds its cap, so the type was not read.
    Unknown,
    /// `PR_RECIPIENT_TYPE` holds a value other than To, Cc or Bcc.
    Unrecognized,
}

/// Reads `PR_RECIPIENT_TYPE` (0x0C15, PT_LONG) from a recipient
/// `__properties_version1.0` stream.
///
/// A missing stream or property keeps the historical To default.
fn recipient_type(prop_bytes: Option<&[u8]>) -> RecipientType {
    let Some(records) = prop_bytes.and_then(|bytes| bytes.get(8..)) else {
        return RecipientType::To;
    };
    let (chunks, _) = records.as_chunks::<16>();
    let value = chunks.iter().find_map(|chunk| {
        let prop_type = u16::from_le_bytes([chunk[0], chunk[1]]);
        let prop_id = u16::from_le_bytes([chunk[2], chunk[3]]);
        (prop_id == 0x0C15 && prop_type == 0x0003)
            .then(|| u32::from_le_bytes([chunk[8], chunk[9], chunk[10], chunk[11]]))
    });
    match value {
        None | Some(1) => RecipientType::To,
        Some(2) => RecipientType::Cc,
        Some(3) => RecipientType::Bcc,
        Some(_) => RecipientType::Unrecognized,
    }
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
    let mut bcc_recipients = Vec::new();
    let mut unknown_recipients = Vec::new();
    let mut oversized_properties = 0usize;
    let mut unrecognized_types = 0usize;
    let mut oversized_strings = 0usize;

    for r_path in &recip_storages {
        let mut string_too_large = false;
        let mut string_property = |comp: &mut cfb::CompoundFile<_>, prop_id| {
            read_string_property_checked(comp, r_path, prop_id, max_stream_bytes).unwrap_or_else(
                |StreamTooLarge| {
                    string_too_large = true;
                    None
                },
            )
        };
        let r_name = string_property(&mut comp, "3001");
        let r_email = string_property(&mut comp, "3003");
        if string_too_large {
            oversized_strings += 1;
        }

        let prop_path = format!("{r_path}/__properties_version1.0");
        let recip_type = match read_stream_checked(&mut comp, &prop_path, MAX_SUB_PROPERTIES_BYTES)
        {
            Ok(prop_bytes) => recipient_type(prop_bytes.as_deref()),
            // The type cannot be read, so the recipient may be Cc or Bcc.
            Err(StreamTooLarge) => {
                oversized_properties += 1;
                RecipientType::Unknown
            }
        };
        if recip_type == RecipientType::Unrecognized {
            unrecognized_types += 1;
        }

        let formatted = match (r_name, r_email) {
            (Some(n), Some(e)) => format!("{n} <{e}>"),
            (Some(n), None) => n,
            (None, Some(e)) => e,
            (None, None) => continue,
        };

        match recip_type {
            RecipientType::To => to_recipients.push(formatted),
            RecipientType::Cc => cc_recipients.push(formatted),
            RecipientType::Bcc => bcc_recipients.push(formatted),
            RecipientType::Unknown | RecipientType::Unrecognized => {
                unknown_recipients.push(formatted)
            }
        }
    }

    // Never include recipient data: name only the condition and a count.
    for (count, condition) in [
        (
            oversized_properties,
            "properties stream exceeds the size limit; recipient type is unknown",
        ),
        (
            unrecognized_types,
            "type value is not recognized; recipient type is unknown",
        ),
        (
            oversized_strings,
            "name or address stream exceeds the size limit and was ignored",
        ),
    ] {
        if count > 0 {
            let noun = if count == 1 {
                "recipient"
            } else {
                "recipients"
            };
            warnings.push(Warning::new(
                WarningCode::UnsupportedNode,
                format!("MSG recipient {condition} ({count} {noun})"),
            ));
        }
    }

    let to_str = (!to_recipients.is_empty()).then(|| to_recipients.join(", "));
    let cc_str = (!cc_recipients.is_empty()).then(|| cc_recipients.join(", "));
    let bcc_str = (!bcc_recipients.is_empty()).then(|| bcc_recipients.join(", "));
    let unknown_str = (!unknown_recipients.is_empty()).then(|| unknown_recipients.join(", "));

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
    if let Some(ref bcc_val) = bcc_str {
        header_inlines.push(Inline::Strong {
            content: vec![Inline::Text {
                text: "Bcc: ".to_owned(),
            }],
        });
        header_inlines.push(Inline::Text { text: nfc(bcc_val) });
        header_inlines.push(Inline::LineBreak {});
    }
    if let Some(ref unknown_val) = unknown_str {
        header_inlines.push(Inline::Strong {
            content: vec![Inline::Text {
                text: UNKNOWN_RECIPIENTS_LABEL.to_owned(),
            }],
        });
        header_inlines.push(Inline::Text {
            text: nfc(unknown_val),
        });
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
            id: Default::default(),
            style: None,
            direct: Default::default(),
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
            Err(error) => {
                // The error reports only limits and counts, never body text.
                warnings.push(Warning::new(
                    WarningCode::UnsupportedNode,
                    format!("MSG HTML body could not be converted: {error}; trying other bodies"),
                ));
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
        let decompressed =
            read_stream_bounded(&mut comp, "/__substg1.0_10090102", max_stream_bytes).and_then(
                |data| match decompress_rtf(&data) {
                    Ok(decompressed) => Some(decompressed),
                    Err(error) => {
                        // The error names only the header condition, never body text.
                        warnings.push(Warning::new(
                            WarningCode::UnsupportedNode,
                            format!("MSG RTF body was dropped: {error}"),
                        ));
                        None
                    }
                },
            );
        if let Some(decompressed) = decompressed {
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
            if let Some(prop_bytes) =
                read_stream_bounded(&mut comp, &prop_path, MAX_SUB_PROPERTIES_BYTES)
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

        let clean_name = strip_bidi_controls(&name);
        let item_text = format!("{clean_name} ({size} bytes)");
        attachment_items.push(ListItem {
            checked: None,
            blocks: vec![Block::Paragraph {
                id: Default::default(),
                style: None,
                direct: Default::default(),
                content: vec![Inline::Text {
                    text: nfc(&item_text),
                }],
            }],
        });
    }

    if !attachment_items.is_empty() {
        blocks.push(Block::Heading {
            id: Default::default(),
            style: None,
            direct: Default::default(),
            level: 2,
            content: vec![Inline::Text {
                text: "Attachments".to_owned(),
            }],
        });
        blocks.push(Block::List {
            id: Default::default(),
            style: None,
            direct: Default::default(),
            ordered: false,
            start: None,
            tight: true,
            items: attachment_items,
        });
    }

    if blocks.len() > limits.max_blocks as usize {
        return Err(ReadError::TooManyBlocks {
            actual: blocks.len() as u64,
            limit: limits.max_blocks,
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

    Ok(ReadOutput {
        document: document.finalize_import(),
        warnings,
    })
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
                    Block::Paragraph { content, .. } => content.iter().any(|c| match c {
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
    fn lzfu_decompression_spec_vector() {
        let comp = b"#\x00\x00\x00\"\x00\x00\x00LZFu3\\\xe8t\x03\x00\n\x00rcpg125\x922\n\xf3 t\x07\x90t}\x0f\x10";
        let out = decompress_rtf(comp).expect("LZFu decompression failed");
        assert_eq!(out, b"{\\rtf1\\ansi\\ansicpg1252\\pard test}");
    }

    /// MS-OXRTFCP 3.1.1, "Example 1: Simple Compressed RTF".
    const OXRTFCP_EXAMPLE_1: &[u8] = &[
        0x2d, 0x00, 0x00, 0x00, 0x2b, 0x00, 0x00, 0x00, 0x4c, 0x5a, 0x46, 0x75, 0xf1, 0xc5, 0xc7,
        0xa7, 0x03, 0x00, 0x0a, 0x00, 0x72, 0x63, 0x70, 0x67, 0x31, 0x32, 0x35, 0x42, 0x32, 0x0a,
        0xf3, 0x20, 0x68, 0x65, 0x6c, 0x09, 0x00, 0x20, 0x62, 0x77, 0x05, 0xb0, 0x6c, 0x64, 0x7d,
        0x0a, 0x80, 0x0f, 0xa0,
    ];

    /// MS-OXRTFCP 3.1.2, "Reading a Token from the Dictionary that Crosses
    /// WritePosition".
    const OXRTFCP_EXAMPLE_2: &[u8] = &[
        0x1a, 0x00, 0x00, 0x00, 0x1c, 0x00, 0x00, 0x00, 0x4c, 0x5a, 0x46, 0x75, 0xe2, 0xd4, 0x4b,
        0x51, 0x41, 0x00, 0x04, 0x20, 0x57, 0x58, 0x59, 0x5a, 0x0d, 0x6e, 0x7d, 0x01, 0x0e, 0xb0,
    ];

    #[test]
    fn lzfu_decompresses_ms_oxrtfcp_examples() {
        assert_eq!(
            decompress_rtf(OXRTFCP_EXAMPLE_1).unwrap(),
            b"{\\rtf1\\ansi\\ansicpg1252\\pard hello world}\r\n"
        );
        assert_eq!(
            decompress_rtf(OXRTFCP_EXAMPLE_2).unwrap(),
            b"{\\rtf1 WXYZWXYZWXYZWXYZWXYZ}"
        );
    }

    #[test]
    fn rtf_crc_matches_ms_oxrtfcp() {
        // 2.1.2.2.1 table entries and the 3.3.1 worked example.
        assert_eq!(RTF_CRC_TABLE[1], 0x7707_3096);
        assert_eq!(RTF_CRC_TABLE[3], 0x9909_51ba);
        assert_eq!(RTF_CRC_TABLE[255], 0x2d02_ef8d);
        assert_eq!(rtf_crc(&[0x03]), 0x9909_51ba);
        assert_eq!(rtf_crc(&OXRTFCP_EXAMPLE_1[16..]), 0xa7c7_c5f1);
        assert_eq!(rtf_crc(&OXRTFCP_EXAMPLE_2[16..]), 0x514b_d4e2);
    }

    #[test]
    fn lzfu_rejects_crc_mismatch() {
        for example in [OXRTFCP_EXAMPLE_1, OXRTFCP_EXAMPLE_2] {
            let mut corrupt = example.to_vec();
            corrupt[12] ^= 0x01;
            assert_eq!(decompress_rtf(&corrupt).unwrap_err(), RTF_CRC_MISMATCH);
            // A changed CONTENTS byte is caught as well.
            let mut corrupt = example.to_vec();
            *corrupt.last_mut().unwrap() ^= 0x01;
            assert_eq!(decompress_rtf(&corrupt).unwrap_err(), RTF_CRC_MISMATCH);
        }
    }

    #[test]
    fn rtf_fuzz_seeds_cover_valid_and_mismatched_crc() {
        let valid = include_bytes!("../../../../../fuzz/seeds/reader_msg/rtf.msg");
        let out = read(valid, &Limits::local()).unwrap();
        assert_eq!(out.warnings.len(), 1, "{:?}", out.warnings);
        assert_eq!(
            out.warnings[0].message,
            "RTF-compressed body converted to plain text"
        );
        let bad = include_bytes!("../../../../../fuzz/seeds/reader_msg/rtf-bad-crc.msg");
        let out = read(bad, &Limits::local()).unwrap();
        assert_eq!(out.warnings.len(), 1, "{:?}", out.warnings);
        assert_eq!(
            out.warnings[0].message,
            "MSG RTF body was dropped: RTF CRC mismatch"
        );
    }

    #[test]
    fn mela_follows_ms_oxrtfcp_uncompressed_layout() {
        // MS-OXRTFCP 2.1.3.1.1 and 2.3.3.1: COMPSIZE = content length + 12,
        // RAWSIZE = content length, COMPTYPE = "MELA", CRC = 0, raw CONTENTS.
        // The specification gives no byte example for this layout.
        let content = b"{\\rtf1\\ansi\\ansicpg1252\\pard hello world}\r\n";
        let mut mela = Vec::new();
        mela.extend_from_slice(&(content.len() as u32 + 12).to_le_bytes());
        mela.extend_from_slice(&(content.len() as u32).to_le_bytes());
        mela.extend_from_slice(b"MELA");
        mela.extend_from_slice(&0u32.to_le_bytes());
        mela.extend_from_slice(content);
        assert_eq!(decompress_rtf(&mela).unwrap(), content);
        // 2.2.3.1: a reader MUST NOT validate the CRC of uncompressed input.
        mela[12..16].copy_from_slice(&0xdead_beefu32.to_le_bytes());
        assert_eq!(decompress_rtf(&mela).unwrap(), content);
    }

    #[test]
    fn lzfu_rejects_short_and_unknown_headers() {
        assert_eq!(
            decompress_rtf(&OXRTFCP_EXAMPLE_1[..15]).unwrap_err(),
            "RTF compressed data too short"
        );
        let mut unknown = OXRTFCP_EXAMPLE_1.to_vec();
        unknown[8..12].copy_from_slice(b"ABCD");
        assert_eq!(
            decompress_rtf(&unknown).unwrap_err(),
            "unknown RTF compression magic: 0x44434241"
        );
    }

    #[test]
    fn uncompressed_rtf_mema_and_mela() {
        // MEMA
        let mut mema = Vec::new();
        mema.extend_from_slice(&16u32.to_le_bytes()); // comp_size = 12 + len(payload)
        mema.extend_from_slice(&4u32.to_le_bytes()); // uncomp_size = 4
        mema.extend_from_slice(&0x414d_454du32.to_le_bytes()); // "MEMA"
        mema.extend_from_slice(&0u32.to_le_bytes()); // crc
        mema.extend_from_slice(b"test");
        assert_eq!(decompress_rtf(&mema).unwrap(), b"test");

        // MELA
        let mut mela = Vec::new();
        mela.extend_from_slice(&16u32.to_le_bytes());
        mela.extend_from_slice(&4u32.to_le_bytes());
        mela.extend_from_slice(&0x414c_454du32.to_le_bytes()); // "MELA"
        mela.extend_from_slice(&0u32.to_le_bytes());
        mela.extend_from_slice(b"test");
        assert_eq!(decompress_rtf(&mela).unwrap(), b"test");
    }

    #[test]
    fn parses_msg_with_to_cc_bcc() {
        use std::io::Write;
        let mut comp = cfb::CompoundFile::create(Cursor::new(Vec::new())).unwrap();

        // Recipient 1: To
        comp.create_storage("/__recip_version1.0_#00000000")
            .unwrap();
        {
            let mut s = comp
                .create_stream("/__recip_version1.0_#00000000/__substg1.0_3001001F")
                .unwrap();
            s.write_all(&encode_utf16le("To Recipient")).unwrap();
        }
        {
            let mut s = comp
                .create_stream("/__recip_version1.0_#00000000/__properties_version1.0")
                .unwrap();
            // 8 byte header + 16 byte property entry (prop_type=0x0003, prop_id=0x0C15, val=1)
            let mut p = vec![0u8; 8];
            p.extend_from_slice(&0x0003u16.to_le_bytes());
            p.extend_from_slice(&0x0C15u16.to_le_bytes());
            p.extend_from_slice(&0u32.to_le_bytes()); // flags
            p.extend_from_slice(&1u32.to_le_bytes()); // value = 1 (MAPI_TO)
            p.extend_from_slice(&0u32.to_le_bytes()); // padding to 16 bytes
            s.write_all(&p).unwrap();
        }

        // Recipient 2: Cc
        comp.create_storage("/__recip_version1.0_#00000001")
            .unwrap();
        {
            let mut s = comp
                .create_stream("/__recip_version1.0_#00000001/__substg1.0_3001001F")
                .unwrap();
            s.write_all(&encode_utf16le("Cc Recipient")).unwrap();
        }
        {
            let mut s = comp
                .create_stream("/__recip_version1.0_#00000001/__properties_version1.0")
                .unwrap();
            let mut p = vec![0u8; 8];
            p.extend_from_slice(&0x0003u16.to_le_bytes());
            p.extend_from_slice(&0x0C15u16.to_le_bytes());
            p.extend_from_slice(&0u32.to_le_bytes());
            p.extend_from_slice(&2u32.to_le_bytes()); // value = 2 (MAPI_CC)
            p.extend_from_slice(&0u32.to_le_bytes());
            s.write_all(&p).unwrap();
        }

        // Recipient 3: Bcc
        comp.create_storage("/__recip_version1.0_#00000002")
            .unwrap();
        {
            let mut s = comp
                .create_stream("/__recip_version1.0_#00000002/__substg1.0_3001001F")
                .unwrap();
            s.write_all(&encode_utf16le("Bcc Recipient")).unwrap();
        }
        {
            let mut s = comp
                .create_stream("/__recip_version1.0_#00000002/__properties_version1.0")
                .unwrap();
            let mut p = vec![0u8; 8];
            p.extend_from_slice(&0x0003u16.to_le_bytes());
            p.extend_from_slice(&0x0C15u16.to_le_bytes());
            p.extend_from_slice(&0u32.to_le_bytes());
            p.extend_from_slice(&3u32.to_le_bytes()); // value = 3 (MAPI_BCC)
            p.extend_from_slice(&0u32.to_le_bytes());
            s.write_all(&p).unwrap();
        }

        let bytes = comp.into_inner().into_inner();
        let out = read(&bytes, &Limits::local()).unwrap();

        let header_fields = match &out.document.body[0] {
            Block::Paragraph { content, .. } => {
                let mut fields = std::collections::HashMap::new();
                let mut current_label: Option<String> = None;
                for inln in content {
                    match inln {
                        Inline::Strong { content } => {
                            if let Some(Inline::Text { text }) = content.first() {
                                current_label = Some(text.trim_end_matches(": ").to_string());
                            }
                        }
                        Inline::Text { text } => {
                            if let Some(label) = current_label.take() {
                                fields.insert(label, text.clone());
                            }
                        }
                        _ => {}
                    }
                }
                fields
            }
            _ => panic!("Expected header paragraph"),
        };
        assert_eq!(
            header_fields.get("To").map(String::as_str),
            Some("To Recipient")
        );
        assert_eq!(
            header_fields.get("Cc").map(String::as_str),
            Some("Cc Recipient")
        );
        assert_eq!(
            header_fields.get("Bcc").map(String::as_str),
            Some("Bcc Recipient")
        );
        assert!(!header_fields["To"].contains("Cc Recipient"));
        assert!(!header_fields["To"].contains("Bcc Recipient"));
        assert!(!header_fields["Cc"].contains("To Recipient"));
        assert!(!header_fields["Cc"].contains("Bcc Recipient"));
        assert!(!header_fields["Bcc"].contains("To Recipient"));
        assert!(!header_fields["Bcc"].contains("Cc Recipient"));
    }

    #[test]
    fn parses_msg_with_html_body() {
        use std::io::Write;
        let mut comp = cfb::CompoundFile::create(Cursor::new(Vec::new())).unwrap();
        {
            let mut stream = comp.create_stream("/__substg1.0_10130102").unwrap();
            stream.write_all(b"<p>Hello <b>from HTML</b></p>").unwrap();
        }
        let bytes = comp.into_inner().into_inner();
        let out = read(&bytes, &Limits::local()).unwrap();
        let found = out.document.body.iter().any(|b| match b {
            Block::Paragraph { content, .. } => {
                content.iter().any(|c| matches!(c, Inline::Strong { .. }))
            }
            _ => false,
        });
        assert!(found, "HTML bold tag should be parsed");
    }

    #[test]
    fn parses_msg_with_rtf_compressed_body() {
        use std::io::Write;
        let mut comp = cfb::CompoundFile::create(Cursor::new(Vec::new())).unwrap();
        {
            let mut stream = comp.create_stream("/__substg1.0_10090102").unwrap();
            stream
                .write_all(b"#\x00\x00\x00\"\x00\x00\x00LZFu3\\\xe8t\x03\x00\n\x00rcpg125\x922\n\xf3 t\x07\x90t}\x0f\x10")
                .unwrap();
        }
        let bytes = comp.into_inner().into_inner();
        let out = read(&bytes, &Limits::local()).unwrap();
        assert!(
            out.warnings
                .iter()
                .any(|w| w.code == WarningCode::UnsupportedNode)
        );
        let found = out.document.body.iter().any(|b| match b {
            Block::Paragraph { content, .. } => content.iter().any(|c| match c {
                Inline::Text { text } => text.contains("test"),
                _ => false,
            }),
            _ => false,
        });
        assert!(found, "RTF body content should be extracted");
    }

    #[test]
    fn rtf_crc_mismatch_drops_body_with_warning() {
        use std::io::Write;
        let mut stream_bytes = OXRTFCP_EXAMPLE_1.to_vec();
        stream_bytes[12..16].copy_from_slice(&0u32.to_le_bytes());
        let mut comp = cfb::CompoundFile::create(Cursor::new(Vec::new())).unwrap();
        {
            let mut stream = comp.create_stream("/__substg1.0_0037001F").unwrap();
            stream.write_all(&encode_utf16le("Subject line")).unwrap();
        }
        {
            let mut stream = comp.create_stream("/__substg1.0_10090102").unwrap();
            stream.write_all(&stream_bytes).unwrap();
        }
        let bytes = comp.into_inner().into_inner();
        let out = read(&bytes, &Limits::local()).unwrap();
        assert_eq!(out.warnings.len(), 1, "{:?}", out.warnings);
        assert_eq!(out.warnings[0].code, WarningCode::UnsupportedNode);
        assert_eq!(
            out.warnings[0].message,
            "MSG RTF body was dropped: RTF CRC mismatch"
        );
        // No other body exists, so only the header paragraph remains.
        assert_eq!(out.document.body.len(), 1);
        let Block::Paragraph { content, .. } = &out.document.body[0] else {
            panic!("expected the header paragraph, got {:?}", out.document.body);
        };
        assert_eq!(
            content,
            &vec![
                Inline::Strong {
                    content: vec![Inline::Text {
                        text: "Subject: ".to_owned()
                    }]
                },
                Inline::Text {
                    text: "Subject line".to_owned()
                },
            ]
        );
    }

    #[test]
    fn parses_msg_with_filetime_date() {
        use std::io::Write;
        let mut comp = cfb::CompoundFile::create(Cursor::new(Vec::new())).unwrap();
        {
            let mut stream = comp.create_stream("/__properties_version1.0").unwrap();
            let mut p = vec![0u8; 32]; // 32-byte header for root properties
            // PT_SYSTIME = 0x0040, PR_CLIENT_SUBMIT_TIME = 0x0039
            p.extend_from_slice(&0x0040u16.to_le_bytes());
            p.extend_from_slice(&0x0039u16.to_le_bytes());
            p.extend_from_slice(&0u32.to_le_bytes()); // flags
            // 2026-11-04T12:00:00Z: (1_793_793_600 s + 11_644_473_600 s) * 10^7.
            let ft: i64 = 134_382_672_000_000_000;
            p.extend_from_slice(&ft.to_le_bytes());
            stream.write_all(&p).unwrap();
        }
        let bytes = comp.into_inner().into_inner();
        let out = read(&bytes, &Limits::local()).unwrap();
        assert_eq!(
            out.document.meta.date.as_deref(),
            Some("2026-11-04T12:00:00Z")
        );
    }

    #[test]
    fn filetime_boundaries_format_exactly() {
        const UNIX_EPOCH: i64 = 116_444_736_000_000_000;
        assert_eq!(
            format_filetime_rfc3339(UNIX_EPOCH).as_deref(),
            Some("1970-01-01T00:00:00Z")
        );
        // Instants before 1970 are not formatted.
        assert_eq!(format_filetime_rfc3339(UNIX_EPOCH - 1), None);
        assert_eq!(format_filetime_rfc3339(0), None);
        // Leap day, with the sub-second remainder truncated.
        assert_eq!(
            format_filetime_rfc3339(133_537_247_999_999_999).as_deref(),
            Some("2024-02-29T23:59:59Z")
        );
    }

    #[test]
    fn pre_1970_filetime_leaves_date_unset() {
        use std::io::Write;
        let mut comp = cfb::CompoundFile::create(Cursor::new(Vec::new())).unwrap();
        {
            let mut stream = comp.create_stream("/__properties_version1.0").unwrap();
            let mut p = vec![0u8; 32];
            p.extend_from_slice(&0x0040u16.to_le_bytes());
            p.extend_from_slice(&0x0039u16.to_le_bytes());
            p.extend_from_slice(&0u32.to_le_bytes());
            p.extend_from_slice(&(116_444_736_000_000_000i64 - 1).to_le_bytes());
            stream.write_all(&p).unwrap();
        }
        let bytes = comp.into_inner().into_inner();
        let out = read(&bytes, &Limits::local()).unwrap();
        assert_eq!(out.document.meta.date, None);
    }

    /// A recipient `__properties_version1.0` stream holding only
    /// `PR_RECIPIENT_TYPE`, padded with zero bytes to `len` when longer.
    fn recipient_properties(recip_type: u32, len: usize) -> Vec<u8> {
        let mut p = vec![0u8; 8];
        p.extend_from_slice(&0x0003u16.to_le_bytes());
        p.extend_from_slice(&0x0C15u16.to_le_bytes());
        p.extend_from_slice(&0u32.to_le_bytes());
        p.extend_from_slice(&recip_type.to_le_bytes());
        p.extend_from_slice(&0u32.to_le_bytes());
        p.resize(p.len().max(len), 0);
        p
    }

    /// Adds recipient storage `index` with a display name, an address and a
    /// properties stream.
    fn add_recipient(
        comp: &mut cfb::CompoundFile<Cursor<Vec<u8>>>,
        index: u32,
        name: &str,
        email: &str,
        properties: &[u8],
    ) {
        use std::io::Write;
        let storage = format!("/__recip_version1.0_#{index:08X}");
        comp.create_storage(&storage).unwrap();
        for (prop_id, value) in [("3001001F", name), ("3003001F", email)] {
            let mut s = comp
                .create_stream(format!("{storage}/__substg1.0_{prop_id}"))
                .unwrap();
            s.write_all(&encode_utf16le(value)).unwrap();
        }
        let mut s = comp
            .create_stream(format!("{storage}/__properties_version1.0"))
            .unwrap();
        s.write_all(properties).unwrap();
    }

    /// Header label to value pairs of the first body paragraph.
    fn header_fields(document: &Document) -> Vec<(String, String)> {
        let Block::Paragraph { content, .. } = &document.body[0] else {
            panic!("expected the header paragraph, got {:?}", document.body);
        };
        let mut fields = Vec::new();
        let mut label = None;
        for inline in content {
            match inline {
                Inline::Strong { content } => {
                    if let Some(Inline::Text { text }) = content.first() {
                        label = Some(text.clone());
                    }
                }
                Inline::Text { text } => {
                    if let Some(label) = label.take() {
                        fields.push((label, text.clone()));
                    }
                }
                _ => {}
            }
        }
        fields
    }

    #[test]
    fn oversized_bcc_properties_stream_is_never_to() {
        let mut comp = cfb::CompoundFile::create(Cursor::new(Vec::new())).unwrap();
        add_recipient(
            &mut comp,
            0,
            "To Person",
            "to@example.com",
            &recipient_properties(1, 0),
        );
        add_recipient(
            &mut comp,
            1,
            "Cc Person",
            "cc@example.com",
            &recipient_properties(2, 0),
        );
        add_recipient(
            &mut comp,
            2,
            "Bcc Person",
            "bcc@example.com",
            &recipient_properties(3, 0),
        );
        add_recipient(
            &mut comp,
            3,
            "Hidden Bcc",
            "hidden@example.com",
            &recipient_properties(3, MAX_SUB_PROPERTIES_BYTES + 1),
        );
        let bytes = comp.into_inner().into_inner();
        let out = read(&bytes, &Limits::local()).unwrap();

        assert_eq!(
            header_fields(&out.document),
            vec![
                ("To: ".to_owned(), "To Person <to@example.com>".to_owned()),
                ("Cc: ".to_owned(), "Cc Person <cc@example.com>".to_owned()),
                (
                    "Bcc: ".to_owned(),
                    "Bcc Person <bcc@example.com>".to_owned()
                ),
                (
                    UNKNOWN_RECIPIENTS_LABEL.to_owned(),
                    "Hidden Bcc <hidden@example.com>".to_owned()
                ),
            ]
        );
        assert_eq!(out.warnings.len(), 1, "{:?}", out.warnings);
        assert_eq!(out.warnings[0].code, WarningCode::UnsupportedNode);
        assert_eq!(
            out.warnings[0].message,
            "MSG recipient properties stream exceeds the size limit; \
             recipient type is unknown (1 recipient)"
        );
        assert!(!out.warnings[0].message.contains("Hidden"));
        assert!(!out.warnings[0].message.contains("hidden@"));

        // Every writer keeps the recipient on the unknown-type line only.
        let typst = crate::writer::typst::write(
            &out.document,
            &crate::writer::typst::TypstOptions::default(),
        )
        .markup;
        let json = serde_json::to_string(&out.document).unwrap();
        for (writer, rendered) in [
            (
                "markdown",
                crate::writer::markdown::write(&out.document).content,
            ),
            ("html", crate::writer::html::write(&out.document).content),
            ("text", crate::writer::text::write(&out.document).content),
            ("typst", typst),
            ("json", json),
        ] {
            let hidden = rendered
                .find("Hidden Bcc")
                .unwrap_or_else(|| panic!("{writer} drops the recipient: {rendered}"));
            // Markdown moves the colon outside the strong span.
            let label = rendered
                .find("Recipients (type unknown)")
                .unwrap_or_else(|| panic!("{writer} drops the label: {rendered}"));
            let bcc = rendered.find("Bcc Person").unwrap();
            // The recipient follows the unknown label, which follows every
            // To, Cc and Bcc value, so it cannot render under any of them.
            assert!(bcc < label && label < hidden, "{writer}: {rendered}");
            assert_eq!(rendered.matches("Hidden Bcc").count(), 1, "{writer}");
        }
    }

    #[test]
    fn oversized_properties_streams_share_one_warning() {
        let mut comp = cfb::CompoundFile::create(Cursor::new(Vec::new())).unwrap();
        for (index, recip_type) in [(0, 1), (1, 2), (2, 3)] {
            add_recipient(
                &mut comp,
                index,
                "Secret Recipient",
                "secret@example.com",
                &recipient_properties(recip_type, MAX_SUB_PROPERTIES_BYTES + 1),
            );
        }
        let bytes = comp.into_inner().into_inner();
        let out = read(&bytes, &Limits::local()).unwrap();
        assert_eq!(out.warnings.len(), 1, "{:?}", out.warnings);
        assert_eq!(out.warnings[0].code, WarningCode::UnsupportedNode);
        assert_eq!(
            out.warnings[0].message,
            "MSG recipient properties stream exceeds the size limit; \
             recipient type is unknown (3 recipients)"
        );
        assert!(!out.warnings[0].message.contains("Secret"));
        assert!(!out.warnings[0].message.contains("secret@"));
        let fields = header_fields(&out.document);
        assert_eq!(fields.len(), 1, "{fields:?}");
        assert_eq!(fields[0].0, UNKNOWN_RECIPIENTS_LABEL);
        assert_eq!(fields[0].1.matches("Secret Recipient").count(), 3);
    }

    #[test]
    fn unrecognized_recipient_type_is_unknown() {
        let mut comp = cfb::CompoundFile::create(Cursor::new(Vec::new())).unwrap();
        add_recipient(
            &mut comp,
            0,
            "Origin",
            "origin@example.com",
            &recipient_properties(0, 0),
        );
        add_recipient(
            &mut comp,
            1,
            "Odd",
            "odd@example.com",
            &recipient_properties(7, 0),
        );
        let bytes = comp.into_inner().into_inner();
        let out = read(&bytes, &Limits::local()).unwrap();
        assert_eq!(
            header_fields(&out.document),
            vec![(
                UNKNOWN_RECIPIENTS_LABEL.to_owned(),
                "Origin <origin@example.com>, Odd <odd@example.com>".to_owned()
            )]
        );
        assert_eq!(out.warnings.len(), 1, "{:?}", out.warnings);
        assert_eq!(
            out.warnings[0].message,
            "MSG recipient type value is not recognized; \
             recipient type is unknown (2 recipients)"
        );
    }

    #[test]
    fn missing_recipient_type_keeps_to_default() {
        assert_eq!(recipient_type(None), RecipientType::To);
        assert_eq!(recipient_type(Some(&[0; 7])), RecipientType::To);
        assert_eq!(recipient_type(Some(&[0; 24])), RecipientType::To);
        assert_eq!(
            recipient_type(Some(&recipient_properties(2, 0))),
            RecipientType::Cc
        );
    }

    #[test]
    fn oversized_recipient_string_stream_is_reported() {
        use std::io::Write;
        let mut comp = cfb::CompoundFile::create(Cursor::new(Vec::new())).unwrap();
        comp.create_storage("/__recip_version1.0_#00000000")
            .unwrap();
        {
            let mut s = comp
                .create_stream("/__recip_version1.0_#00000000/__substg1.0_3001001F")
                .unwrap();
            s.write_all(&encode_utf16le("Recipient")).unwrap();
        }
        let path = "/__recip_version1.0_#00000000";
        let name = encode_utf16le("Recipient");
        assert!(matches!(
            read_string_property_checked(&mut comp, path, "3001", name.len() - 1),
            Err(StreamTooLarge)
        ));
        assert!(matches!(
            read_string_property_checked(&mut comp, path, "3001", name.len()),
            Ok(Some(ref text)) if text == "Recipient"
        ));
        assert!(matches!(
            read_string_property_checked(&mut comp, path, "3003", 1),
            Ok(None)
        ));
    }

    #[test]
    fn html_body_failure_warns_and_falls_back_to_plain_text() {
        use std::io::Write;
        let mut comp = cfb::CompoundFile::create(Cursor::new(Vec::new())).unwrap();
        {
            let mut stream = comp.create_stream("/__substg1.0_10130102").unwrap();
            let html = "<p>secret</p>".repeat(8);
            stream.write_all(html.as_bytes()).unwrap();
        }
        {
            let mut stream = comp.create_stream("/__substg1.0_1000001F").unwrap();
            stream.write_all(&encode_utf16le("Plain body")).unwrap();
        }
        let bytes = comp.into_inner().into_inner();
        let limits = Limits {
            max_blocks: 3,
            ..Limits::local()
        };
        let out = read(&bytes, &limits).unwrap();
        assert_eq!(out.warnings.len(), 1, "{:?}", out.warnings);
        assert_eq!(out.warnings[0].code, WarningCode::UnsupportedNode);
        assert_eq!(
            out.warnings[0].message,
            "MSG HTML body could not be converted: HTML contains more than the configured \
             3 block limit; trying other bodies"
        );
        assert!(!out.warnings[0].message.contains("secret"));
        let plain = out.document.body.iter().any(|b| match b {
            Block::Paragraph { content, .. } => content
                .iter()
                .any(|c| matches!(c, Inline::Text { text } if text == "Plain body")),
            _ => false,
        });
        assert!(plain, "plain text body must replace the failed HTML body");
    }

    #[test]
    fn strips_bidi_controls_in_attachments() {
        use std::io::Write;
        let mut comp = cfb::CompoundFile::create(Cursor::new(Vec::new())).unwrap();
        comp.create_storage("/__attach_version1.0_#00000000")
            .unwrap();
        {
            let mut stream = comp
                .create_stream("/__attach_version1.0_#00000000/__substg1.0_3707001F")
                .unwrap();
            // "evil\u{202E}fdp.exe" (RLO trick)
            stream
                .write_all(&encode_utf16le("evil\u{202E}fdp.exe"))
                .unwrap();
        }
        let bytes = comp.into_inner().into_inner();
        let out = read(&bytes, &Limits::local()).unwrap();
        let has_stripped = out.document.body.iter().any(|b| match b {
            Block::List { items, .. } => items.iter().any(|item| {
                item.blocks.iter().any(|ib| match ib {
                    Block::Paragraph { content, .. } => content.iter().any(|c| match c {
                        Inline::Text { text } => text.contains("evilfdp.exe"),
                        _ => false,
                    }),
                    _ => false,
                })
            }),
            _ => false,
        });
        assert!(has_stripped);
    }

    #[test]
    fn enforces_max_blocks_limit() {
        use std::io::Write;
        let mut comp = cfb::CompoundFile::create(Cursor::new(Vec::new())).unwrap();
        {
            let mut stream = comp.create_stream("/__substg1.0_0037001F").unwrap();
            stream.write_all(&encode_utf16le("Subject")).unwrap();
        }
        {
            let mut stream = comp.create_stream("/__substg1.0_1000001F").unwrap();
            stream.write_all(&encode_utf16le("Body")).unwrap();
        }
        let bytes = comp.into_inner().into_inner();
        let limits = Limits {
            max_blocks: 1,
            ..Limits::local()
        };
        let err = read(&bytes, &limits).unwrap_err();
        match err {
            ReadError::TooManyBlocks { actual, limit } => {
                assert!(actual > 1);
                assert_eq!(limit, 1);
            }
            _ => panic!("Expected TooManyBlocks error, got {err:?}"),
        }
    }
}
