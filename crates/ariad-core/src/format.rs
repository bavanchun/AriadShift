use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A format supported by the core registry.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub enum Format {
    #[serde(rename = "markdown")]
    Markdown,
    #[serde(rename = "html")]
    Html,
    #[serde(rename = "docx")]
    Docx,
    #[serde(rename = "epub")]
    Epub,
    #[serde(rename = "pdf")]
    Pdf,
    #[serde(rename = "png")]
    Png,
    #[serde(rename = "pptx")]
    Pptx,
    #[serde(rename = "xlsx")]
    Xlsx,
    #[serde(rename = "jpeg")]
    Jpeg,
    #[serde(rename = "tiff")]
    Tiff,
    #[serde(rename = "webp")]
    Webp,
    #[serde(rename = "ariad-ir+json")]
    AriadIrJson,
    #[serde(rename = "pandoc+json")]
    PandocJson,
    #[serde(rename = "docling+json")]
    DoclingJson,
}

impl Format {
    /// The stable registry identifier for this format.
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::Markdown => "markdown",
            Self::Html => "html",
            Self::Docx => "docx",
            Self::Epub => "epub",
            Self::Pdf => "pdf",
            Self::Png => "png",
            Self::Pptx => "pptx",
            Self::Xlsx => "xlsx",
            Self::Jpeg => "jpeg",
            Self::Tiff => "tiff",
            Self::Webp => "webp",
            Self::AriadIrJson => "ariad-ir+json",
            Self::PandocJson => "pandoc+json",
            Self::DoclingJson => "docling+json",
        }
    }

    /// Resolves a registry identifier such as `markdown` or `ariad-ir+json`.
    #[must_use]
    pub fn from_id(id: &str) -> Option<Self> {
        serde_json::from_value(serde_json::Value::String(id.to_owned())).ok()
    }

    /// File extensions associated with this format, without a leading dot.
    #[must_use]
    pub const fn extensions(self) -> &'static [&'static str] {
        match self {
            Self::Markdown => &["md", "markdown", "mdown"],
            Self::Html => &["html", "htm"],
            Self::Docx => &["docx"],
            Self::Epub => &["epub"],
            Self::Pdf => &["pdf"],
            Self::Png => &["png"],
            Self::Pptx => &["pptx"],
            Self::Xlsx => &["xlsx"],
            Self::Jpeg => &["jpg", "jpeg"],
            Self::Tiff => &["tif", "tiff"],
            Self::Webp => &["webp"],
            Self::AriadIrJson | Self::PandocJson | Self::DoclingJson => &["json"],
        }
    }

    /// The MIME type associated with this format.
    #[must_use]
    pub const fn media_type(self) -> &'static str {
        match self {
            Self::Markdown => "text/markdown",
            Self::Html => "text/html",
            Self::Docx => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            Self::Epub => "application/epub+zip",
            Self::Pdf => "application/pdf",
            Self::Png => "image/png",
            Self::Pptx => {
                "application/vnd.openxmlformats-officedocument.presentationml.presentation"
            }
            Self::Xlsx => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            Self::Jpeg => "image/jpeg",
            Self::Tiff => "image/tiff",
            Self::Webp => "image/webp",
            Self::AriadIrJson | Self::PandocJson | Self::DoclingJson => "application/json",
        }
    }

    /// Resolves a supported extension without regard to ASCII case.
    #[must_use]
    pub fn from_extension(extension: &str) -> Option<Self> {
        let extension = extension.trim_start_matches('.');
        if ["md", "markdown", "mdown"]
            .iter()
            .any(|candidate| extension.eq_ignore_ascii_case(candidate))
        {
            Some(Self::Markdown)
        } else if ["html", "htm"]
            .iter()
            .any(|candidate| extension.eq_ignore_ascii_case(candidate))
        {
            Some(Self::Html)
        } else if extension.eq_ignore_ascii_case("docx") {
            Some(Self::Docx)
        } else if extension.eq_ignore_ascii_case("epub") {
            Some(Self::Epub)
        } else if extension.eq_ignore_ascii_case("pdf") {
            Some(Self::Pdf)
        } else if extension.eq_ignore_ascii_case("png") {
            Some(Self::Png)
        } else if extension.eq_ignore_ascii_case("pptx") {
            Some(Self::Pptx)
        } else if extension.eq_ignore_ascii_case("xlsx") {
            Some(Self::Xlsx)
        } else if extension.eq_ignore_ascii_case("jpg") || extension.eq_ignore_ascii_case("jpeg") {
            Some(Self::Jpeg)
        } else if extension.eq_ignore_ascii_case("tif") || extension.eq_ignore_ascii_case("tiff") {
            Some(Self::Tiff)
        } else if extension.eq_ignore_ascii_case("webp") {
            Some(Self::Webp)
        } else if extension.eq_ignore_ascii_case("json") {
            Some(Self::AriadIrJson)
        } else {
            None
        }
    }
}

