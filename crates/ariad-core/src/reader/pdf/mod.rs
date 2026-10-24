//! PDF text-layer reader: turns the text of a digital PDF into document IR.
//!
//! The reader is I/O-free and WASM-safe. It parses the file with `lopdf`, interprets page
//! content streams itself, and rebuilds reading order, paragraphs, headings, lists and running
//! headers and footers from glyph positions. Pages without a text layer produce an
//! `ocr_required` warning.
//!
//! Work is bounded: the page count by [`Limits::max_pages`], the object count by
//! [`MAX_OBJECTS`], decoded stream bytes by [`Limits::max_decompressed_bytes`] (every use of a
//! shared stream is charged again), each page by [`MAX_PAGE_OPERATORS`] operators and
//! [`MAX_PAGE_GLYPHS`] glyphs, and form XObject nesting by [`MAX_FORM_DEPTH`]. An amplification
//! cap of [`CONTENT_BUDGET_BASE`] plus [`CONTENT_BUDGET_PER_INPUT_BYTE`] per input byte bounds
//! decoded bytes plus 16 bytes and the decoded text of every glyph, so a small file cannot
//! expand into unbounded work or output. Exceeding either limit is
//! [`ReadError::DecompressedTooLarge`].

mod assemble;
mod cmap;
mod encoding;
mod font;
mod interpreter;
mod layout;
mod lexer;
mod lists;
mod objects;
mod symbolic;

use std::collections::{BTreeMap, HashSet};

use lopdf::{DecompressError, Dictionary, Document, LoadOptions, Object};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub use interpreter::{MAX_FORM_DEPTH, MAX_PAGE_GLYPHS, MAX_PAGE_OPERATORS};

use self::{
    assemble::{PageBlocks, assemble},
    interpreter::{DocState, Matrix, interpret_page},
    layout::layout_page,
    objects::{Budget, get, get_dict, number, rectangle, resolve},
};
use crate::{
    format::Format,
    ir::{Document as IrDocument, LayoutIndex, PageGeometry, Provenance},
    limits::{Limits, LimitsError},
    reader::{ReadOutput, common::nfc},
    warning::{Warning, WarningCode},
};

/// Key of the reader's statistics inside [`IrDocument::provenance`].
pub const PROVENANCE_KEY: &str = "pdf";

/// Most indirect objects a document may declare.
pub const MAX_OBJECTS: usize = 2_000_000;

/// Amplification budget, in bytes of decoded streams and glyph work, every document may use
/// regardless of its size.
pub const CONTENT_BUDGET_BASE: u64 = 32 * 1024 * 1024;

/// Additional amplification budget per input byte.
pub const CONTENT_BUDGET_PER_INPUT_BYTE: u64 = 64;

/// Deepest `/Parent` chain followed for inherited page attributes.
const MAX_PAGE_TREE_DEPTH: usize = 64;

/// Page size used when a page has no usable `/MediaBox` (US Letter).
const DEFAULT_PAGE: [f64; 4] = [0.0, 0.0, 612.0, 792.0];

/// Largest page side accepted, in points (the PDF limit is 14,400 user units).
const MAX_PAGE_SIDE: f64 = 1_000_000.0;

/// Page and text-layer statistics recorded by the reader.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PdfStats {
    /// Number of pages in the document.
    pub pages: u32,
    /// Pages with at least one decoded glyph.
    pub text_pages: u32,
    /// Pages without text that paint at least one image.
    pub image_only_pages: u32,
    /// Whether the file was encrypted (and opened with the empty user password).
    pub encrypted: bool,
    /// The `/Producer` entry of the document information dictionary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub producer: Option<String>,
}

impl PdfStats {
    /// Whether at least one page lacks a text layer, so reading it fully needs OCR.
    #[must_use]
    pub const fn needs_ocr(&self) -> bool {
        self.text_pages < self.pages
    }

