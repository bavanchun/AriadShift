#![no_main]

//! Archive preflight for untrusted DOCX, EPUB and Office containers: the raw EOCD and central
//! directory parser, the streaming inflate check, the entry-name listing used for format
//! detection, the OLE2 compound file directory reader, and `classify_file`.

use std::{
    collections::HashSet,
    fs::{self, File},
    io::Cursor,
    path::{Path, PathBuf},
    sync::OnceLock,
};

use ariad_core::{format, limits::Limits};
use ariad_host::{
    archive::{
        ArchiveError, MAX_ZIP_ENTRY_NAME_BYTES, parse_central_directory, preflight_archive,
        read_cfb_entry_names, read_zip_entry_names,
    },
    convert::{MAX_ZIP_ENTRIES, classify_file},
};
use libfuzzer_sys::fuzz_target;

const MAX_INPUT_BYTES: usize = 64 * 1024;
/// The CFB reader follows at most 64 directory sectors of at most 4096 bytes, and each
/// directory entry takes 128 bytes.
const MAX_CFB_NAMES: usize = 64 * 4096 / 128;

/// A per-process scratch file, so parallel fuzzing jobs never share an input.
fn input_path() -> &'static PathBuf {
    static PATH: OnceLock<PathBuf> = OnceLock::new();
    PATH.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!(
            "ariad-fuzz-archive-preflight-{}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("create fuzz scratch directory");
        dir.join("input.docx")
    })
}

/// Entry names that passed preflight never escape the extraction root.
fn assert_safe_names(names: &[String]) {
    let mut seen = HashSet::new();
    for name in names {
        assert!(
            !name.starts_with('/') && !name.starts_with('\\'),
            "absolute entry name {name:?} accepted"
        );
        assert!(
            !name.contains(':'),
            "drive or stream entry name {name:?} accepted"
        );
        assert!(
            name.split(['/', '\\']).all(|segment| segment != ".."),
            "traversing entry name {name:?} accepted"
        );
        assert!(
            seen.insert(name.as_str()),
            "duplicate entry name {name:?} accepted"
        );
    }
}

fn check(path: &Path, limits: &Limits) -> bool {
    match preflight_archive(path, limits) {
        Ok(()) => {
            let mut file = File::open(path).expect("reopen preflighted input");
            let summary = parse_central_directory(&mut file, limits)
                .expect("an archive that passed preflight has a valid central directory");
            assert!(summary.entry_count <= limits.max_archive_entries as usize);
            assert_eq!(summary.entry_count, summary.entry_names.len());
            assert!(summary.declared_decompressed_bytes <= limits.max_decompressed_bytes);
            assert_safe_names(&summary.entry_names);
            true
        }
        Err(ArchiveError::EntryCountExceeded { count, limit }) => {
            assert_eq!(limit, limits.max_archive_entries);
            assert!(count > limit as usize);
            false
        }
        Err(ArchiveError::DecompressedSizeExceeded { size, limit }) => {
            assert_eq!(limit, limits.max_decompressed_bytes);
            assert!(size > limit);
            false
        }
        Err(
            ArchiveError::CentralDirectorySizeExceeded { .. }
            | ArchiveError::InvalidEntryName { .. }
            | ArchiveError::DuplicateEntryName { .. }
            | ArchiveError::Encrypted
            | ArchiveError::EntryCountMismatch { .. }
            | ArchiveError::Io(_)
            | ArchiveError::Zip(_),
        ) => false,
    }
}

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_INPUT_BYTES {
        return;
    }
    let path = input_path();
    fs::write(path, data).expect("write fuzz input");

    // Production local limits, then small limits that make the entry and size budgets reachable.
    let local = Limits::local();
    let small = Limits {
        max_archive_entries: 8,
        max_decompressed_bytes: 16 * 1024,
        ..Limits::local()
    };
    let small_ok = check(path, &small);
    let local_ok = check(path, &local);
    // Raising the budgets never rejects an archive that the smaller budgets accepted.
    assert!(
        !small_ok || local_ok,
        "preflight is not monotonic in its limits"
    );

    // The listing used for format detection agrees with the central directory parser.
    let mut file = File::open(path).expect("reopen fuzz input");
    let names = read_zip_entry_names(&mut file, MAX_ZIP_ENTRIES, MAX_ZIP_ENTRY_NAME_BYTES);
    if let Some(names) = &names {
        assert!(names.len() <= MAX_ZIP_ENTRIES);
        assert!(
            names
                .iter()
                .all(|name| name.len() <= MAX_ZIP_ENTRY_NAME_BYTES)
        );
        assert_safe_names(names);
        // Lossy UTF-8 decoding can lengthen a name past the listing cap, so only a listing
        // that succeeded is compared with the preflighted central directory.
        if local_ok {
            let mut file = File::open(path).expect("reopen fuzz input");
            let summary = parse_central_directory(&mut file, &local).expect("central directory");
            assert_eq!(names, &summary.entry_names);
        }
    }

    // The OLE2 directory walk is bounded whatever the sector chain says.
    if let Some(cfb_names) = read_cfb_entry_names(&mut Cursor::new(data)) {
        assert!(
            cfb_names.len() <= MAX_CFB_NAMES,
            "compound file listing returned {} names",
            cfb_names.len()
        );
        let refs: Vec<&str> = cfb_names.iter().map(String::as_str).collect();
        let _ = format::classify(data, Some(&refs), Some("doc"));
    }

    // Format detection over the same prefix and entries never panics.
    if let Some(names) = &names {
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let _ = format::classify(data, Some(&refs), Some("docx"));
    }
    let classification = classify_file(path).expect("classify a regular file");
    assert_eq!(classification.bytes, data.len() as u64);
});
