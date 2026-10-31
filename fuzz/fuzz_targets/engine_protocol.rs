#![no_main]

//! Engine protocol (`ariad-engine/1`) NDJSON decoding: the request line an engine reads
//! (`engines::common::read_request`) and the event lines the host reads from an engine's
//! stdout through `runner::parse_event`, the host's compromised-engine boundary.

use std::{io::Cursor, path::Path};

use ariad_core::protocol::{Event, Message, Request};
use ariad_host::{engines::common::read_request, runner::parse_event};
use libfuzzer_sys::fuzz_target;
use serde::{Serialize, de::DeserializeOwned};

const MAX_INPUT_BYTES: usize = 64 * 1024;
const OUTPUT_DIR: &str = "/work/out";

/// A decoded protocol value serializes and decodes back to itself.
fn assert_round_trip<T>(value: &T)
where
    T: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug,
{
    let encoded = serde_json::to_vec(value).expect("protocol value serializes");
    assert!(
        !encoded.contains(&b'\n'),
        "an encoded protocol line contains a newline"
    );
    let decoded: T = serde_json::from_slice(&encoded).expect("encoded protocol value decodes");
    assert_eq!(
        &decoded, value,
        "protocol value changed across a JSON round trip"
    );
}

/// Independently check confinement without using Path::components, as the host does.
fn assert_confined(path: &str) {
    let segments: Vec<&str> = path
        .split('/')
        .filter(|segment| !segment.is_empty() && *segment != ".")
        .collect();
    assert!(
        path.starts_with('/'),
        "relative artifact accepted: {path:?}"
    );
    assert!(
        segments.len() > 2 && segments[..2] == ["work", "out"],
        "artifact outside output directory: {path:?}"
    );
    assert!(
        !segments.contains(&".."),
        "parent traversal accepted: {path:?}"
    );
}

fn assert_consistent(event: &Event) {
    if let Event::Result { ok, error, .. } = event {
        assert_eq!(*ok, error.is_none(), "inconsistent result accepted");
    }
}

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_INPUT_BYTES {
        return;
    }

    // Engine side: the first line is the request.
    if let Ok(request) = read_request(&mut Cursor::new(data)) {
        assert_round_trip(&request);
        let first_line = data.split(|byte| *byte == b'\n').next().unwrap_or_default();
        let direct: Request =
            serde_json::from_slice(first_line).expect("read_request agrees with a direct decode");
        assert_eq!(direct, request);
    }

    // Validate both convert and describe streams, including rejected-line side effects.
    let mut convert_artifacts = Vec::new();
    let mut describe_artifacts = Vec::new();
    for line in data.split(|byte| *byte == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let decoded = serde_json::from_slice::<Event>(line).ok();
        if let Some(event) = &decoded {
            assert_round_trip(event);
        }
        if let Ok(message) = serde_json::from_slice::<Message>(line) {
            assert_round_trip(&message);
        }
        let before = convert_artifacts.len();
        match parse_event(line, Some(OUTPUT_DIR), &mut convert_artifacts) {
            Ok(event) => {
                assert_eq!(decoded.as_ref(), Some(&event));
                assert_consistent(&event);
                assert!(!matches!(event, Event::Capabilities { .. }));
                if let Event::Artifact { path, format } = &event {
                    assert_confined(path);
                    assert_eq!(convert_artifacts.len(), before + 1);
                    assert_eq!(convert_artifacts[before].path, Path::new(path));
                    assert_eq!(&convert_artifacts[before].format, format);
                } else {
                    assert_eq!(convert_artifacts.len(), before);
                }
            }
            Err(_) => assert_eq!(convert_artifacts.len(), before, "refused artifact recorded"),
        }
        if let Ok(event) = parse_event(line, None, &mut describe_artifacts) {
            assert_eq!(decoded.as_ref(), Some(&event));
            assert_consistent(&event);
            assert!(!matches!(event, Event::Artifact { .. }));
        }
        assert!(describe_artifacts.is_empty());
    }
});
