//! Inspect command: format sniffing, document metrics, counts, and reachable targets.

use std::path::Path;

use ariad_core::{
    format::Format,
    ir::Metadata,
    limits::Limits,
    planner::{self, Profile},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::{
    convert::{ConvertError, ConvertWarning, read_archive_to_ir},
    workspace::Workspace,
};

pub use crate::convert::{
    FileClassification, MAX_ZIP_ENTRIES, MAX_ZIP_ENTRY_NAME_BYTES, classify_file,
};
pub use ariad_core::inspect::{DocumentCounts, count_document};

/// Result of inspecting a document.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct InspectOutput {
    pub format: Format,
    pub bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_count: Option<u64>,
    pub meta: Option<Metadata>,
    pub counts: Option<DocumentCounts>,
    pub warnings: Vec<ConvertWarning>,
    pub reachable: Vec<Format>,
}

/// Returns the user-facing format targets reachable from `from`.
#[must_use]
pub fn reachable_from(from: Format, caps: &planner::Capabilities) -> Vec<Format> {
    let registered = caps.registered_engines();
    let mut targets = planner::reachable_formats(caps, from, Profile::Editable, &registered);
    targets.retain(|&f| f != Format::AriadIrJson && f != Format::PandocJson && f != from);
    targets.sort_by_key(|f| f.id());
    targets.dedup();
    targets
}

/// Executes inspection on an input document file.
pub fn inspect(
    path: &Path,
    engine_program: Option<&Path>,
    caps: &planner::Capabilities,
    cancel: CancellationToken,
) -> Result<InspectOutput, ConvertError> {
    if cancel.is_cancelled() {
        return Err(ConvertError::Interrupted);
    }

    let classification = classify_file(path).map_err(|_| ConvertError::InputIo)?;
    let format = classification
        .format
        .ok_or_else(|| ConvertError::UnsupportedRoute {
            detail: Some("unknown input format".to_owned()),
        })?;

    let limits = Limits::local();
    let mut warnings = Vec::new();
    if classification.extension_mismatch {
        warnings.push(ConvertWarning {
            code: "format_mismatch".to_owned(),
            message: "The input file extension does not match the detected format.".to_owned(),
        });
    }

    let reachable = reachable_from(format, caps);

    if format == Format::Pdf {
        let docling_ready = crate::engines::docling::availability(cancel.clone())
            .is_some_and(|docling| docling.available);
        if !docling_ready {
            warnings.push(ConvertWarning {
                code: "engine_missing".to_owned(),
                message: "requires the docling engine (roadmap 1b)".to_owned(),
            });
        }
        return Ok(InspectOutput {
            format,
            bytes: classification.bytes,
            page_count: None,
            meta: None,
            counts: None,
            warnings,
            reachable,
        });
    }

    if limits
        .max_input_bytes
        .is_some_and(|max| classification.bytes > max)
    {
        return Err(ConvertError::LimitExceeded);
    }

    if matches!(
        format,
        Format::Docx | Format::Epub | Format::Pptx | Format::Xlsx
    ) {
        crate::archive::preflight_archive(path, &limits)?;
    }

    if classification.bytes > crate::convert::ANALYSIS_MAX_BYTES {
        if format == Format::Markdown {
            crate::convert::validate_utf8_file_streaming(path)?;
        }
        warnings.push(ConvertWarning {
            code: "document_too_large".to_owned(),
            message: "document too large to analyse".to_owned(),
        });
        return Ok(InspectOutput {
            format,
            bytes: classification.bytes,
            page_count: None,
            meta: None,
            counts: None,
            warnings,
            reachable,
        });
    }

    let document = match format {
        Format::Markdown => {
            let res = crate::convert::read_document_to_ir(
                path,
                crate::convert::DocumentFormat::Markdown,
                &limits,
            )?;
            warnings.extend(res.warnings);
            res.document
        }
        Format::Html => {
            let res = crate::convert::read_document_to_ir(
                path,
                crate::convert::DocumentFormat::Html,
                &limits,
            )?;
            warnings.extend(res.warnings);
            res.document
        }
        Format::DoclingJson => {
            let res = crate::convert::read_docling_to_ir(
                path,
                None,
                &limits,
                cancel.clone(),
                &mut |_| {},
            )?;
            warnings.extend(res.warnings);
            res.document
        }
        Format::Docx | Format::Epub => {
            let format_str = if format == Format::Docx {
                "docx"
            } else {
                "epub"
            };
            let default_exe = std::env::current_exe().map_err(|_| ConvertError::Failed)?;
            let engine = engine_program.unwrap_or(&default_exe);
            let mut workspace = Workspace::new().map_err(|_| ConvertError::Failed)?;
            let res = read_archive_to_ir(
                path,
                format_str,
                &mut workspace,
                &limits,
                engine,
                cancel.clone(),
            );
            let _ = workspace.close();
            let archive_out = res?;
            warnings.extend(archive_out.warnings);
            archive_out.document
        }
        // Presentations, workbooks and images are only read through an engine; report what
        // the file is and where it can go without converting it.
        Format::Pptx | Format::Xlsx | Format::Png | Format::Jpeg | Format::Tiff | Format::Webp => {
            return Ok(InspectOutput {
                format,
                bytes: classification.bytes,
                page_count: None,
                meta: None,
                counts: None,
                warnings,
                reachable,
            });
        }
        _ => {
            return Err(ConvertError::UnsupportedRoute {
                detail: Some(format!("format {} cannot be inspected", format.id())),
            });
        }
    };

    let counts = count_document(&document);
    let meta = Some(document.meta);

    Ok(InspectOutput {
        format,
        bytes: classification.bytes,
        page_count: None,
        meta,
        counts: Some(counts),
        warnings,
        reachable,
    })
}

