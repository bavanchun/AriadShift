use std::{
    env,
    fs::File,
    io::{self, Read, Write},
    path::Path,
};

use jiff::Timestamp;
use quick_xml::{Reader, Writer, events::Event};
use tempfile::NamedTempFile;
use thiserror::Error;
use zip::{ZipArchive, ZipWriter};

const CORE_PROPERTIES: &str = "docProps/core.xml";
const MAX_ENTRY_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Error)]
pub enum DocxMetaError {
    #[error("DOCX metadata I/O failed")]
    Io(#[from] io::Error),
    #[error("DOCX archive is invalid")]
    Archive(#[from] zip::result::ZipError),
    #[error("DOCX contains an entry larger than 64 MiB")]
    EntryTooLarge,
    #[error("DOCX core properties are missing")]
    MissingCoreProperties,
    #[error("DOCX core properties are invalid")]
    InvalidCoreProperties,
    #[error("SOURCE_DATE_EPOCH is not a supported Unix timestamp")]
    InvalidSourceDateEpoch,
}

/// Stamps DOCX created and modified metadata, preserving every other entry's raw ZIP data.
pub fn stamp(path: &Path, timestamp: Timestamp) -> Result<(), DocxMetaError> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let temporary = NamedTempFile::new_in(parent)?;
    let input = File::open(path)?;
    let mut archive = ZipArchive::new(input)?;
    let mut writer = ZipWriter::new(temporary.reopen()?);
    writer.set_raw_comment(archive.comment().to_vec().into_boxed_slice())?;
    let timestamp = timestamp.to_string();
    let mut found_core_properties = false;

    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        if entry.size() > MAX_ENTRY_BYTES {
            return Err(DocxMetaError::EntryTooLarge);
        }
        if entry.name() != CORE_PROPERTIES {
            writer.raw_copy_file(entry)?;
            continue;
        }
        if found_core_properties {
            return Err(DocxMetaError::InvalidCoreProperties);
        }
        found_core_properties = true;

        let name = entry.name().to_owned();
        let options = entry.options().into_full_options();
        let mut xml = Vec::with_capacity(entry.size() as usize);
        (&mut entry)
            .take(MAX_ENTRY_BYTES + 1)
            .read_to_end(&mut xml)?;
        if xml.len() as u64 > MAX_ENTRY_BYTES {
            return Err(DocxMetaError::EntryTooLarge);
        }
        let stamped = stamp_core_properties(&xml, &timestamp)?;
        writer.start_file(name, options)?;
        writer.write_all(&stamped)?;
    }

    if !found_core_properties {
        return Err(DocxMetaError::MissingCoreProperties);
    }

    drop(archive);
    let output = writer.finish()?;
    output.sync_all()?;
    drop(output);
    temporary
        .persist(path)
        .map_err(|error| DocxMetaError::Io(error.error))?;
    Ok(())
}

/// Selects the reproducible timestamp when SOURCE_DATE_EPOCH is set, otherwise the current time.
pub fn conversion_time() -> Result<Timestamp, DocxMetaError> {
    match env::var_os("SOURCE_DATE_EPOCH") {
        Some(value) => timestamp_from_source_date_epoch(Some(
            value
                .to_str()
                .ok_or(DocxMetaError::InvalidSourceDateEpoch)?,
        )),
        None => timestamp_from_source_date_epoch(None),
    }
}

fn timestamp_from_source_date_epoch(value: Option<&str>) -> Result<Timestamp, DocxMetaError> {
    match value {
        Some(value) => value
            .parse::<i64>()
            .ok()
            .and_then(|seconds| Timestamp::from_second(seconds).ok())
            .ok_or(DocxMetaError::InvalidSourceDateEpoch),
        None => Ok(Timestamp::now()),
    }
}

