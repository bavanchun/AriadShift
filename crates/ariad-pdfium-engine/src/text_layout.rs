//! `pdf.text_layout`: per-page characters, words and lines with bounding boxes, font name,
//! font size and fill color in reading order.

use std::fs;

use ariad_core::{
    limits::Limits,
    protocol::ErrorCode,
    tools::{DuplicatePolicy, PageRange, params::TextLayoutParams},
};
use ariad_engine_io::EngineFailure;
use ariad_pdf::{document::PdfDoc, error::PdfError};
use pdfium_render::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::output::{Metrics, ToolContext, ToolResult};

/// Maximum number of visible characters allowed across a single `pdf.text_layout` job.
pub const MAX_TEXT_LAYOUT_CHARS: usize = 500_000;

/// Hard upper bound on serialized `text-layout.json` output size (32 MiB).
pub const MAX_TEXT_LAYOUT_BYTES: u64 = 32 * 1024 * 1024;

/// Top-level JSON document produced by `pdf.text_layout`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextLayoutDocument {
    /// Schema identifier (`"ariad-pdf-text-layout/1"`).
    pub schema: String,
    /// Coordinate unit (`"pdf_points"`, 1/72 inch).
    pub coord_system: String,
    /// Coordinate origin (`"bottom_left"`).
    pub origin: String,
    /// Coordinate tuple ordering (`["left", "bottom", "right", "top"]`).
    pub bbox_order: [String; 4],
    /// Total pages in the source PDF.
    pub page_count: usize,
    /// Extracted layout for each selected page.
    pub pages: Vec<PageTextLayout>,
}

/// Extracted text layout for a single page.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PageTextLayout {
    /// 1-based page number.
    pub page: u32,
    /// Page width in PDF points.
    pub width_pt: f64,
    /// Page height in PDF points.
    pub height_pt: f64,
    /// Page rotation in degrees clockwise (`0`, `90`, `180`, `270`).
    pub rotation_deg: u16,
    /// Visible characters on the page in reading order.
    pub chars: Vec<CharBox>,
    /// Words grouped from `chars` in reading order (top-to-bottom, left-to-right).
    pub words: Vec<WordBox>,
    /// Lines grouped from `words` in reading order (top-to-bottom, left-to-right).
    pub lines: Vec<LineBox>,
}

/// A single extracted character with geometry and style metadata.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CharBox {
    /// Character text (single Unicode scalar value).
    pub text: String,
    /// Bounding box `[left, bottom, right, top]` in PDF points (origin bottom-left).
    pub bbox: [f64; 4],
    /// PostScript / TrueType font name reported by PDFium.
    pub font_name: String,
    /// Scaled font size in PDF points.
    pub font_size: f64,
    /// Fill color formatted as `#RRGGBB` (or `#RRGGBBAA` when translucent).
    pub fill_color: String,
}

/// A word grouped from adjacent characters on the same baseline.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WordBox {
    /// Concatenated word text.
    pub text: String,
    /// Union bounding box `[left, bottom, right, top]` in PDF points.
    pub bbox: [f64; 4],
    /// Dominant font name of the word.
    pub font_name: String,
    /// Dominant font size of the word in PDF points.
    pub font_size: f64,
    /// Dominant fill color of the word.
    pub fill_color: String,
    /// Inclusive start index into [`PageTextLayout::chars`].
    pub char_start: usize,
    /// Exclusive end index into [`PageTextLayout::chars`].
    pub char_end: usize,
}

/// A horizontal text line grouped from words in reading order.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LineBox {
    /// Line text (words joined by a single ASCII space).
    pub text: String,
    /// Union bounding box `[left, bottom, right, top]` in PDF points.
    pub bbox: [f64; 4],
    /// Dominant font name of the line.
    pub font_name: String,
    /// Dominant font size of the line in PDF points.
    pub font_size: f64,
    /// Dominant fill color of the line.
    pub fill_color: String,
    /// Inclusive start index into [`PageTextLayout::words`].
    pub word_start: usize,
    /// Exclusive end index into [`PageTextLayout::words`].
    pub word_end: usize,
}

