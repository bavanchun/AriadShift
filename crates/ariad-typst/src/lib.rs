//! Compiles Typst markup produced by `ariad_core::writer::typst` into PDF bytes.
//!
//! The compiler sees a closed, in-memory world: the generated main source, the virtual image
//! files the writer registered, and fonts embedded in this crate. It has no file system, package
//! or network access, and its clock is the conversion timestamp, so identical input and timestamp
//! give byte-identical PDFs.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::OnceLock,
};

use ariad_core::{
    ir::Document,
    warning::Warning,
    writer::typst::{self as writer, TypstOptions, TypstOutput},
};
use thiserror::Error;
use typst::{
    Library, LibraryExt, World,
    diag::{FileError, FileResult, SourceDiagnostic},
    foundations::{Bytes, Datetime, Duration},
    layout::{Frame, FrameItem},
    syntax::{FileId, RootedPath, Source, VirtualPath, VirtualRoot},
    text::{Font, FontBook},
    utils::LazyHash,
};
use typst_pdf::{PdfOptions, Timestamp};

const MAIN_PATH: &str = "main.typ";

/// The fonts the PDF writer embeds; see `fonts/SOURCES.txt` for their origin and licence.
const FONT_FILES: [&[u8]; 7] = [
    include_bytes!("../fonts/LibertinusSerif-Regular.otf"),
    include_bytes!("../fonts/LibertinusSerif-Bold.otf"),
    include_bytes!("../fonts/LibertinusSerif-Italic.otf"),
    include_bytes!("../fonts/LibertinusSerif-BoldItalic.otf"),
    include_bytes!("../fonts/LibertinusMath-Regular.otf"),
    include_bytes!("../fonts/SourceCodePro-Regular.otf"),
    include_bytes!("../fonts/SourceCodePro-Bold.otf"),
];

/// A compiled PDF with the warnings collected on the way.
#[derive(Clone, Debug)]
pub struct RenderedPdf {
    pub bytes: Vec<u8>,
    /// Pages produced by the layout compiler.
    pub page_count: usize,
    /// Warnings from the Typst writer.
    pub writer_warnings: Vec<Warning>,
    /// Typst compiler warning messages. They may quote generated markup, so protocol output
    /// reports only their number.
    pub typst_warnings: Vec<String>,
    /// Characters that no embedded font covers; they render as empty boxes.
    pub missing_glyphs: usize,
}

/// Why Typst could not produce a PDF.
///
/// The display messages are fixed; the diagnostics stay available for tests and local debugging
/// and are never written to protocol output.
#[derive(Debug, Error)]
pub enum RenderError {
    #[error("the conversion timestamp is outside the supported date range")]
    InvalidTimestamp,
    #[error("Typst could not compile the document")]
    Compile { diagnostics: Vec<String> },
    #[error("Typst could not export the document as PDF")]
    Export { diagnostics: Vec<String> },
}

/// Writes `document` as Typst markup and compiles it to PDF.
///
/// When compilation fails while images are embedded, images that Typst cannot decode on their own
/// are replaced by their alt text and the document is compiled once more.
pub fn render(
    document: &Document,
    options: &TypstOptions,
    timestamp: i64,
) -> Result<RenderedPdf, RenderError> {
    let output = writer::write(document, options);
    let error = match compile_pdf(&output, timestamp) {
        Ok(rendered) => return Ok(rendered),
        Err(error @ RenderError::Compile { .. }) if !output.files.is_empty() => error,
        Err(error) => return Err(error),
    };
    let rejected = undecodable_assets(document, &options.rejected_assets, timestamp)?;
    if rejected.is_empty() {
        return Err(error);
    }
    let mut retry = options.clone();
    retry.rejected_assets.extend(rejected);
    compile_pdf(&writer::write(document, &retry), timestamp)
}

/// Compiles writer output to PDF bytes.
pub fn compile_pdf(output: &TypstOutput, timestamp: i64) -> Result<RenderedPdf, RenderError> {
    let compiled = compile(output, timestamp, Mode::Pdf)?;
    Ok(RenderedPdf {
        bytes: compiled.pdf,
        page_count: compiled.page_count,
        writer_warnings: output.warnings.clone(),
        typst_warnings: compiled.warnings,
        missing_glyphs: compiled.missing_glyphs,
    })
}

