use super::*;
use ariad_host::package_meta::conversion_time;
use std::{
    io::Cursor,
    sync::atomic::{AtomicUsize, Ordering},
};

const XML: &[u8] = b"<properties><created>old</created><modified>old</modified></properties>";
const RAW: &[u8] = b"source.txt";

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = scratch_dir().join(format!("test-{}", NEXT.fetch_add(1, Ordering::Relaxed)));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
        let _ = fs::remove_dir(scratch_dir());
    }
}

// Write physical headers directly: ZIP writers normalize malformed extra fields.
fn package(raw: &[u8], extra: &[u8]) -> Vec<u8> {
    let mut local = Vec::new();
    let mut central = Vec::new();
    for (name, content, extra) in [
        (raw, &b"payload"[..], extra),
        (&b"docProps/core.xml"[..], XML, &b""[..]),
    ] {
        let offset = local.len() as u32;
        let mut header = [0u8; 30];
        header[..4].copy_from_slice(b"PK\x03\x04");
        header[4..6].copy_from_slice(&20u16.to_le_bytes());
        header[14..18].copy_from_slice(&crc32(content).to_le_bytes());
        header[18..22].copy_from_slice(&(content.len() as u32).to_le_bytes());
        header[22..26].copy_from_slice(&(content.len() as u32).to_le_bytes());
        header[26..28].copy_from_slice(&(name.len() as u16).to_le_bytes());
        header[28..30].copy_from_slice(&(extra.len() as u16).to_le_bytes());
        local.extend_from_slice(&header);
        local.extend_from_slice(name);
        local.extend_from_slice(extra);
        local.extend_from_slice(content);
        let mut record = [0u8; 46];
        record[..4].copy_from_slice(b"PK\x01\x02");
        record[4..6].copy_from_slice(&20u16.to_le_bytes());
        record[6..28].copy_from_slice(&header[4..26]);
        record[28..32].copy_from_slice(&header[26..30]);
        record[42..46].copy_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(&record);
        central.extend_from_slice(name);
        central.extend_from_slice(extra);
    }
    let mut end = [0u8; 22];
    end[..4].copy_from_slice(b"PK\x05\x06");
    end[8..10].copy_from_slice(&2u16.to_le_bytes());
    end[10..12].copy_from_slice(&2u16.to_le_bytes());
    end[12..16].copy_from_slice(&(central.len() as u32).to_le_bytes());
    end[16..20].copy_from_slice(&(local.len() as u32).to_le_bytes());
    local.extend(central);
    local.extend_from_slice(&end);
    assert!(local.len() <= MAX_INPUT_BYTES);
    local
}

fn field(id: u16, content: &[u8]) -> Vec<u8> {
    let mut out = id.to_le_bytes().to_vec();
    out.extend_from_slice(&(content.len() as u16).to_le_bytes());
    out.extend_from_slice(content);
    out
}
fn unicode(version: u8, name: &[u8], crc: u32) -> Vec<u8> {
    let mut content = vec![version];
    content.extend_from_slice(&crc.to_le_bytes());
    content.extend_from_slice(name);
    field(0x7075, &content)
}
fn check_name(dir: &Path, raw: &[u8], extra: &[u8], expected: &str) {
    let bytes = package(raw, extra);
    let path = dir.join("names.zip");
    fs::write(&path, &bytes).unwrap();
    assert_eq!(
        entry_names(&path),
        Some(vec![expected.into(), "docProps/core.xml".into()])
    );
    // Reference decoding is test-only; the preservation oracle never uses indexed names.
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    assert_eq!(zip.by_index(0).unwrap().name().unwrap(), expected);
}

