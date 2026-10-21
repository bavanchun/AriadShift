//! Inspect command: format sniffing, document metrics, counts, and reachable targets.

use std::{collections::BTreeMap, path::Path};

use ariad_core::{
    format::Format,
    ir::{Block, Document, Inline, Metadata},
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

/// Structural block, inline, and word counts for an inspected document.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DocumentCounts {
    pub headings: BTreeMap<u8, usize>,
    pub paragraphs: usize,
    pub tables: usize,
    pub images: usize,
    pub links: usize,
    pub footnotes: usize,
    pub words: usize,
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
    pub warnings: Vec<ConvertWarning>,
    pub reachable: Vec<Format>,
}

/// Recursively computes structural counts across a document's body and furniture.
#[must_use]
pub fn count_document(doc: &Document) -> DocumentCounts {
    let mut counts = DocumentCounts::default();
    count_blocks(&doc.body, &mut counts);
    count_blocks(&doc.furniture, &mut counts);
    counts
}

fn count_blocks(blocks: &[Block], counts: &mut DocumentCounts) {
    for block in blocks {
        match block {
            Block::Heading { level, content } => {
                *counts.headings.entry(*level).or_default() += 1;
                count_inlines(content, counts);
            }
            Block::Paragraph { content } => {
                counts.paragraphs += 1;
                count_inlines(content, counts);
            }
            Block::Code { text, .. } => {
                counts.words += text.split_whitespace().count();
            }
            Block::Math { .. } => {}
            Block::Quote { blocks } => {
                count_blocks(blocks, counts);
            }
            Block::PageBreak {} => {}
            Block::List { items, .. } => {
                for item in items {
                    count_blocks(&item.blocks, counts);
                }
            }
            Block::Table {
                caption,
                head,
                body,
                footnotes,
                ..
            } => {
                counts.tables += 1;
                if let Some(caption) = caption {
                    count_inlines(caption, counts);
                }
                for row in head.iter().chain(body.iter()) {
                    for cell in row {
                        count_blocks(&cell.blocks, counts);
                    }
                }
                count_inlines(footnotes, counts);
            }
            Block::Figure { caption, .. } => {
                counts.images += 1;
                count_inlines(caption, counts);
            }
            Block::Footnote { blocks, .. } => {
                counts.footnotes += 1;
                count_blocks(blocks, counts);
            }
            Block::Raw { .. } => {}
        }
    }
}

fn count_inlines(inlines: &[Inline], counts: &mut DocumentCounts) {
    for inline in inlines {
        match inline {
            Inline::Text { text } | Inline::Code { text } => {
                counts.words += text.split_whitespace().count();
            }
            Inline::Emph { content }
            | Inline::Strong { content }
            | Inline::Strikeout { content }
            | Inline::Superscript { content }
            | Inline::Subscript { content } => {
                count_inlines(content, counts);
            }
            Inline::Link { content, .. } => {
                counts.links += 1;
                count_inlines(content, counts);
            }
            Inline::Image { .. } => {
                counts.images += 1;
            }
            Inline::FootnoteRef { .. } => {}
            Inline::SoftBreak {}
            | Inline::LineBreak {}
            | Inline::Math { .. }
            | Inline::Raw { .. } => {}
        }
    }
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
        warnings.push(ConvertWarning {
            code: "engine_missing".to_owned(),
            message: "requires the docling engine (roadmap 1b)".to_owned(),
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

    if limits
        .max_input_bytes
        .is_some_and(|max| classification.bytes > max)
    {
        return Err(ConvertError::LimitExceeded);
    }

    if format == Format::Docx || format == Format::Epub {
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
