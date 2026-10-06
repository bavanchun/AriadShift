use std::{
    collections::HashSet,
    fs::File,
    io::{self, Read, Seek, SeekFrom},
    path::Path,
};

use ariad_core::limits::Limits;
use thiserror::Error;
use zip::ZipArchive;

#[derive(Debug, Error)]
pub enum ArchiveError {
    #[error("archive entry count {count} exceeds the configured limit of {limit}")]
    EntryCountExceeded { count: usize, limit: u32 },
    #[error("archive decompressed size {size} exceeds the configured limit of {limit}")]
    DecompressedSizeExceeded { size: u64, limit: u64 },
    #[error("archive entry name `{name}` is invalid or traverses directories")]
    InvalidEntryName { name: String },
    #[error("archive contains duplicate entry name `{name}`")]
    DuplicateEntryName { name: String },
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

/// Preflights an untrusted ZIP archive (e.g. DOCX or EPUB) before any processing starts.
///
/// 1. Reads the raw ZIP central directory headers from the directory start reported by ZipArchive.
///    - Detects duplicate entry names (which zip::ZipArchive collapses, allowing bypasses).
///    - Compares the raw entry count and EOCD count against the indexed count to detect hidden entries.
///    - Checks entry count against max_archive_entries, declared size against max_decompressed_bytes,
///      and verifies entry names (no absolute paths, drive prefixes, or `..` traversals).
/// 2. Stream-inflates every entry through a bounded reader `take(remaining + 1)`. Rejects as soon as
///    the actual inflated bytes exceed max_decompressed_bytes, defeating lying zip bombs.
pub fn preflight_archive(path: &Path, limits: &Limits) -> Result<(), ArchiveError> {
    let mut file = File::open(path)?;
    let mut raw_file = file.try_clone()?;
    let mut archive = ZipArchive::new(&mut file)?;

    let entry_count = archive.len();
    if entry_count as u64 > limits.max_archive_entries as u64 {
        return Err(ArchiveError::EntryCountExceeded {
            count: entry_count,
            limit: limits.max_archive_entries,
        });
    }

    // Step 1: Raw central directory walk, duplicate entry detection, and declared size validation
    let cd_start = archive.central_directory_start();
    raw_file.seek(SeekFrom::Start(cd_start))?;

    let mut seen_names = HashSet::new();
    let mut raw_cd_entries = 0usize;
    let mut declared_total: u64 = 0;

    let mut header = [0u8; 46];
    loop {
        let pos_before_read = raw_file.stream_position()?;
        let n = raw_file.read(&mut header)?;
        if n == 0 {
            break;
        }
        if n < 4 {
            return Err(ArchiveError::Zip(zip::result::ZipError::InvalidArchive(
                std::borrow::Cow::Borrowed("truncated central directory"),
            )));
        }
        let sig = &header[0..4];
        if sig == [0x50, 0x4b, 0x01, 0x02] {
            // Central directory file header
            if n < 46 {
                raw_file.read_exact(&mut header[n..46])?;
            }
            raw_cd_entries = raw_cd_entries.saturating_add(1);
            if raw_cd_entries as u64 > limits.max_archive_entries as u64 {
                return Err(ArchiveError::EntryCountExceeded {
                    count: raw_cd_entries,
                    limit: limits.max_archive_entries,
                });
            }

            let uncomp_size = u32::from_le_bytes(header[24..28].try_into().unwrap()) as u64;
            let fn_len = u16::from_le_bytes(header[28..30].try_into().unwrap()) as usize;
            let extra_len = u16::from_le_bytes(header[30..32].try_into().unwrap()) as usize;
            let comment_len = u16::from_le_bytes(header[32..34].try_into().unwrap()) as usize;

            let mut name_bytes = vec![0u8; fn_len];
            raw_file.read_exact(&mut name_bytes)?;
            let name = String::from_utf8_lossy(&name_bytes).into_owned();

            if !seen_names.insert(name.clone()) {
                return Err(ArchiveError::DuplicateEntryName { name });
            }

            validate_entry_name(&name)?;

            let size = if uncomp_size == 0xFFFF_FFFF {
                read_zip64_uncompressed_size(&mut raw_file, extra_len)?
            } else {
                raw_file.seek(SeekFrom::Current(extra_len as i64))?;
                uncomp_size
            };

            raw_file.seek(SeekFrom::Current(comment_len as i64))?;

            declared_total = declared_total.saturating_add(size);
            if declared_total > limits.max_decompressed_bytes {
                return Err(ArchiveError::DecompressedSizeExceeded {
                    size: declared_total,
                    limit: limits.max_decompressed_bytes,
                });
            }
        } else if sig == [0x50, 0x4b, 0x05, 0x06] {
            // End of central directory record
            let mut eocd_rest = [0u8; 18];
            let rest_read = if n > 4 {
                let to_copy = (n - 4).min(18);
                eocd_rest[..to_copy].copy_from_slice(&header[4..4 + to_copy]);
                to_copy
            } else {
                0
            };
            if rest_read < 18 {
                raw_file.read_exact(&mut eocd_rest[rest_read..18])?;
            }
            let eocd_entries = u16::from_le_bytes(eocd_rest[6..8].try_into().unwrap()) as usize;
            let cd_offset = u32::from_le_bytes(eocd_rest[12..16].try_into().unwrap());
            if eocd_entries == 0xFFFF || cd_offset == 0xFFFF_FFFF {
                if pos_before_read < 20 {
                    return Err(ArchiveError::Zip(zip::result::ZipError::InvalidArchive(
                        std::borrow::Cow::Borrowed("truncated zip64 locator"),
                    )));
                }
                raw_file.seek(SeekFrom::Start(pos_before_read - 20))?;
                let mut loc_buf = [0u8; 20];
                raw_file.read_exact(&mut loc_buf)?;
                if loc_buf[0..4] != [0x50, 0x4b, 0x06, 0x07] {
                    return Err(ArchiveError::Zip(zip::result::ZipError::InvalidArchive(
                        std::borrow::Cow::Borrowed("invalid zip64 locator signature"),
                    )));
                }
                let z64_offset = u64::from_le_bytes(loc_buf[8..16].try_into().unwrap());
                raw_file.seek(SeekFrom::Start(z64_offset))?;
                let mut z64_buf = [0u8; 56];
                raw_file.read_exact(&mut z64_buf)?;
                if z64_buf[0..4] != [0x50, 0x4b, 0x06, 0x06] {
                    return Err(ArchiveError::Zip(zip::result::ZipError::InvalidArchive(
                        std::borrow::Cow::Borrowed(
                            "invalid zip64 end of central directory signature",
                        ),
                    )));
                }
                let zip64_entries =
                    u64::from_le_bytes(z64_buf[32..40].try_into().unwrap()) as usize;
                if zip64_entries != raw_cd_entries {
                    return Err(ArchiveError::EntryCountMismatch {
                        eocd_count: zip64_entries,
                        archive_count: raw_cd_entries,
                    });
                }
            } else if eocd_entries != raw_cd_entries {
                return Err(ArchiveError::EntryCountMismatch {
                    eocd_count: eocd_entries,
                    archive_count: raw_cd_entries,
                });
            }
            break;
        } else if sig == [0x50, 0x4b, 0x06, 0x06] {
            // Zip64 end of central directory record
            let mut zip64_rest = [0u8; 52];
            let already_read = (n.saturating_sub(4)).min(52);
            if already_read > 0 {
                zip64_rest[..already_read].copy_from_slice(&header[4..4 + already_read]);
            }
            if already_read < 52 {
                raw_file.read_exact(&mut zip64_rest[already_read..52])?;
            }
            let zip64_entries = u64::from_le_bytes(zip64_rest[28..36].try_into().unwrap()) as usize;
            if zip64_entries != raw_cd_entries {
                return Err(ArchiveError::EntryCountMismatch {
                    eocd_count: zip64_entries,
                    archive_count: raw_cd_entries,
                });
            }
            break;
        } else {
            break;
        }
    }

    if raw_cd_entries != entry_count {
        return Err(ArchiveError::EntryCountMismatch {
            eocd_count: raw_cd_entries,
            archive_count: entry_count,
        });
    }

    // Step 2: Stream-inflate every entry through a counting reader bounded by take(remaining + 1)
    let mut inflated_total: u64 = 0;
    let mut buffer = [0u8; 64 * 1024];

    for i in 0..entry_count {
        let mut entry = archive.by_index(i)?;
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
    use tempfile::NamedTempFile;
    use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

    use super::{ArchiveError, preflight_archive};

    #[test]
    fn valid_archive_with_stored_and_deflated_entries_passes() {
        let temp = NamedTempFile::new().unwrap();
        {
            let file = File::create(temp.path()).unwrap();
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

        assert!(preflight_archive(temp.path(), &Limits::local()).is_ok());
    }

    #[test]
    fn honest_bomb_rejected_by_declared_size_before_inflating() {
        let temp = NamedTempFile::new().unwrap();
        {
            let file = File::create(temp.path()).unwrap();
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

        let err = preflight_archive(temp.path(), &limits).unwrap_err();
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

        let temp = NamedTempFile::new().unwrap();
        std::fs::write(temp.path(), &buffer).unwrap();

        let mut limits = Limits::local();
        limits.max_decompressed_bytes = 500;

        // Step 1 sees declared size = 100 <= 500.
        // Step 2 inflates stream and hits 501 > 500 -> rejected!
        let err = preflight_archive(temp.path(), &limits).unwrap_err();
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
        let temp = NamedTempFile::new().unwrap();
        {
            let file = File::create(temp.path()).unwrap();
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

        let err = preflight_archive(temp.path(), &limits).unwrap_err();
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
        let temp = NamedTempFile::new().unwrap();
        {
            let file = File::create(temp.path()).unwrap();
            let mut zip = ZipWriter::new(file);
            zip.start_file("../escape.txt", SimpleFileOptions::default())
                .unwrap();
            zip.write_all(b"evil").unwrap();
            zip.finish().unwrap();
        }

        let err = preflight_archive(temp.path(), &Limits::local()).unwrap_err();
        match err {
            ArchiveError::InvalidEntryName { name } => {
                assert_eq!(name, "../escape.txt");
            }
            other => panic!("expected InvalidEntryName, got {other:?}"),
        }
    }

    #[test]
    fn duplicate_entry_names_rejected_even_if_bomb_is_first() {
        let temp = NamedTempFile::new().unwrap();
        {
            let file = File::create(temp.path()).unwrap();
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
        let mut bytes = std::fs::read(temp.path()).unwrap();
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

        std::fs::write(temp.path(), &bytes).unwrap();

        let mut limits = Limits::local();
        limits.max_decompressed_bytes = 1000;

        let err = preflight_archive(temp.path(), &limits).unwrap_err();
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

        let temp = NamedTempFile::new().unwrap();
        std::fs::write(temp.path(), &buffer).unwrap();

        let err = preflight_archive(temp.path(), &Limits::local()).unwrap_err();
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

        let temp = NamedTempFile::new().unwrap();
        std::fs::write(temp.path(), &tampered).unwrap();

        let err = preflight_archive(temp.path(), &Limits::local()).unwrap_err();
        match err {
            ArchiveError::DuplicateEntryName { .. } => {}
            other => panic!("expected DuplicateEntryName, got {other:?}"),
        }
    }

    #[test]
    fn valid_zip64_archive_passes_preflight() {
        let temp = NamedTempFile::new().unwrap();
        let py_script = format!(
            r#"
import zipfile
zipfile.ZIP_FILECOUNT_LIMIT = 0
with zipfile.ZipFile(r'{}', 'w') as z:
    z.writestr('doc1.txt', b'content one')
    z.writestr('doc2.txt', b'content two')
"#,
            temp.path().display()
        );
        let status = std::process::Command::new("python3")
            .args(["-c", &py_script])
            .status()
            .expect("run python3 to create zip64");
        assert!(status.success(), "python3 script must succeed");

        assert!(preflight_archive(temp.path(), &Limits::local()).is_ok());
    }

    #[test]
    fn zip64_archive_with_duplicate_entry_name_is_rejected() {
        let temp = NamedTempFile::new().unwrap();
        let py_script = format!(
            r#"
import zipfile
zipfile.ZIP_FILECOUNT_LIMIT = 0
with zipfile.ZipFile(r'{}', 'w') as z:
    z.writestr('a.txt', b'first')
    z.writestr('b.txt', b'second')
"#,
            temp.path().display()
        );
        let status = std::process::Command::new("python3")
            .args(["-c", &py_script])
            .status()
            .expect("run python3 to create zip64");
        assert!(status.success());

        let bytes = std::fs::read(temp.path()).unwrap();
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

        std::fs::write(temp.path(), &tampered).unwrap();
        let err = preflight_archive(temp.path(), &Limits::local()).unwrap_err();
        match err {
            ArchiveError::DuplicateEntryName { name } => assert_eq!(name, "a.txt"),
            other => panic!("expected DuplicateEntryName, got {other:?}"),
        }
    }

    #[test]
    fn zip64_archive_with_hidden_entry_is_rejected() {
        let temp = NamedTempFile::new().unwrap();
        let py_script = format!(
            r#"
import zipfile
zipfile.ZIP_FILECOUNT_LIMIT = 0
with zipfile.ZipFile(r'{}', 'w') as z:
    z.writestr('a.txt', b'first')
    z.writestr('b.txt', b'second')
"#,
            temp.path().display()
        );
        let status = std::process::Command::new("python3")
            .args(["-c", &py_script])
            .status()
            .expect("run python3");
        assert!(status.success());

        let mut bytes = std::fs::read(temp.path()).unwrap();
        let z64_sig = b"PK\x06\x06";
        let z64_pos = bytes
            .windows(4)
            .rposition(|w| w == z64_sig)
            .expect("find Z64");
        bytes[z64_pos + 24..z64_pos + 32].copy_from_slice(&3u64.to_le_bytes());
        bytes[z64_pos + 32..z64_pos + 40].copy_from_slice(&3u64.to_le_bytes());

        std::fs::write(temp.path(), &bytes).unwrap();
        let err = preflight_archive(temp.path(), &Limits::local()).unwrap_err();
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
        let temp = NamedTempFile::new().unwrap();
        let py_script = format!(
            r#"
import zipfile
zipfile.ZIP_FILECOUNT_LIMIT = 0
with zipfile.ZipFile(r'{}', 'w') as z:
    z.writestr('hello.txt', b'data')
"#,
            temp.path().display()
        );
        let status = std::process::Command::new("python3")
            .args(["-c", &py_script])
            .status()
            .unwrap();
        assert!(status.success());

        let base_bytes = std::fs::read(temp.path()).unwrap();
        let z64_sig = b"PK\x06\x06";
        let z64_pos = base_bytes
            .windows(4)
            .rposition(|w| w == z64_sig)
            .expect("find Z64");

        // 1. Truncate at every byte boundary starting from the Zip64 EOCD
        for truncate_len in z64_pos..base_bytes.len() {
            let truncated = &base_bytes[..truncate_len];
            let corrupt_file = NamedTempFile::new().unwrap();
            std::fs::write(corrupt_file.path(), truncated).unwrap();
            let _ = preflight_archive(corrupt_file.path(), &Limits::local());
        }

        // 2. Overwrite Zip64 EOCD with garbage bytes
        let mut garbage_z64 = base_bytes.clone();
        for b in &mut garbage_z64[z64_pos + 4..z64_pos + 50] {
            *b = 0xFF;
        }
        let corrupt_file = NamedTempFile::new().unwrap();
        std::fs::write(corrupt_file.path(), &garbage_z64).unwrap();
        let _ = preflight_archive(corrupt_file.path(), &Limits::local());

        // 3. Overwrite Zip64 locator with garbage bytes
        let loc_sig = b"PK\x06\x07";
        if let Some(loc_pos) = base_bytes.windows(4).rposition(|w| w == loc_sig) {
            let mut garbage_loc = base_bytes.clone();
            garbage_loc[loc_pos..loc_pos + 4].copy_from_slice(b"NOPE");
            let corrupt_file = NamedTempFile::new().unwrap();
            std::fs::write(corrupt_file.path(), &garbage_loc).unwrap();
            let _ = preflight_archive(corrupt_file.path(), &Limits::local());
        }
    }
}