    /// Reads the statistics the reader stored in a document's provenance.
    #[must_use]
    pub fn from_document(document: &IrDocument) -> Option<Self> {
        let value = document.provenance.as_ref()?.0.get(PROVENANCE_KEY)?;
        serde_json::from_value(value.clone()).ok()
    }
}

/// Typed failures of the PDF reader. Messages never include document content.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum ReadError {
    #[error("the file is not a readable PDF")]
    Malformed,
    #[error("the PDF is encrypted and does not open with an empty password")]
    Encrypted,
    #[error("input of {actual} bytes exceeds the limit of {limit} bytes")]
    InputTooLarge { actual: u64, limit: u64 },
    #[error("the PDF has {actual} pages, more than the limit of {limit}")]
    TooManyPages { actual: u64, limit: u32 },
    #[error("the PDF declares more than {limit} objects")]
    TooManyObjects { limit: usize },
    #[error("decompressed PDF streams exceed the limit of {limit} bytes")]
    DecompressedTooLarge { limit: u64 },
    #[error("nesting exceeds the configured depth limit of {limit}")]
    NestingTooDeep { limit: u16 },
    #[error("document exceeds the configured block limit of {limit}")]
    TooManyBlocks { limit: u32 },
    #[error(transparent)]
    InvalidLimits(#[from] LimitsError),
}

/// Reads a PDF into document IR.
///
/// The returned document carries `meta.source_format = pdf`, metadata from the information
/// dictionary, `furniture` for running headers and footers, a [`crate::ir::LayoutIndex`] with
/// page geometry and block boxes, and [`PdfStats`] under [`PROVENANCE_KEY`].
pub fn read(bytes: &[u8], limits: &Limits) -> Result<ReadOutput, ReadError> {
    limits.validate()?;
    if let Some(limit) = limits.max_input_bytes
        && bytes.len() as u64 > limit
    {
        return Err(ReadError::InputTooLarge {
            actual: bytes.len() as u64,
            limit,
        });
    }
    let amplification = CONTENT_BUDGET_BASE
        .saturating_add(CONTENT_BUDGET_PER_INPUT_BYTE.saturating_mul(bytes.len() as u64))
        .min((usize::MAX / 4) as u64);
    let (doc, budget) = load(bytes, limits, amplification)?;
    if doc.objects.len() > MAX_OBJECTS || doc.reference_table.entries.len() > MAX_OBJECTS {
        return Err(ReadError::TooManyObjects { limit: MAX_OBJECTS });
    }

    let mut seen = HashSet::new();
    let mut page_ids = Vec::new();
    // The upstream iterator's size hint sums untrusted /Count values.
    for id in doc.page_iter() {
        if seen.insert(id) {
            page_ids.push(id);
        }
    }
    if page_ids.is_empty() {
        return Err(ReadError::Malformed);
    }
    if let Some(limit) = limits.max_pages
        && page_ids.len() as u64 > u64::from(limit)
    {
        return Err(ReadError::TooManyPages {
            actual: page_ids.len() as u64,
            limit,
        });
    }

    let mut state = DocState::new(&doc, budget);
    let mut stats = PdfStats {
        pages: u32::try_from(page_ids.len()).unwrap_or(u32::MAX),
        encrypted: doc.encryption_state.is_some(),
        ..PdfStats::default()
    };
    let mut geometry = Vec::with_capacity(page_ids.len());
    let mut pages = Vec::with_capacity(page_ids.len());
    let mut unmapped = 0_u64;
    let mut truncated = 0_u32;
    let mut block_total = 0_usize;
    for (index, &page_id) in page_ids.iter().enumerate() {
        let page_no = u32::try_from(index + 1).unwrap_or(u32::MAX);
        let page = doc.get_dictionary(page_id).ok();
        let (display, width, height) = page_geometry(&doc, page);
        geometry.push(PageGeometry {
            page_no,
            width,
            height,
        });
        let resources = page.and_then(|page| {
            inherited(&doc, page, b"Resources").and_then(|object| object.as_dict().ok())
        });
        let contents = if page.is_some() {
            doc.get_page_contents(page_id)
        } else {
            Vec::new()
        };
        let content = interpret_page(&mut state, &contents, resources, display)?;
        let has_text = content
            .glyphs
            .iter()
            .any(|glyph| glyph.text_end > glyph.text_start);
        if has_text {
            stats.text_pages += 1;
        } else if content.images > 0 {
            stats.image_only_pages += 1;
        }
        unmapped += content.unmapped;
        truncated += u32::from(content.truncated);
        let blocks = layout_page(&content, &state.styles, width, height);
        block_total += blocks.len();
        if block_total > limits.max_blocks as usize {
            return Err(ReadError::TooManyBlocks {
                limit: limits.max_blocks,
            });
        }
        pages.push(PageBlocks {
            page_no,
            height,
            blocks,
        });
    }

    let assembled = assemble(&pages, limits)?;
    let mut document = IrDocument {
        body: assembled.body,
        furniture: assembled.furniture,
        ..IrDocument::default()
    };
    if document.block_count() > limits.max_blocks as usize {
        return Err(ReadError::TooManyBlocks {
            limit: limits.max_blocks,
        });
    }
    fill_metadata(&doc, &mut document, &mut stats);
    document.layout = Some(LayoutIndex {
        pages: geometry,
        blocks: assembled.positions,
    });
    let stats_value = serde_json::to_value(&stats).map_err(|_| ReadError::Malformed)?;
    document.provenance = Some(Provenance(BTreeMap::from([(
        PROVENANCE_KEY.to_owned(),
        stats_value,
    )])));

    let mut warnings = Vec::new();
    if stats.needs_ocr() {
        warnings.push(Warning::new(
            WarningCode::OcrRequired,
            format!(
                "{} of {} pages have no text layer; reading them needs OCR",
                stats.pages - stats.text_pages,
                stats.pages
            ),
        ));
    }
    if unmapped > 0 {
        warnings.push(Warning::new(
            WarningCode::TextUnmapped,
            format!("{unmapped} glyphs have no Unicode mapping and were left out"),
        ));
    }
    if truncated > 0 {
        warnings.push(Warning::new(
            WarningCode::UnsupportedNode,
            format!(
                "{truncated} pages exceed {MAX_PAGE_OPERATORS} content operators or \
                 {MAX_PAGE_GLYPHS} glyphs; the rest of their content was skipped"
            ),
        ));
    }
    Ok(ReadOutput { document, warnings })
}

