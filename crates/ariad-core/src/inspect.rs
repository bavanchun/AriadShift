//! Document statistics and in-memory inspection shared by every surface.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    format::{self, Format},
    ir::{Block, Document, Inline, Metadata},
    limits::Limits,
    pipeline::{self, Notice, PipelineError},
    planner::{self, Capabilities, Profile},
};

/// Maximum input bytes allowed for document inspection and structural analysis (32 MiB).
pub const ANALYSIS_MAX_BYTES: u64 = 32 * 1024 * 1024;

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

/// Result of inspecting a document held in memory.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MemoryInspection {
    /// Detected format.
    pub format: Format,
    /// Input size in bytes.
    pub bytes: u64,
    /// Document metadata, when the document was read.
    pub meta: Option<Metadata>,
    /// Structural counts, when the document was read.
    pub counts: Option<DocumentCounts>,
    /// Non-fatal findings.
    pub warnings: Vec<Notice>,
    /// Targets reachable with the engines of the inspecting runtime, sorted by id.
    pub reachable: Vec<Format>,
    /// Targets reachable only with engines of other runtimes, sorted by id.
    pub reachable_elsewhere: Vec<Format>,
}

/// Returns the targets reachable from `from` through `caps`, excluding `from` itself
/// and engine-internal interchange formats.
#[must_use]
pub fn reachable_targets(caps: &Capabilities, from: Format) -> Vec<Format> {
    let engines = caps.registered_engines();
    let mut targets = planner::reachable_formats(caps, from, Profile::Editable, &engines);
    targets.retain(|&target| target != from && target != Format::PandocJson);
    targets.sort_by_key(|target| target.id());
    targets.dedup();
    targets
}

/// Inspects a document held in memory.
///
/// `runtime_caps` are the capabilities the caller can execute, `all_caps` every known
/// capability. `name` is the file name; only its extension is used. Inputs larger than
/// [`ANALYSIS_MAX_BYTES`] are classified but not read, and carry a `document_too_large`
/// notice; a caller may pass only a prefix of such a file together with `total_bytes`.
pub fn inspect_bytes(
    bytes: &[u8],
    name: &str,
    total_bytes: Option<u64>,
    runtime_caps: &Capabilities,
    all_caps: &Capabilities,
    limits: &Limits,
) -> Result<MemoryInspection, PipelineError> {
    let extension = name.rsplit_once('.').map(|(_, extension)| extension);
    let content_format = format::classify(bytes, None, None);
    let format = format::classify(bytes, None, extension)
        .ok_or(PipelineError::UnknownFormat { format: None })?;
    let size = total_bytes.unwrap_or(bytes.len() as u64);

    let mut warnings = Vec::new();
    if let (Some(content), Some(extension)) = (content_format, extension)
        && Format::from_extension(extension) != Some(content)
    {
        warnings.push(Notice::new(
            "format_mismatch",
            "The file extension does not match the detected format.",
        ));
    }

    let reachable = reachable_targets(runtime_caps, format);
    let mut reachable_elsewhere = reachable_targets(all_caps, format);
    reachable_elsewhere.retain(|target| !reachable.contains(target));

    let mut inspection = MemoryInspection {
        format,
        bytes: size,
        meta: None,
        counts: None,
        warnings,
        reachable,
        reachable_elsewhere,
    };

    if !pipeline::can_read(format) {
        inspection.warnings.push(Notice::new(
            "reader_unavailable",
            "This format cannot be read here; its structure was not analysed.",
        ));
        return Ok(inspection);
    }
    if size > ANALYSIS_MAX_BYTES || bytes.len() as u64 > ANALYSIS_MAX_BYTES {
        inspection.warnings.push(Notice::new(
            "document_too_large",
            "The document is too large to analyse.",
        ));
        return Ok(inspection);
    }

    let read = pipeline::read(format, bytes, limits)?;
    inspection.counts = Some(count_document(&read.document));
    inspection.meta = Some(read.document.meta);
    inspection.warnings.extend(read.warnings);
    Ok(inspection)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planner::{Runtime, embedded};

    fn browser_caps() -> Capabilities {
        embedded().restricted_to(Runtime::Wasm)
    }

    #[test]
    fn counts_headings_paragraphs_links_and_words() {
        let read = pipeline::read(
            Format::Markdown,
            b"# Title\n\nSome [link](https://example.test) words.\n\n## Part\n",
            &Limits::local(),
        )
        .unwrap();
        let counts = count_document(&read.document);
        assert_eq!(counts.headings.get(&1), Some(&1));
        assert_eq!(counts.headings.get(&2), Some(&1));
        assert_eq!(counts.paragraphs, 1);
        assert_eq!(counts.links, 1);
        assert_eq!(counts.words, 5);
    }

    #[test]
    fn inspects_markdown_with_browser_and_elsewhere_targets() {
        let inspection = inspect_bytes(
            b"# Notes\n\nHello.\n",
            "notes.md",
            None,
            &browser_caps(),
            embedded(),
            &Limits::local(),
        )
        .unwrap();
        assert_eq!(inspection.format, Format::Markdown);
        assert_eq!(inspection.bytes, 16);
        assert_eq!(
            inspection.reachable,
            vec![Format::AriadIrJson, Format::Html]
        );
        assert_eq!(
            inspection.reachable_elsewhere,
            vec![Format::Docx, Format::Epub]
        );
        assert!(inspection.counts.is_some());
        assert!(inspection.warnings.is_empty());
    }

    #[test]
    fn flags_extension_mismatch() {
        let inspection = inspect_bytes(
            b"<!doctype html><html><body><p>Hi</p></body></html>",
            "page.md",
            None,
            &browser_caps(),
            embedded(),
            &Limits::local(),
        )
        .unwrap();
        assert_eq!(inspection.format, Format::Html);
        assert_eq!(inspection.warnings[0].code, "format_mismatch");
    }

    #[test]
    fn classifies_unreadable_formats_without_reading_them() {
        let inspection = inspect_bytes(
            b"PK\x03\x04rest",
            "report.docx",
            None,
            &browser_caps(),
            embedded(),
            &Limits::local(),
        )
        .unwrap();
        assert_eq!(inspection.format, Format::Docx);
        assert!(inspection.counts.is_none());
        assert!(inspection.reachable.is_empty());
        assert!(inspection.reachable_elsewhere.contains(&Format::Markdown));
        assert_eq!(inspection.warnings[0].code, "reader_unavailable");
    }

    #[test]
    fn skips_analysis_above_the_cap() {
        let inspection = inspect_bytes(
            b"# Big",
            "big.md",
            Some(ANALYSIS_MAX_BYTES + 1),
            &browser_caps(),
            embedded(),
            &Limits::local(),
        )
        .unwrap();
        assert_eq!(inspection.bytes, ANALYSIS_MAX_BYTES + 1);
        assert!(inspection.counts.is_none());
        assert_eq!(inspection.warnings[0].code, "document_too_large");
    }

    #[test]
    fn rejects_unknown_formats() {
        let error = inspect_bytes(
            b"\x00\x01",
            "blob.bin",
            None,
            &browser_caps(),
            embedded(),
            &Limits::local(),
        )
        .unwrap_err();
        assert_eq!(error, PipelineError::UnknownFormat { format: None });
    }
}