/// Reads and validates the input PDF through `ariad-pdf` so encrypted documents (including
/// owner-password-only PDFs), structural bombs, and corrupt files are rejected with typed errors
/// before invoking PDFium.
pub(crate) fn read_and_preflight_pdf(
    ctx: &ToolContext<'_>,
) -> Result<(Vec<u8>, PdfDoc), EngineFailure> {
    ctx.check()?;
    let input_path = ctx.input();
    let meta = fs::metadata(input_path).map_err(|_| EngineFailure::io())?;
    if let Some(max_in) = ctx.limits().max_input_bytes
        && meta.len() > max_in
    {
        return Err(EngineFailure::limit_exceeded());
    }
    let bytes = fs::read(input_path).map_err(|_| EngineFailure::io())?;
    ctx.check()?;
    if has_encrypt_in_trailer(&bytes) {
        return Err(EngineFailure::new(
            ErrorCode::EngineFailure,
            "The PDF is encrypted; run pdf.decrypt first.",
        ));
    }
    let pdf_doc = ariad_pdf::document::open(&bytes, ctx.limits()).map_err(map_pdf_error)?;
    Ok((bytes, pdf_doc))
}

fn has_encrypt_in_trailer(bytes: &[u8]) -> bool {
    const TRAILER: &[u8] = b"trailer";
    const ENCRYPT: &[u8] = b"/Encrypt";
    let mut pos = 0usize;
    while pos + TRAILER.len() <= bytes.len() {
        let Some(rel) = bytes[pos..]
            .windows(TRAILER.len())
            .position(|w| w == TRAILER)
        else {
            break;
        };
        let start = pos + rel + TRAILER.len();
        let end = start.saturating_add(4096).min(bytes.len());
        let window = &bytes[start..end];
        for (idx, w) in window.windows(ENCRYPT.len()).enumerate() {
            if w == ENCRYPT {
                let next = window.get(idx + ENCRYPT.len()).copied().unwrap_or(b' ');
                if matches!(
                    next,
                    b'\0'
                        | b'\t'
                        | b'\n'
                        | b'\x0c'
                        | b'\r'
                        | b' '
                        | b'/'
                        | b'<'
                        | b'>'
                        | b'['
                        | b']'
                        | b'('
                        | b')'
                ) {
                    return true;
                }
            }
        }
        pos = start;
    }
    false
}

/// Maps an [`ariad_pdf::error::PdfError`] to an [`EngineFailure`].
pub(crate) fn map_pdf_error(err: PdfError) -> EngineFailure {
    if err.is_cancelled() {
        return EngineFailure::new(ErrorCode::EngineFailure, "The job was cancelled.");
    }
    if err.is_timed_out() {
        return EngineFailure::new(ErrorCode::LimitExceeded, "The job exceeded its time limit.");
    }
    match err {
        PdfError::Encrypted { .. } => EngineFailure::new(
            ErrorCode::EngineFailure,
            "The PDF is encrypted; run pdf.decrypt first.",
        ),
        PdfError::LimitExceeded(_) => EngineFailure::limit_exceeded(),
        PdfError::InvalidParams(msg) => EngineFailure::owned(ErrorCode::InvalidRequest, msg),
        PdfError::Malformed | PdfError::NotImplemented(_) => EngineFailure::new(
            ErrorCode::EngineFailure,
            "The PDF is damaged or cannot be read.",
        ),
    }
}

/// Maps a [`PdfiumError`] returned when loading or inspecting a document to an [`EngineFailure`].
pub(crate) fn map_pdfium_error(err: PdfiumError) -> EngineFailure {
    match err {
        PdfiumError::PdfiumLibraryInternalError(
            PdfiumInternalError::PasswordError | PdfiumInternalError::SecurityError,
        ) => EngineFailure::new(
            ErrorCode::EngineFailure,
            "The PDF is encrypted; run pdf.decrypt first.",
        ),
        _ => EngineFailure::new(
            ErrorCode::EngineFailure,
            "The PDF is damaged or cannot be read by PDFium.",
        ),
    }
}

/// Expands a [`PageRange`] against `total_pages` with [`DuplicatePolicy::Reject`].
pub(crate) fn expand_pages(
    range: &PageRange,
    total_pages: usize,
) -> Result<Vec<u32>, EngineFailure> {
    let total_u32 = u32::try_from(total_pages).map_err(|_| EngineFailure::limit_exceeded())?;
    range
        .expand(total_u32, DuplicatePolicy::Reject)
        .map_err(|err| EngineFailure::owned(ErrorCode::InvalidRequest, err.to_string()))
}

/// Converts a [`PdfPageRenderRotation`] to clockwise degrees.
pub(crate) const fn rotation_degrees(rot: PdfPageRenderRotation) -> u16 {
    match rot {
        PdfPageRenderRotation::None => 0,
        PdfPageRenderRotation::Degrees90 => 90,
        PdfPageRenderRotation::Degrees180 => 180,
        PdfPageRenderRotation::Degrees270 => 270,
    }
}

/// Executes `pdf.text_layout`.
pub fn run(ctx: &mut ToolContext<'_>, params: TextLayoutParams) -> ToolResult {
    run_with_char_limit(ctx, params, MAX_TEXT_LAYOUT_CHARS)
}