/// Parses the file, opening encrypted documents with the empty user password.
fn load(
    bytes: &[u8],
    limits: &Limits,
    amplification: u64,
) -> Result<(Document, Budget), ReadError> {
    let load_limit = limits.max_decompressed_bytes.min(amplification);
    let options = LoadOptions {
        // Strict mode propagates object-stream limit errors instead of silently skipping them.
        strict: true,
        max_decompressed_size: Some(load_limit as usize),
        ..LoadOptions::default()
    };
    let doc = Document::load_mem_with_options(bytes, options).map_err(|error| match error {
        lopdf::Error::Decompress(DecompressError::MemoryLimitExceeded { .. }) => {
            ReadError::DecompressedTooLarge { limit: load_limit }
        }
        lopdf::Error::Decryption(_) | lopdf::Error::InvalidPassword => ReadError::Encrypted,
        _ => ReadError::Malformed,
    })?;
    // `lopdf` leaves `/Encrypt` in the trailer when no password opened the file.
    if doc.trailer.get(b"Encrypt").is_ok() {
        return Err(ReadError::Encrypted);
    }
    // lopdf's loader cap is per stream. Charge retained object/xref streams against the
    // document total before interpreting pages; encrypted object streams also pass here.
    let mut budget = Budget::new(limits.max_decompressed_bytes, amplification);
    for object in doc.objects.values() {
        if let Object::Stream(stream) = object
            && (stream.dict.has_type(b"ObjStm") || stream.dict.has_type(b"XRef"))
        {
            budget.decode(stream)?;
        }
    }
    if doc.catalog().is_err() {
        return Err(ReadError::Malformed);
    }
    Ok((doc, budget))
}