/// Classifies a document format from a byte prefix, optional ZIP central directory entries,
/// and an optional file extension. Pure-memory, I/O-free, and WASM-safe.
#[must_use]
pub fn classify(
    prefix: &[u8],
    zip_entries: Option<&[&str]>,
    extension: Option<&str>,
) -> Option<Format> {
    // 1. ZIP inspection (EPUB or DOCX)
    if let Some(entries) = zip_entries {
        // EPUB rule: a ZIP whose first entry is `mimetype` with stored content `application/epub+zip`.
        const EPUB_MIMETYPE_PAYLOAD: &[u8] = b"application/epub+zip";
        if entries.first() == Some(&"mimetype")
            && prefix.starts_with(b"PK\x03\x04")
            && prefix.len() >= 30
        {
            let fn_len = u16::from_le_bytes([prefix[26], prefix[27]]) as usize;
            let extra_len = u16::from_le_bytes([prefix[28], prefix[29]]) as usize;
            let data_offset = 30 + fn_len + extra_len;
            if prefix.len() >= 30 + fn_len
                && &prefix[30..30 + fn_len] == b"mimetype"
                && prefix.len() >= data_offset + EPUB_MIMETYPE_PAYLOAD.len()
                && &prefix[data_offset..data_offset + EPUB_MIMETYPE_PAYLOAD.len()]
                    == EPUB_MIMETYPE_PAYLOAD
            {
                return Some(Format::Epub);
            }
        }

        // DOCX rule: a ZIP with `[Content_Types].xml` and `word/document.xml`.
        let has_content_types = entries.contains(&"[Content_Types].xml");
        if has_content_types && entries.contains(&"word/document.xml") {
            return Some(Format::Docx);
        }
        if has_content_types && entries.contains(&"ppt/presentation.xml") {
            return Some(Format::Pptx);
        }
        if has_content_types && entries.contains(&"xl/workbook.xml") {
            return Some(Format::Xlsx);
        }
    }

    // 2. PDF rule: `%PDF-` is PDF.
    if prefix.starts_with(b"%PDF-") {
        return Some(Format::Pdf);
    }

    // 3. Raster image signatures.
    if let Some(format) = classify_image(prefix) {
        return Some(format);
    }

    // 4. DoclingDocument JSON names its schema in the first key.
    if is_docling_json(prefix) {
        return Some(Format::DoclingJson);
    }

    // 5. HTML rule: a leading `<!doctype html` or `<html`, ignoring case and after a BOM or whitespace.
    let mut rest = prefix;
    if rest.starts_with(b"\xEF\xBB\xBF") {
        rest = &rest[3..];
    }
    while let Some((first, remainder)) = rest.split_first() {
        if first.is_ascii_whitespace() {
            rest = remainder;
        } else {
            break;
        }
    }
    if is_leading_html(rest) {
        return Some(Format::Html);
    }

    // 6. Otherwise the extension decides.
    extension.and_then(Format::from_extension)
}

