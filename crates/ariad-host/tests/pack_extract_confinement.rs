mod support;

use ariad_host::packs::{self, PackError, Stats, Unpack};
use std::{fs, path::Path};
use support::{Entry, gzip, tar_bytes, zip_bytes, zstd};

fn archive(entries: &[Entry], unpack: Unpack) -> Vec<u8> {
    match unpack {
        Unpack::TarGz => gzip(&tar_bytes(entries)),
        Unpack::TarZst => zstd(&tar_bytes(entries)),
        Unpack::Zip => zip_bytes(entries),
        Unpack::File => panic!("plain files do not have archive entries"),
    }
}

fn assert_siblings_unchanged(parent: &Path) {
    assert_eq!(
        fs::read(parent.join("outside/sentinel")).unwrap(),
        b"untouched"
    );
    assert_eq!(fs::read_dir(parent.join("outside")).unwrap().count(), 1);
    let mut names: Vec<_> = fs::read_dir(parent)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    names.sort();
    assert_eq!(names, ["component.bin", "outside", "root"]);
}

fn extract_case(
    entries: &[Entry],
    unpack: Unpack,
) -> (tempfile::TempDir, Result<Stats, PackError>) {
    let dir = support::temp();
    fs::create_dir(dir.path().join("root")).unwrap();
    fs::create_dir(dir.path().join("outside")).unwrap();
    fs::write(dir.path().join("outside/sentinel"), b"untouched").unwrap();
    let input = dir.path().join("component.bin");
    fs::write(&input, archive(entries, unpack)).unwrap();
    let result = packs::extract_for_fuzzing(
        &input,
        unpack,
        "runtime",
        &dir.path().join("root"),
        1 << 20,
        64,
    );
    assert_siblings_unchanged(dir.path());
    (dir, result)
}

#[test]
fn real_writer_extracts_supported_formats_and_enforces_budgets() {
    for unpack in [Unpack::TarGz, Unpack::TarZst, Unpack::Zip] {
        let (dir, result) = extract_case(
            &[
                Entry::Dir("bin/"),
                Entry::File("bin/engine", b"payload".to_vec(), 0o755),
            ],
            unpack,
        );
        assert_eq!(
            result.unwrap(),
            Stats {
                bytes: 7,
                entries: 2
            }
        );
        assert_eq!(
            fs::read(dir.path().join("root/bin/engine")).unwrap(),
            b"payload"
        );
        let root = dir.path().join("limited");
        fs::create_dir(&root).unwrap();
        let error = packs::extract_for_fuzzing(
            &dir.path().join("component.bin"),
            unpack,
            "runtime",
            &root,
            6,
            64,
        )
        .unwrap_err();
        assert!(
            matches!(
                error,
                PackError::ExtractLimit {
                    what: "unpacked size",
                    limit: 6,
                    ..
                }
            ),
            "{error}"
        );
        assert!(
            fs::metadata(root.join("bin/engine")).unwrap().len() <= 6,
            "partial output exceeded the byte budget"
        );
        fs::remove_dir_all(&root).unwrap();
        fs::create_dir(&root).unwrap();
        let error = packs::extract_for_fuzzing(
            &dir.path().join("component.bin"),
            unpack,
            "runtime",
            &root,
            7,
            1,
        )
        .unwrap_err();
        assert!(
            matches!(
                error,
                PackError::ExtractLimit {
                    what: "entry count",
                    limit: 1,
                    ..
                }
            ),
            "{error}"
        );
    }
    let dir = support::temp();
    let input = dir.path().join("input");
    let root = dir.path().join("root");
    fs::create_dir(&root).unwrap();
    fs::write(&input, b"plain").unwrap();
    assert_eq!(
        packs::extract_for_fuzzing(&input, Unpack::File, "payload.bin", &root, 5, 1).unwrap(),
        Stats {
            bytes: 5,
            entries: 1
        }
    );
    assert_eq!(fs::read(root.join("payload.bin")).unwrap(), b"plain");
}

#[test]
fn parent_escapes_leave_successful_partial_outputs_inside_root() {
    for unpack in [Unpack::TarGz, Unpack::TarZst, Unpack::Zip] {
        let (dir, result) = extract_case(
            &[
                Entry::File("kept", b"inside".to_vec(), 0o644),
                Entry::File("../outside/escaped", b"outside".to_vec(), 0o644),
            ],
            unpack,
        );
        assert!(
            matches!(result, Err(PackError::UnsafeArchive { .. })),
            "{result:?}"
        );
        assert_eq!(fs::read(dir.path().join("root/kept")).unwrap(), b"inside");
        assert_eq!(fs::read_dir(dir.path().join("root")).unwrap().count(), 1);
    }
}

#[cfg(unix)]
#[test]
fn symlink_prefixes_chains_and_final_link_check_confine_the_writer() {
    for unpack in [Unpack::TarGz, Unpack::TarZst, Unpack::Zip] {
        let (dir, result) = extract_case(
            &[
                Entry::Dir("bin/"),
                Entry::File("bin/engine", b"inside".to_vec(), 0o755),
                Entry::Symlink("bin/alias", "engine"),
            ],
            unpack,
        );
        assert_eq!(
            result.unwrap(),
            Stats {
                bytes: 6,
                entries: 3
            }
        );
        assert_eq!(
            fs::read(dir.path().join("root/bin/alias")).unwrap(),
            b"inside"
        );
        for entries in [
            vec![
                Entry::Symlink("up", "../outside"),
                Entry::File("up/escaped", b"x".to_vec(), 0o644),
            ],
            vec![
                Entry::Dir("deep/"),
                Entry::Symlink("deep/t", ".."),
                Entry::Symlink("deep/l", "t/.."),
                Entry::File("deep/l/outside/escaped", b"x".to_vec(), 0o644),
            ],
            vec![
                Entry::Dir("deep/"),
                Entry::Symlink("deep/t", ".."),
                Entry::Symlink("deep/l", "t/.."),
            ],
            vec![Entry::Symlink("dangling", "absent")],
        ] {
            let (_dir, result) = extract_case(&entries, unpack);
            assert!(
                matches!(result, Err(PackError::UnsafeArchive { .. })),
                "{result:?}"
            );
        }
    }
}

#[test]
fn tar_hard_links_use_the_install_writer_copy_rules() {
    for unpack in [Unpack::TarGz, Unpack::TarZst] {
        let (dir, result) = extract_case(
            &[
                Entry::File("source", b"copy".to_vec(), 0o644),
                Entry::HardLink("copy", "source"),
            ],
            unpack,
        );
        assert_eq!(
            result.unwrap(),
            Stats {
                bytes: 8,
                entries: 2
            }
        );
        assert_eq!(fs::read(dir.path().join("root/copy")).unwrap(), b"copy");
        let (_dir, result) =
            extract_case(&[Entry::HardLink("copy", "../outside/sentinel")], unpack);
        assert!(
            matches!(result, Err(PackError::UnsafeArchive { .. })),
            "{result:?}"
        );
    }
}