/// An attribute of a page or, failing that, of its nearest ancestor in the page tree.
fn inherited<'a>(doc: &'a Document, page: &'a Dictionary, key: &[u8]) -> Option<&'a Object> {
    let mut node = page;
    for _ in 0..MAX_PAGE_TREE_DEPTH {
        if let Some(value) = get(doc, node, key) {
            return Some(value);
        }
        node = get_dict(doc, node, b"Parent")?;
    }
    None
}

/// The transform from user space to display space (points, origin top-left, y down) and the
/// displayed width and height, from the crop box, media box and rotation.
fn page_geometry(doc: &Document, page: Option<&Dictionary>) -> (Matrix, f64, f64) {
    let valid = |rect: &[f64; 4]| {
        let (width, height) = (rect[2] - rect[0], rect[3] - rect[1]);
        (1.0..=MAX_PAGE_SIDE).contains(&width) && (1.0..=MAX_PAGE_SIDE).contains(&height)
    };
    let media = page
        .and_then(|page| inherited(doc, page, b"MediaBox"))
        .and_then(|object| rectangle(doc, object))
        .filter(valid)
        .unwrap_or(DEFAULT_PAGE);
    let crop = page
        .and_then(|page| inherited(doc, page, b"CropBox"))
        .and_then(|object| rectangle(doc, object))
        .map(|crop| {
            [
                crop[0].max(media[0]),
                crop[1].max(media[1]),
                crop[2].min(media[2]),
                crop[3].min(media[3]),
            ]
        })
        .filter(valid)
        .unwrap_or(media);
    let rotation = page
        .and_then(|page| inherited(doc, page, b"Rotate"))
        .and_then(|object| resolve(doc, object))
        .and_then(number)
        .map_or(0, |degrees| ((degrees / 90.0).round() as i64).rem_euclid(4));
    let [x0, y0, x1, y1] = crop;
    let (width, height) = (x1 - x0, y1 - y0);
    match rotation {
        1 => (Matrix([0.0, 1.0, 1.0, 0.0, -y0, -x0]), height, width),
        2 => (Matrix([-1.0, 0.0, 0.0, 1.0, x1, -y0]), width, height),
        3 => (Matrix([0.0, -1.0, -1.0, 0.0, y1, x1]), height, width),
        _ => (Matrix([1.0, 0.0, 0.0, -1.0, -x0, y1]), width, height),
    }
}

/// Fills metadata from the information dictionary and the catalog's `/Lang`.
fn fill_metadata(doc: &Document, document: &mut IrDocument, stats: &mut PdfStats) {
    document.meta.source_format = Some(Format::Pdf);
    let info = doc
        .trailer
        .get(b"Info")
        .ok()
        .and_then(|object| resolve(doc, object))
        .and_then(|object| object.as_dict().ok());
    if let Some(info) = info {
        let text = |key: &[u8]| info_text(doc, info, key);
        document.meta.title = text(b"Title");
        if let Some(author) = text(b"Author") {
            document.meta.authors = split_list(&author, &[';']);
        }
        document.meta.subject = text(b"Subject");
        if let Some(keywords) = text(b"Keywords") {
            document.meta.keywords = split_list(&keywords, &[',', ';']);
        }
        stats.producer = text(b"Producer");
    }
    document.meta.language = doc
        .catalog()
        .ok()
        .and_then(|catalog| info_text(doc, catalog, b"Lang"))
        .filter(|lang| {
            lang.len() <= 35
                && lang
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        });
}

fn info_text(doc: &Document, dict: &Dictionary, key: &[u8]) -> Option<String> {
    let decoded = decode_text_string(get(doc, dict, key)?)?;
    let cleaned: String = decoded
        .chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect();
    let trimmed = cleaned.trim();
    (!trimmed.is_empty()).then(|| nfc(trimmed))
}

