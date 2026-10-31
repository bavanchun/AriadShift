#![no_main]

//! Pack components through both the scanner and the real install writer, budgets and final
//! link check. Each format gets an isolated temporary root. Partial outputs on errors must
//! stay inside the root, and links from successful extractions must resolve inside it.
//! Signature and component hash checks belong to install and are deliberately bypassed here.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use ariad_host::packs::{Unpack, extract_for_fuzzing, scan};
use libfuzzer_sys::fuzz_target;

const MAX_INPUT_BYTES: usize = 16 * 1024;
const ZIP_CENTRAL_RECORD_BYTES: usize = 46;
const MAX_EXTRACT_BYTES: u64 = 1 << 20;
const MAX_EXTRACT_ENTRIES: u64 = 1024;

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        loop {
            let path = std::env::temp_dir().join(format!(
                "ariad-fuzz-pack-archive-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("create isolated scratch directory: {error}"),
            }
        }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).expect("remove isolated extraction scratch directory");
    }
}

/// Never follows directory links. Failed extraction may leave rejected/dangling links for
/// install's staging cleanup, but every partial regular file must still be confined/bounded.
fn check_tree(root: &Path, successful: bool) {
    let canonical_root = fs::canonicalize(root).expect("resolve install root");
    let mut pending = vec![root.to_path_buf()];
    let mut bytes = 0u64;
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(dir).expect("list extracted directory") {
            let path = entry.expect("extracted entry").path();
            let metadata = fs::symlink_metadata(&path).expect("inspect extracted entry");
            if metadata.file_type().is_symlink() {
                let target = fs::read_link(&path).expect("read extracted link");
                assert!(target.is_relative(), "absolute link {path:?} -> {target:?}");
                if successful {
                    let real = fs::canonicalize(&path)
                        .expect("successful extraction left a dangling link");
                    assert!(
                        real.starts_with(&canonical_root),
                        "link {path:?} resolves to {real:?}"
                    );
                }
            } else {
                assert!(
                    fs::canonicalize(&path)
                        .expect("resolve partial output")
                        .starts_with(&canonical_root)
                );
                if metadata.is_dir() {
                    pending.push(path);
                } else {
                    assert!(metadata.is_file(), "special file extracted: {path:?}");
                    bytes += metadata.len();
                }
            }
        }
    }
    assert!(
        bytes <= MAX_EXTRACT_BYTES,
        "partial output exceeded extraction byte budget"
    );
}

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_INPUT_BYTES {
        return;
    }
    let scratch = Scratch::new();
    let input = scratch.0.join("component.bin");
    let outside = scratch.0.join("outside");
    fs::write(&input, data).expect("write fuzz input");
    fs::create_dir(&outside).expect("create outside sentinel directory");
    fs::write(outside.join("sentinel"), b"untouched").expect("write outside sentinel");
    for unpack in [Unpack::File, Unpack::Zip, Unpack::TarGz, Unpack::TarZst] {
        let root = scratch.0.join("root");
        fs::create_dir(&root).expect("create install root");
        let result = extract_for_fuzzing(
            &input,
            unpack,
            "payload.bin",
            &root,
            MAX_EXTRACT_BYTES,
            MAX_EXTRACT_ENTRIES,
        );
        // Check both successful and failed writes before inspecting scanner agreement.
        check_tree(&root, result.is_ok());
        assert_eq!(
            fs::read(outside.join("sentinel")).expect("read sentinel"),
            b"untouched"
        );
        assert_eq!(
            fs::read_dir(&outside)
                .expect("list sentinel directory")
                .count(),
            1
        );
        assert_eq!(
            fs::read(&input).expect("read original archive"),
            data,
            "archive overwritten"
        );
        let mut siblings: Vec<_> = fs::read_dir(&scratch.0)
            .expect("list scratch directory")
            .map(|entry| entry.expect("scratch entry").file_name())
            .collect();
        siblings.sort();
        assert_eq!(
            siblings,
            ["component.bin", "outside", "root"],
            "write beside install root"
        );

        let scanned = scan(&input, unpack, "payload.bin");
        if unpack == Unpack::File {
            // Every input fits these budgets; rejecting raw bytes is a regression.
            let scanned = scanned.expect("a plain file always scans");
            assert_eq!(scanned.entries, 1);
            assert_eq!(scanned.bytes, data.len() as u64);
            let written = result.expect("a bounded plain file always extracts");
            assert_eq!(written, scanned, "plain-file scan and writer counts differ");
        } else {
            if let Ok(stats) = &scanned
                && unpack == Unpack::Zip
            {
                assert!(stats.entries as usize <= data.len() / ZIP_CENTRAL_RECORD_BYTES);
            }
            if let Ok(written) = result {
                assert_eq!(
                    scanned.ok(),
                    Some(written),
                    "scan declares different counts than the writer"
                );
            }
        }
        fs::remove_dir_all(root).expect("remove install root");
    }
});