fn classify_image(prefix: &[u8]) -> Option<Format> {
    if prefix.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(Format::Png)
    } else if prefix.starts_with(b"\xff\xd8\xff") {
        Some(Format::Jpeg)
    } else if prefix.starts_with(b"II*\0") || prefix.starts_with(b"MM\0*") {
        Some(Format::Tiff)
    } else if prefix.len() >= 12 && &prefix[..4] == b"RIFF" && &prefix[8..12] == b"WEBP" {
        Some(Format::Webp)
    } else {
        None
    }
}

/// Docling writes `schema_name` as the first member of every DoclingDocument.
fn is_docling_json(prefix: &[u8]) -> bool {
    let mut rest = prefix.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(prefix);
    for token in [&b"{"[..], b"\"schema_name\"", b":", b"\"DoclingDocument\""] {
        rest = skip_json_whitespace(rest);
        match rest.strip_prefix(token) {
            Some(remainder) => rest = remainder,
            None => return false,
        }
    }
    true
}

fn skip_json_whitespace(mut bytes: &[u8]) -> &[u8] {
    while let Some((first, remainder)) = bytes.split_first() {
        if matches!(first, b' ' | b'\t' | b'\n' | b'\r') {
            bytes = remainder;
        } else {
            break;
        }
    }
    bytes
}

