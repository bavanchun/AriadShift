use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A format supported by the core registry.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize, JsonSchema)]
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
    #[serde(rename = "ariad-ir+json")]
    AriadIrJson,
    #[serde(rename = "pandoc+json")]
    PandocJson,
    #[serde(rename = "docling+json")]
    DoclingJson,
    #[serde(rename = "pptx")]
    Pptx,
    #[serde(rename = "xlsx")]
    Xlsx,
    #[serde(rename = "odt")]
    Odt,
    #[serde(rename = "ods")]
    Ods,
    #[serde(rename = "odp")]
    Odp,
    #[serde(rename = "doc")]
    Doc,
    #[serde(rename = "xls")]
    Xls,
    #[serde(rename = "ppt")]
    Ppt,
    #[serde(rename = "rtf")]
    Rtf,
    #[serde(rename = "jpeg")]
    Jpeg,
    #[serde(rename = "webp")]
    Webp,
    #[serde(rename = "tiff")]
    Tiff,
}

/// The stored `mimetype` payloads that identify ZIP containers by their first entry.
const MIMETYPE_FORMATS: &[(&[u8], Format)] = &[
    (b"application/epub+zip", Format::Epub),
    (b"application/vnd.oasis.opendocument.text", Format::Odt),
    (
        b"application/vnd.oasis.opendocument.spreadsheet",
        Format::Ods,
    ),
    (
        b"application/vnd.oasis.opendocument.presentation",
        Format::Odp,
    ),
];

/// OOXML main parts that identify a ZIP holding `[Content_Types].xml`.
const OOXML_MAIN_PARTS: &[(&str, Format)] = &[
    ("word/document.xml", Format::Docx),
    ("xl/workbook.xml", Format::Xlsx),
    ("ppt/presentation.xml", Format::Pptx),
];

/// Streams that identify legacy Office documents inside an OLE2 compound file.
const OLE_STREAMS: &[(&str, Format)] = &[
    ("WordDocument", Format::Doc),
    ("Workbook", Format::Xls),
    ("Book", Format::Xls),
    ("PowerPoint Document", Format::Ppt),
];

/// The OLE2 compound file signature.
pub const OLE_MAGIC: &[u8] = b"\xD0\xCF\x11\xE0\xA1\xB1\x1A\xE1";

/// The ZIP local file header signature.
pub const ZIP_MAGIC: &[u8] = b"PK\x03\x04";

