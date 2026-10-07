use std::{
    borrow::Cow,
    collections::HashSet,
    fs::File,
    io::{self, Read, Seek, SeekFrom},
    path::Path,
};

use ariad_core::limits::Limits;
use thiserror::Error;
use zip::ZipArchive;

pub const MAX_ZIP_ENTRY_NAME_BYTES: usize = 2048;
pub const MAX_CENTRAL_DIRECTORY_BYTES: u64 = 32 * 1024 * 1024;

#[derive(Debug, Error)]
pub enum ArchiveError {
    #[error("archive central directory size {size} exceeds the configured limit of {limit}")]
    CentralDirectorySizeExceeded { size: u64, limit: u64 },
    #[error("archive entry count {count} exceeds the configured limit of {limit}")]
    EntryCountExceeded { count: usize, limit: u32 },
    #[error("archive decompressed size {size} exceeds the configured limit of {limit}")]
    DecompressedSizeExceeded { size: u64, limit: u64 },
    #[error("archive entry name `{name}` is invalid or traverses directories")]
    InvalidEntryName { name: String },
    #[error("archive contains duplicate entry name `{name}`")]
    DuplicateEntryName { name: String },
    #[error("archive is encrypted: password-protected archives are not supported")]
    Encrypted,
    #[error(
        "archive entry count mismatch: end-of-central-directory reports {eocd_count}, but zip archive indexed {archive_count}"
    )]
    EntryCountMismatch {
        eocd_count: usize,
        archive_count: usize,
    },
    #[error("I/O error during archive preflight: {0}")]
    Io(#[from] io::Error),
    #[error("ZIP error during archive preflight: {0}")]
    Zip(#[from] zip::result::ZipError),
}

/// End-of-central-directory metadata extracted from raw ZIP records.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EocdInfo {
    pub cd_offset: u64,
    pub cd_size: u64,
    pub entry_count: u64,
}

/// Reads the end-of-central-directory record (standard and Zip64) from the end of the file.
pub fn read_eocd_info(file: &mut File) -> Result<EocdInfo, ArchiveError> {
    let file_len = file.metadata()?.len();
    if file_len < 22 {
        return Err(ArchiveError::Zip(zip::result::ZipError::InvalidArchive(
            Cow::Borrowed("file is too short to be a valid zip archive"),
        )));
    }

    let search_len = (file_len.min(65535 + 22)) as usize;
    let search_start = file_len - search_len as u64;
    file.seek(SeekFrom::Start(search_start))?;
    let mut buf = vec![0u8; search_len];
    file.read_exact(&mut buf)?;

    let mut eocd_idx = None;
    for i in (0..=search_len.saturating_sub(22)).rev() {
        if buf[i..i + 4] == [0x50, 0x4b, 0x05, 0x06] {
            let comment_len = u16::from_le_bytes([buf[i + 20], buf[i + 21]]) as usize;
            if i + 22 + comment_len == search_len {
                eocd_idx = Some(i);
                break;
            }
        }
    }

    let idx = match eocd_idx {
        Some(i) => i,
        None => {
            let mut last = None;
            for i in (0..=search_len.saturating_sub(22)).rev() {
                if buf[i..i + 4] == [0x50, 0x4b, 0x05, 0x06] {
                    last = Some(i);
                    break;
                }
            }
            last.ok_or(ArchiveError::Zip(zip::result::ZipError::InvalidArchive(
                Cow::Borrowed("end of central directory record not found"),
            )))?
        }
    };

    let eocd_pos = search_start + idx as u64;
    let disk_entries = u16::from_le_bytes([buf[idx + 8], buf[idx + 9]]);
    let total_entries = u16::from_le_bytes([buf[idx + 10], buf[idx + 11]]);
    let cd_size =
        u32::from_le_bytes([buf[idx + 12], buf[idx + 13], buf[idx + 14], buf[idx + 15]]) as u64;
    let cd_offset =
        u32::from_le_bytes([buf[idx + 16], buf[idx + 17], buf[idx + 18], buf[idx + 19]]) as u64;

    let has_zip64_locator = idx >= 20 && buf[idx - 20..idx - 16] == [0x50, 0x4b, 0x06, 0x07];
    let is_zip64 = has_zip64_locator
        || total_entries == 0xFFFF
        || cd_size == 0xFFFFFFFF
        || cd_offset == 0xFFFFFFFF
        || disk_entries == 0xFFFF;

    if is_zip64 {
        if eocd_pos < 20 {
            return Err(ArchiveError::Zip(zip::result::ZipError::InvalidArchive(
                Cow::Borrowed("truncated zip64 locator"),
            )));
        }
        file.seek(SeekFrom::Start(eocd_pos - 20))?;
        let mut loc = [0u8; 20];
        file.read_exact(&mut loc)?;
        if loc[0..4] != [0x50, 0x4b, 0x06, 0x07] {
            return Err(ArchiveError::Zip(zip::result::ZipError::InvalidArchive(
                Cow::Borrowed("invalid zip64 locator signature"),
            )));
        }
        let z64_offset = u64::from_le_bytes(loc[8..16].try_into().unwrap());
        let mut z64_record = None;
        if z64_offset.saturating_add(56) <= file_len {
            file.seek(SeekFrom::Start(z64_offset))?;
            let mut z64 = [0u8; 56];
            if file.read_exact(&mut z64).is_ok() && z64[0..4] == [0x50, 0x4b, 0x06, 0x06] {
                z64_record = Some(z64);
            }
        }
        if z64_record.is_none() && idx >= 76 {
            for j in (0..=idx - 20 - 56).rev() {
                if buf[j..j + 4] == [0x50, 0x4b, 0x06, 0x06] {
                    let mut z64 = [0u8; 56];
                    z64.copy_from_slice(&buf[j..j + 56]);
                    z64_record = Some(z64);
                    break;
                }
            }
        }
        let z64 = z64_record.ok_or(ArchiveError::Zip(zip::result::ZipError::InvalidArchive(
            Cow::Borrowed("invalid zip64 end of central directory signature"),
        )))?;
        let z64_entries = u64::from_le_bytes(z64[32..40].try_into().unwrap());
        let z64_cd_size = u64::from_le_bytes(z64[40..48].try_into().unwrap());
        let z64_cd_offset = u64::from_le_bytes(z64[48..56].try_into().unwrap());
        Ok(EocdInfo {
            cd_offset: z64_cd_offset,
            cd_size: z64_cd_size,
            entry_count: z64_entries,
        })
    } else {
        Ok(EocdInfo {
            cd_offset,
            cd_size,
            entry_count: total_entries as u64,
        })
    }
}

