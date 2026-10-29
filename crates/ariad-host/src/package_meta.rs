#[cfg(test)]
use ariad_core::package_meta::MAX_ENTRY_BYTES;
pub use ariad_core::package_meta::PackageMetaError;
use jiff::Timestamp;
use std::{env, fs::File, path::Path};
use tempfile::NamedTempFile;

/// Stamps DOCX created/modified timestamps.
pub fn stamp_docx(path: &Path, timestamp: Timestamp) -> Result<(), PackageMetaError> {
    stamp(path, timestamp, false)
}
/// Stamps EPUB publication/modified timestamps.
pub fn stamp_epub(path: &Path, timestamp: Timestamp) -> Result<(), PackageMetaError> {
    stamp(path, timestamp, true)
}
fn stamp(path: &Path, timestamp: Timestamp, epub: bool) -> Result<(), PackageMetaError> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let temporary = NamedTempFile::new_in(parent)?;
    let text = if epub {
        timestamp.strftime("%Y-%m-%dT%H:%M:%SZ").to_string()
    } else {
        timestamp.to_string()
    };
    let output =
        ariad_core::package_meta::stamp_into(File::open(path)?, temporary.reopen()?, epub, &text)?;
    output.sync_all()?;
    drop(output);
    temporary
        .persist(path)
        .map_err(|e| PackageMetaError::Io(e.error))?;
    Ok(())
}

/// Selects the reproducible timestamp when SOURCE_DATE_EPOCH is set, otherwise the current time.
pub fn conversion_time() -> Result<Timestamp, PackageMetaError> {
    match env::var_os("SOURCE_DATE_EPOCH") {
        Some(value) => timestamp_from_source_date_epoch(Some(
            value
                .to_str()
                .ok_or(PackageMetaError::InvalidSourceDateEpoch)?,
        )),
        None => timestamp_from_source_date_epoch(None),
    }
}