impl Format {
    /// Every registry format, in a stable order.
    pub const ALL: &'static [Self] = &[
        Self::Markdown,
        Self::Html,
        Self::Docx,
        Self::Epub,
        Self::Pdf,
        Self::Png,
        Self::AriadIrJson,
        Self::PandocJson,
        Self::DoclingJson,
        Self::Pptx,
        Self::Xlsx,
        Self::Odt,
        Self::Ods,
        Self::Odp,
        Self::Doc,
        Self::Xls,
        Self::Ppt,
        Self::Rtf,
        Self::Jpeg,
        Self::Webp,
        Self::Tiff,
    ];

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
            Self::AriadIrJson => "ariad-ir+json",
            Self::PandocJson => "pandoc+json",
            Self::DoclingJson => "docling+json",
            Self::Pptx => "pptx",
            Self::Xlsx => "xlsx",
            Self::Odt => "odt",
            Self::Ods => "ods",
            Self::Odp => "odp",
            Self::Doc => "doc",
            Self::Xls => "xls",
            Self::Ppt => "ppt",
            Self::Rtf => "rtf",
            Self::Jpeg => "jpeg",
            Self::Webp => "webp",
            Self::Tiff => "tiff",
        }
    }

    /// Resolves a registry identifier such as `markdown` or `ariad-ir+json`.
    #[must_use]
    pub fn from_id(id: &str) -> Option<Self> {
        serde_json::from_value(serde_json::Value::String(id.to_owned())).ok()
    }

    /// File extensions associated with this format, without a leading dot.
    ///
    /// The first extension is the default for output files.
    #[must_use]
    pub const fn extensions(self) -> &'static [&'static str] {
        match self {
            Self::Markdown => &["md", "markdown", "mdown"],
            Self::Html => &["html", "htm"],
            Self::Docx => &["docx"],
            Self::Epub => &["epub"],
            Self::Pdf => &["pdf"],
            Self::Png => &["png"],
            Self::AriadIrJson | Self::PandocJson | Self::DoclingJson => &["json"],
            Self::Pptx => &["pptx"],
            Self::Xlsx => &["xlsx"],
            Self::Odt => &["odt"],
            Self::Ods => &["ods"],
            Self::Odp => &["odp"],
            Self::Doc => &["doc"],
            Self::Xls => &["xls"],
            Self::Ppt => &["ppt"],
            Self::Rtf => &["rtf"],
            Self::Jpeg => &["jpg", "jpeg"],
            Self::Webp => &["webp"],
            Self::Tiff => &["tif", "tiff"],
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
            Self::AriadIrJson | Self::PandocJson | Self::DoclingJson => "application/json",
            Self::Pptx => {
                "application/vnd.openxmlformats-officedocument.presentationml.presentation"
            }
            Self::Xlsx => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            Self::Odt => "application/vnd.oasis.opendocument.text",
            Self::Ods => "application/vnd.oasis.opendocument.spreadsheet",
            Self::Odp => "application/vnd.oasis.opendocument.presentation",
            Self::Doc => "application/msword",
            Self::Xls => "application/vnd.ms-excel",
            Self::Ppt => "application/vnd.ms-powerpoint",
            Self::Rtf => "application/rtf",
            Self::Jpeg => "image/jpeg",
            Self::Webp => "image/webp",
            Self::Tiff => "image/tiff",
        }
    }

    /// The short name shown to users: the default extension, so `markdown` reads `md` and
    /// `ariad-ir+json` reads `json`.
    #[must_use]
    pub const fn user_name(self) -> &'static str {
        self.extensions()[0]
    }

    /// Whether files of this format are ZIP containers that need archive preflight before an
    /// engine opens them.
    #[must_use]
    pub const fn is_zip_container(self) -> bool {
        matches!(
            self,
            Self::Docx | Self::Epub | Self::Pptx | Self::Xlsx | Self::Odt | Self::Ods | Self::Odp
        )
    }

    /// Whether users may name this format on the command line or in tool calls.
    #[must_use]
    pub const fn is_user_facing(self) -> bool {
        !matches!(self, Self::PandocJson | Self::DoclingJson)
    }

    /// Resolves a supported extension without regard to ASCII case.
    ///
    /// `json` resolves to IR JSON.
    #[must_use]
    pub fn from_extension(extension: &str) -> Option<Self> {
        let extension = extension.trim_start_matches('.');
        Self::ALL
            .iter()
            .copied()
            .filter(|format| format.is_user_facing())
            .find(|format| {
                format
                    .extensions()
                    .iter()
                    .any(|candidate| extension.eq_ignore_ascii_case(candidate))
            })
    }

    /// Resolves a user-supplied format name: a user-facing registry id or an extension,
    /// without regard to ASCII case. `json` names IR JSON.
    #[must_use]
    pub fn from_user_name(name: &str) -> Option<Self> {
        let name = name.trim();
        Self::ALL
            .iter()
            .copied()
            .filter(|format| format.is_user_facing())
            .find(|format| format.id().eq_ignore_ascii_case(name))
            .or_else(|| Self::from_extension(name))
    }
}

/// Classifies a document format from a byte prefix, optional container entry names, and an
/// optional file extension. Pure-memory, I/O-free, and WASM-safe.
///
/// `container_entries` holds the ZIP central-directory names when the prefix starts with a ZIP
/// local header, or the directory entry names of an OLE2 compound file when the prefix starts
/// with the compound file signature.
#[must_use]
pub fn classify(
    prefix: &[u8],
    container_entries: Option<&[&str]>,
    extension: Option<&str>,
) -> Option<Format> {
    if let Some(entries) = container_entries {
        if prefix.starts_with(OLE_MAGIC) {
            if let Some(format) = classify_ole(entries) {
                return Some(format);
            }
        } else if let Some(format) = classify_zip(prefix, entries) {
            return Some(format);
        }
    }

    if let Some(format) = classify_magic(prefix) {
        return Some(format);
    }

    if is_docling_json(prefix) {
        return Some(Format::DoclingJson);
    }

    // HTML rule: a leading `<!doctype html` or `<html`, ignoring case and after a BOM or whitespace.
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

    // Otherwise the extension decides.
    extension.and_then(Format::from_extension)
}

