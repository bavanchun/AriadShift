use ariad_core::{
    ir::{Asset, Document},
    limits::Limits,
    pipeline::PipelineError,
};
use ariad_host::ir_io::{self, ReadError};
use sha2::{Digest, Sha256};

fn read_both(bytes: &[u8], limits: &Limits) -> [Result<Document, ReadError>; 2] {
    [
        ir_io::read(bytes, limits),
        ir_io::read_versioned(bytes, limits),
    ]
}

#[test]
fn both_native_ir_readers_refuse_excessive_semantic_depth() {
    let json = format!(
        "{{\"version\":\"ariad-ir/1\",\"body\":[{}{{\"type\":\"paragraph\",\"content\":[]}}{}]}}",
        "{\"type\":\"quote\",\"blocks\":[".repeat(286),
        "]}".repeat(286),
    );
    for result in read_both(json.as_bytes(), &Limits::local()) {
        let error = result.unwrap_err();
        assert!(error.is_limit_exceeded());
        assert!(
            matches!(error, ReadError::Validation(PipelineError::LimitExceeded { limit })
            if limit == "max_nesting_depth")
        );
    }
}

#[test]
fn both_native_ir_readers_refuse_invalid_assets() {
    let json = br#"{"version":"ariad-ir/1","assets":{"wrong":{"media_type":"image/png","bytes":"AQIDBA=="}}}"#;
    for result in read_both(json, &Limits::local()) {
        let error = result.unwrap_err();
        assert!(!error.is_limit_exceeded());
        assert!(matches!(
            error,
            ReadError::Validation(PipelineError::InvalidIr { .. })
        ));
    }
}

#[test]
fn native_asset_limit_errors_remain_resource_failures() {
    let bytes = vec![1, 2, 3, 4];
    let mut document = Document::default();
    document.assets.insert(
        hex::encode(Sha256::digest(&bytes)),
        Asset {
            media_type: "image/png".into(),
            bytes,
        },
    );
    let json = serde_json::to_vec(&document).unwrap();
    for result in read_both(
        &json,
        &Limits {
            max_asset_bytes: Some(4),
            ..Limits::local()
        },
    ) {
        assert_eq!(result.unwrap(), document);
    }
    for result in read_both(
        &json,
        &Limits {
            max_asset_bytes: Some(3),
            ..Limits::local()
        },
    ) {
        let error = result.unwrap_err();
        assert!(error.is_limit_exceeded());
        assert!(
            matches!(error, ReadError::Validation(PipelineError::LimitExceeded { limit })
            if limit == "max_asset_bytes")
        );
    }
}
