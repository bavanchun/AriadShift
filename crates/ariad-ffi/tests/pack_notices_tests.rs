use ariad_ffi::{FfiError, pack_notices};

/// Runs against an empty data directory; the process-wide variable is set once, before any
/// pack lookup, and this test binary holds no other test.
#[test]
fn notices_are_empty_without_packs_and_unknown_packs_are_refused() {
    let data = tempfile::tempdir().unwrap();
    // SAFETY: this is the only test in the binary, so no other thread reads the environment.
    unsafe { std::env::set_var("ASHIFT_DATA_DIR", data.path()) };

    assert_eq!(pack_notices(None, None).unwrap(), Vec::new());
    let error = pack_notices(Some("probe".to_owned()), None).unwrap_err();
    assert!(
        matches!(&error, FfiError::Internal { message } if message.contains("not_installed")),
        "{error}"
    );
    let error = pack_notices(Some("Bad Name".to_owned()), None).unwrap_err();
    assert!(matches!(error, FfiError::InvalidRequest { .. }), "{error}");
}
