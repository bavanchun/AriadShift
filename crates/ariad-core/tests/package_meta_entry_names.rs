use std::io::{Cursor, Read, Seek, SeekFrom};

use ariad_core::package_meta::stamp_bytes;

const TIMESTAMP: &str = "2025-10-09T08:53:20Z";
const UTF8_OVERLAP: &[u8] = include_bytes!("../../../fuzz/seeds/docx_meta/cp437-utf8-overlap.zip");
const CP437_FF: &[u8] = include_bytes!("../../../fuzz/seeds/docx_meta/cp437-ff.zip");

/// Reads raw wire records without the ZIP library's name decoder or index.
fn central_names(bytes: &[u8]) -> Vec<(u16, Vec<u8>)> {
    let end = bytes
        .windows(4)
        .rposition(|sig| sig == b"PK\x05\x06")
        .unwrap();
    let count = u16::from_le_bytes(bytes[end + 10..end + 12].try_into().unwrap());
    let offset = u32::from_le_bytes(bytes[end + 16..end + 20].try_into().unwrap());
    let mut input = Cursor::new(bytes);
    input.set_position(u64::from(offset));
    let mut names = Vec::new();
    for _ in 0..count {
        let mut header = [0; 46];
        input.read_exact(&mut header).unwrap();
        assert_eq!(&header[..4], b"PK\x01\x02");
        let flags = u16::from_le_bytes(header[8..10].try_into().unwrap());
        let name_len = u16::from_le_bytes(header[28..30].try_into().unwrap());
        let extra_len = u16::from_le_bytes(header[30..32].try_into().unwrap());
        let comment_len = u16::from_le_bytes(header[32..34].try_into().unwrap());
        let mut name = vec![0; usize::from(name_len)];
        input.read_exact(&mut name).unwrap();
        names.push((flags, name));
        input
            .seek(SeekFrom::Current(
                i64::from(extra_len) + i64::from(comment_len),
            ))
            .unwrap();
    }
    names
}

/// A consumer decoder independent of zip, covering the bytes in these probes.
fn consumer_name(entry: &(u16, Vec<u8>)) -> String {
    if entry.0 & 0x800 != 0 {
        return String::from_utf8(entry.1.clone()).unwrap();
    }
    entry
        .1
        .iter()
        .map(|byte| match byte {
            0..=0x7f => char::from(*byte),
            0xc3 => '\u{251c}',
            0xa9 => '\u{2310}',
            0xff => '\u{a0}',
            _ => panic!("unexpected probe name byte: {byte}"),
        })
        .collect()
}

#[test]
fn changing_flagless_utf8_consumer_names_requires_a_deliberate_api_decision() {
    let before = central_names(UTF8_OVERLAP);
    assert_eq!(before[0], (0, b"\xc3\xa9.txt".to_vec()));
    // A spec consumer decodes flag-zero c3/a9 as CP437 U+251C/U+2310.
    let cp437_name = consumer_name(&before[0]);
    assert_eq!(cp437_name, "├⌐.txt");
    let stamped = stamp_bytes(UTF8_OVERLAP, false, TIMESTAMP).unwrap();
    let after = central_names(&stamped);
    assert_eq!(before.len(), after.len());
    assert_eq!(after[0], (0x800, b"\xc3\xa9.txt".to_vec()));
    // UTF-8 flag + these bytes gives a different consumer-visible name.
    assert_eq!(consumer_name(&after[0]), "é.txt");
    assert_ne!(cp437_name, consumer_name(&after[0]));
}

#[test]
fn non_utf8_cp437_name_retains_its_consumer_visible_meaning() {
    let before = central_names(CP437_FF);
    assert_eq!(before[0], (0, b"\xff.txt".to_vec()));
    let stamped = stamp_bytes(CP437_FF, false, TIMESTAMP).unwrap();
    let after = central_names(&stamped);
    assert_eq!(before.len(), after.len());
    // CP437 ff is U+00A0; writing its UTF-8 spelling keeps the same name.
    assert_eq!(after[0], (0x800, "\u{a0}.txt".as_bytes().to_vec()));
    assert_eq!(consumer_name(&before[0]), consumer_name(&after[0]));
}