#[test]
fn every_version_crc_utf8_and_repeated_field_decision_retains_names() {
    let dir = Scratch::new();
    let crc = crc32(RAW);
    for version in 0..=u8::MAX {
        let valid = unicode(version, b"logical.txt", crc);
        let invalid = unicode(version, b"\xff.txt", crc);
        let wrong_crc = unicode(version, b"wrong.txt", crc ^ 1);
        let second = unicode(version, b"last.txt", crc);
        check_name(&dir.0, RAW, &valid, "logical.txt");
        check_name(&dir.0, RAW, &invalid, "source.txt");
        check_name(&dir.0, RAW, &wrong_crc, "source.txt");
        check_name(&dir.0, RAW, &unicode(version, b"", crc), "");
        check_name(
            &dir.0,
            RAW,
            &[invalid.clone(), valid.clone()].concat(),
            "logical.txt",
        );
        check_name(
            &dir.0,
            RAW,
            &[valid.clone(), invalid].concat(),
            "logical.txt",
        );
        check_name(
            &dir.0,
            RAW,
            &[valid.clone(), wrong_crc.clone()].concat(),
            "logical.txt",
        );
        check_name(
            &dir.0,
            RAW,
            &[wrong_crc, valid.clone()].concat(),
            "logical.txt",
        );
        check_name(&dir.0, RAW, &[valid.clone(), second].concat(), "last.txt");
        check_name(
            &dir.0,
            RAW,
            &[
                valid,
                unicode(version, b"rebased.txt", crc32(b"logical.txt")),
            ]
            .concat(),
            "logical.txt",
        );
    }
}

#[test]
fn raw_utf8_cp437_empty_names_and_tolerated_header_padding_have_fallbacks() {
    let dir = Scratch::new();
    for (raw, expected) in [
        (&b"source.txt"[..], "source.txt"),
        (&b"\xc3\xa9.txt"[..], "é.txt"),
        (&b"\xff.txt"[..], "\u{a0}.txt"),
        (&b""[..], ""),
    ] {
        for extra in [
            vec![],
            vec![0],
            vec![0, 0],
            vec![0, 0, 0],
            unicode(255, b"\xff", crc32(raw)),
        ] {
            check_name(&dir.0, raw, &extra, expected);
        }
    }
    let valid = unicode(2, b"logical.txt", crc32(RAW));
    for padding in [vec![0], vec![0, 0], vec![0, 0, 0]] {
        check_name(
            &dir.0,
            RAW,
            &[valid.clone(), padding].concat(),
            "logical.txt",
        );
    }
    check_name(
        &dir.0,
        RAW,
        &[field(0xbeef, b""), field(0xbeef, &valid), valid.clone()].concat(),
        "logical.txt",
    );
    check_name(
        &dir.0,
        RAW,
        &[field(0x6375, &[0; 5]), valid.clone()].concat(),
        "logical.txt",
    );
    // ZIP64 without sentinel sizes tolerates a truncated discarded payload.
    check_name(
        &dir.0,
        RAW,
        &[valid, vec![1, 0, 255, 0, 0]].concat(),
        "logical.txt",
    );
}

const ADMITTED: &[(&str, &str)] = &[
    ("unicode-invalid-utf8.zip", "source.txt"),
    ("unicode-crc-mismatch.zip", "source.txt"),
    ("unicode-empty-name.zip", ""),
    ("unicode-invalid-then-valid.zip", "logical.txt"),
    ("unicode-valid-then-invalid.zip", "logical.txt"),
    ("unicode-two-valid.zip", "last.txt"),
    ("unicode-trailing-byte.zip", "logical.txt"),
    ("unicode-trailing-unknown-id.zip", "logical.txt"),
    ("unicode-trailing-unknown-length.zip", "logical.txt"),
    ("unicode-unknown-before-valid.zip", "logical.txt"),
    ("unicode-comment-before-valid.zip", "logical.txt"),
    ("unicode-valid-then-wrong-crc.zip", "logical.txt"),
    ("unicode-wrong-crc-then-valid.zip", "logical.txt"),
    ("unicode-truncated-ignored-zip64.zip", "logical.txt"),
];
fn seed(name: &str) -> Vec<u8> {
    fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("seeds/docx_meta")
            .join(name),
    )
    .unwrap()
}