/// Reads entry names from a ZIP central directory with entry count and name length caps.
///
/// Bounded to `max_entries` and `max_name_bytes`. Returns `None` if the file is not a valid
/// ZIP archive or if the entry count in EOCD exceeds `max_entries`.
pub fn read_zip_entry_names(
    file: &mut File,
    max_entries: usize,
    max_name_bytes: usize,
) -> Option<Vec<String>> {
    let limits = Limits {
        max_archive_entries: max_entries as u32,
        ..Limits::local()
    };
    let summary = parse_central_directory(file, &limits).ok()?;
    if summary
        .entry_names
        .iter()
        .any(|name| name.len() > max_name_bytes)
    {
        return None;
    }
    Some(summary.entry_names)
}

/// Summary of central directory headers parsed from an archive.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ZipArchiveSummary {
    pub entry_count: usize,
    pub entry_names: Vec<String>,
    pub declared_decompressed_bytes: u64,
}

/// Reads the central directory record and parses entry headers with strict bounds.
///
/// Validates EOCD/Zip64 EOCD records, checks entry count and CD size bounds
/// BEFORE allocating or reading entry headers, and verifies each entry name
/// (no traversal, no duplicate names, bounded name lengths).
pub fn parse_central_directory(
    file: &mut File,
    limits: &Limits,
) -> Result<ZipArchiveSummary, ArchiveError> {
    let file_len = file.metadata()?.len();
    let eocd = read_eocd_info(file)?;
    if eocd.cd_size > MAX_CENTRAL_DIRECTORY_BYTES {
        return Err(ArchiveError::CentralDirectorySizeExceeded {
            size: eocd.cd_size,
            limit: MAX_CENTRAL_DIRECTORY_BYTES,
        });
    }
    if eocd.entry_count > limits.max_archive_entries as u64 {
        return Err(ArchiveError::EntryCountExceeded {
            count: if eocd.entry_count > usize::MAX as u64 {
                usize::MAX
            } else {
                eocd.entry_count as usize
            },
            limit: limits.max_archive_entries,
        });
    }
    if eocd.cd_offset.saturating_add(eocd.cd_size) > file_len {
        return Err(ArchiveError::Zip(zip::result::ZipError::InvalidArchive(
            Cow::Borrowed("central directory bounds exceed file size"),
        )));
    }
    file.seek(SeekFrom::Start(eocd.cd_offset))?;
    let count = eocd.entry_count as usize;
    let mut names = Vec::with_capacity(count.min(1024));
    let mut seen_names = HashSet::with_capacity(count.min(1024));
    let mut seen_offsets = HashSet::with_capacity(count.min(1024));
    let mut declared_total: u64 = 0;
    let mut raw_cd_entries = 0usize;
    let mut header = [0u8; 46];

    for _ in 0..count {
        let mut sig_read = 0;
        while sig_read < 4 {
            let n = file.read(&mut header[sig_read..4])?;
            if n == 0 {
                break;
            }
            sig_read += n;
        }
        if sig_read < 4 {
            break;
        }
        if header[0..4] == [0x50, 0x4b, 0x05, 0x06] || header[0..4] == [0x50, 0x4b, 0x06, 0x06] {
            break;
        }
        if header[0..4] != [0x50, 0x4b, 0x01, 0x02] {
            return Err(ArchiveError::Zip(zip::result::ZipError::InvalidArchive(
                Cow::Borrowed("invalid central directory header signature"),
            )));
        }
        file.read_exact(&mut header[4..46])?;
        raw_cd_entries += 1;
        let flags = u16::from_le_bytes([header[8], header[9]]);
        if flags & 1 != 0 {
            return Err(ArchiveError::Encrypted);
        }
        let uncomp_size =
            u32::from_le_bytes([header[24], header[25], header[26], header[27]]) as u64;
        let fn_len = u16::from_le_bytes([header[28], header[29]]) as usize;
        let extra_len = u16::from_le_bytes([header[30], header[31]]) as usize;
        let comment_len = u16::from_le_bytes([header[32], header[33]]) as usize;

        let local_offset =
            u32::from_le_bytes([header[42], header[43], header[44], header[45]]) as u64;

        if fn_len > MAX_ZIP_ENTRY_NAME_BYTES {
            return Err(ArchiveError::InvalidEntryName {
                name: "entry name exceeds maximum allowed length".to_owned(),
            });
        }
        let mut name_bytes = vec![0u8; fn_len];
        file.read_exact(&mut name_bytes)?;
        let name = String::from_utf8_lossy(&name_bytes).into_owned();

        if !seen_names.insert(name.clone()) {
            return Err(ArchiveError::DuplicateEntryName { name });
        }
        validate_entry_name(&name)?;

        if local_offset != 0xFFFF_FFFF && !seen_offsets.insert(local_offset) {
            return Err(ArchiveError::Zip(zip::result::ZipError::InvalidArchive(
                Cow::Borrowed("duplicate local header offset in central directory"),
            )));
        }

        let size = if uncomp_size == 0xFFFF_FFFF {
            read_zip64_uncompressed_size(file, extra_len)?
        } else {
            file.seek(SeekFrom::Current(extra_len as i64))?;
            uncomp_size
        };

        file.seek(SeekFrom::Current(comment_len as i64))?;

        declared_total = declared_total.saturating_add(size);
        if declared_total > limits.max_decompressed_bytes {
            return Err(ArchiveError::DecompressedSizeExceeded {
                size: declared_total,
                limit: limits.max_decompressed_bytes,
            });
        }
        names.push(name);
    }

    if raw_cd_entries != count {
        return Err(ArchiveError::EntryCountMismatch {
            eocd_count: count,
            archive_count: raw_cd_entries,
        });
    }

    Ok(ZipArchiveSummary {
        entry_count: count,
        entry_names: names,
        declared_decompressed_bytes: declared_total,
    })
}