fn stamp_core_properties(xml: &[u8], timestamp: &str) -> Result<Vec<u8>, DocxMetaError> {
    #[derive(Clone, Copy, Eq, PartialEq)]
    enum Field {
        Created,
        Modified,
    }

    fn field(name: &str) -> Option<Field> {
        match name {
            "created" => Some(Field::Created),
            "modified" => Some(Field::Modified),
            _ => None,
        }
    }

    let mut reader = Reader::from_reader(xml);
    let mut writer = Writer::new(Vec::with_capacity(xml.len()));
    let mut active = None;
    let mut created = false;
    let mut modified = false;

    loop {
        let event = reader
            .read_event()
            .map_err(|_| DocxMetaError::InvalidCoreProperties)?;
        match event {
            Event::Start(start) => {
                let target = field(start.name().local_name().as_ref());
                if let Some(target) = target {
                    if active.is_some()
                        || match target {
                            Field::Created => created,
                            Field::Modified => modified,
                        }
                    {
                        return Err(DocxMetaError::InvalidCoreProperties);
                    }
                    active = Some(target);
                }
                writer
                    .write_event(Event::Start(start))
                    .map_err(DocxMetaError::Io)?;
            }
            Event::Text(_) | Event::CData(_) if active.is_some() => {}
            Event::End(end) => {
                if let Some(target) = active {
                    if field(end.name().local_name().as_ref()) != Some(target) {
                        return Err(DocxMetaError::InvalidCoreProperties);
                    }
                    writer
                        .write_event(Event::Text(quick_xml::events::BytesText::new(timestamp)))
                        .map_err(DocxMetaError::Io)?;
                    match target {
                        Field::Created => created = true,
                        Field::Modified => modified = true,
                    }
                    active = None;
                }
                writer
                    .write_event(Event::End(end))
                    .map_err(DocxMetaError::Io)?;
            }
            Event::Empty(empty) if field(empty.name().local_name().as_ref()).is_some() => {
                return Err(DocxMetaError::InvalidCoreProperties);
            }
            Event::Eof => break,
            other => writer.write_event(other).map_err(DocxMetaError::Io)?,
        }
    }

    if !created || !modified || active.is_some() {
        return Err(DocxMetaError::InvalidCoreProperties);
    }
    Ok(writer.into_inner())
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        fs::File,
        io::{Read, Write},
    };

    use jiff::Timestamp;
    use tempfile::tempdir;
    use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

    use super::{DocxMetaError, MAX_ENTRY_BYTES, stamp, timestamp_from_source_date_epoch};

    const CORE_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?><cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:dcterms="http://purl.org/dc/terms/" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"><dc:title>Preserved title</dc:title><dcterms:created xsi:type="dcterms:W3CDTF">2000-01-01T00:00:00Z</dcterms:created><dcterms:modified xsi:type="dcterms:W3CDTF">2000-01-01T00:00:00Z</dcterms:modified></cp:coreProperties>"#;

    fn make_docx(path: &std::path::Path, include_large_entry: bool) {
        let file = File::create(path).unwrap();
        let mut writer = ZipWriter::new(file);
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        writer.start_file("[Content_Types].xml", options).unwrap();
        writer.write_all(b"<Types/> ").unwrap();
        writer.start_file("word/document.xml", options).unwrap();
        writer
            .write_all(b"<w:document>unchanged</w:document>")
            .unwrap();
        if include_large_entry {
            writer.start_file("word/large.xml", options).unwrap();
            let chunk = [b'x'; 1024 * 1024];
            for _ in 0..(MAX_ENTRY_BYTES / chunk.len() as u64 + 1) {
                writer.write_all(&chunk).unwrap();
            }
        }
        writer.start_file("docProps/core.xml", options).unwrap();
        writer.write_all(CORE_XML.as_bytes()).unwrap();
        writer.finish().unwrap();
    }

    fn compressed_entry_bytes(archive_bytes: &[u8], name: &str) -> Vec<u8> {
        let mut archive = ZipArchive::new(std::io::Cursor::new(archive_bytes)).unwrap();
        let entry = archive.by_name(name).unwrap();
        let start = entry.data_start().unwrap() as usize;
        let end = start + entry.compressed_size() as usize;
        archive_bytes[start..end].to_vec()
    }

    #[test]
    fn changes_only_core_property_timestamps_and_preserves_other_entries() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("document.docx");
        make_docx(&path, false);
        let original_archive = fs::read(&path).unwrap();
        let timestamp = Timestamp::from_second(1_700_000_000).unwrap();

        stamp(&path, timestamp).unwrap();

        let stamped_archive = fs::read(&path).unwrap();
        for name in ["[Content_Types].xml", "word/document.xml"] {
            assert_eq!(
                compressed_entry_bytes(&original_archive, name),
                compressed_entry_bytes(&stamped_archive, name),
                "raw compressed bytes for {name} should remain identical"
            );
        }

        let mut archive = ZipArchive::new(File::open(path).unwrap()).unwrap();
        let mut core = String::new();
        archive
            .by_name("docProps/core.xml")
            .unwrap()
            .read_to_string(&mut core)
            .unwrap();
        assert!(core.contains("<dc:title>Preserved title</dc:title>"));
        assert_eq!(core.matches("2023-11-14T22:13:20Z").count(), 2);
        assert!(!core.contains("2000-01-01T00:00:00Z"));

        for (name, expected) in [
            ("[Content_Types].xml", "<Types/> "),
            ("word/document.xml", "<w:document>unchanged</w:document>"),
        ] {
            let mut content = String::new();
            archive
                .by_name(name)
                .unwrap()
                .read_to_string(&mut content)
                .unwrap();
            assert_eq!(content, expected);
        }
    }

    #[test]
    fn source_date_epoch_is_parsed_as_the_conversion_timestamp() {
        let timestamp = timestamp_from_source_date_epoch(Some("0")).unwrap();
        assert_eq!(timestamp.to_string(), "1970-01-01T00:00:00Z");
        assert!(matches!(
            timestamp_from_source_date_epoch(Some("invalid")),
            Err(DocxMetaError::InvalidSourceDateEpoch)
        ));
    }

    #[test]
    fn rejects_a_zip_entry_over_the_uncompressed_size_cap() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("oversized.docx");
        make_docx(&path, true);

        assert!(matches!(
            stamp(&path, Timestamp::UNIX_EPOCH),
            Err(DocxMetaError::EntryTooLarge)
        ));
    }
}