/// Decodes a PDF text string: UTF-16 with either byte-order mark (some producers write
/// little-endian), UTF-8 with a byte-order mark, or PDFDocEncoding.
fn decode_text_string(object: &Object) -> Option<String> {
    let bytes = object.as_str().ok()?;
    let utf16 = |rest: &[u8], big_endian: bool| {
        let units = rest.as_chunks::<2>().0.iter().map(|pair| {
            if big_endian {
                u16::from_be_bytes([pair[0], pair[1]])
            } else {
                u16::from_le_bytes([pair[0], pair[1]])
            }
        });
        char::decode_utf16(units)
            .map(|unit| unit.unwrap_or(char::REPLACEMENT_CHARACTER))
            .collect::<String>()
    };
    if let Some(rest) = bytes.strip_prefix(b"\xFE\xFF") {
        Some(utf16(rest, true))
    } else if let Some(rest) = bytes.strip_prefix(b"\xFF\xFE") {
        Some(utf16(rest, false))
    } else if let Some(rest) = bytes.strip_prefix(b"\xEF\xBB\xBF") {
        Some(String::from_utf8_lossy(rest).into_owned())
    } else {
        lopdf::decode_text_string(object).ok()
    }
}

fn split_list(value: &str, separators: &[char]) -> Vec<String> {
    value
        .split(separators)
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{decode_text_string, page_geometry, split_list};
    use lopdf::{Document, Object, dictionary};

    #[test]
    fn rotation_maps_the_crop_box_to_a_top_left_display_space() {
        let doc = Document::new();
        let box_ = |values: [i64; 4]| values.map(Object::Integer).to_vec();
        let page = dictionary! { "MediaBox" => box_([0, 0, 600, 800]), "CropBox" => box_([10, 20, 610, 700]) };
        let (matrix, width, height) = page_geometry(&doc, Some(&page));
        assert_eq!((width, height), (590.0, 680.0));
        assert_eq!(matrix.apply(10.0, 700.0), (0.0, 0.0));

        // Where the user-space origin (the unrotated bottom-left corner) is displayed.
        for (rotate, corner, size) in [
            (90, (0.0, 0.0), (800.0, 600.0)),
            (180, (600.0, 0.0), (600.0, 800.0)),
            (270, (800.0, 600.0), (800.0, 600.0)),
            (-90, (800.0, 600.0), (800.0, 600.0)),
        ] {
            let page = dictionary! { "MediaBox" => box_([0, 0, 600, 800]), "Rotate" => Object::Integer(rotate) };
            let (matrix, width, height) = page_geometry(&doc, Some(&page));
            assert_eq!((width, height), size, "rotate {rotate}");
            assert_eq!(matrix.apply(0.0, 0.0), corner, "rotate {rotate}");
        }

        let (_, width, height) = page_geometry(&doc, None);
        assert_eq!((width, height), (612.0, 792.0));
    }

    #[test]
    fn decodes_text_strings_in_every_encoding() {
        let string = |bytes: &[u8]| Object::String(bytes.to_vec(), lopdf::StringFormat::Literal);
        assert_eq!(
            decode_text_string(&string(b"\xFE\xFF\x00A\x1E\xA1")).as_deref(),
            Some("Aạ")
        );
        assert_eq!(
            decode_text_string(&string(b"\xFF\xFEA\x00\xA1\x1E")).as_deref(),
            Some("Aạ")
        );
        assert_eq!(
            decode_text_string(&string("\u{FEFF}Hà".as_bytes())).as_deref(),
            Some("Hà")
        );
        assert_eq!(decode_text_string(&string(b"A\xE9")).as_deref(), Some("Aé"));
        assert_eq!(decode_text_string(&Object::Integer(1)), None);
    }

    #[test]
    fn splits_metadata_lists() {
        assert_eq!(split_list(" a, b ;c,, ", &[',', ';']), vec!["a", "b", "c"]);
    }
}
