#![cfg_attr(not(test), no_main)]

//! DOCX and EPUB package metadata stamping (`ariad_host::package_meta`): the ZIP rewrite and
//! the `docProps/core.xml` and OPF XML rewriters.
//!
//! An input that starts with a ZIP local header is stamped as a whole package. Any other input
//! is the XML payload: it is stored as `docProps/core.xml` and as `EPUB/package.opf` in a
//! freshly built package, so the fuzzer reaches the XML rewriters without forging CRCs.

use std::{
    fs,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::OnceLock,
};

use ariad_host::{
    archive::MAX_CENTRAL_DIRECTORY_BYTES,
    package_meta::{stamp_docx, stamp_epub},
};
#[cfg(not(test))]
use libfuzzer_sys::fuzz_target;

const MAX_INPUT_BYTES: usize = 64 * 1024;
const MAX_ENTRIES: usize = 1024;
/// Fixed so the environment cannot disable stamping or change a reproducer.
#[cfg(not(test))]
const STAMP_TIME: &str = "2025-10-09T08:53:20Z";

/// A per-process scratch directory, so parallel fuzzing jobs never share a package.
fn scratch_dir() -> &'static PathBuf {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!("ariad-fuzz-docx-meta-{}", std::process::id()));
        fs::create_dir_all(&dir).expect("create fuzz scratch directory");
        dir
    })
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// Builds a ZIP archive whose entries use the stored method.
fn stored_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, payload) in entries {
        let offset = out.len() as u32;
        let crc = crc32(payload);
        let size = payload.len() as u32;
        let name_len = name.len() as u16;

        out.extend_from_slice(b"PK\x03\x04");
        out.extend_from_slice(&20u16.to_le_bytes()); // version needed
        out.extend_from_slice(&0u16.to_le_bytes()); // flags
        out.extend_from_slice(&0u16.to_le_bytes()); // method: stored
        out.extend_from_slice(&0u16.to_le_bytes()); // time
        out.extend_from_slice(&0x21u16.to_le_bytes()); // date: 1980-01-01
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&name_len.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // extra length
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(payload);

        central.extend_from_slice(b"PK\x01\x02");
        central.extend_from_slice(&20u16.to_le_bytes()); // version made by
        central.extend_from_slice(&20u16.to_le_bytes()); // version needed
        central.extend_from_slice(&0u16.to_le_bytes()); // flags
        central.extend_from_slice(&0u16.to_le_bytes()); // method: stored
        central.extend_from_slice(&0u16.to_le_bytes()); // time
        central.extend_from_slice(&0x21u16.to_le_bytes()); // date
        central.extend_from_slice(&crc.to_le_bytes());
        central.extend_from_slice(&size.to_le_bytes());
        central.extend_from_slice(&size.to_le_bytes());
        central.extend_from_slice(&name_len.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes()); // extra length
        central.extend_from_slice(&0u16.to_le_bytes()); // comment length
        central.extend_from_slice(&0u16.to_le_bytes()); // disk number
        central.extend_from_slice(&0u16.to_le_bytes()); // internal attributes
        central.extend_from_slice(&0u32.to_le_bytes()); // external attributes
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name.as_bytes());
    }
    let central_offset = out.len() as u32;
    let central_size = central.len() as u32;
    let count = entries.len() as u16;
    out.extend_from_slice(&central);
    out.extend_from_slice(b"PK\x05\x06");
    out.extend_from_slice(&0u16.to_le_bytes()); // this disk
    out.extend_from_slice(&0u16.to_le_bytes()); // central directory disk
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&central_size.to_le_bytes());
    out.extend_from_slice(&central_offset.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes()); // comment length
    out
}