/// The laid-out text of writer output, one string per page.
#[doc(hidden)]
pub fn page_texts(output: &TypstOutput, timestamp: i64) -> Result<Vec<String>, RenderError> {
    compile(output, timestamp, Mode::Text).map(|compiled| compiled.pages)
}

/// Ids of image assets that Typst cannot decode, ignoring those already in `known`.
pub fn undecodable_assets(
    document: &Document,
    known: &BTreeSet<String>,
    timestamp: i64,
) -> Result<BTreeSet<String>, RenderError> {
    let mut rejected = BTreeSet::new();
    for (id, asset) in &document.assets {
        let Some(extension) = writer::image_extension(&asset.media_type) else {
            continue;
        };
        if known.contains(id) {
            continue;
        }
        let path = format!("image.{extension}");
        let probe = TypstOutput {
            markup: format!("#image(\"{path}\")\n"),
            files: BTreeMap::from([(path, asset.bytes.clone())]),
            warnings: Vec::new(),
        };
        match compile(&probe, timestamp, Mode::Layout) {
            Ok(_) => {}
            Err(RenderError::Compile { .. } | RenderError::Export { .. }) => {
                rejected.insert(id.clone());
            }
            Err(error @ RenderError::InvalidTimestamp) => return Err(error),
        }
    }
    Ok(rejected)
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Mode {
    /// Lay out only.
    Layout,
    /// Lay out and export the PDF.
    Pdf,
    /// Lay out and collect the text of each page.
    Text,
}

#[derive(Default)]
struct Compiled {
    pdf: Vec<u8>,
    page_count: usize,
    warnings: Vec<String>,
    missing_glyphs: usize,
    pages: Vec<String>,
}

fn compile(output: &TypstOutput, timestamp: i64, mode: Mode) -> Result<Compiled, RenderError> {
    let now = utc_datetime(timestamp, 0).ok_or(RenderError::InvalidTimestamp)?;
    let world = MemoryWorld::new(output, timestamp);
    let warned = typst::compile(&world);
    let warnings = warned
        .warnings
        .iter()
        .map(|warning| warning.message.to_string())
        .collect();
    let document = warned.output.map_err(|errors| RenderError::Compile {
        diagnostics: messages(&errors),
    })?;

    let mut compiled = Compiled {
        warnings,
        ..Compiled::default()
    };
    if mode == Mode::Pdf {
        let options = PdfOptions {
            timestamp: Some(Timestamp::new_utc(now)),
            ..PdfOptions::default()
        };
        compiled.pdf =
            typst_pdf::pdf(&document, &options).map_err(|errors| RenderError::Export {
                diagnostics: messages(&errors),
            })?;
    }
    compiled.page_count = document.pages().len();
    for page in document.pages() {
        compiled.missing_glyphs += missing_glyphs(&page.frame);
        if mode == Mode::Text {
            compiled.pages.push(frame_text(&page.frame));
        }
    }
    Ok(compiled)
}

fn messages(diagnostics: &[SourceDiagnostic]) -> Vec<String> {
    diagnostics
        .iter()
        .map(|diagnostic| diagnostic.message.to_string())
        .collect()
}

/// Text runs of a frame and its groups in layout order.
fn frame_items(frame: &Frame) -> impl Iterator<Item = &FrameItem> {
    let mut stack = vec![frame.items()];
    std::iter::from_fn(move || {
        loop {
            let items = stack.last_mut()?;
            match items.next() {
                Some((_, FrameItem::Group(group))) => stack.push(group.frame.items()),
                Some((_, item)) => return Some(item),
                None => {
                    stack.pop();
                }
            }
        }
    })
}

/// Counts glyphs that fell back to `.notdef` for visible characters.
fn missing_glyphs(frame: &Frame) -> usize {
    frame_items(frame)
        .filter_map(|item| match item {
            FrameItem::Text(text) => Some(text),
            _ => None,
        })
        .map(|text| {
            text.glyphs
                .iter()
                .filter(|glyph| glyph.id == 0)
                .filter(|glyph| {
                    text.text
                        .get(glyph.range())
                        .is_some_and(|chars| chars.chars().any(|ch| !ch.is_whitespace()))
                })
                .count()
        })
        .sum()
}

fn frame_text(frame: &Frame) -> String {
    let mut text = String::new();
    for item in frame_items(frame) {
        if let FrameItem::Text(run) = item {
            if !text.is_empty() {
                text.push(' ');
            }
            text.push_str(&run.text);
        }
    }
    text
}

/// The calendar date and time of a Unix timestamp shifted by `offset_seconds`.
fn utc_datetime(timestamp: i64, offset_seconds: i32) -> Option<Datetime> {
    let offset = jiff::tz::Offset::from_seconds(offset_seconds).ok()?;
    let time = jiff::Timestamp::from_second(timestamp)
        .ok()?
        .to_zoned(offset.to_time_zone());
    Datetime::from_ymd_hms(
        i32::from(time.year()),
        u8::try_from(time.month()).ok()?,
        u8::try_from(time.day()).ok()?,
        u8::try_from(time.hour()).ok()?,
        u8::try_from(time.minute()).ok()?,
        u8::try_from(time.second()).ok()?,
    )
}

struct FontSet {
    book: LazyHash<FontBook>,
    fonts: Vec<Font>,
}

fn fonts() -> &'static FontSet {
    static FONTS: OnceLock<FontSet> = OnceLock::new();
    FONTS.get_or_init(|| {
        let fonts: Vec<Font> = FONT_FILES
            .iter()
            .flat_map(|data| Font::iter(Bytes::new(*data)))
            .collect();
        FontSet {
            book: LazyHash::new(FontBook::from_fonts(&fonts)),
            fonts,
        }
    })
}

