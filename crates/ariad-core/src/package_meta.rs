//! Bounded in-memory DOCX/EPUB package metadata stamping shared by native and browser engines.
use quick_xml::{
    Reader, Writer,
    events::{BytesStart, BytesText, Event},
};
use std::io::{self, Cursor, Read, Seek, SeekFrom, Write};
use thiserror::Error;
use zip::{ZipArchive, ZipWriter};
const CORE_PROPERTIES: &str = "docProps/core.xml";
pub const MAX_ENTRY_BYTES: u64 = 64 * 1024 * 1024;
#[derive(Debug, Error)]
pub enum PackageMetaError {
    #[error("package metadata I/O failed")]
    Io(#[from] io::Error),
    #[error("package archive is invalid")]
    Archive(#[from] zip::result::ZipError),
    #[error(
        "package central directory contains {raw_count} records but the ZIP index contains {indexed_count}"
    )]
    EntryCountMismatch {
        raw_count: usize,
        indexed_count: usize,
    },
    #[error("package contains an entry larger than 64 MiB")]
    EntryTooLarge,
    #[error("package properties are missing")]
    MissingProperties,
    #[error("package properties are invalid")]
    InvalidProperties,
    #[error("SOURCE_DATE_EPOCH is not a supported Unix timestamp")]
    InvalidSourceDateEpoch,
}

/// Stamps dates without changing compressed data in unrelated entries.
pub fn stamp_into<R: Read + Seek, W: Write + Seek>(
    input: R,
    output: W,
    epub: bool,
    timestamp: &str,
) -> Result<W, PackageMetaError> {
    let archive = ZipArchive::new(input)?;
    // An EOCD signature in the preserved comment can be selected as a false
    // directory end after rewriting changes the archive layout.
    if archive
        .comment()
        .windows(4)
        .any(|bytes| bytes == b"PK\x05\x06")
    {
        return Err(zip::result::ZipError::InvalidArchive(
            "archive comment contains an ambiguous end record".into(),
        )
        .into());
    }
    let indexed_count = archive.len();
    let directory_start = archive.central_directory_start();
    let mut input = archive.into_inner();
    let raw_count = count_central_directory_records(&mut input, directory_start)?;
    if raw_count != indexed_count {
        return Err(PackageMetaError::EntryCountMismatch {
            raw_count,
            indexed_count,
        });
    }
    let mut archive = ZipArchive::new(input)?;
    let mut writer = ZipWriter::new(output);
    writer.set_raw_comment(archive.comment().to_vec().into_boxed_slice())?;
    let mut found = false;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let name = entry.name()?.into_owned();
        let target = if epub {
            name.ends_with(".opf") && !name.starts_with("__MACOSX")
        } else {
            name == CORE_PROPERTIES
        };
        if !target {
            writer.raw_copy_file(entry)?;
            continue;
        }
        // Only the XML being rewritten is buffered; unrelated entries are copied raw.
        if entry.size() > MAX_ENTRY_BYTES {
            return Err(PackageMetaError::EntryTooLarge);
        }
        if found {
            return Err(PackageMetaError::InvalidProperties);
        }
        found = true;
        let options = entry.options().into_full_options();
        let mut xml = Vec::new();
        (&mut entry)
            .take(MAX_ENTRY_BYTES + 1)
            .read_to_end(&mut xml)?;
        if xml.len() as u64 > MAX_ENTRY_BYTES {
            return Err(PackageMetaError::EntryTooLarge);
        }
        let stamped = stamp_properties(&xml, timestamp, epub)?;
        writer.start_file(name, options)?;
        writer.write_all(&stamped)?;
    }
    if !found {
        return Err(PackageMetaError::MissingProperties);
    }
    Ok(writer.finish()?)
}

/// Stamps a package held entirely in memory.
pub fn stamp_bytes(bytes: &[u8], epub: bool, timestamp: &str) -> Result<Vec<u8>, PackageMetaError> {
    // Memory-only callers also need a total budget for raw-copied entries.
    let mut archive = ZipArchive::new(Cursor::new(bytes))?;
    let mut total = 0u64;
    for index in 0..archive.len() {
        total = total.saturating_add(archive.by_index(index)?.size());
        if total > 256 * 1024 * 1024 {
            return Err(PackageMetaError::EntryTooLarge);
        }
    }
    stamp_into(Cursor::new(bytes), Cursor::new(Vec::new()), epub, timestamp).map(Cursor::into_inner)
}