#[cfg(test)]
mod tests {
    use std::{fs::File, io::Write};

    use ariad_core::format::Format;
    use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

    use super::classify_file;

    #[test]
    fn classify_file_handles_truncated_zips() {
        let dir = tempfile::tempdir().unwrap();
        let temp_path = dir.path().join("truncated_zip.bin");
        // Starts with PK\x03\x04 but is truncated
        std::fs::write(&temp_path, b"PK\x03\x04truncated_zip_bytes").unwrap();

        // With no known extension, returns None format
        let res = classify_file(&temp_path).unwrap();
        assert_eq!(res.format, None);
        assert!(!res.extension_mismatch);

        // With .docx extension, falls back to extension without crashing
        let docx_path = dir.path().join("corrupt.docx");
        std::fs::write(&docx_path, b"PK\x03\x04truncated_zip_bytes").unwrap();
        let res = classify_file(&docx_path).unwrap();
        assert_eq!(res.format, Some(Format::Docx));
        assert!(!res.extension_mismatch);
    }

    #[test]
    fn classify_file_detects_bom_html_and_mismatched_extension() {
        let dir = tempfile::tempdir().unwrap();
        let misnamed = dir.path().join("report.txt");
        let content = b"\xEF\xBB\xBF<!DOCTYPE html><html><body><h1>Header</h1></body></html>";
        std::fs::write(&misnamed, content).unwrap();

        let res = classify_file(&misnamed).unwrap();
        assert_eq!(res.format, Some(Format::Html));
        assert!(res.extension_mismatch);
    }

    #[test]
    fn classify_file_detects_misnamed_pdf() {
        let dir = tempfile::tempdir().unwrap();
        let misnamed = dir.path().join("notes.docx");
        let content =
            b"%PDF-1.7\n1 0 obj\n<< /Type /Catalog >>\nendobj\ntrailer\n<< /Root 1 0 R >>\n%%EOF";
        std::fs::write(&misnamed, content).unwrap();

        let res = classify_file(&misnamed).unwrap();
        assert_eq!(res.format, Some(Format::Pdf));
        assert!(res.extension_mismatch);
    }

    #[test]
    fn classify_file_epub_mimetype_rule() {
        let dir = tempfile::tempdir().unwrap();
        let epub_path = dir.path().join("valid_epub.zip");

        // 1. Valid EPUB: mimetype is entry 0, stored uncompressed
        {
            let file = File::create(&epub_path).unwrap();
            let mut zip = ZipWriter::new(file);
            zip.start_file(
                "mimetype",
                SimpleFileOptions::default().compression_method(CompressionMethod::Stored),
            )
            .unwrap();
            zip.write_all(b"application/epub+zip").unwrap();

            zip.start_file(
                "META-INF/container.xml",
                SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
            )
            .unwrap();
            zip.write_all(b"<container/>").unwrap();
            zip.finish().unwrap();
        }

        let res = classify_file(&epub_path).unwrap();
        assert_eq!(res.format, Some(Format::Epub));
        assert!(res.extension_mismatch); // .zip vs epub

        // 2. Invalid EPUB: mimetype is NOT entry 0
        let not_epub_path = dir.path().join("not_epub.zip");
        {
            let file = File::create(&not_epub_path).unwrap();
            let mut zip = ZipWriter::new(file);
            zip.start_file(
                "readme.txt",
                SimpleFileOptions::default().compression_method(CompressionMethod::Stored),
            )
            .unwrap();
            zip.write_all(b"readme").unwrap();

            zip.start_file(
                "mimetype",
                SimpleFileOptions::default().compression_method(CompressionMethod::Stored),
            )
            .unwrap();
            zip.write_all(b"application/epub+zip").unwrap();
            zip.finish().unwrap();
        }

        let res = classify_file(&not_epub_path).unwrap();
        assert_ne!(res.format, Some(Format::Epub));
    }

    #[test]
    fn classify_file_docx_with_document_after_large_media() {
        let dir = tempfile::tempdir().unwrap();
        let docx_path = dir.path().join("large_media.bin");

        {
            let file = File::create(&docx_path).unwrap();
            let mut zip = ZipWriter::new(file);

            // Large media entry first: 2 MB
            zip.start_file(
                "word/media/image1.png",
                SimpleFileOptions::default().compression_method(CompressionMethod::Stored),
            )
            .unwrap();
            zip.write_all(&vec![0xAA; 2 * 1024 * 1024]).unwrap();

            // Document entries after the large media
            zip.start_file(
                "[Content_Types].xml",
                SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
            )
            .unwrap();
            zip.write_all(b"<Types/>").unwrap();

            zip.start_file(
                "word/document.xml",
                SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
            )
            .unwrap();
            zip.write_all(b"<w:document/>").unwrap();

            zip.finish().unwrap();
        }

        let res = classify_file(&docx_path).unwrap();
        assert_eq!(res.format, Some(Format::Docx));
        assert!(res.extension_mismatch);
    }
}