fn is_leading_html(bytes: &[u8]) -> bool {
    const DOCTYPE: &[u8] = b"<!doctype html";
    if bytes.len() >= DOCTYPE.len() && bytes[..DOCTYPE.len()].eq_ignore_ascii_case(DOCTYPE) {
        return true;
    }
    const HTML: &[u8] = b"<html";
    if bytes.len() >= HTML.len() && bytes[..HTML.len()].eq_ignore_ascii_case(HTML) {
        let next_byte = bytes.get(HTML.len());
        match next_byte {
            None => true,
            Some(&b) if b.is_ascii_whitespace() || b == b'>' || b == b'/' => true,
            _ => false,
        }
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::Format;

    #[test]
    fn extensions_are_case_insensitive_and_json_defaults_to_ir() {
        assert_eq!(Format::from_extension("MD"), Some(Format::Markdown));
        assert_eq!(Format::from_extension(".Markdown"), Some(Format::Markdown));
        assert_eq!(Format::from_extension("HTM"), Some(Format::Html));
        assert_eq!(Format::from_extension("DocX"), Some(Format::Docx));
        assert_eq!(Format::from_extension("EPUB"), Some(Format::Epub));
        assert_eq!(Format::from_extension(".epub"), Some(Format::Epub));
        assert_eq!(Format::from_extension("PDF"), Some(Format::Pdf));
        assert_eq!(Format::from_extension(".pdf"), Some(Format::Pdf));
        assert_eq!(Format::from_extension("PNG"), Some(Format::Png));
        assert_eq!(Format::from_extension(".png"), Some(Format::Png));
        assert_eq!(Format::from_extension("JSON"), Some(Format::AriadIrJson));
        assert_eq!(Format::from_extension("PPTX"), Some(Format::Pptx));
        assert_eq!(Format::from_extension("xlsx"), Some(Format::Xlsx));
        assert_eq!(Format::from_extension("JPG"), Some(Format::Jpeg));
        assert_eq!(Format::from_extension(".jpeg"), Some(Format::Jpeg));
        assert_eq!(Format::from_extension("tif"), Some(Format::Tiff));
        assert_eq!(Format::from_extension("TIFF"), Some(Format::Tiff));
        assert_eq!(Format::from_extension("webp"), Some(Format::Webp));
        assert_eq!(Format::from_extension("unknown"), None);
    }

    #[test]
    fn serde_names_equal_id_for_every_variant() {
        let variants = [
            Format::Markdown,
            Format::Html,
            Format::Docx,
            Format::Epub,
            Format::Pdf,
            Format::Png,
            Format::Pptx,
            Format::Xlsx,
            Format::Jpeg,
            Format::Tiff,
            Format::Webp,
            Format::AriadIrJson,
            Format::PandocJson,
            Format::DoclingJson,
        ];
        for variant in variants {
            let json = serde_json::to_string(&variant).expect("serialize format");
            let expected = format!("\"{}\"", variant.id());
            assert_eq!(json, expected, "serialized name for {:?}", variant);
            let round_trip: Format = serde_json::from_str(&json).expect("deserialize format");
            assert_eq!(round_trip, variant, "round trip for {:?}", variant);
            assert_eq!(Format::from_id(variant.id()), Some(variant));
        }
    }

    #[test]
    fn from_id_rejects_extensions_and_unknown_ids() {
        assert_eq!(Format::from_id("md"), None);
        assert_eq!(Format::from_id("Markdown"), None);
        assert_eq!(Format::from_id(""), None);
    }

    #[test]
    fn classify_identifies_pdf_from_magic_bytes() {
        use super::classify;
        assert_eq!(classify(b"%PDF-1.7\n...", None, None), Some(Format::Pdf));
        assert_eq!(classify(b"%PDF-2.0", None, Some("docx")), Some(Format::Pdf));
    }

    #[test]
    fn classify_identifies_html_with_bom_whitespace_and_case_variants() {
        use super::classify;
        // Standard doctype
        assert_eq!(
            classify(b"<!DOCTYPE html><html>", None, None),
            Some(Format::Html)
        );
        assert_eq!(
            classify(b"<!doctype html>", None, Some("md")),
            Some(Format::Html)
        );

        // UTF-8 BOM + whitespace
        let with_bom = b"\xEF\xBB\xBF  \t\r\n<!DOCTYPE HTML>";
        assert_eq!(classify(with_bom, None, None), Some(Format::Html));

        // Leading <html
        assert_eq!(
            classify(b"<html><head></head>", None, None),
            Some(Format::Html)
        );
        assert_eq!(
            classify(b"  \n<HTML lang=\"vi\">", None, None),
            Some(Format::Html)
        );
        assert_eq!(classify(b"<html/>", None, None), Some(Format::Html));

        // Non-HTML tag starting with html
        assert_eq!(classify(b"<html5>", None, None), None);
        assert_eq!(
            classify(b"<html5>", None, Some("md")),
            Some(Format::Markdown)
        );
    }

    #[test]
    fn classify_identifies_epub_strictly_by_first_entry_and_content() {
        use super::classify;
        let mut prefix = Vec::new();
        prefix.extend_from_slice(b"PK\x03\x04");
        prefix.extend_from_slice(&[0; 22]);
        prefix.extend_from_slice(&8u16.to_le_bytes()); // fn_len = 8 ("mimetype")
        prefix.extend_from_slice(&0u16.to_le_bytes()); // extra_len = 0
        prefix.extend_from_slice(b"mimetype");
        prefix.extend_from_slice(b"application/epub+zip");

        // Valid EPUB
        let entries = ["mimetype", "META-INF/container.xml", "EPUB/content.opf"];
        assert_eq!(classify(&prefix, Some(&entries), None), Some(Format::Epub));

        // First entry not mimetype
        let wrong_order = ["META-INF/container.xml", "mimetype"];
        assert_eq!(classify(&prefix, Some(&wrong_order), None), None);

        // Fake EPUB: first entry is mimetype but payload is text/plain, with epub payload later
        let mut fake_prefix = Vec::new();
        fake_prefix.extend_from_slice(b"PK\x03\x04");
        fake_prefix.extend_from_slice(&[0; 22]);
        fake_prefix.extend_from_slice(&8u16.to_le_bytes());
        fake_prefix.extend_from_slice(&0u16.to_le_bytes());
        fake_prefix.extend_from_slice(b"mimetype");
        fake_prefix.extend_from_slice(b"text/plain");
        fake_prefix.extend_from_slice(b"extra padding application/epub+zip in secondary data");
        assert_eq!(classify(&fake_prefix, Some(&entries), None), None);

        // Mimetype without payload in prefix
        let prefix_no_payload = b"PK\x03\x04other_stuff";
        assert_eq!(classify(prefix_no_payload, Some(&entries), None), None);
    }

    #[test]
    fn classify_identifies_docx_by_content_types_and_document() {
        use super::classify;
        let prefix = b"PK\x03\x04";
        let valid_entries = [
            "[Content_Types].xml",
            "_rels/.rels",
            "word/document.xml",
            "word/media/image1.png",
        ];
        assert_eq!(
            classify(prefix, Some(&valid_entries), None),
            Some(Format::Docx)
        );

        // Missing document.xml
        let missing_doc = ["[Content_Types].xml", "word/theme/theme1.xml"];
        assert_eq!(classify(prefix, Some(&missing_doc), None), None);
        assert_eq!(
            classify(prefix, Some(&missing_doc), Some("docx")),
            Some(Format::Docx)
        );
    }

    #[test]
    fn classify_identifies_presentations_and_workbooks_by_zip_parts() {
        use super::classify;
        let prefix = b"PK\x03\x04";
        let pptx = ["[Content_Types].xml", "ppt/presentation.xml"];
        assert_eq!(
            classify(prefix, Some(&pptx), Some("zip")),
            Some(Format::Pptx)
        );
        let xlsx = ["[Content_Types].xml", "xl/workbook.xml"];
        assert_eq!(classify(prefix, Some(&xlsx), None), Some(Format::Xlsx));
        let neither = ["[Content_Types].xml", "ppt/slides/slide1.xml"];
        assert_eq!(classify(prefix, Some(&neither), None), None);
    }

    #[test]
    fn classify_identifies_raster_images_by_signature() {
        use super::classify;
        assert_eq!(
            classify(b"\x89PNG\r\n\x1a\n\0\0", None, Some("jpg")),
            Some(Format::Png)
        );
        assert_eq!(
            classify(b"\xff\xd8\xff\xe0", None, None),
            Some(Format::Jpeg)
        );
        assert_eq!(classify(b"II*\0\x08\0", None, None), Some(Format::Tiff));
        assert_eq!(classify(b"MM\0*\0\0", None, None), Some(Format::Tiff));
        assert_eq!(
            classify(b"RIFF\x10\0\0\0WEBPVP8 ", None, None),
            Some(Format::Webp)
        );
        assert_eq!(classify(b"RIFF\x10\0\0\0WAVE", None, None), None);
    }

    #[test]
    fn classify_identifies_docling_json_by_its_first_member() {
        use super::classify;
        assert_eq!(
            classify(
                b"{\n  \"schema_name\": \"DoclingDocument\",\n  \"version\": \"1.10.0\"",
                None,
                Some("json")
            ),
            Some(Format::DoclingJson)
        );
        assert_eq!(
            classify(
                b"\xEF\xBB\xBF{\"schema_name\":\"DoclingDocument\"}",
                None,
                None
            ),
            Some(Format::DoclingJson)
        );
        assert_eq!(
            classify(b"{\"version\":\"ariad-ir/0\"}", None, Some("json")),
            Some(Format::AriadIrJson)
        );
        assert_eq!(
            classify(b"{\"schema_name\":\"Other\"}", None, Some("json")),
            Some(Format::AriadIrJson)
        );
    }

    #[test]
    fn classify_falls_back_to_extension_when_content_unmatched() {
        use super::classify;
        assert_eq!(
            classify(b"# Title\nProse text", None, Some("md")),
            Some(Format::Markdown)
        );
        assert_eq!(
            classify(b"# Title\nProse text", None, Some("markdown")),
            Some(Format::Markdown)
        );
        assert_eq!(
            classify(b"random bytes", None, Some("docx")),
            Some(Format::Docx)
        );
        assert_eq!(
            classify(b"random bytes", None, Some(".epub")),
            Some(Format::Epub)
        );
        assert_eq!(classify(b"random bytes", None, Some("unknown")), None);
        assert_eq!(classify(b"random bytes", None, None), None);
    }
}