/// Executes `pdf.text_layout` with an explicit maximum character limit (used by tests to exercise
/// character-cap refusal without constructing multi-megabyte fixtures).
pub fn run_with_char_limit(
    ctx: &mut ToolContext<'_>,
    params: TextLayoutParams,
    max_chars: usize,
) -> ToolResult {
    let (bytes, _lopdf_doc) = read_and_preflight_pdf(ctx)?;
    let pdfium = ctx.pdfium()?;
    let doc = pdfium
        .load_pdf_from_byte_slice(&bytes, None)
        .map_err(map_pdfium_error)?;

    let total_pages =
        usize::try_from(doc.pages().len()).map_err(|_| EngineFailure::limit_exceeded())?;
    if total_pages == 0 {
        return Err(EngineFailure::new(
            ErrorCode::EngineFailure,
            "The PDF has no pages.",
        ));
    }
    ctx.check_page_count(total_pages)?;

    let selected_pages = expand_pages(&params.pages, total_pages)?;
    ctx.check_page_count(selected_pages.len())?;

    let max_output_bytes = effective_output_byte_limit(ctx.limits());

    let mut page_layouts = Vec::with_capacity(selected_pages.len());
    let mut total_chars: usize = 0;
    let mut total_words: usize = 0;
    let mut total_lines: usize = 0;
    let selected_total = u64::try_from(selected_pages.len()).unwrap_or(u64::MAX);

    for (idx, &page_num) in selected_pages.iter().enumerate() {
        ctx.check()?;
        let page_index =
            PdfPageIndex::try_from(page_num - 1).map_err(|_| EngineFailure::limit_exceeded())?;
        let page = doc.pages().get(page_index).map_err(map_pdfium_error)?;

        let page_layout = extract_page_layout(ctx, &page, page_num, &mut total_chars, max_chars)?;
        total_words = total_words.saturating_add(page_layout.words.len());
        total_lines = total_lines.saturating_add(page_layout.lines.len());
        page_layouts.push(page_layout);

        let done = u64::try_from(idx + 1).unwrap_or(u64::MAX);
        ctx.progress("text_layout", done, selected_total)?;
    }

    let report = TextLayoutDocument {
        schema: "ariad-pdf-text-layout/1".to_owned(),
        coord_system: "pdf_points".to_owned(),
        origin: "bottom_left".to_owned(),
        bbox_order: [
            "left".to_owned(),
            "bottom".to_owned(),
            "right".to_owned(),
            "top".to_owned(),
        ],
        page_count: total_pages,
        pages: page_layouts,
    };

    let json_bytes = serde_json::to_vec_pretty(&report).map_err(|_| EngineFailure::io())?;
    if u64::try_from(json_bytes.len()).unwrap_or(u64::MAX) > max_output_bytes {
        return Err(EngineFailure::new(
            ErrorCode::LimitExceeded,
            "The extracted text layout exceeds the output size limit.",
        ));
    }

    let stem = ctx.input_stem();
    let artifact_name = format!("{stem}.text-layout.json");
    let artifact_path = ctx.artifact_path(&artifact_name)?;
    fs::write(&artifact_path, &json_bytes).map_err(|_| EngineFailure::io())?;
    ctx.artifact(&artifact_path, "json")?;

    let mut metrics = Metrics::new();
    metrics.insert(
        "pages".to_owned(),
        json!(u64::try_from(selected_pages.len()).unwrap_or(u64::MAX)),
    );
    metrics.insert(
        "chars".to_owned(),
        json!(u64::try_from(total_chars).unwrap_or(u64::MAX)),
    );
    metrics.insert(
        "words".to_owned(),
        json!(u64::try_from(total_words).unwrap_or(u64::MAX)),
    );
    metrics.insert(
        "lines".to_owned(),
        json!(u64::try_from(total_lines).unwrap_or(u64::MAX)),
    );
    Ok(Some(metrics))
}

const fn effective_output_byte_limit(limits: &Limits) -> u64 {
    if limits.max_decompressed_bytes < MAX_TEXT_LAYOUT_BYTES {
        limits.max_decompressed_bytes
    } else {
        MAX_TEXT_LAYOUT_BYTES
    }
}

#[derive(Clone, Debug)]
struct RawWord {
    chars: Vec<CharBox>,
    bbox: [f64; 4],
}

impl RawWord {
    fn from_chars(chars: Vec<CharBox>) -> Self {
        let bbox = union_bboxes(chars.iter().map(|c| c.bbox));
        Self { chars, bbox }
    }