/// Preflights an untrusted ZIP archive (e.g. DOCX or EPUB) before any processing starts.
///
/// 1. Reads raw EOCD/Zip64 and central directory headers with entry count and name length caps.
///    - Checks entry count before allocating or indexing entries.
///    - Detects duplicate entry names (which zip::ZipArchive collapses, allowing bypasses).
///    - Validates entry names (no absolute paths, drive prefixes, or `..` traversals).
///    - Validates declared uncompressed size against max_decompressed_bytes.
/// 2. Stream-inflates every entry through a bounded reader `take(remaining + 1)`. Rejects as soon as
///    the actual inflated bytes exceed max_decompressed_bytes, defeating lying zip bombs.
pub fn preflight_archive(path: &Path, limits: &Limits) -> Result<(), ArchiveError> {
    let mut file = File::open(path)?;
    let summary = parse_central_directory(&mut file, limits)?;

    // Step 2: Stream-inflate every entry through a counting reader bounded by take(remaining + 1)
    file.seek(SeekFrom::Start(0))?;
    let mut archive = ZipArchive::new(&mut file)?;
    let entry_count = archive.len();
    if entry_count != summary.entry_count {
        return Err(ArchiveError::EntryCountMismatch {
            eocd_count: summary.entry_count,
            archive_count: entry_count,
        });
    }

    let mut inflated_total: u64 = 0;
    let mut buffer = [0u8; 64 * 1024];

    for i in 0..entry_count {
        let mut entry = match archive.by_index(i) {
            Ok(e) => e,
            Err(zip::result::ZipError::UnsupportedArchive(msg))
                if msg.to_ascii_lowercase().contains("password") =>
            {
                return Err(ArchiveError::Encrypted);
            }
            Err(e) => return Err(ArchiveError::Zip(e)),
        };
        let remaining = limits.max_decompressed_bytes.saturating_sub(inflated_total);

        let mut counting_reader = (&mut entry).take(remaining.saturating_add(1));
        loop {
            let n = counting_reader.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            inflated_total = inflated_total.saturating_add(n as u64);
            if inflated_total > limits.max_decompressed_bytes {
                return Err(ArchiveError::DecompressedSizeExceeded {
                    size: inflated_total,
                    limit: limits.max_decompressed_bytes,
                });
            }
        }
    }

    Ok(())
}

fn read_zip64_uncompressed_size(file: &mut File, extra_len: usize) -> Result<u64, io::Error> {
    let mut extra = vec![0u8; extra_len];
    file.read_exact(&mut extra)?;
    let mut offset = 0;
    while offset + 4 <= extra.len() {
        let tag = u16::from_le_bytes([extra[offset], extra[offset + 1]]);
        let size = u16::from_le_bytes([extra[offset + 2], extra[offset + 3]]) as usize;
        offset += 4;
        if tag == 0x0001 && size >= 8 && offset + 8 <= extra.len() {
            return Ok(u64::from_le_bytes(
                extra[offset..offset + 8].try_into().unwrap(),
            ));
        }
        offset += size;
    }
    Ok(0)
}

