//! Inspect command: format sniffing, document metrics, counts, and reachable targets.

use std::path::Path;

use ariad_core::{
    format::Format,
    ir::{Document, Metadata},
    limits::Limits,
    planner::{self, Profile},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::{
    convert::{ConvertError, ConvertWarning, read_to_ir_with_engine},
    engines::EngineInvocation,
    workspace::Workspace,
};

pub use crate::convert::{
    FileClassification, MAX_ZIP_ENTRIES, MAX_ZIP_ENTRY_NAME_BYTES, classify_file,
};
pub use ariad_core::inspect::{DocumentCounts, count_document};

/// Page and text-layer facts of an inspected PDF.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PdfSummary {
    /// Number of pages.
    pub pages: u32,
    /// Pages with a text layer.
    pub text_pages: u32,
    /// Pages without text that show at least one image, typically scans.
    pub image_only_pages: u32,
    /// Whether at least one page lacks a text layer, so reading it fully needs OCR.
    pub needs_ocr: bool,
    /// Whether the file is encrypted (and opened with the empty user password).
    pub encrypted: bool,
    /// The `/Producer` entry of the document information dictionary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub producer: Option<String>,
}

impl PdfSummary {
    /// Reads the statistics the PDF engine stores in the IR provenance under `pdf`.
    #[must_use]
    pub fn from_document(document: &Document) -> Option<Self> {
        #[derive(Deserialize)]
        struct Stats {
            pages: u32,
            text_pages: u32,
            image_only_pages: u32,
            #[serde(default)]
            encrypted: bool,
            #[serde(default)]
            producer: Option<String>,
        }
        let value = document.provenance.as_ref()?.0.get("pdf")?;
        let stats: Stats = serde_json::from_value(value.clone()).ok()?;
        Some(Self {
            pages: stats.pages,
            text_pages: stats.text_pages,
            image_only_pages: stats.image_only_pages,
            needs_ocr: stats.text_pages < stats.pages,
            encrypted: stats.encrypted,
            producer: stats
                .producer
                .map(|producer| crate::convert::sanitize_identifier(&producer, 256)),
        })
    }
}

/// Result of inspecting a document.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct InspectOutput {
    pub format: Format,
    pub bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_count: Option<u64>,
    pub meta: Option<Metadata>,
    pub counts: Option<DocumentCounts>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pdf: Option<PdfSummary>,
    pub warnings: Vec<ConvertWarning>,
    pub reachable: Vec<Format>,
}

/// Returns the user-facing format targets reachable from `from`.
#[must_use]
pub fn reachable_from(from: Format, caps: &planner::Capabilities) -> Vec<Format> {
    let registered = caps.registered_engines();
    let mut targets = planner::reachable_formats(caps, from, Profile::Editable, &registered);
    targets.retain(|&f| f.is_user_facing() && f != from);
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

    if limits
        .max_input_bytes
        .is_some_and(|max| classification.bytes > max)
    {
        return Err(ConvertError::LimitExceeded);
    }

    if format.is_zip_container() {
        crate::archive::preflight_archive(path, &limits)?;
    }

    // Formats without a structural reader in the host report their kind, size and routes only.
    if !matches!(
        format,
        Format::Markdown
            | Format::Html
            | Format::Docx
            | Format::Epub
            | Format::Pdf
            | Format::DoclingJson
    ) {
        return Ok(InspectOutput {
            format,
            bytes: classification.bytes,
            page_count: None,
            meta: None,
            counts: None,
            pdf: None,
            warnings,
            reachable,
        });
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
            pdf: None,
            warnings,
            reachable,
        });
    }

    let document = match format {
        Format::DoclingJson => {
            let read = crate::convert::read_docling_to_ir(path, &limits)?;
            warnings.extend(read.warnings);
            read.document
        }

        Format::Markdown => {
            let res = crate::convert::read_document_to_ir(path, Format::Markdown, &limits)?;
            warnings.extend(res.warnings);
            res.document
        }
        Format::Html => {
            let res = crate::convert::read_document_to_ir(path, Format::Html, &limits)?;
            warnings.extend(res.warnings);
            res.document
        }
        Format::Docx | Format::Epub | Format::Pdf => {
            let engine = if format == Format::Pdf {
                crate::engines::PDF.id
            } else {
                crate::engines::PANDOC.id
            };
            let default_exe = std::env::current_exe().map_err(|_| ConvertError::Failed)?;
            let program = engine_program.unwrap_or(&default_exe);
            let mut workspace = Workspace::new().map_err(|_| ConvertError::Failed)?;
            let res = read_to_ir_with_engine(
                path,
                format,
                engine,
                &mut workspace,
                &limits,
                &EngineInvocation::new(program),
                cancel.clone(),
            );
            let _ = workspace.close();
            let read = res?;
            warnings.extend(read.warnings);
            read.document
        }
        _ => {
            return Err(ConvertError::UnsupportedRoute {
                detail: Some(format!("format {} cannot be inspected", format.id())),
            });
        }
    };

    let counts = count_document(&document);
    let pdf = (format == Format::Pdf)
        .then(|| PdfSummary::from_document(&document))
        .flatten();
    let page_count = pdf.as_ref().map(|summary| u64::from(summary.pages));
    let meta = Some(document.meta);

    Ok(InspectOutput {
        format,
        bytes: classification.bytes,
        page_count,
        meta,
        counts: Some(counts),
        pdf,
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