    fn center_y(&self) -> f64 {
        (self.bbox[1] + self.bbox[3]) * 0.5
    }

    fn height(&self) -> f64 {
        let h = self.bbox[3] - self.bbox[1];
        let font_h = self.chars.first().map_or(0.0, |c| c.font_size);
        h.max(font_h).max(1.0)
    }
}

fn extract_page_layout(
    ctx: &ToolContext<'_>,
    page: &PdfPage<'_>,
    page_number: u32,
    total_chars: &mut usize,
    max_chars: usize,
) -> Result<PageTextLayout, EngineFailure> {
    let width_pt = round3(f64::from(page.width().value));
    let height_pt = round3(f64::from(page.height().value));
    let rotation_deg = page.rotation().map_or(0, rotation_degrees);

    let text_page = page.text().map_err(map_pdfium_error)?;
    let chars_collection = text_page.chars();

    let mut raw_words: Vec<RawWord> = Vec::new();
    let mut current_word_chars: Vec<CharBox> = Vec::new();

    for (idx, char_item) in chars_collection.iter().enumerate() {
        if idx % 128 == 0 {
            ctx.check()?;
        }

        let text_str = char_item
            .unicode_string()
            .or_else(|| char_item.unicode_char().map(|ch| ch.to_string()))
            .unwrap_or_default();

        let filtered: String = text_str
            .chars()
            .filter(|&c| c != '\0' && c != '\u{feff}')
            .collect();

        if filtered.is_empty() {
            continue;
        }

        if filtered
            .chars()
            .all(|c| c.is_whitespace() || c.is_control())
        {
            if !current_word_chars.is_empty() {
                raw_words.push(RawWord::from_chars(std::mem::take(&mut current_word_chars)));
            }
            continue;
        }

        *total_chars = total_chars.saturating_add(1);
        if *total_chars > max_chars {
            return Err(EngineFailure::new(
                ErrorCode::LimitExceeded,
                "The document exceeds the maximum character limit for text_layout.",
            ));
        }

        let bbox = extract_char_bbox(&char_item);
        let font_name = sanitize_font_name(&char_item.font_name());
        let scaled_size = f64::from(char_item.scaled_font_size().value);
        let font_size = round2(if scaled_size > 0.0 {
            scaled_size
        } else {
            f64::from(char_item.unscaled_font_size().value).max(0.0)
        });
        let fill_color = char_item
            .fill_color()
            .map_or_else(|_| "#000000".to_owned(), format_color);

        let char_box = CharBox {
            text: filtered,
            bbox,
            font_name,
            font_size,
            fill_color,
        };

        if let Some(prev) = current_word_chars.last()
            && should_break_word(prev, &char_box)
        {
            raw_words.push(RawWord::from_chars(std::mem::take(&mut current_word_chars)));
        }

        current_word_chars.push(char_box);
    }

    if !current_word_chars.is_empty() {
        raw_words.push(RawWord::from_chars(current_word_chars));
    }

    let (chars, words, lines) = assemble_reading_order(raw_words);

    Ok(PageTextLayout {
        page: page_number,
        width_pt,
        height_pt,
        rotation_deg,
        chars,
        words,
        lines,
    })
}

fn extract_char_bbox(char_item: &PdfPageTextChar<'_>) -> [f64; 4] {
    let rect = char_item
        .tight_bounds()
        .or_else(|_| char_item.loose_bounds());
    match rect {
        Ok(r) => {
            let l = f64::from(r.left().value);
            let right = f64::from(r.right().value);
            let b = f64::from(r.bottom().value);
            let t = f64::from(r.top().value);
            [
                round3(l.min(right)),
                round3(b.min(t)),
                round3(l.max(right)),
                round3(b.max(t)),
            ]
        }
        Err(_) => [0.0, 0.0, 0.0, 0.0],
    }
}

fn sanitize_font_name(raw: &str) -> String {
    raw.trim_matches('\0').trim().to_owned()
}

fn format_color(color: PdfColor) -> String {
    let r = color.red();
    let g = color.green();
    let b = color.blue();
    let a = color.alpha();
    if a == 255 {
        format!("#{r:02X}{g:02X}{b:02X}")
    } else {
        format!("#{r:02X}{g:02X}{b:02X}{a:02X}")
    }
}

fn should_break_word(prev: &CharBox, next: &CharBox) -> bool {
    let ref_size = prev.font_size.max(next.font_size).max(4.0);
    let prev_cy = (prev.bbox[1] + prev.bbox[3]) * 0.5;
    let next_cy = (next.bbox[1] + next.bbox[3]) * 0.5;
    let dy = (next_cy - prev_cy).abs();
    if dy > ref_size * 0.65 {
        return true;
    }
    let dx = next.bbox[0] - prev.bbox[2];
    if dx > ref_size * 0.32 {
        return true;
    }
    let backward = prev.bbox[0] - next.bbox[0];
    backward > ref_size * 0.5
}