fn library() -> &'static LazyHash<Library> {
    static LIBRARY: OnceLock<LazyHash<Library>> = OnceLock::new();
    LIBRARY.get_or_init(|| LazyHash::new(Library::builder().build()))
}

/// A Typst world holding one generated source and its virtual files.
struct MemoryWorld {
    main: FileId,
    source: Source,
    files: BTreeMap<String, Bytes>,
    timestamp: i64,
}

impl MemoryWorld {
    fn new(output: &TypstOutput, timestamp: i64) -> Self {
        let main = project_file(MAIN_PATH);
        Self {
            main,
            source: Source::new(main, output.markup.clone()),
            files: output
                .files
                .iter()
                .map(|(path, bytes)| (path.clone(), Bytes::new(bytes.clone())))
                .collect(),
            timestamp,
        }
    }

    /// The project-relative path of a file id, or `None` for package files.
    fn project_path(id: FileId) -> Option<&'static str> {
        let path = id.get();
        match path.root() {
            VirtualRoot::Project => Some(path.vpath().get_without_slash()),
            VirtualRoot::Package(_) => None,
        }
    }
}

fn project_file(path: &str) -> FileId {
    let path = VirtualPath::new(path).expect("the main source path is a valid virtual path");
    RootedPath::new(VirtualRoot::Project, path).intern()
}

impl World for MemoryWorld {
    fn library(&self) -> &LazyHash<Library> {
        library()
    }

    fn book(&self) -> &LazyHash<FontBook> {
        &fonts().book
    }

    fn main(&self) -> FileId {
        self.main
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        if id == self.main {
            return Ok(self.source.clone());
        }
        match Self::project_path(id) {
            Some(path) if self.files.contains_key(path) => Err(FileError::NotSource),
            Some(path) => Err(FileError::NotFound(PathBuf::from(path))),
            None => Err(FileError::AccessDenied),
        }
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        if id == self.main {
            return Ok(Bytes::from_string(self.source.text().to_owned()));
        }
        match Self::project_path(id) {
            Some(path) => self
                .files
                .get(path)
                .cloned()
                .ok_or_else(|| FileError::NotFound(PathBuf::from(path))),
            None => Err(FileError::AccessDenied),
        }
    }

    fn font(&self, index: usize) -> Option<Font> {
        fonts().fonts.get(index).cloned()
    }

