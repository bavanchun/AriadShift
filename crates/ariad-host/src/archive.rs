use std::{
    fs::File,
    io::{self, Read},
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
    #[error("I/O error during archive preflight: {0}")]
    Io(#[from] io::Error),
    #[error("ZIP error during archive preflight: {0}")]
    Zip(#[from] zip::result::ZipError),
}

/// Preflights an untrusted ZIP archive (e.g. DOCX or EPUB) before any processing starts.
///
/// 1. Reads the ZIP central directory as a cheap filter. Rejects if entry count > max_archive_entries,
///    if declared decompressed size > max_decompressed_bytes, or if any entry name is invalid (absolute,
///    drive prefix, or contains `..`).
/// 2. Stream-inflates every entry through a bounded reader `take(remaining + 1)`. Rejects as soon as
///    the actual inflated bytes exceed max_decompressed_bytes, defeating lying zip bombs.
pub fn preflight_archive(path: &Path, limits: &Limits) -> Result<(), ArchiveError> {
    let file = File::open(path)?;
    let mut archive = ZipArchive::new(file)?;

    let entry_count = archive.len();
    if entry_count as u64 > limits.max_archive_entries as u64 {
        return Err(ArchiveError::EntryCountExceeded {
            count: entry_count,
            limit: limits.max_archive_entries,
        });
    }

    // Step 1: Cheap central directory check
    let mut declared_total: u64 = 0;
    for i in 0..entry_count {
        let (name, size) = {
            let entry = archive.by_index(i)?;
            (entry.name().to_owned(), entry.size())
        };
        validate_entry_name(&name)?;
        declared_total = declared_total.saturating_add(size);
        if declared_total > limits.max_decompressed_bytes {
            return Err(ArchiveError::DecompressedSizeExceeded {
                size: declared_total,
                limit: limits.max_decompressed_bytes,
            });
        }
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
}
