//! Normative output artifact naming helpers (`docs/tools.md` §6).
//!
//! Single-artifact tools emit `<stem>.<suffix>.<ext>` (for example `report.rotated.pdf`) and
//! multi-artifact tools such as `pdf.split` emit `<stem>.part-NNN.pdf` (1-based index, zero-padded
//! to at least 3 digits and widening to 4+ digits when `total_parts >= 1,000`).

use lopdf::{Object, StringFormat};

use crate::document::PdfDoc;

/// Internal trailer key used to carry the sanitized input file stem on an opened [`PdfDoc`]
/// during in-process tool execution (stripped automatically before saving).
pub(crate) const INPUT_STEM_TRAILER_KEY: &[u8] = b"AriadInputStem";

/// Returns the normative default filename suffix for `tool_id` (`docs/tools.md` §6).
#[must_use]
pub fn tool_suffix(tool_id: &str) -> &'static str {
    match tool_id {
        "pdf.info" => "info",
        "pdf.merge" => "merged",
        "pdf.split" => "part",
        "pdf.extract_pages" => "extracted",
        "pdf.delete_pages" => "trimmed",
        "pdf.reorder" => "reordered",
        "pdf.alternate_mix" => "mixed",
        "pdf.insert_blank" => "inserted",
        "pdf.rotate" => "rotated",
        "pdf.crop" => "cropped",
        "pdf.scale" => "scaled",
        "pdf.single_page" => "single-page",
        "pdf.rotate_content" => "rotated-content",
        "pdf.decompress" => "decompressed",
        "pdf.nup" => "nup",
        "pdf.booklet" => "booklet",
        "pdf.overlay" => "overlay",
        "pdf.divide_pages" => "divided",
        "pdf.posterize" => "posterized",
        "pdf.metadata" => "metadata",
        "pdf.outline" => "outline",
        "pdf.attachments" => "attachments",
        "pdf.page_labels" => "labeled",
        "pdf.viewer_prefs" => "prefs",
        "pdf.portfolio" => "portfolio",
        "pdf.layers" => "layers",
        _ => "output",
    }
}

/// Extracts and sanitizes a filename stem from a path or filename string.
#[must_use]
pub fn sanitize_stem(input_name_or_stem: &str) -> String {
    let base = input_name_or_stem
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(input_name_or_stem);
    let raw_stem = match base.rsplit_once('.') {
        Some((stem, ext))
            if !stem.is_empty()
                && matches!(
                    ext.to_ascii_lowercase().as_str(),
                    "pdf"
                        | "json"
                        | "txt"
                        | "md"
                        | "html"
                        | "docx"
                        | "epub"
                        | "png"
                        | "jpg"
                        | "jpeg"
                        | "tiff"
                        | "webp"
                        | "bin"
                ) =>
        {
            stem
        }
        _ => base,
    };
    let filtered: String = raw_stem
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | ' ' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let trimmed = filtered.trim_matches('.');
    if trimmed.is_empty() || trimmed.contains("..") {
        "document".to_owned()
    } else {
        trimmed.to_owned()
    }
}

/// Formats a single-artifact output filename `<stem>.<suffix>.<ext>` (`docs/tools.md` §6).
#[must_use]
pub fn single_output_name(stem: &str, suffix: &str, ext: &str) -> String {
    let clean_stem = sanitize_stem(stem);
    let clean_suffix = suffix.trim_matches('.');
    let clean_ext = ext.trim_start_matches('.');
    format!("{clean_stem}.{clean_suffix}.{clean_ext}")
}

/// Formats asingle-artifact PDF filename `<stem>.<tool-suffix>.pdf` for `tool_id`.
#[must_use]
pub fn default_pdf_output_name(stem: &str, tool_id: &str) -> String {
    single_output_name(stem, tool_suffix(tool_id), "pdf")
}

/// Formats a multi-part PDF output filename `<stem>.part-NNN.pdf` (`docs/tools.md` §6).
///
/// Uses 1-based indexing zero-padded to at least 3 digits (`001`..`999`), widening to 4+ digits
/// when `total_parts >= 1,000`.
#[must_use]
pub fn part_output_name(stem: &str, index_1_based: usize, total_parts: usize) -> String {
    let clean_stem = sanitize_stem(stem);
    let index = index_1_based.max(1);
    let max_idx = total_parts.max(index);
    let width = max_idx.to_string().len().max(3);
    format!("{clean_stem}.part-{index:0width$}.pdf")
}

/// Attaches the sanitized input stem to an opened mutable [`PdfDoc`] so multi-input tools such as
/// `pdf.merge` can title per-file outline entries with the input file stem.
pub fn set_doc_stem(doc: &mut PdfDoc, stem: &str) {
    let clean = sanitize_stem(stem);
    if let Ok(inner) = doc.inner_mut() {
        inner.trailer.set(
            INPUT_STEM_TRAILER_KEY.to_vec(),
            Object::String(clean.into_bytes(), StringFormat::Literal),
        );
    }
}

/// Reads the input file stem previously attached to `doc`, if present.
#[must_use]
pub fn doc_stem(doc: &PdfDoc) -> Option<String> {
    doc.inner()
        .trailer
        .get(INPUT_STEM_TRAILER_KEY)
        .ok()
        .and_then(|obj| obj.as_str().ok())
        .and_then(|bytes| std::str::from_utf8(bytes).ok())
        .map(ToOwned::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_and_part_output_names_follow_normative_suffixes() {
        assert_eq!(
            default_pdf_output_name("report.pdf", "pdf.rotate"),
            "report.rotated.pdf"
        );
        assert_eq!(
            default_pdf_output_name("first.pdf", "pdf.merge"),
            "first.merged.pdf"
        );
        assert_eq!(
            default_pdf_output_name("report", "pdf.extract_pages"),
            "report.extracted.pdf"
        );
        assert_eq!(
            default_pdf_output_name("report", "pdf.delete_pages"),
            "report.trimmed.pdf"
        );
        assert_eq!(
            default_pdf_output_name("report", "pdf.single_page"),
            "report.single-page.pdf"
        );
        assert_eq!(
            default_pdf_output_name("report", "pdf.decompress"),
            "report.decompressed.pdf"
        );
        assert_eq!(
            single_output_name("report.pdf", "outline", "json"),
            "report.outline.json"
        );

        assert_eq!(part_output_name("book.pdf", 1, 5), "book.part-001.pdf");
        assert_eq!(part_output_name("book.pdf", 12, 999), "book.part-012.pdf");
        assert_eq!(part_output_name("book.pdf", 1, 1000), "book.part-0001.pdf");
        assert_eq!(
            part_output_name("book.pdf", 1000, 1000),
            "book.part-1000.pdf"
        );
    }
}