fn validate_entry_name(name: &str) -> Result<(), ArchiveError> {
    if name.starts_with('/') || name.starts_with('\\') {
        return Err(ArchiveError::InvalidEntryName {
            name: name.to_owned(),
        });
    }
    if name.contains(':') {
        return Err(ArchiveError::InvalidEntryName {
            name: name.to_owned(),
        });
    }
    for segment in name.split(['/', '\\']) {
        if segment == ".." {
            return Err(ArchiveError::InvalidEntryName {
                name: name.to_owned(),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        fs::File,
        io::{Cursor, Write},
        path::Path,
    };

    use ariad_core::limits::Limits;
    use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

    use super::{ArchiveError, preflight_archive};

    fn python3_available() -> bool {
        std::process::Command::new("python3")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    #[test]
    fn valid_archive_with_stored_and_deflated_entries_passes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("valid.zip");
        {
            let file = File::create(&path).unwrap();
            let mut zip = ZipWriter::new(file);
            zip.start_file(
                "stored.txt",
                SimpleFileOptions::default().compression_method(CompressionMethod::Stored),
            )
            .unwrap();
            zip.write_all(b"Hello stored").unwrap();

            zip.start_file(
                "deflated.txt",
                SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
            )
            .unwrap();
            zip.write_all(b"Hello deflated").unwrap();
            zip.finish().unwrap();
        }

        assert!(preflight_archive(&path, &Limits::local()).is_ok());
    }

    #[test]
    fn honest_bomb_rejected_by_declared_size_before_inflating() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("honest_bomb.zip");
        {
            let file = File::create(&path).unwrap();
            let mut zip = ZipWriter::new(file);
            zip.start_file(
                "big.txt",
                SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
            )
            .unwrap();
            zip.write_all(&[b'A'; 2000]).unwrap();
            zip.finish().unwrap();
        }

        let mut limits = Limits::local();
        limits.max_decompressed_bytes = 1000;

        let err = preflight_archive(&path, &limits).unwrap_err();
        match err {
            ArchiveError::DecompressedSizeExceeded { size, limit } => {
                assert_eq!(size, 2000);
                assert_eq!(limit, 1000);
            }
            other => panic!("expected DecompressedSizeExceeded, got {other:?}"),
        }
    }

    #[test]
    fn lying_bomb_rejected_by_streaming_inflate() {
        let mut buffer = Vec::new();
        {
            let mut zip = ZipWriter::new(Cursor::new(&mut buffer));
            zip.start_file(
                "bomb.txt",
                SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
            )
            .unwrap();
            // 5000 zeros deflate to very few bytes
            zip.write_all(&[0u8; 5000]).unwrap();
            zip.finish().unwrap();
        }

        // Corrupt the central directory entry to lie about uncompressed size: declare 100 bytes
        // Central directory signature is PK\x01\x02
        let cd_sig = b"PK\x01\x02";
        let cd_pos = buffer
            .windows(4)
            .position(|window| window == cd_sig)
            .expect("central directory found");
        // In CD header: offset 24 is uncompressed size (4 bytes, little-endian)
        let uncomp_size_pos = cd_pos + 24;
        buffer[uncomp_size_pos..uncomp_size_pos + 4].copy_from_slice(&100u32.to_le_bytes());

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lying_bomb.zip");
        std::fs::write(&path, &buffer).unwrap();

        let mut limits = Limits::local();
        limits.max_decompressed_bytes = 500;

        // Step 1 sees declared size = 100 <= 500.
        // Step 2 inflates stream and hits 501 > 500 -> rejected!
        let err = preflight_archive(&path, &limits).unwrap_err();
        match err {
            ArchiveError::DecompressedSizeExceeded { size, limit } => {
                assert!(size > 500);
                assert_eq!(limit, 500);
            }
            other => panic!("expected DecompressedSizeExceeded, got {other:?}"),
        }
    }

    #[test]
    fn too_many_entries_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("too_many.zip");
        {
            let file = File::create(&path).unwrap();
            let mut zip = ZipWriter::new(file);
            for i in 0..5 {
                zip.start_file(format!("file_{i}.txt"), SimpleFileOptions::default())
                    .unwrap();
                zip.write_all(b"content").unwrap();
            }
            zip.finish().unwrap();
        }

        let mut limits = Limits::local();
        limits.max_archive_entries = 3;

        let err = preflight_archive(&path, &limits).unwrap_err();
        match err {
            ArchiveError::EntryCountExceeded { count, limit } => {
                assert_eq!(count, 5);
                assert_eq!(limit, 3);
            }
            other => panic!("expected EntryCountExceeded, got {other:?}"),
        }
    }

    #[test]
    fn path_traversal_dot_dot_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dot_dot.zip");
        {
            let file = File::create(&path).unwrap();
            let mut zip = ZipWriter::new(file);
            zip.start_file("../escape.txt", SimpleFileOptions::default())
                .unwrap();
            zip.write_all(b"evil").unwrap();
            zip.finish().unwrap();
        }

        let err = preflight_archive(&path, &Limits::local()).unwrap_err();
        match err {
            ArchiveError::InvalidEntryName { name } => {
                assert_eq!(name, "../escape.txt");
            }
            other => panic!("expected InvalidEntryName, got {other:?}"),
        }
    }

    #[test]
    fn duplicate_entry_names_rejected_even_if_bomb_is_first() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dup_first.zip");
        {
            let file = File::create(&path).unwrap();
            let mut zip = ZipWriter::new(file);

            // First entry: bomb with 5000 zeros
            zip.start_file(
                "dup1.txt",
                SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
            )
            .unwrap();
            zip.write_all(&[0u8; 5000]).unwrap();

            // Second entry: benign small entry
            zip.start_file(
                "dup2.txt",
                SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
            )
            .unwrap();
            zip.write_all(b"small").unwrap();

            zip.finish().unwrap();
        }

        // Patch dup2.txt to dup1.txt across local header and central directory
        let mut bytes = std::fs::read(&path).unwrap();
        let first_pos = bytes
            .windows(8)
            .position(|w| w == b"dup2.txt")
            .expect("local header dup2.txt found");
        bytes[first_pos..first_pos + 8].copy_from_slice(b"dup1.txt");
        let cd_pos = bytes[first_pos + 8..]
            .windows(8)
            .position(|w| w == b"dup2.txt")
            .expect("cd header dup2.txt found");
        bytes[first_pos + 8 + cd_pos..first_pos + 8 + cd_pos + 8].copy_from_slice(b"dup1.txt");

        // Make entry 1 a lying bomb: patch declared uncompressed size to 10 bytes
        // Local file header 1: uncompressed size at 22..26
        bytes[22..26].copy_from_slice(&10u32.to_le_bytes());
        // CD header 1: find first PK\x01\x02
        let cd1_pos = bytes
            .windows(4)
            .position(|w| w == b"PK\x01\x02")
            .expect("first CD header");
        bytes[cd1_pos + 24..cd1_pos + 28].copy_from_slice(&10u32.to_le_bytes());

        std::fs::write(&path, &bytes).unwrap();

        let mut limits = Limits::local();
        limits.max_decompressed_bytes = 1000;

        let err = preflight_archive(&path, &limits).unwrap_err();
        match err {
            ArchiveError::DuplicateEntryName { name } => {
                assert_eq!(name, "dup1.txt");
            }
            other => panic!("expected DuplicateEntryName, got {other:?}"),
        }
    }

    #[test]
    fn eocd_entry_count_mismatch_rejected() {
        let mut buffer = Vec::new();
        {
            let mut zip = ZipWriter::new(Cursor::new(&mut buffer));
            zip.start_file("a.txt", SimpleFileOptions::default())
                .unwrap();
            zip.write_all(b"hello").unwrap();
            zip.start_file("b.txt", SimpleFileOptions::default())
                .unwrap();
            zip.write_all(b"world").unwrap();
            zip.finish().unwrap();
        }

        // Locate EOCD signature PK\x05\x06
        let eocd_sig = b"PK\x05\x06";
        let eocd_pos = buffer
            .windows(4)
            .rposition(|window| window == eocd_sig)
            .expect("EOCD signature found");

        // Offset 10 in EOCD is total entries (u16). Change 2 to 3 to simulate mismatch.
        buffer[eocd_pos + 10..eocd_pos + 12].copy_from_slice(&3u16.to_le_bytes());

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mismatch.zip");
        std::fs::write(&path, &buffer).unwrap();

        let err = preflight_archive(&path, &Limits::local()).unwrap_err();
        match err {
            ArchiveError::EntryCountMismatch {
                eocd_count,
                archive_count,
            } => {
                assert_eq!(eocd_count, 3);
                assert_eq!(archive_count, 2);
            }
            other => panic!("expected EntryCountMismatch, got {other:?}"),
        }
    }

    #[test]
    fn duplicate_entry_name_in_docx_fixture_rejected() {
        let fixture_path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/docx/vi-styled-report.docx");
        if !fixture_path.exists() {
            return;
        }
        let docx_bytes = std::fs::read(&fixture_path).unwrap();

        // Find central directory start and duplicate the first CD entry in front
        let cd_sig = b"PK\x01\x02";
        let first_cd_pos = docx_bytes
            .windows(4)
            .position(|window| window == cd_sig)
            .expect("first CD entry found");

        // The first CD entry header is 46 bytes + fn_len + extra_len + comment_len
        let fn_len = u16::from_le_bytes(
            docx_bytes[first_cd_pos + 28..first_cd_pos + 30]
                .try_into()
                .unwrap(),
        ) as usize;
        let extra_len = u16::from_le_bytes(
            docx_bytes[first_cd_pos + 30..first_cd_pos + 32]
                .try_into()
                .unwrap(),
        ) as usize;
        let comment_len = u16::from_le_bytes(
            docx_bytes[first_cd_pos + 32..first_cd_pos + 34]
                .try_into()
                .unwrap(),
        ) as usize;
        let first_cd_len = 46 + fn_len + extra_len + comment_len;
        let first_cd_slice = &docx_bytes[first_cd_pos..first_cd_pos + first_cd_len];

        // Duplicate the first CD entry: insert duplicate right at first_cd_pos
        let mut tampered = Vec::new();
        tampered.extend_from_slice(&docx_bytes[..first_cd_pos]);
        tampered.extend_from_slice(first_cd_slice); // duplicate entry!
        tampered.extend_from_slice(&docx_bytes[first_cd_pos..]);

        // Fix up EOCD total entries and CD size
        let eocd_sig = b"PK\x05\x06";
        let eocd_pos = tampered
            .windows(4)
            .rposition(|window| window == eocd_sig)
            .expect("EOCD found");
        let old_entries =
            u16::from_le_bytes(tampered[eocd_pos + 10..eocd_pos + 12].try_into().unwrap());
        tampered[eocd_pos + 8..eocd_pos + 10].copy_from_slice(&(old_entries + 1).to_le_bytes());
        tampered[eocd_pos + 10..eocd_pos + 12].copy_from_slice(&(old_entries + 1).to_le_bytes());
        let old_cd_size =
            u32::from_le_bytes(tampered[eocd_pos + 12..eocd_pos + 16].try_into().unwrap());
        tampered[eocd_pos + 12..eocd_pos + 16]
            .copy_from_slice(&(old_cd_size + first_cd_len as u32).to_le_bytes());

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tampered.docx");
        std::fs::write(&path, &tampered).unwrap();

        let err = preflight_archive(&path, &Limits::local()).unwrap_err();
        match err {
            ArchiveError::DuplicateEntryName { .. } => {}
            other => panic!("expected DuplicateEntryName, got {other:?}"),
        }
    }

    #[test]
    fn valid_zip64_archive_passes_preflight() {
        if !python3_available() {
            eprintln!("python3 not found; skipping test");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("valid_z64.zip");
        let py_script = format!(
            r#"
import zipfile
zipfile.ZIP_FILECOUNT_LIMIT = 0
with zipfile.ZipFile(r'{}', 'w') as z:
    z.writestr('doc1.txt', b'content one')
    z.writestr('doc2.txt', b'content two')
"#,
            path.display()
        );
        let status = std::process::Command::new("python3")
            .args(["-c", &py_script])
            .status()
            .expect("run python3 to create zip64");
        assert!(status.success(), "python3 script must succeed");

        assert!(preflight_archive(&path, &Limits::local()).is_ok());
    }

    #[test]
    fn zip64_archive_with_duplicate_entry_name_is_rejected() {
        if !python3_available() {
            eprintln!("python3 not found; skipping test");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dup_z64.zip");
        let py_script = format!(
            r#"
import zipfile
zipfile.ZIP_FILECOUNT_LIMIT = 0
with zipfile.ZipFile(r'{}', 'w') as z:
    z.writestr('a.txt', b'first')
    z.writestr('b.txt', b'second')
"#,
            path.display()
        );
        let status = std::process::Command::new("python3")
            .args(["-c", &py_script])
            .status()
            .expect("run python3 to create zip64");
        assert!(status.success());

        let bytes = std::fs::read(&path).unwrap();
        let cd_sig = b"PK\x01\x02";
        let cd_pos = bytes.windows(4).position(|w| w == cd_sig).expect("find CD");
        let fn_len =
            u16::from_le_bytes(bytes[cd_pos + 28..cd_pos + 30].try_into().unwrap()) as usize;
        let extra_len =
            u16::from_le_bytes(bytes[cd_pos + 30..cd_pos + 32].try_into().unwrap()) as usize;
        let comment_len =
            u16::from_le_bytes(bytes[cd_pos + 32..cd_pos + 34].try_into().unwrap()) as usize;
        let cd_len = 46 + fn_len + extra_len + comment_len;
        let cd_entry = &bytes[cd_pos..cd_pos + cd_len];

        let mut tampered = Vec::new();
        tampered.extend_from_slice(&bytes[..cd_pos]);
        tampered.extend_from_slice(cd_entry);
        tampered.extend_from_slice(&bytes[cd_pos..]);

        let z64_sig = b"PK\x06\x06";
        let z64_pos = tampered
            .windows(4)
            .rposition(|w| w == z64_sig)
            .expect("find Z64");
        let old_entries =
            u64::from_le_bytes(tampered[z64_pos + 32..z64_pos + 40].try_into().unwrap());
        tampered[z64_pos + 24..z64_pos + 32].copy_from_slice(&(old_entries + 1).to_le_bytes());
        tampered[z64_pos + 32..z64_pos + 40].copy_from_slice(&(old_entries + 1).to_le_bytes());

        std::fs::write(&path, &tampered).unwrap();
        let err = preflight_archive(&path, &Limits::local()).unwrap_err();
        match err {
            ArchiveError::DuplicateEntryName { name } => assert_eq!(name, "a.txt"),
            other => panic!("expected DuplicateEntryName, got {other:?}"),
        }
    }

    #[test]
    fn zip64_archive_with_hidden_entry_is_rejected() {
        if !python3_available() {
            eprintln!("python3 not found; skipping test");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hidden_z64.zip");
        let py_script = format!(
            r#"
import zipfile
zipfile.ZIP_FILECOUNT_LIMIT = 0
with zipfile.ZipFile(r'{}', 'w') as z:
    z.writestr('a.txt', b'first')
    z.writestr('b.txt', b'second')
"#,
            path.display()
        );
        let status = std::process::Command::new("python3")
            .args(["-c", &py_script])
            .status()
            .expect("run python3");
        assert!(status.success());

        let mut bytes = std::fs::read(&path).unwrap();
        let z64_sig = b"PK\x06\x06";
        let z64_pos = bytes
            .windows(4)
            .rposition(|w| w == z64_sig)
            .expect("find Z64");
        bytes[z64_pos + 24..z64_pos + 32].copy_from_slice(&3u64.to_le_bytes());
        bytes[z64_pos + 32..z64_pos + 40].copy_from_slice(&3u64.to_le_bytes());

        std::fs::write(&path, &bytes).unwrap();
        let err = preflight_archive(&path, &Limits::local()).unwrap_err();
        match err {
            ArchiveError::EntryCountMismatch {
                eocd_count,
                archive_count,
            } => {
                assert_eq!(eocd_count, 3);
                assert_eq!(archive_count, 2);
            }
            other => panic!("expected EntryCountMismatch, got {other:?}"),
        }
    }

    #[test]
    fn fuzz_style_truncated_and_garbage_eocd_records_never_panic() {
        if !python3_available() {
            eprintln!("python3 not found; skipping test");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fuzz_base.zip");
        let py_script = format!(
            r#"
import zipfile
zipfile.ZIP_FILECOUNT_LIMIT = 0
with zipfile.ZipFile(r'{}', 'w') as z:
    z.writestr('hello.txt', b'data')
"#,
            path.display()
        );
        let status = std::process::Command::new("python3")
            .args(["-c", &py_script])
            .status()
            .unwrap();
        assert!(status.success());

        let base_bytes = std::fs::read(&path).unwrap();
        let z64_sig = b"PK\x06\x06";
        let z64_pos = base_bytes
            .windows(4)
            .rposition(|w| w == z64_sig)
            .expect("find Z64");

        // 1. Truncate at every byte boundary starting from the Zip64 EOCD
        for truncate_len in z64_pos..base_bytes.len() {
            let truncated = &base_bytes[..truncate_len];
            let corrupt_path = dir.path().join("corrupt_trunc.zip");
            std::fs::write(&corrupt_path, truncated).unwrap();
            let _ = preflight_archive(&corrupt_path, &Limits::local());
        }

        // 2. Overwrite Zip64 EOCD with garbage bytes
        let mut garbage_z64 = base_bytes.clone();
        for b in &mut garbage_z64[z64_pos + 4..z64_pos + 50] {
            *b = 0xFF;
        }
        let corrupt_path = dir.path().join("corrupt_z64.zip");
        std::fs::write(&corrupt_path, &garbage_z64).unwrap();
        let _ = preflight_archive(&corrupt_path, &Limits::local());

        // 3. Overwrite Zip64 locator with garbage bytes
        let loc_sig = b"PK\x06\x07";
        if let Some(loc_pos) = base_bytes.windows(4).rposition(|w| w == loc_sig) {
            let mut garbage_loc = base_bytes.clone();
            garbage_loc[loc_pos..loc_pos + 4].copy_from_slice(b"NOPE");
            let corrupt_path = dir.path().join("corrupt_loc.zip");
            std::fs::write(&corrupt_path, &garbage_loc).unwrap();
            let _ = preflight_archive(&corrupt_path, &Limits::local());
        }
    }

    #[test]
    fn encrypted_archive_is_rejected_with_typed_error() {
        let mut buffer = Vec::new();
        {
            let mut zip = ZipWriter::new(Cursor::new(&mut buffer));
            zip.start_file("secret.txt", SimpleFileOptions::default())
                .unwrap();
            zip.write_all(b"secret data").unwrap();
            zip.finish().unwrap();
        }

        let cd_sig = b"PK\x01\x02";
        let cd_pos = buffer.windows(4).position(|w| w == cd_sig).unwrap();
        let flags = u16::from_le_bytes(buffer[cd_pos + 8..cd_pos + 10].try_into().unwrap());
        buffer[cd_pos + 8..cd_pos + 10].copy_from_slice(&(flags | 0x0001).to_le_bytes());

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("encrypted.zip");
        std::fs::write(&path, &buffer).unwrap();

        let err = preflight_archive(&path, &Limits::local()).unwrap_err();
        match err {
            ArchiveError::Encrypted => {}
            other => panic!("expected ArchiveError::Encrypted, got {other:?}"),
        }
    }

    #[test]
    fn zip64_with_two_to_sixty_two_entries_rejected_by_eocd_check() {
        if !python3_available() {
            eprintln!("python3 not found; skipping test");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("z64_entries.zip");
        let py_script = format!(
            r#"
import zipfile
zipfile.ZIP_FILECOUNT_LIMIT = 0
with zipfile.ZipFile(r'{}', 'w') as z:
    z.writestr('a.txt', b'first')
"#,
            path.display()
        );
        let status = std::process::Command::new("python3")
            .args(["-c", &py_script])
            .status()
            .expect("run python3");
        assert!(status.success());

        let mut bytes = std::fs::read(&path).unwrap();
        let z64_sig = b"PK\x06\x06";
        let z64_pos = bytes
            .windows(4)
            .rposition(|w| w == z64_sig)
            .expect("find Z64");

        // Set entry count to 2^62
        let huge_count: u64 = 1 << 62;
        bytes[z64_pos + 24..z64_pos + 32].copy_from_slice(&huge_count.to_le_bytes());
        bytes[z64_pos + 32..z64_pos + 40].copy_from_slice(&huge_count.to_le_bytes());
        std::fs::write(&path, &bytes).unwrap();

        let err = preflight_archive(&path, &Limits::local()).unwrap_err();
        match err {
            ArchiveError::EntryCountExceeded { limit, .. } => {
                assert_eq!(limit, 20_000);
            }
            other => panic!("expected EntryCountExceeded, got {other:?}"),
        }

        let mut file = File::open(&path).unwrap();
        assert_eq!(super::read_zip_entry_names(&mut file, 20_000, 2048), None);
    }

    #[test]
    fn lying_central_directory_past_eof_rejected() {
        let mut buffer = Vec::new();
        {
            let mut zip = ZipWriter::new(Cursor::new(&mut buffer));
            zip.start_file("test.txt", SimpleFileOptions::default())
                .unwrap();
            zip.write_all(b"test").unwrap();
            zip.finish().unwrap();
        }

        let eocd_sig = b"PK\x05\x06";
        let eocd_pos = buffer
            .windows(4)
            .rposition(|w| w == eocd_sig)
            .expect("EOCD found");
        // Corrupt CD offset (bytes 16..20) to point past file length
        buffer[eocd_pos + 16..eocd_pos + 20].copy_from_slice(&999999u32.to_le_bytes());

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lying_cd.zip");
        std::fs::write(&path, &buffer).unwrap();

        assert!(preflight_archive(&path, &Limits::local()).is_err());
        let mut file = File::open(&path).unwrap();
        assert_eq!(super::read_zip_entry_names(&mut file, 20_000, 2048), None);
    }

    #[test]
    fn one_million_empty_entries_eocd_rejected_without_allocation() {
        if !python3_available() {
            eprintln!("python3 not found; skipping test");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("million.zip");
        let py_script = format!(
            r#"
import zipfile
zipfile.ZIP_FILECOUNT_LIMIT = 0
with zipfile.ZipFile(r'{}', 'w') as z:
    z.writestr('a.txt', b'first')
"#,
            path.display()
        );
        let status = std::process::Command::new("python3")
            .args(["-c", &py_script])
            .status()
            .expect("run python3");
        assert!(status.success());

        let mut bytes = std::fs::read(&path).unwrap();
        let z64_sig = b"PK\x06\x06";
        let z64_pos = bytes
            .windows(4)
            .rposition(|w| w == z64_sig)
            .expect("find Z64");

        let million: u64 = 1_000_000;
        bytes[z64_pos + 24..z64_pos + 32].copy_from_slice(&million.to_le_bytes());
        bytes[z64_pos + 32..z64_pos + 40].copy_from_slice(&million.to_le_bytes());
        std::fs::write(&path, &bytes).unwrap();

        let err = preflight_archive(&path, &Limits::local()).unwrap_err();
        match err {
            ArchiveError::EntryCountExceeded { count, limit } => {
                assert_eq!(count, 1_000_000);
                assert_eq!(limit, 20_000);
            }
            other => panic!("expected EntryCountExceeded, got {other:?}"),
        }

        let mut file = File::open(&path).unwrap();
        assert_eq!(super::read_zip_entry_names(&mut file, 20_000, 2048), None);
    }

    #[test]
    fn excessively_long_entry_name_rejected() {
        let mut buffer = Vec::new();
        {
            let mut zip = ZipWriter::new(Cursor::new(&mut buffer));
            zip.start_file("valid.txt", SimpleFileOptions::default())
                .unwrap();
            zip.write_all(b"data").unwrap();
            zip.finish().unwrap();
        }

        let cd_sig = b"PK\x01\x02";
        let cd_pos = buffer.windows(4).position(|w| w == cd_sig).unwrap();
        // Set filename length to 3000 (> 2048)
        buffer[cd_pos + 28..cd_pos + 30].copy_from_slice(&3000u16.to_le_bytes());

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("long_name.zip");
        std::fs::write(&path, &buffer).unwrap();

        let err = preflight_archive(&path, &Limits::local()).unwrap_err();
        match err {
            ArchiveError::InvalidEntryName { .. } => {}
            other => panic!("expected InvalidEntryName, got {other:?}"),
        }
    }

    #[test]
    fn read_zip_entry_names_entry_cap_and_overlap() {
        // 1. Overlapping / CD past EOF
        let mut buffer = Vec::new();
        {
            let mut zip = ZipWriter::new(Cursor::new(&mut buffer));
            zip.start_file("entry.txt", SimpleFileOptions::default())
                .unwrap();
            zip.write_all(b"content").unwrap();
            zip.finish().unwrap();
        }
        let eocd_sig = b"PK\x05\x06";
        let eocd_pos = buffer
            .windows(4)
            .rposition(|w| w == eocd_sig)
            .expect("EOCD found");
        buffer[eocd_pos + 16..eocd_pos + 20].copy_from_slice(&999999u32.to_le_bytes());

        let dir = tempfile::tempdir().unwrap();
        let overlap_path = dir.path().join("overlap.zip");
        std::fs::write(&overlap_path, &buffer).unwrap();
        let mut overlap_file = File::open(&overlap_path).unwrap();
        assert_eq!(
            super::read_zip_entry_names(&mut overlap_file, 20_000, 2048),
            None
        );

        // 2. 1,000,000-entry archive
        if !python3_available() {
            eprintln!("python3 not found; skipping 1M entry part");
            return;
        }
        let million_path = dir.path().join("million.zip");
        let py_script = format!(
            r#"
import zipfile
zipfile.ZIP_FILECOUNT_LIMIT = 0
with zipfile.ZipFile(r'{}', 'w') as z:
    z.writestr('a.txt', b'first')
"#,
            million_path.display()
        );
        let status = std::process::Command::new("python3")
            .args(["-c", &py_script])
            .status()
            .expect("run python3");
        assert!(status.success());

        let mut bytes = std::fs::read(&million_path).unwrap();
        let z64_sig = b"PK\x06\x06";
        let z64_pos = bytes
            .windows(4)
            .rposition(|w| w == z64_sig)
            .expect("find Z64");
        let million: u64 = 1_000_000;
        bytes[z64_pos + 24..z64_pos + 32].copy_from_slice(&million.to_le_bytes());
        bytes[z64_pos + 32..z64_pos + 40].copy_from_slice(&million.to_le_bytes());
        std::fs::write(&million_path, &bytes).unwrap();

        let mut million_file = File::open(&million_path).unwrap();
        assert_eq!(
            super::read_zip_entry_names(&mut million_file, 20_000, 2048),
            None
        );
    }

    #[test]
    fn central_directory_size_exceeding_cap_rejected() {
        let mut buffer = Vec::new();
        {
            let mut zip = ZipWriter::new(Cursor::new(&mut buffer));
            zip.start_file("test.txt", SimpleFileOptions::default())
                .unwrap();
            zip.write_all(b"test").unwrap();
            zip.finish().unwrap();
        }
        let eocd_sig = b"PK\x05\x06";
        let eocd_pos = buffer
            .windows(4)
            .rposition(|w| w == eocd_sig)
            .expect("EOCD found");
        // Corrupt CD size (bytes 12..16) to exceed MAX_CENTRAL_DIRECTORY_BYTES (33 MiB)
        let huge_cd_size = (33 * 1024 * 1024u32).to_le_bytes();
        buffer[eocd_pos + 12..eocd_pos + 16].copy_from_slice(&huge_cd_size);

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("huge_cd.zip");
        std::fs::write(&path, &buffer).unwrap();

        let err = preflight_archive(&path, &Limits::local()).unwrap_err();
        match err {
            ArchiveError::CentralDirectorySizeExceeded { size, limit } => {
                assert_eq!(size, 33 * 1024 * 1024);
                assert_eq!(limit, super::MAX_CENTRAL_DIRECTORY_BYTES);
            }
            other => panic!("expected CentralDirectorySizeExceeded, got {other:?}"),
        }
    }
}