fn timestamp_from_source_date_epoch(value: Option<&str>) -> Result<Timestamp, PackageMetaError> {
    match value {
        Some(value) => value
            .parse::<i64>()
            .ok()
            .and_then(|seconds| Timestamp::from_second(seconds).ok())
            .ok_or(PackageMetaError::InvalidSourceDateEpoch),
        None => Ok(Timestamp::now()),
    }
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

    use super::{
        MAX_ENTRY_BYTES, PackageMetaError, stamp_docx, stamp_epub, timestamp_from_source_date_epoch,
    };

    const CORE_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?><cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:dcterms="http://purl.org/dc/terms/" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"><dc:title>Preserved title</dc:title><dcterms:created xsi:type="dcterms:W3CDTF">2000-01-01T00:00:00Z</dcterms:created><dcterms:modified xsi:type="dcterms:W3CDTF">2000-01-01T00:00:00Z</dcterms:modified></cp:coreProperties>"#;

    const OPF_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<package version="3.0" xmlns="http://www.idpf.org/2007/opf" xml:lang="en" unique-identifier="epub-id-1">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:opf="http://www.idpf.org/2007/opf">
    <dc:identifier id="epub-id-1">urn:uuid:12345678-1234-5678-1234-567812345678</dc:identifier>
    <dc:title id="epub-title-1">Preserved EPUB Title</dc:title>
    <dc:date id="epub-date">1970-01-01T00:00:00Z</dc:date>
    <dc:language>en</dc:language>
    <meta property="dcterms:modified">1970-01-01T00:00:00Z</meta>
  </metadata>
</package>"#;

    fn make_docx(path: &std::path::Path, oversized_properties: bool) {
        let file = File::create(path).unwrap();
        let mut writer = ZipWriter::new(file);
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        writer.start_file("[Content_Types].xml", options).unwrap();
        writer.write_all(b"<Types/> ").unwrap();
        writer.start_file("word/document.xml", options).unwrap();
        writer
            .write_all(b"<w:document>unchanged</w:document>")
            .unwrap();
        writer.start_file("docProps/core.xml", options).unwrap();
        writer.write_all(CORE_XML.as_bytes()).unwrap();
        if oversized_properties {
            let chunk = [b' '; 1024 * 1024];
            for _ in 0..(MAX_ENTRY_BYTES / chunk.len() as u64 + 1) {
                writer.write_all(&chunk).unwrap();
            }
        }
        writer.finish().unwrap();
    }

    fn make_epub(path: &std::path::Path, oversized_properties: bool) {
        let file = File::create(path).unwrap();
        let mut writer = ZipWriter::new(file);
        let stored_options =
            SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        let deflated_options =
            SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

        writer.start_file("mimetype", stored_options).unwrap();
        writer.write_all(b"application/epub+zip").unwrap();

        writer
            .start_file("META-INF/container.xml", deflated_options)
            .unwrap();
        writer
            .write_all(b"<container><rootfiles><rootfile full-path=\"EPUB/content.opf\" media-type=\"application/oebps-package+xml\"/></rootfiles></container>")
            .unwrap();

        writer
            .start_file("EPUB/content.opf", deflated_options)
            .unwrap();
        writer.write_all(OPF_XML.as_bytes()).unwrap();
        if oversized_properties {
            let chunk = [b' '; 1024 * 1024];
            for _ in 0..(MAX_ENTRY_BYTES / chunk.len() as u64 + 1) {
                writer.write_all(&chunk).unwrap();
            }
        }
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

        stamp_docx(&path, timestamp).unwrap();

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
    fn changes_only_epub_opf_dates_and_preserves_other_entries() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("book.epub");
        make_epub(&path, false);
        let original_archive = fs::read(&path).unwrap();
        let timestamp = Timestamp::from_second(1_700_000_000).unwrap();

        stamp_epub(&path, timestamp).unwrap();

        let stamped_archive = fs::read(&path).unwrap();
        for name in ["mimetype", "META-INF/container.xml"] {
            assert_eq!(
                compressed_entry_bytes(&original_archive, name),
                compressed_entry_bytes(&stamped_archive, name),
                "raw compressed bytes for {name} should remain identical"
            );
        }

        let mut archive = ZipArchive::new(File::open(path).unwrap()).unwrap();
        {
            let first_entry = archive.by_index(0).unwrap();
            assert_eq!(first_entry.name().unwrap(), "mimetype");
            assert_eq!(first_entry.compression(), CompressionMethod::Stored);
        }

        let mut opf = String::new();
        archive
            .by_name("EPUB/content.opf")
            .unwrap()
            .read_to_string(&mut opf)
            .unwrap();
        assert!(opf.contains("<dc:title id=\"epub-title-1\">Preserved EPUB Title</dc:title>"));
        assert!(opf.contains("<dc:date id=\"epub-date\">2023-11-14T22:13:20Z</dc:date>"));
        assert!(opf.contains("<meta property=\"dcterms:modified\">2023-11-14T22:13:20Z</meta>"));
        assert!(!opf.contains("1970-01-01T00:00:00Z"));
    }

    #[test]
    fn source_date_epoch_is_parsed_as_the_conversion_timestamp() {
        let timestamp = timestamp_from_source_date_epoch(Some("0")).unwrap();
        assert_eq!(timestamp.to_string(), "1970-01-01T00:00:00Z");
        assert!(matches!(
            timestamp_from_source_date_epoch(Some("invalid")),
            Err(PackageMetaError::InvalidSourceDateEpoch)
        ));
    }

    #[test]
    fn rejects_package_properties_over_the_uncompressed_size_cap() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("oversized.docx");
        make_docx(&path, true);

        assert!(matches!(
            stamp_docx(&path, Timestamp::UNIX_EPOCH),
            Err(PackageMetaError::EntryTooLarge)
        ));

        let epub_path = directory.path().join("oversized.epub");
        make_epub(&epub_path, true);
        assert!(matches!(
            stamp_epub(&epub_path, Timestamp::UNIX_EPOCH),
            Err(PackageMetaError::EntryTooLarge)
        ));
    }
}
