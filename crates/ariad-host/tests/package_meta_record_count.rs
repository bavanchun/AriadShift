use std::fs;

use ariad_host::package_meta::{PackageMetaError, stamp_docx, stamp_epub};
use tempfile::tempdir;

#[test]
fn aliases_return_the_typed_error_without_modifying_files_or_leaving_temporaries() {
    let bytes = include_bytes!("../../../fuzz/seeds/docx_meta/unicode-alias.zip");
    for stamp in [stamp_docx, stamp_epub] {
        let directory = tempdir().unwrap();
        let path = directory.path().join("package.zip");
        fs::write(&path, bytes).unwrap();
        let error = stamp(&path, "2025-10-09T08:53:20Z".parse().unwrap()).unwrap_err();
        assert!(matches!(
            error,
            PackageMetaError::EntryCountMismatch {
                raw_count: 3,
                indexed_count: 2
            }
        ));
        assert_eq!(fs::read(&path).unwrap(), bytes);
        let remaining: Vec<_> = fs::read_dir(directory.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(remaining, ["package.zip"]);
    }
}
