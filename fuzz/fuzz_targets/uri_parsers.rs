#![no_main]

//! URI and path-string parsers that guard trust boundaries: `file://` URIs from MCP clients
//! (`confine::parse_file_uri`), the Windows path syntax check that runs on every platform
//! (`confine::validate_windows_path_syntax`), and the link scheme filter applied to document
//! links (`links::allowed_link`).

use ariad_core::links::allowed_link;
use ariad_host::confine::{parse_file_uri, path_to_file_uri, validate_windows_path_syntax};
use libfuzzer_sys::fuzz_target;

const MAX_INPUT_BYTES: usize = 8 * 1024;
/// Independent list of reserved Windows device stems.
const DEVICES: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_INPUT_BYTES {
        return;
    }
    let text = String::from_utf8_lossy(data);

    if let Ok(path) = parse_file_uri(&text) {
        assert!(
            path.is_absolute(),
            "file URI produced a relative path {path:?}"
        );
        assert!(
            !path.as_os_str().to_string_lossy().contains('\0'),
            "file URI produced a path with a NUL byte"
        );
        assert!(text.starts_with("file://"));
        let uri = path_to_file_uri(&path);
        assert_eq!(
            parse_file_uri(&uri).ok().as_ref(),
            Some(&path),
            "{path:?} did not survive a file URI round trip through {uri:?}"
        );
    }

    if validate_windows_path_syntax(&text).is_ok() {
        assert!(!text.contains('\0'), "NUL byte accepted in a Windows path");
        let cleaned = text
            .strip_prefix(r"\\?\UNC\")
            .or_else(|| text.strip_prefix(r"\\?\"))
            .unwrap_or(&text);
        for (index, part) in cleaned.split(['/', '\\']).enumerate() {
            let drive = index == 0
                && part.len() == 2
                && part.as_bytes()[0].is_ascii_alphabetic()
                && part.ends_with(':');
            assert!(
                drive || !part.contains(':'),
                "alternate data stream {part:?} accepted"
            );
            let stem = part.split('.').next().unwrap_or(part).trim();
            assert!(
                !DEVICES
                    .iter()
                    .any(|device| device.eq_ignore_ascii_case(stem)),
                "reserved device name {part:?} accepted"
            );
        }
    }

    // Same-document fragments stay inside the document, so any text after `#` is allowed,
    // whitespace included. Every other allowed link is trimmed and uses an allowed scheme.
    let allowed = allowed_link(&text);
    if text.starts_with('#') {
        assert!(allowed, "same-document fragment rejected");
    } else if allowed {
        assert_eq!(
            text.trim(),
            text,
            "link with surrounding whitespace allowed"
        );
        let scheme = text
            .split_once(':')
            .map(|(scheme, _)| scheme.to_ascii_lowercase())
            .expect("an allowed non-anchor link has a scheme");
        assert!(
            matches!(scheme.as_str(), "http" | "https" | "mailto"),
            "link scheme {scheme:?} allowed"
        );
    }
});