fn classify_zip(prefix: &[u8], entries: &[&str]) -> Option<Format> {
    // EPUB and ODF rule: the first entry is `mimetype`, stored, with a known payload.
    if entries.first() == Some(&"mimetype")
        && let Some(payload) = stored_mimetype_payload(prefix)
        && let Some(&(_, format)) = MIMETYPE_FORMATS
            .iter()
            .find(|(expected, _)| payload == *expected)
    {
        return Some(format);
    }

    // OOXML rule: `[Content_Types].xml` plus the main part of a word, sheet or slide package.
    if entries.contains(&"[Content_Types].xml") {
        return OOXML_MAIN_PARTS
            .iter()
            .find(|(part, _)| entries.contains(part))
            .map(|&(_, format)| format);
    }
    None
}

/// Returns the bytes after the first local header when that entry is named `mimetype`, trimmed
/// to the longest known payload so a longer, unknown media type never matches a prefix.
fn stored_mimetype_payload(prefix: &[u8]) -> Option<&[u8]> {
    if !prefix.starts_with(ZIP_MAGIC) || prefix.len() < 30 {
        return None;
    }
    let method = u16::from_le_bytes([prefix[8], prefix[9]]);
    let stored_size = u32::from_le_bytes([prefix[18], prefix[19], prefix[20], prefix[21]]) as usize;
    let fn_len = u16::from_le_bytes([prefix[26], prefix[27]]) as usize;
    let extra_len = u16::from_le_bytes([prefix[28], prefix[29]]) as usize;
    let name = prefix.get(30..30 + fn_len)?;
    if name != b"mimetype" {
        return None;
    }
    let data_offset = 30 + fn_len + extra_len;
    let available = prefix.get(data_offset..)?;
    // A stored entry with a recorded size bounds the payload exactly. Writers that stream the
    // entry (size 0 with a data descriptor) leave the size unknown, so fall back to a prefix
    // match against the known payloads.
    if method == 0 && stored_size > 0 {
        return available.get(..stored_size).map(<[u8]>::trim_ascii_end);
    }
    MIMETYPE_FORMATS
        .iter()
        .map(|(expected, _)| *expected)
        .filter(|expected| available.starts_with(expected))
        .max_by_key(|expected| expected.len())
}

fn classify_ole(entries: &[&str]) -> Option<Format> {
    OLE_STREAMS
        .iter()
        .find(|(stream, _)| {
            entries
                .iter()
                .any(|entry| entry.eq_ignore_ascii_case(stream))
        })
        .map(|&(_, format)| format)
}