struct LineGroup {
    words: Vec<RawWord>,
    center_y: f64,
    height: f64,
    left: f64,
}

fn assemble_reading_order(
    mut raw_words: Vec<RawWord>,
) -> (Vec<CharBox>, Vec<WordBox>, Vec<LineBox>) {
    if raw_words.is_empty() {
        return (Vec::new(), Vec::new(), Vec::new());
    }

    raw_words.sort_by(|a, b| {
        b.center_y()
            .total_cmp(&a.center_y())
            .then_with(|| a.bbox[0].total_cmp(&b.bbox[0]))
    });

    let mut line_groups: Vec<LineGroup> = Vec::new();
    for word in raw_words {
        let w_cy = word.center_y();
        let w_h = word.height();
        let mut matched_idx = None;
        for (idx, group) in line_groups.iter().enumerate() {
            let tol = 0.55 * group.height.min(w_h).max(4.0);
            if (w_cy - group.center_y).abs() <= tol {
                matched_idx = Some(idx);
                break;
            }
        }
        if let Some(idx) = matched_idx {
            let group = &mut line_groups[idx];
            let n = group.words.len() as f64;
            group.center_y = (group.center_y * n + w_cy) / (n + 1.0);
            group.height = group.height.max(w_h);
            group.left = group.left.min(word.bbox[0]);
            group.words.push(word);
        } else {
            line_groups.push(LineGroup {
                left: word.bbox[0],
                center_y: w_cy,
                height: w_h,
                words: vec![word],
            });
        }
    }

    line_groups.sort_by(|a, b| {
        b.center_y
            .total_cmp(&a.center_y)
            .then_with(|| a.left.total_cmp(&b.left))
    });

    let mut out_chars: Vec<CharBox> = Vec::new();
    let mut out_words: Vec<WordBox> = Vec::new();
    let mut out_lines: Vec<LineBox> = Vec::new();

    for mut group in line_groups {
        group.words.sort_by(|a, b| {
            a.bbox[0]
                .total_cmp(&b.bbox[0])
                .then_with(|| b.bbox[3].total_cmp(&a.bbox[3]))
        });

        let word_start = out_words.len();
        let mut line_word_texts = Vec::with_capacity(group.words.len());

        for raw_word in group.words {
            let char_start = out_chars.len();
            let word_text: String = raw_word.chars.iter().map(|c| c.text.as_str()).collect();
            let first = &raw_word.chars[0];
            let font_name = first.font_name.clone();
            let font_size = first.font_size;
            let fill_color = first.fill_color.clone();

            out_chars.extend(raw_word.chars);
            let char_end = out_chars.len();

            line_word_texts.push(word_text.clone());
            out_words.push(WordBox {
                text: word_text,
                bbox: raw_word.bbox,
                font_name,
                font_size,
                fill_color,
                char_start,
                char_end,
            });
        }

        let word_end = out_words.len();
        let line_slice = &out_words[word_start..word_end];
        let line_bbox = union_bboxes(line_slice.iter().map(|w| w.bbox));
        let first_word = &line_slice[0];
        out_lines.push(LineBox {
            text: line_word_texts.join(" "),
            bbox: line_bbox,
            font_name: first_word.font_name.clone(),
            font_size: first_word.font_size,
            fill_color: first_word.fill_color.clone(),
            word_start,
            word_end,
        });
    }

    (out_chars, out_words, out_lines)
}

fn union_bboxes<I>(iter: I) -> [f64; 4]
where
    I: IntoIterator<Item = [f64; 4]>,
{
    let mut it = iter.into_iter();
    let Some(first) = it.next() else {
        return [0.0, 0.0, 0.0, 0.0];
    };
    let mut left = first[0];
    let mut bottom = first[1];
    let mut right = first[2];
    let mut top = first[3];
    for b in it {
        left = left.min(b[0]);
        bottom = bottom.min(b[1]);
        right = right.max(b[2]);
        top = top.max(b[3]);
    }
    [round3(left), round3(bottom), round3(right), round3(top)]
}

fn round2(v: f64) -> f64 {
    if !v.is_finite() {
        return 0.0;
    }
    (v * 100.0).round() / 100.0
}

fn round3(v: f64) -> f64 {
    if !v.is_finite() {
        return 0.0;
    }
    (v * 1000.0).round() / 1000.0
}
