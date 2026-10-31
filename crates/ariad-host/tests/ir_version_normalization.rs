use ariad_core::{
    ir::{IR_VERSION, is_supported_version},
    limits::Limits,
};
use ariad_host::ir_io::{ReadError, read, read_versioned};

#[test]
fn supported_versions_normalize_without_changing_document_content() {
    for version in ["ariad-ir/0", IR_VERSION] {
        let bytes = format!(
            r#"{{"version":"{version}","body":[{{"type":"paragraph","content":[{{"type":"text","text":"Legacy content"}}]}}],"furniture":[{{"type":"page_break"}}]}}"#
        );
        assert!(is_supported_version(Some(version)));
        let mut plain = read(bytes.as_bytes(), &Limits::local()).unwrap();
        assert_eq!(plain.version, version);
        plain.version = IR_VERSION.to_owned();
        assert_eq!(
            read_versioned(bytes.as_bytes(), &Limits::local()).unwrap(),
            plain
        );
    }
}

#[test]
fn future_missing_and_non_string_versions_are_rejected() {
    for version in [Some(r#""ariad-ir/999""#), None, Some("null"), Some("1")] {
        let field = version.map_or(String::new(), |v| format!(r#""version":{v},"#));
        let bytes = format!(r#"{{{field}"body":[]}}"#);
        assert!(matches!(
            read_versioned(bytes.as_bytes(), &Limits::local()),
            Err(ReadError::UnsupportedVersion)
        ));
    }
}

#[test]
fn normalized_reader_preserves_byte_depth_and_block_limits() {
    let bytes = br#"{"version":"ariad-ir/0","body":[{"type":"page_break"},{"type":"page_break"}]}"#;
    let limits = Limits {
        max_ir_json_bytes: bytes.len() as u64 - 1,
        ..Limits::local()
    };
    assert!(matches!(
        read_versioned(bytes, &limits),
        Err(ReadError::ByteLimitExceeded { .. })
    ));
    let limits = Limits {
        max_blocks: 1,
        ..Limits::local()
    };
    assert!(matches!(
        read_versioned(bytes, &limits),
        Err(ReadError::BlockLimitExceeded { count: 2, limit: 1 })
    ));
    let deep = format!(
        r#"{{"version":"ariad-ir/0","unknown":{}0{}}}"#,
        "[".repeat(100),
        "]".repeat(100)
    );
    let limits = Limits {
        max_nesting_depth: 1,
        ..Limits::local()
    };
    assert!(matches!(
        read_versioned(deep.as_bytes(), &limits),
        Err(ReadError::DepthExceeded(_))
    ));
}