    fn today(&self, offset: Option<Duration>) -> Option<Datetime> {
        let offset_seconds = match offset {
            // Without an explicit offset the conversion runs in UTC; there is no local zone.
            None => 0,
            Some(offset) => {
                let seconds = offset.seconds();
                if !seconds.is_finite() || seconds.abs() > 86_400.0 {
                    return None;
                }
                seconds as i32
            }
        };
        let datetime = utc_datetime(self.timestamp, offset_seconds)?;
        Datetime::from_ymd(datetime.year()?, datetime.month()?, datetime.day()?)
    }
}

#[cfg(test)]
mod tests {
    use typst::{World, foundations::Duration};

    use super::{FONT_FILES, MemoryWorld, TypstOutput, fonts, utc_datetime};

    #[test]
    fn every_embedded_font_loads_into_the_book() {
        let set = fonts();
        assert_eq!(set.fonts.len(), FONT_FILES.len());
        for family in ["Libertinus Serif", "Libertinus Math", "Source Code Pro"] {
            assert!(set.book.contains_family(&family.to_lowercase()), "{family}");
        }
    }

    #[test]
    fn today_follows_the_timestamp_in_utc() {
        let world = MemoryWorld::new(&TypstOutput::default(), 1_700_000_000);
        let today = world.today(None).unwrap();
        assert_eq!(
            (today.year(), today.month(), today.day()),
            (Some(2023), Some(11), Some(14))
        );
        let east = world
            .today(Some(Duration::construct(0, 0, 23, 0, 0)))
            .unwrap();
        assert_eq!(east.day(), Some(15));
        assert!(
            world
                .today(Some(Duration::construct(0, 0, 0, 2, 0)))
                .is_none()
        );
        assert!(utc_datetime(i64::MAX, 0).is_none());
    }

    #[test]
    fn the_world_serves_only_its_own_files() {
        let mut output = TypstOutput::default();
        output
            .files
            .insert("assets/a.png".to_owned(), vec![1, 2, 3]);
        let world = MemoryWorld::new(&output, 0);
        let asset = super::project_file("assets/a.png");
        assert_eq!(world.file(asset).unwrap().as_slice(), &[1, 2, 3]);
        assert!(world.file(super::project_file("etc/passwd")).is_err());
        assert!(world.source(asset).is_err());
        assert!(world.file(world.main()).is_ok());
    }
}

/// Compiles bounded IR with the native writer and embedded fonts.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = renderPdf)]
pub fn render_pdf(
    bytes: &[u8],
    title: Option<String>,
    timestamp: f64,
) -> Result<wasm_bindgen::JsValue, wasm_bindgen::JsValue> {
    use wasm_bindgen::JsValue;
    let failure = |message: String| {
        js_sys::JSON::parse(
            &serde_json::json!({ "code": "invalid_input", "message": message }).to_string(),
        )
        .unwrap_or_else(|_| JsValue::from_str("PDF compilation failed"))
    };
    let document = ariad_core::pipeline::read_ir_json(bytes, &ariad_core::limits::Limits::local())
        .map_err(|e| failure(e.to_string()))?;
    let options = TypstOptions {
        title_fallback: title,
        ..TypstOptions::default()
    };
    let rendered =
        render(&document, &options, timestamp as i64).map_err(|e| failure(e.to_string()))?;
    let mut warnings: Vec<ariad_core::pipeline::Notice> = rendered
        .writer_warnings
        .iter()
        .map(ariad_core::pipeline::Notice::from)
        .collect();
    if rendered.missing_glyphs > 0 {
        warnings.push(ariad_core::pipeline::Notice::new(
            "missing_glyphs",
            format!(
                "{} characters are not covered by the embedded fonts.",
                rendered.missing_glyphs
            ),
        ));
    }
    if !rendered.typst_warnings.is_empty() {
        warnings.push(ariad_core::pipeline::Notice::new(
            "typst_warning",
            format!(
                "Typst reported {} layout warnings.",
                rendered.typst_warnings.len()
            ),
        ));
    }
    let result = js_sys::JSON::parse(&serde_json::json!({ "format": "pdf", "media_type": "application/pdf", "extension": "pdf", "route": [], "warnings": warnings }).to_string())?;
    js_sys::Reflect::set(
        &result,
        &JsValue::from_str("bytes"),
        &js_sys::Uint8Array::from(rendered.bytes.as_slice()),
    )?;
    Ok(result)
}