fn classify_magic(prefix: &[u8]) -> Option<Format> {
    if prefix.starts_with(b"%PDF-") {
        Some(Format::Pdf)
    } else if prefix.starts_with(b"{\\rtf") {
        Some(Format::Rtf)
    } else if prefix.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(Format::Png)
    } else if prefix.starts_with(b"\xFF\xD8\xFF") {
        Some(Format::Jpeg)
    } else if prefix.len() >= 12 && &prefix[..4] == b"RIFF" && &prefix[8..12] == b"WEBP" {
        Some(Format::Webp)
    } else if prefix.starts_with(b"II*\0")
        || prefix.starts_with(b"MM\0*")
        || prefix.starts_with(b"II+\0")
        || prefix.starts_with(b"MM\0+")
    {
        Some(Format::Tiff)
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
    use super::{Format, OLE_MAGIC, classify};

    fn stored_zip_prefix(name: &[u8], payload: &[u8], recorded_size: bool) -> Vec<u8> {
        let mut prefix = Vec::new();
        prefix.extend_from_slice(b"PK\x03\x04");
        prefix.extend_from_slice(&[20, 0, 0, 0]); // version, flags
        prefix.extend_from_slice(&0u16.to_le_bytes()); // method: stored
        prefix.extend_from_slice(&[0; 8]); // time, date, crc
        let size = if recorded_size {
            payload.len() as u32
        } else {
            0
        };
        prefix.extend_from_slice(&size.to_le_bytes()); // compressed size
        prefix.extend_from_slice(&size.to_le_bytes()); // uncompressed size
        prefix.extend_from_slice(&(name.len() as u16).to_le_bytes());
        prefix.extend_from_slice(&0u16.to_le_bytes());
        prefix.extend_from_slice(name);
        prefix.extend_from_slice(payload);
        prefix
    }

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
        assert_eq!(Format::from_extension("unknown"), None);
    }

    #[test]
    fn office_and_image_extensions_resolve() {
        let cases = [
            ("pptx", Format::Pptx),
            ("XLSX", Format::Xlsx),
            ("odt", Format::Odt),
            ("ods", Format::Ods),
            ("odp", Format::Odp),
            ("DOC", Format::Doc),
            ("xls", Format::Xls),
            ("ppt", Format::Ppt),
            ("rtf", Format::Rtf),
            ("jpg", Format::Jpeg),
            ("JPEG", Format::Jpeg),
            ("webp", Format::Webp),
            ("tif", Format::Tiff),
            ("tiff", Format::Tiff),
        ];
        for (extension, format) in cases {
            assert_eq!(
                Format::from_extension(extension),
                Some(format),
                "{extension}"
            );
        }
    }

    #[test]
    fn user_names_accept_ids_and_extensions_but_not_internal_formats() {
        assert_eq!(Format::from_user_name("json"), Some(Format::AriadIrJson));
        assert_eq!(
            Format::from_user_name("ariad-ir+json"),
            Some(Format::AriadIrJson)
        );
        assert_eq!(Format::from_user_name("md"), Some(Format::Markdown));
        assert_eq!(Format::from_user_name("Markdown"), Some(Format::Markdown));
        assert_eq!(Format::from_user_name("jpg"), Some(Format::Jpeg));
        assert_eq!(Format::from_user_name("tiff"), Some(Format::Tiff));
        assert_eq!(Format::from_user_name("pandoc+json"), None);
        assert_eq!(Format::from_user_name("docling+json"), None);
        assert_eq!(Format::from_user_name("exe"), None);
        assert_eq!(Format::AriadIrJson.user_name(), "json");
        assert_eq!(Format::Markdown.user_name(), "md");
        assert_eq!(Format::Jpeg.user_name(), "jpg");
    }

    #[test]
    fn serde_names_equal_id_for_every_variant() {
        for &variant in Format::ALL {
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
    fn all_lists_each_variant_once_with_a_unique_id() {
        let mut ids: Vec<&str> = Format::ALL.iter().map(|f| f.id()).collect();
        let count = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), count);
        assert_eq!(count, 21);
    }

    #[test]
    fn classify_identifies_pdf_from_magic_bytes() {
        assert_eq!(classify(b"%PDF-1.7\n...", None, None), Some(Format::Pdf));
        assert_eq!(classify(b"%PDF-2.0", None, Some("docx")), Some(Format::Pdf));
    }

    #[test]
    fn classify_identifies_rtf_and_images_from_magic_bytes() {
        assert_eq!(
            classify(b"{\\rtf1\\ansi hello}", None, Some("txt")),
            Some(Format::Rtf)
        );
        assert_eq!(
            classify(b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR", None, None),
            Some(Format::Png)
        );
        assert_eq!(
            classify(b"\xFF\xD8\xFF\xE0\0\x10JFIF", None, None),
            Some(Format::Jpeg)
        );
        assert_eq!(
            classify(b"RIFF\x24\0\0\0WEBPVP8 ", None, None),
            Some(Format::Webp)
        );
        assert_eq!(classify(b"II*\0\x08\0\0\0", None, None), Some(Format::Tiff));
        assert_eq!(classify(b"MM\0*\0\0\0\x08", None, None), Some(Format::Tiff));
        assert_eq!(classify(b"RIFF\x24\0\0\0WAVEfmt ", None, None), None);
    }

    #[test]
    fn classify_identifies_html_with_bom_whitespace_and_case_variants() {
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
        let prefix = stored_zip_prefix(b"mimetype", b"application/epub+zip", false);

        // Valid EPUB
        let entries = ["mimetype", "META-INF/container.xml", "EPUB/content.opf"];
        assert_eq!(classify(&prefix, Some(&entries), None), Some(Format::Epub));

        // First entry not mimetype
        let wrong_order = ["META-INF/container.xml", "mimetype"];
        assert_eq!(classify(&prefix, Some(&wrong_order), None), None);

        // Fake EPUB: first entry is mimetype but payload is text/plain, with epub payload later
        let fake_prefix = stored_zip_prefix(
            b"mimetype",
            b"text/plainextra padding application/epub+zip in secondary data",
            false,
        );
        assert_eq!(classify(&fake_prefix, Some(&entries), None), None);

        // Mimetype without payload in prefix
        let prefix_no_payload = b"PK\x03\x04other_stuff";
        assert_eq!(classify(prefix_no_payload, Some(&entries), None), None);
    }

    #[test]
    fn classify_identifies_odf_by_mimetype_payload() {
        let entries = ["mimetype", "content.xml", "META-INF/manifest.xml"];
        let cases: [(&[u8], Format); 3] = [
            (b"application/vnd.oasis.opendocument.text", Format::Odt),
            (
                b"application/vnd.oasis.opendocument.spreadsheet",
                Format::Ods,
            ),
            (
                b"application/vnd.oasis.opendocument.presentation",
                Format::Odp,
            ),
        ];
        for (payload, format) in cases {
            for recorded in [true, false] {
                let prefix = stored_zip_prefix(b"mimetype", payload, recorded);
                assert_eq!(
                    classify(&prefix, Some(&entries), Some("bin")),
                    Some(format),
                    "{format:?} recorded={recorded}"
                );
            }
        }

        // A template media type that merely starts with a known payload is not that format.
        let template = stored_zip_prefix(
            b"mimetype",
            b"application/vnd.oasis.opendocument.text-template",
            true,
        );
        assert_eq!(classify(&template, Some(&entries), None), None);
    }

    #[test]
    fn classify_identifies_docx_by_content_types_and_document() {
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
    fn classify_identifies_xlsx_and_pptx_by_main_part() {
        let prefix = b"PK\x03\x04";
        let workbook = ["[Content_Types].xml", "_rels/.rels", "xl/workbook.xml"];
        assert_eq!(
            classify(prefix, Some(&workbook), Some("zip")),
            Some(Format::Xlsx)
        );
        let deck = ["[Content_Types].xml", "ppt/presentation.xml"];
        assert_eq!(classify(prefix, Some(&deck), None), Some(Format::Pptx));
        let no_types = ["xl/workbook.xml"];
        assert_eq!(classify(prefix, Some(&no_types), None), None);
    }

    #[test]
    fn classify_identifies_legacy_office_by_ole_streams() {
        let mut prefix = OLE_MAGIC.to_vec();
        prefix.extend_from_slice(&[0; 32]);
        let word = ["Root Entry", "1Table", "WordDocument"];
        assert_eq!(
            classify(&prefix, Some(&word), Some("xls")),
            Some(Format::Doc)
        );
        let sheet = ["Root Entry", "Workbook"];
        assert_eq!(classify(&prefix, Some(&sheet), None), Some(Format::Xls));
        let old_sheet = ["Root Entry", "Book"];
        assert_eq!(classify(&prefix, Some(&old_sheet), None), Some(Format::Xls));
        let deck = ["Root Entry", "PowerPoint Document", "Current User"];
        assert_eq!(classify(&prefix, Some(&deck), None), Some(Format::Ppt));
        // An unknown compound file (for example an MSI) falls back to the extension.
        let other = ["Root Entry", "SummaryInformation"];
        assert_eq!(classify(&prefix, Some(&other), None), None);
        assert_eq!(
            classify(&prefix, Some(&other), Some("doc")),
            Some(Format::Doc)
        );
    }

    #[test]
    fn classify_falls_back_to_extension_when_content_unmatched() {
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
}
