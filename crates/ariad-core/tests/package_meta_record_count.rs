use std::io::Cursor;

use ariad_core::package_meta::{PackageMetaError, stamp_bytes, stamp_into};

const TIMESTAMP: &str = "2025-10-09T08:53:20Z";
const ALIASES: &[u8] = include_bytes!("../../../fuzz/seeds/docx_meta/unicode-alias.zip");

#[test]
fn unicode_path_alias_count_mismatch_is_typed_and_does_not_write_output() {
    let sentinel = b"existing output must stay unchanged";
    let mut output = Cursor::new(sentinel.to_vec());
    let error = stamp_into(Cursor::new(ALIASES), &mut output, false, TIMESTAMP).unwrap_err();
    assert!(matches!(
        error,
        PackageMetaError::EntryCountMismatch {
            raw_count: 3,
            indexed_count: 2
        }
    ));
    assert_eq!(output.into_inner(), sentinel);
}

#[test]
fn memory_stamping_rejects_aliases_with_the_same_typed_error() {
    assert!(matches!(
        stamp_bytes(ALIASES, false, TIMESTAMP),
        Err(PackageMetaError::EntryCountMismatch {
            raw_count: 3,
            indexed_count: 2
        })
    ));
}