// IBM CP437's high half; ASCII occupies the unchanged lower half.
const CP437_HIGH: [char; 128] = [
    'Ç', 'ü', 'é', 'â', 'ä', 'à', 'å', 'ç', 'ê', 'ë', 'è', 'ï', 'î', 'ì', 'Ä', 'Å', 'É', 'æ', 'Æ',
    'ô', 'ö', 'ò', 'û', 'ù', 'ÿ', 'Ö', 'Ü', '¢', '£', '¥', '₧', 'ƒ', 'á', 'í', 'ó', 'ú', 'ñ', 'Ñ',
    'ª', 'º', '¿', '⌐', '¬', '½', '¼', '¡', '«', '»', '░', '▒', '▓', '│', '┤', '╡', '╢', '╖', '╕',
    '╣', '║', '╗', '╝', '╜', '╛', '┐', '└', '┴', '┬', '├', '─', '┼', '╞', '╟', '╚', '╔', '╩', '╦',
    '╠', '═', '╬', '╧', '╨', '╤', '╥', '╙', '╘', '╒', '╓', '╫', '╪', '┘', '┌', '█', '▄', '▌', '▐',
    '▀', 'α', 'ß', 'Γ', 'π', 'Σ', 'σ', 'µ', 'τ', 'Φ', 'Θ', 'Ω', 'δ', '∞', 'φ', 'ε', '∩', '≡', '±',
    '≥', '≤', '⌠', '⌡', '÷', '≈', '°', '∙', '·', '√', 'ⁿ', '²', '■', '\u{a0}',
];

/// Match the stamper's documented UTF-8-first decoding, including Unicode Path
/// extras, without indexing records. This is not a spec-level preservation check:
/// flag-zero valid UTF-8 names have a separately tested consumer-visible change.
fn display_name<'a>(mut name: &'a [u8], mut extra: &'a [u8]) -> String {
    let raw_crc = crc32(name);
    while !extra.is_empty() {
        // Tolerated padding or a discarded truncated field retains the last name.
        // Malformed fields that ZIP rejects cannot reach a successful stamp.
        let Some(header) = extra.get(..4) else {
            break;
        };
        let id = u16::from_le_bytes([header[0], header[1]]);
        let len = usize::from(u16::from_le_bytes([header[2], header[3]]));
        let Some(field) = extra.get(4..4 + len) else {
            break;
        };
        if id == 0x7075
            && field.len() >= 5
            && u32::from_le_bytes([field[1], field[2], field[3], field[4]]) == raw_crc
            && std::str::from_utf8(&field[5..]).is_ok()
        {
            // The stamper discards the version byte without validating it.
            // Invalid replacements leave the previous name intact; later fields
            // still use the CRC of the original raw name, even after replacement.
            name = &field[5..];
        }
        extra = &extra[4 + len..];
    }
    match std::str::from_utf8(name) {
        Ok(name) => name.to_owned(),
        Err(_) => name
            .iter()
            .map(|&byte| {
                if byte < 128 {
                    char::from(byte)
                } else {
                    CP437_HIGH[usize::from(byte - 128)]
                }
            })
            .collect(),
    }
}

/// Read one name per raw central-directory record without consulting the ZIP
/// index for names or counts: Unicode aliases can collapse while it is built.
fn entry_names(path: &Path) -> Option<Vec<String>> {
    let mut file = fs::File::open(path).ok()?;
    // Ask the stamper's parser only which directory it selected, never for its
    // indexed entries or count. Tolerant EOCD scans can select a different ZIP
    // when malformed input contains decoy records or trailing bytes.
    let start = zip::ZipArchive::new(file.try_clone().ok()?)
        .ok()?
        .central_directory_start();
    let end = start
        .checked_add(MAX_CENTRAL_DIRECTORY_BYTES)?
        .min(file.metadata().ok()?.len());
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut names = Vec::new();
    while file.stream_position().ok()? < end {
        let mut header = [0; 46];
        file.read_exact(&mut header[..4]).ok()?;
        if matches!(&header[..4], b"PK\x05\x06" | b"PK\x06\x06" | b"PK\x05\x05") {
            return Some(names);
        }
        if &header[..4] != b"PK\x01\x02" || names.len() >= MAX_ENTRIES {
            return None;
        }
        file.read_exact(&mut header[4..]).ok()?;
        let name_len = usize::from(u16::from_le_bytes(header[28..30].try_into().ok()?));
        let extra_len = usize::from(u16::from_le_bytes(header[30..32].try_into().ok()?));
        let comment_len = u16::from_le_bytes(header[32..34].try_into().ok()?);
        let next = file
            .stream_position()
            .ok()?
            .checked_add(name_len as u64 + extra_len as u64 + u64::from(comment_len))?;
        if next > end {
            return None;
        }
        let mut name = vec![0; name_len];
        let mut extra = vec![0; extra_len];
        file.read_exact(&mut name).ok()?;
        file.read_exact(&mut extra).ok()?;
        names.push(display_name(&name, &extra));
        file.seek(SeekFrom::Start(next)).ok()?;
    }
    None
}