/// Count physical records without relying on the ZIP index, which merges aliases.
fn count_central_directory_records<R: Read + Seek>(
    input: &mut R,
    start: u64,
) -> Result<usize, PackageMetaError> {
    let end = input.seek(SeekFrom::End(0))?;
    input.seek(SeekFrom::Start(start))?;
    let mut count = 0usize;
    loop {
        let mut header = [0; 46];
        input.read_exact(&mut header[..4])?;
        if matches!(&header[..4], b"PK\x05\x06" | b"PK\x06\x06" | b"PK\x05\x05") {
            return Ok(count);
        }
        if &header[..4] != b"PK\x01\x02" {
            return Err(zip::result::ZipError::InvalidArchive(
                "invalid central directory record".into(),
            )
            .into());
        }
        input.read_exact(&mut header[4..])?;
        let variable_bytes = u64::from(u16::from_le_bytes([header[28], header[29]]))
            + u64::from(u16::from_le_bytes([header[30], header[31]]))
            + u64::from(u16::from_le_bytes([header[32], header[33]]));
        let next = input
            .stream_position()?
            .checked_add(variable_bytes)
            .filter(|&next| next <= end)
            .ok_or_else(|| io::Error::other("central directory record exceeds the archive"))?;
        input.seek(SeekFrom::Start(next))?;
        count = count
            .checked_add(1)
            .ok_or_else(|| io::Error::other("package record count overflows usize"))?;
    }
}

#[derive(Clone, Copy)]
enum TimestampField {
    Created,
    Modified,
}

impl TimestampField {
    fn local_name(self, epub: bool) -> &'static str {
        match (self, epub) {
            (Self::Created, false) => "created",
            (Self::Created, true) => "date",
            (Self::Modified, false) => "modified",
            (Self::Modified, true) => "meta",
        }
    }
}

fn timestamp_field(element: &BytesStart<'_>, epub: bool) -> Option<TimestampField> {
    let name = element.name().local_name();
    if name.as_ref() == TimestampField::Created.local_name(epub) {
        Some(TimestampField::Created)
    } else if name.as_ref() == TimestampField::Modified.local_name(epub)
        && (!epub
            || element.attributes().flatten().any(|attribute| {
                attribute.key.local_name().as_ref() == "property"
                    && attribute.value.as_ref() == "dcterms:modified"
            }))
    {
        Some(TimestampField::Modified)
    } else {
        None
    }
}