#[test]
fn every_admitted_regression_seed_runs_preservation_checks_with_the_real_stamper() {
    let dir = Scratch::new();
    for &(name, expected) in ADMITTED {
        let bytes = seed(name);
        let path = dir.0.join("input.docx");
        fs::write(&path, &bytes).unwrap();
        assert_eq!(
            entry_names(&path),
            Some(vec![expected.into(), "docProps/core.xml".into()]),
            "{name}"
        );
        fs::remove_file(path).unwrap();
        assert!(
            stamp_case(&dir.0, "input.docx", &bytes, |p| stamp_docx(
                p,
                conversion_time().unwrap()
            ))
            .is_some(),
            "{name}"
        );
        // These DOCX fixtures have no OPF; rejection must preserve bytes too.
        assert!(
            stamp_case(&dir.0, "input.epub", &bytes, |p| stamp_epub(
                p,
                conversion_time().unwrap()
            ))
            .is_none(),
            "{name}"
        );
    }
}

#[test]
fn truncated_known_fields_are_rejected_without_modifying_the_input() {
    let dir = Scratch::new();
    for name in [
        "unicode-short-field-0.zip",
        "unicode-short-field-1.zip",
        "unicode-short-field-2.zip",
        "unicode-short-field-3.zip",
        "unicode-short-field-4.zip",
        "unicode-truncated-content.zip",
        "unicode-trailing-known-id.zip",
        "unicode-trailing-known-length.zip",
        "unicode-truncated-unknown-content.zip",
    ] {
        let bytes = seed(name);
        assert!(zip::ZipArchive::new(Cursor::new(&bytes)).is_err(), "{name}");
        assert!(
            stamp_case(&dir.0, "input.docx", &bytes, |p| stamp_docx(
                p,
                conversion_time().unwrap()
            ))
            .is_none(),
            "{name}"
        );
    }
    // All recognized IDs reject a missing length; unknown IDs above are padding.
    for id in [0x0001u16, 0x000a, 0x5455, 0x6375, 0x7075, 0x9901, 0xa11e] {
        for length in 2..=3 {
            let mut extra = id.to_le_bytes().to_vec();
            if length == 3 {
                extra.push(0);
            }
            assert!(
                zip::ZipArchive::new(Cursor::new(package(RAW, &extra))).is_err(),
                "{id:x}/{length}"
            );
        }
    }
}

#[test]
fn malformed_optional_fields_never_hide_count_order_or_name_damage() {
    let dir = Scratch::new();
    for &(name, expected) in ADMITTED {
        let original = seed(name);
        let corruptions = [
            (
                stored_zip(&[("docProps/core.xml", XML)]),
                "stamping changed the raw entry count",
            ),
            (
                stored_zip(&[("docProps/core.xml", XML), (expected, b"payload")]),
                "stamping changed raw entry order or logical names",
            ),
            (
                stored_zip(&[("changed.txt", b"payload"), ("docProps/core.xml", XML)]),
                "stamping changed raw entry order or logical names",
            ),
        ];
        for (damaged, message) in corruptions {
            let result = std::panic::catch_unwind(|| {
                stamp_case(&dir.0, "input.docx", &original, |p| {
                    fs::write(p, &damaged).unwrap();
                    Ok::<(), ()>(())
                })
            });
            let _ = fs::remove_file(dir.0.join("input.docx"));
            let panic = match result {
                Err(panic) => panic,
                Ok(_) => panic!("{name}: oracle accepted deliberate damage"),
            };
            let text = panic
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| panic.downcast_ref::<&str>().copied())
                .unwrap_or("");
            assert!(text.contains(message), "{name}: unexpected panic {text}");
        }
    }
}

#[test]
fn successful_stamps_cannot_skip_an_unreadable_input_directory() {
    let dir = Scratch::new();
    let valid = stored_zip(&[("docProps/core.xml", XML)]);
    let result = std::panic::catch_unwind(|| {
        stamp_case(&dir.0, "input.docx", b"not a ZIP", |p| {
            fs::write(p, &valid).unwrap();
            Ok::<(), ()>(())
        })
    });
    assert!(
        result.is_err(),
        "a successful stamp skipped input directory checks"
    );
}