/// Stamps one package. A failed stamp leaves the file untouched, a successful stamp keeps
/// every logical entry name, and neither leaves a temporary file behind. Returns the stamped package.
fn stamp_case<F, E>(dir: &Path, file_name: &str, package: &[u8], stamp: F) -> Option<Vec<u8>>
where
    F: Fn(&Path) -> Result<(), E>,
{
    let path = dir.join(file_name);
    fs::write(&path, package).expect("write fuzz package");
    let names_before = entry_names(&path);

    let ok = stamp(path.as_path()).is_ok();
    let after = fs::read(&path).expect("read package after stamping");
    if ok {
        let before = names_before
            .expect("a successful stamp requires a readable input raw central directory");
        let after = entry_names(&path)
            .expect("a successful stamp keeps the raw central directory readable");
        assert_eq!(
            before.len(),
            after.len(),
            "stamping changed the raw entry count"
        );
        assert_eq!(
            before, after,
            "stamping changed raw entry order or logical names"
        );
    } else {
        assert_eq!(after, package, "a failed stamp modified the package");
    }

    let leftovers: Vec<_> = fs::read_dir(dir)
        .expect("list scratch directory")
        .filter_map(Result::ok)
        .map(|entry| entry.file_name())
        .filter(|name| name != file_name)
        .collect();
    assert!(
        leftovers.is_empty(),
        "stamping left files behind: {leftovers:?}"
    );

    fs::remove_file(&path).expect("remove fuzz package");
    ok.then_some(after)
}

#[cfg(not(test))]
fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_INPUT_BYTES {
        return;
    }
    let timestamp = STAMP_TIME.parse().expect("the fixed stamping time parses");
    let dir = scratch_dir();
    let docx = |path: &Path| stamp_docx(path, timestamp);
    let epub = |path: &Path| stamp_epub(path, timestamp);

    if data.starts_with(b"PK\x03\x04") {
        stamp_case(dir, "input.docx", data, docx);
        stamp_case(dir, "input.epub", data, epub);
        return;
    }

    let package = stored_zip(&[
        ("mimetype", &b"application/epub+zip"[..]),
        ("[Content_Types].xml", &b"<Types/>"[..]),
        ("docProps/core.xml", data),
        ("EPUB/package.opf", data),
    ]);
    // A stamped package is itself a valid package, so stamping it again succeeds.
    if let Some(stamped) = stamp_case(dir, "wrapped.docx", &package, docx) {
        assert!(
            stamp_case(dir, "wrapped.docx", &stamped, docx).is_some(),
            "a stamped DOCX package could not be stamped again"
        );
    }
    if let Some(stamped) = stamp_case(dir, "wrapped.epub", &package, epub) {
        assert!(
            stamp_case(dir, "wrapped.epub", &stamped, epub).is_some(),
            "a stamped EPUB package could not be stamped again"
        );
    }
});

#[cfg(test)]
#[path = "docx_meta_tests.rs"]
mod tests;