fn stamp_properties(xml: &[u8], timestamp: &str, epub: bool) -> Result<Vec<u8>, PackageMetaError> {
    let mut reader = Reader::from_reader(xml);
    let mut writer = Writer::new(Vec::with_capacity(xml.len()));
    let mut active: Option<TimestampField> = None;
    let mut stamped = [false; 2];

    loop {
        let event = reader
            .read_event()
            .map_err(|_| PackageMetaError::InvalidProperties)?;
        match event {
            Event::Start(start) => {
                if let Some(target) = timestamp_field(&start, epub) {
                    if active.is_some() || stamped[target as usize] {
                        return Err(PackageMetaError::InvalidProperties);
                    }
                    active = Some(target);
                }
                writer.write_event(Event::Start(start))?;
            }
            Event::Text(_) | Event::CData(_) if active.is_some() => {}
            Event::End(end) => {
                if let Some(target) = active {
                    if end.name().local_name().as_ref() != target.local_name(epub) {
                        return Err(PackageMetaError::InvalidProperties);
                    }
                    writer.write_event(Event::Text(BytesText::new(timestamp)))?;
                    stamped[target as usize] = true;
                    active = None;
                }
                writer.write_event(Event::End(end))?;
            }
            Event::Empty(empty) => {
                if let Some(target) = timestamp_field(&empty, epub) {
                    if !epub || active.is_some() || stamped[target as usize] {
                        return Err(PackageMetaError::InvalidProperties);
                    }
                    writer.write_event(Event::Start(empty.borrow()))?;
                    writer.write_event(Event::Text(BytesText::new(timestamp)))?;
                    writer.write_event(Event::End(empty.to_end()))?;
                    stamped[target as usize] = true;
                } else {
                    writer.write_event(Event::Empty(empty))?;
                }
            }
            Event::Eof => break,
            other => writer.write_event(other)?,
        }
    }

    if stamped != [true; 2] || active.is_some() {
        return Err(PackageMetaError::InvalidProperties);
    }
    Ok(writer.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use zip::write::SimpleFileOptions;

    const TIMESTAMP: &str = "2026-10-24T15:00:00Z";

    fn properties(epub: bool) -> (&'static str, &'static [u8]) {
        if epub {
            (
                "EPUB/content.opf",
                br#"<metadata><date>old</date><meta property="dcterms:modified">old</meta></metadata>"#,
            )
        } else {
            (
                CORE_PROPERTIES,
                b"<properties><created>old</created><modified>old</modified></properties>",
            )
        }
    }

    fn write_repeated_entry(
        writer: &mut ZipWriter<Cursor<Vec<u8>>>,
        name: &str,
        size: u64,
        compression: zip::CompressionMethod,
    ) {
        writer
            .start_file(
                name,
                SimpleFileOptions::default().compression_method(compression),
            )
            .unwrap();
        io::copy(&mut io::repeat(b'x').take(size), writer).unwrap();
    }

    fn raw_entry<'a>(bytes: &'a [u8], name: &str) -> &'a [u8] {
        let mut archive = ZipArchive::new(Cursor::new(bytes)).unwrap();
        let entry = archive.by_name(name).unwrap();
        let start = entry.data_start().unwrap() as usize;
        &bytes[start..start + entry.compressed_size() as usize]
    }

    #[test]
    fn rejects_crash_seed_with_ambiguous_archive_comment() {
        let input = include_bytes!("../../../fuzz/seeds/docx_meta/embedded-eocd-comment.zip");
        let archive = ZipArchive::new(Cursor::new(input)).unwrap();
        assert!(
            archive
                .comment()
                .windows(4)
                .any(|bytes| bytes == b"PK\x05\x06")
        );
        assert!(matches!(
            stamp_bytes(input, false, TIMESTAMP),
            Err(PackageMetaError::Archive(_))
        ));
    }

    #[test]
    fn copies_large_stored_media_without_changing_bytes() {
        for epub in [false, true] {
            let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
            let media_name = if epub {
                "EPUB/media/image.png"
            } else {
                "word/media/image.png"
            };
            write_repeated_entry(
                &mut writer,
                media_name,
                65 * 1024 * 1024,
                zip::CompressionMethod::Stored,
            );
            let (name, xml) = properties(epub);
            writer
                .start_file(name, SimpleFileOptions::default())
                .unwrap();
            writer.write_all(xml).unwrap();
            let input = writer.finish().unwrap().into_inner();

            let stamped = stamp_into(
                Cursor::new(&input),
                Cursor::new(Vec::new()),
                epub,
                TIMESTAMP,
            )
            .unwrap()
            .into_inner();
            assert_eq!(
                raw_entry(&input, media_name),
                raw_entry(&stamped, media_name)
            );
            let mut archive = ZipArchive::new(Cursor::new(&stamped)).unwrap();
            let mut xml = String::new();
            archive
                .by_name(name)
                .unwrap()
                .read_to_string(&mut xml)
                .unwrap();
            assert_eq!(xml.matches(TIMESTAMP).count(), 2);

            // Memory-only stamping accepts this media within its total budget, too.
            assert!(stamp_bytes(&input, epub, TIMESTAMP).is_ok());
        }
    }

    #[test]
    fn refuses_oversized_target_xml_in_both_formats() {
        for epub in [false, true] {
            let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
            write_repeated_entry(
                &mut writer,
                properties(epub).0,
                MAX_ENTRY_BYTES + 1,
                zip::CompressionMethod::Deflated,
            );
            let input = writer.finish().unwrap().into_inner();
            assert!(matches!(
                stamp_into(
                    Cursor::new(&input),
                    Cursor::new(Vec::new()),
                    epub,
                    TIMESTAMP
                ),
                Err(PackageMetaError::EntryTooLarge)
            ));
            assert!(matches!(
                stamp_bytes(&input, epub, TIMESTAMP),
                Err(PackageMetaError::EntryTooLarge)
            ));
        }
    }

    #[test]
    fn memory_total_budget_is_inclusive_and_does_not_limit_streaming() {
        let (name, xml) = properties(false);
        for extra in [0, 1] {
            let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
            for index in 0..4 {
                let size = if index == 3 {
                    MAX_ENTRY_BYTES - xml.len() as u64 + extra
                } else {
                    MAX_ENTRY_BYTES
                };
                write_repeated_entry(
                    &mut writer,
                    &format!("word/media/{index}.bin"),
                    size,
                    zip::CompressionMethod::Deflated,
                );
            }
            writer
                .start_file(name, SimpleFileOptions::default())
                .unwrap();
            writer.write_all(xml).unwrap();
            let input = writer.finish().unwrap().into_inner();

            assert!(
                stamp_into(
                    Cursor::new(&input),
                    Cursor::new(Vec::new()),
                    false,
                    TIMESTAMP,
                )
                .is_ok()
            );
            let result = stamp_bytes(&input, false, TIMESTAMP);
            if extra == 0 {
                assert!(result.is_ok());
            } else {
                assert!(matches!(result, Err(PackageMetaError::EntryTooLarge)));
            }
        }
    }

    #[test]
    fn stamps_both_formats_and_preserves_other_properties() {
        for (epub, xml) in [
            (
                false,
                r#"<properties><title>Keep</title><created>old</created><modified><![CDATA[old]]></modified></properties>"#,
            ),
            (
                true,
                r#"<metadata><title>Keep</title><date>old</date><meta property="dcterms:modified"><![CDATA[old]]></meta><meta property="other">Keep</meta></metadata>"#,
            ),
        ] {
            let stamped = stamp_properties(xml.as_bytes(), TIMESTAMP, epub).unwrap();
            let stamped = String::from_utf8(stamped).unwrap();
            assert_eq!(stamped.matches(TIMESTAMP).count(), 2);
            assert!(!stamped.contains("old"));
            assert!(stamped.contains("<title>Keep</title>"));
            if epub {
                assert!(stamped.contains(r#"<meta property="other">Keep</meta>"#));
            }
        }
    }

    #[test]
    fn expands_empty_epub_dates_but_rejects_empty_docx_dates() {
        let xml = br#"<metadata><date/><meta property="dcterms:modified"/></metadata>"#;
        let stamped = stamp_properties(xml, TIMESTAMP, true).unwrap();
        let stamped = String::from_utf8(stamped).unwrap();
        assert!(stamped.contains(&format!("<date>{TIMESTAMP}</date>")));
        assert!(stamped.contains(&format!(
            r#"<meta property="dcterms:modified">{TIMESTAMP}</meta>"#
        )));
        assert!(matches!(
            stamp_properties(
                b"<properties><created/><modified>old</modified></properties>",
                TIMESTAMP,
                false
            ),
            Err(PackageMetaError::InvalidProperties)
        ));
    }

    #[test]
    fn rejects_missing_duplicate_or_nested_timestamp_fields() {
        for (epub, xml) in [
            (false, "<properties><created>old</created></properties>"),
            (
                false,
                "<properties><created>old</created><created>old</created><modified>old</modified></properties>",
            ),
            (
                false,
                "<properties><created><nested>old</nested></created><modified>old</modified></properties>",
            ),
            (
                true,
                r#"<metadata><date>old</date><meta property="other">old</meta></metadata>"#,
            ),
            (
                true,
                r#"<metadata><date/><date/><meta property="dcterms:modified"/></metadata>"#,
            ),
            (
                true,
                r#"<metadata><date><nested>old</nested></date><meta property="dcterms:modified"/></metadata>"#,
            ),
        ] {
            assert!(
                matches!(
                    stamp_properties(xml.as_bytes(), TIMESTAMP, epub),
                    Err(PackageMetaError::InvalidProperties)
                ),
                "{xml}"
            );
        }
    }

    #[test]
    fn stamps_a_memory_package_and_preserves_archive_comment_and_unrelated_data() {
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        writer.set_comment("preserved comment").unwrap();
        writer
            .start_file("unrelated.bin", SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"unchanged").unwrap();
        writer
            .start_file(CORE_PROPERTIES, SimpleFileOptions::default())
            .unwrap();
        writer
            .write_all(b"<properties><created>old</created><modified>old</modified></properties>")
            .unwrap();
        let bytes = writer.finish().unwrap().into_inner();

        let bytes = stamp_bytes(&bytes, false, TIMESTAMP).unwrap();
        let mut archive = ZipArchive::new(Cursor::new(bytes)).unwrap();
        assert_eq!(archive.comment(), b"preserved comment");
        let mut unchanged = Vec::new();
        archive
            .by_name("unrelated.bin")
            .unwrap()
            .read_to_end(&mut unchanged)
            .unwrap();
        assert_eq!(unchanged, b"unchanged");
        let mut properties = String::new();
        archive
            .by_name(CORE_PROPERTIES)
            .unwrap()
            .read_to_string(&mut properties)
            .unwrap();
        assert_eq!(properties.matches(TIMESTAMP).count(), 2);
    }
}
