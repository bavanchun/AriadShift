//! Exercise the validator at the compromised-engine stdout boundary.

use ariad_core::protocol::Event;
use ariad_host::runner::{Artifact, RunError, parse_event};
use std::path::PathBuf;

#[test]
fn confined_artifacts_are_recorded_once_and_rejections_preserve_the_list() {
    let temp = tempfile::tempdir().unwrap();
    let output = temp.path().join("out");
    std::fs::create_dir(&output).unwrap();
    let directory = output.to_str().unwrap();
    let line = |path: &std::path::Path| {
        serde_json::json!({
            "type": "artifact", "path": path, "format": "docx"
        })
        .to_string()
    };
    let inside = output.join("document.docx");
    std::fs::write(&inside, b"document").unwrap();
    let mut artifacts = Vec::new();
    assert!(matches!(
        parse_event(line(&inside).as_bytes(), Some(directory), &mut artifacts),
        Ok(Event::Artifact { .. })
    ));
    let expected = vec![Artifact {
        path: inside.clone(),
        format: "docx".to_owned(),
    }];
    assert_eq!(artifacts, expected);
    for path in [
        output.join("..").join("escape.docx"),
        output.with_file_name("out-sibling").join("x.docx"),
        output.clone(),
        output.parent().unwrap().join("outside.docx"),
        PathBuf::from("relative.docx"),
    ] {
        assert!(
            matches!(
                parse_event(line(&path).as_bytes(), Some(directory), &mut artifacts),
                Err(RunError::ProtocolViolation(_))
            ),
            "{path:?}"
        );
        assert_eq!(artifacts, expected);
    }
    assert!(matches!(
        parse_event(line(&inside).as_bytes(), None, &mut artifacts),
        Err(RunError::ProtocolViolation(_))
    ));
    assert_eq!(artifacts, expected);
}

#[test]
fn capabilities_require_describe_and_results_require_consistent_error_fields() {
    let capabilities = br#"{"type":"capabilities","engine":"probe","version":"0.1.0","tool":{"name":"probe","status":"found"},"license":"MIT","routes":[],"enforces_memory_limit":false}"#;
    let mut artifacts = Vec::new();
    assert!(matches!(
        parse_event(capabilities, None, &mut artifacts),
        Ok(Event::Capabilities { .. })
    ));
    let output = std::env::temp_dir().join("out");
    let directory = output.to_str().unwrap();
    assert!(matches!(
        parse_event(capabilities, Some(directory), &mut artifacts),
        Err(RunError::ProtocolViolation(_))
    ));
    for output in [None, Some(directory)] {
        for line in [
            br#"{"type":"result","ok":true,"error":{"code":"limit_exceeded","message":"x"}}"#
                .as_slice(),
            br#"{"type":"result","ok":false}"#.as_slice(),
            b"not JSON",
        ] {
            assert!(matches!(
                parse_event(line, output, &mut artifacts),
                Err(RunError::ProtocolViolation(_))
            ));
        }
        for line in [
            br#"{"type":"result","ok":true}"#.as_slice(),
            br#"{"type":"result","ok":false,"error":{"code":"limit_exceeded","message":"x"}}"#
                .as_slice(),
        ] {
            assert!(matches!(
                parse_event(line, output, &mut artifacts),
                Ok(Event::Result { .. })
            ));
        }
    }
    assert!(artifacts.is_empty());
}
