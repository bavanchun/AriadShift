//! Character mapping for invisible OCR text layers.
//!
//! Each distinct Unicode scalar receives a two-byte CID; the ToUnicode CMap preserves
//! supplementary characters as UTF-16 surrogate pairs. The single-glyph font and the
//! CID-to-glyph map let PDF readers select and copy text without painting visible marks.
use crate::PdfError;
use ariad_core::ocr::OcrPage;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

/// Glyphless Tesseract font, version 5.5.4, SHA-256
/// `c7845420925a23d88ed830a63957b8af85a66a8daf8d9fc90e843673b2ef1a59`.
pub const GLYPHLESS_FONT: &[u8] = include_bytes!("../assets/glyphless.ttf");

/// Validated character mapping for a page's OCR text.
#[derive(Debug)]
pub struct TextEncoding {
    maps: Vec<BTreeMap<char, u16>>,
    word_fonts: Vec<usize>,
}
impl TextEncoding {
    /// Construct a deterministic map in reading order; reject unrepresentable pages.
    pub fn new(page: &OcrPage) -> Result<Self, PdfError> {
        page.validate()
            .map_err(|e| PdfError::InvalidParams(e.to_string()))?;
        let new_map = || BTreeMap::from([(' ', 1)]);
        let mut maps: Vec<BTreeMap<char, u16>> = vec![new_map()];
        let mut word_fonts = Vec::new();
        for paragraph in &page.paragraphs {
            for line in &paragraph.lines {
                for word in &line.words {
                    let distinct: BTreeSet<char> = word.text.chars().collect();
                    let current = maps.last().expect("one mapping exists");
                    let additional = distinct.iter().filter(|c| !current.contains_key(c)).count();
                    if current.len() + additional > usize::from(u16::MAX) {
                        maps.push(new_map());
                    }
                    let index = maps.len() - 1;
                    let chars = &mut maps[index];
                    for character in word.text.chars() {
                        if !chars.contains_key(&character) {
                            let cid =
                                u16::try_from(chars.len() + 1).map_err(|_| PdfError::Malformed)?;
                            chars.insert(character, cid);
                        }
                    }
                    word_fonts.push(index);
                }
            }
        }
        Ok(Self { maps, word_fonts })
    }

    /// Encode mapped text as big-endian CID bytes for an Identity-H PDF font.
    pub fn encode(&self, font_index: usize, text: &str) -> Result<Vec<u8>, PdfError> {
        let mut bytes = Vec::with_capacity(text.len() * 2);
        for c in text.chars() {
            let cid = self
                .maps
                .get(font_index)
                .and_then(|chars| chars.get(&c))
                .ok_or_else(|| PdfError::InvalidParams("unmapped OCR character".to_owned()))?;
            bytes.extend_from_slice(&cid.to_be_bytes());
        }
        Ok(bytes)
    }

    /// Generate a PDF ToUnicode CMap for the assigned CIDs.
    #[must_use]
    pub fn to_unicode_cmap(&self, font_index: usize) -> Option<Vec<u8>> {
        let chars = self.maps.get(font_index)?;
        let mut result = String::from(
            "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n/CMapName /AriadOCR def\n/CMapType 2 def\n1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n",
        );
        let mut pairs: Vec<_> = chars
            .iter()
            .map(|(&character, &cid)| (cid, character))
            .collect();
        pairs.sort_unstable_by_key(|(cid, _)| *cid);
        for chunk in pairs.chunks(100) {
            writeln!(result, "{} beginbfchar", chunk.len()).expect("string write");
            for &(cid, character) in chunk {
                write!(result, "<{cid:04X}> <").expect("string write");
                for unit in character.encode_utf16(&mut [0; 2]).iter() {
                    write!(result, "{unit:04X}").expect("string write");
                }
                result.push_str(">\n");
            }
            result.push_str("endbfchar\n");
        }
        result.push_str("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
        Some(result.into_bytes())
    }

    /// Map all assigned CIDs to the glyphless font's single nonzero glyph.
    #[must_use]
    pub fn cid_to_gid_map(&self, font_index: usize) -> Option<Vec<u8>> {
        let mut map = vec![0; (self.maps.get(font_index)?.len() + 1) * 2];
        for value in map.iter_mut().skip(3).step_by(2) {
            *value = 1;
        }
        Some(map)
    }
}

/// Add an invisible text layer to a page, preserving its existing streams and resources.
/// OCR coordinates refer to the upright image after `rotation_cw`, using a top-left origin.
pub fn add_text_layer(
    pdf: &mut crate::PdfDoc,
    page_number: u32,
    page: &OcrPage,
    ctx: &crate::ToolCtx,
) -> Result<(), PdfError> {
    ctx.check()?;
    let encoding = TextEncoding::new(page)?;
    let mut output = pdf.clone();
    add_text_layer_inner(&mut output, page_number, page, &encoding, ctx)?;
    *pdf = output;
    Ok(())
}

fn add_text_layer_inner(
    pdf: &mut crate::PdfDoc,
    page_number: u32,
    page: &OcrPage,
    encoding: &TextEncoding,
    ctx: &crate::ToolCtx,
) -> Result<(), PdfError> {
    use lopdf::{Object, Stream, dictionary};
    ctx.check()?;
    let index = page_number
        .checked_sub(1)
        .ok_or_else(|| PdfError::InvalidParams("OCR page number is outside the document".into()))?
        as usize;
    let page_ids = crate::tools::rotate::collect_page_ids(pdf.inner(), ctx)?;
    let page_id = *page_ids
        .get(index)
        .ok_or_else(|| PdfError::InvalidParams("OCR page number is outside the document".into()))?;
    let mut budget = pdf.decode_budget();
    validate_page_graphics_state(pdf, page_id, ctx, &mut budget)?;
    let rect = crate::xobject::resolve_page_crop_box(pdf.inner(), page_id);
    let rotation = crate::xobject::resolve_page_rotation(pdf.inner(), page_id).rem_euclid(360);
    if !matches!(rotation, 0 | 90 | 180 | 270)
        || !rect
            .to_array()
            .iter()
            .all(|value| value.is_finite() && value.abs() <= crate::paper::MAX_PAPER_SIDE_PT)
        || rect.width() <= 0.0
        || rect.height() <= 0.0
    {
        return Err(PdfError::InvalidParams(
            "unsupported OCR page geometry".into(),
        ));
    }
    let mut resources = crate::xobject::resolve_page_resources(pdf.inner(), page_id);
    let mut fonts = match resources.get(b"Font") {
        Ok(value) => pdf
            .inner()
            .dereference(value)
            .ok()
            .and_then(|(_, value)| value.as_dict().ok())
            .cloned()
            .ok_or(PdfError::Malformed)?,
        Err(_) => lopdf::Dictionary::new(),
    };
    budget.charge(GLYPHLESS_FONT.len())?;
    let doc = pdf.inner_mut()?;
    let font_file = doc.add_object(Stream::new(
        dictionary! {"Length1" => GLYPHLESS_FONT.len() as i64},
        GLYPHLESS_FONT.to_vec(),
    ));
    let descriptor = doc.add_object(dictionary! {
        "Type" => "FontDescriptor", "FontName" => "GlyphLessFont", "Flags" => 5,
        "FontBBox" => vec![0.into(), 0.into(), 500.into(), 1000.into()],
        "ItalicAngle" => 0, "Ascent" => 1000, "Descent" => -1, "CapHeight" => 1000,
        "StemV" => 80, "FontFile2" => font_file,
    });
    let mut suffix = 0u32;
    let mut font_names = Vec::with_capacity(encoding.maps.len());
    for font_index in 0..encoding.maps.len() {
        ctx.check()?;
        let font_name = loop {
            let name = format!("AriadOCR{suffix}");
            suffix = suffix.checked_add(1).ok_or(PdfError::Malformed)?;
            if !fonts.has(name.as_bytes()) {
                break name;
            }
        };
        let map_bytes = encoding
            .cid_to_gid_map(font_index)
            .ok_or(PdfError::Malformed)?;
        budget.charge(map_bytes.len())?;
        let gid_map = doc.add_object(Stream::new(dictionary! {}, map_bytes));
        let descendant = doc.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "CIDFontType2", "BaseFont" => "GlyphLessFont",
            "CIDSystemInfo" => dictionary! {"Registry" => Object::string_literal("Adobe"), "Ordering" => Object::string_literal("Identity"), "Supplement" => 0},
            "FontDescriptor" => descriptor, "CIDToGIDMap" => gid_map, "DW" => 500,
        });
        let cmap_bytes = encoding
            .to_unicode_cmap(font_index)
            .ok_or(PdfError::Malformed)?;
        budget.charge(cmap_bytes.len())?;
        let cmap = doc.add_object(Stream::new(dictionary! {}, cmap_bytes));
        let font = doc.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "Type0", "BaseFont" => "GlyphLessFont", "Encoding" => "Identity-H",
            "DescendantFonts" => vec![Object::Reference(descendant)], "ToUnicode" => cmap,
        });
        fonts.set(font_name.as_bytes(), Object::Reference(font));
        font_names.push(font_name);
    }
    resources.set("Font", fonts);
    let mut stream = String::from("q\n");
    let map = |x: f64, y: f64| -> (f64, f64) {
        let (u, v) = match page.rotation_cw {
            90 => (y, 1.0 - x),
            180 => (1.0 - x, 1.0 - y),
            270 => (1.0 - y, x),
            _ => (x, y),
        };
        match rotation {
            90 => (
                rect.left + v * rect.width(),
                rect.bottom + u * rect.height(),
            ),
            180 => (
                rect.right - u * rect.width(),
                rect.bottom + v * rect.height(),
            ),
            270 => (rect.right - v * rect.width(), rect.top - u * rect.height()),
            _ => (rect.left + u * rect.width(), rect.top - v * rect.height()),
        }
    };
    let mut word_index = 0usize;
    let mut charged = 0usize;
    for paragraph in &page.paragraphs {
        for line in &paragraph.lines {
            for (position, word) in line.words.iter().enumerate() {
                ctx.check()?;
                let font_index = encoding.word_fonts[word_index];
                word_index += 1;
                let font_name = &font_names[font_index];
                let b = &word.bbox;
                let x0 = f64::from(b.x0) / f64::from(page.width_px);
                let x1 = f64::from(b.x1) / f64::from(page.width_px);
                let y0 = f64::from(b.y0) / f64::from(page.height_px);
                let y1 = f64::from(b.y1) / f64::from(page.height_px);
                let origin = map(x0, y1);
                let right = map(x1, y1);
                let top = map(x0, y0);
                let glyphs = word.text.chars().count() as f64;
                let ax = 2.0 * (right.0 - origin.0) / glyphs;
                let ay = 2.0 * (right.1 - origin.1) / glyphs;
                let bx = top.0 - origin.0;
                let by = top.1 - origin.1;
                stream.push_str("/Span << /ActualText <FEFF");
                for unit in word.text.encode_utf16() {
                    write!(stream, "{unit:04X}").expect("string write");
                }
                if position + 1 < line.words.len() {
                    stream.push_str("0020");
                }
                stream.push_str("> >> BDC\n");
                write!(
                    stream,
                    "BT /{font_name} 1 Tf 100 Tz 3 Tr {ax:.6} {ay:.6} {bx:.6} {by:.6} {:.6} {:.6} Tm <",
                    origin.0, origin.1
                )
                .expect("string write");
                for byte in encoding.encode(font_index, &word.text)? {
                    write!(stream, "{byte:02X}").expect("string write");
                }
                stream.push_str("> Tj ");
                if position + 1 < line.words.len() {
                    // Give the explicit separator the actual inter-word advance so readers
                    // do not infer an additional space from the geometric gap.
                    let next = &line.words[position + 1].bbox;
                    let gap = next.x0.saturating_sub(b.x1) as f64;
                    let width = f64::from(b.x1 - b.x0);
                    let spacing = (100.0 * gap * glyphs / width).clamp(0.01, 10_000.0);
                    write!(stream, "{spacing:.6} Tz <").expect("string write");
                    for byte in encoding.encode(font_index, " ")? {
                        write!(stream, "{byte:02X}").expect("string write");
                    }
                    stream.push_str("> Tj ");
                }
                stream.push_str("ET\nEMC\n");
                budget.charge(stream.len() - charged)?;
                charged = stream.len();
            }
        }
    }
    stream.push_str("Q\n");
    budget.charge(stream.len() - charged)?;
    let new_stream = doc.add_object(Stream::new(dictionary! {}, stream.into_bytes()));
    let old = doc
        .get_object(page_id)
        .and_then(Object::as_dict)
        .map_err(|_| PdfError::Malformed)?
        .get(b"Contents")
        .ok()
        .cloned();
    let mut contents = match old {
        None => Vec::new(),
        Some(value) => match doc.dereference(&value).map_err(|_| PdfError::Malformed)?.1 {
            Object::Array(items) => items.clone(),
            Object::Stream(_) => vec![value],
            _ => return Err(PdfError::Malformed),
        },
    };
    let save = doc.add_object(Stream::new(dictionary! {}, b"q\n".to_vec()));
    let restore = doc.add_object(Stream::new(dictionary! {}, b"Q\n".to_vec()));
    contents.insert(0, Object::Reference(save));
    contents.push(Object::Reference(restore));
    contents.push(Object::Reference(new_stream));
    let page_dict = doc
        .get_object_mut(page_id)
        .and_then(Object::as_dict_mut)
        .map_err(|_| PdfError::Malformed)?;
    page_dict.set("Resources", resources);
    page_dict.set("Contents", Object::Array(contents));
    Ok(())
}

// Refuse content that could consume the protective graphics-state wrapper or leave a
// text object open. This keeps the new layer independent from prior page operations.
fn validate_page_graphics_state(
    pdf: &crate::PdfDoc,
    page_id: lopdf::ObjectId,
    ctx: &crate::ToolCtx,
    budget: &mut crate::document::DecodeBudget,
) -> Result<(), PdfError> {
    use lopdf::Object;
    let doc = pdf.inner();
    let page = doc
        .get_object(page_id)
        .and_then(Object::as_dict)
        .map_err(|_| PdfError::Malformed)?;
    let Ok(contents) = page.get(b"Contents") else {
        return Ok(());
    };
    let resolved = doc
        .dereference(contents)
        .map_err(|_| PdfError::Malformed)?
        .1;
    let pieces: Vec<&Object> = match resolved {
        Object::Array(items) => items.iter().collect(),
        Object::Stream(_) => vec![contents],
        _ => return Err(PdfError::Malformed),
    };
    let mut bytes = Vec::new();
    for piece in pieces {
        ctx.check()?;
        let stream = doc
            .dereference(piece)
            .map_err(|_| PdfError::Malformed)?
            .1
            .as_stream()
            .map_err(|_| PdfError::Malformed)?;
        let decoded = budget.decode(stream)?.ok_or(PdfError::Malformed)?;
        bytes.extend_from_slice(&decoded);
        bytes.push(b'\n');
    }
    let content = crate::scan_text::decode_checked(&bytes, ctx)?;
    let mut depth = 0usize;
    let mut marked_depth = 0usize;
    let mut compatibility_depth = 0usize;
    let mut in_text = false;
    for op in content.operations {
        ctx.check()?;
        match op.operator.as_str() {
            "q" => depth = depth.checked_add(1).ok_or(PdfError::Malformed)?,
            "Q" => depth = depth.checked_sub(1).ok_or(PdfError::Malformed)?,
            "BMC" | "BDC" => {
                marked_depth = marked_depth.checked_add(1).ok_or(PdfError::Malformed)?
            }
            "EMC" => marked_depth = marked_depth.checked_sub(1).ok_or(PdfError::Malformed)?,
            "BX" => {
                compatibility_depth = compatibility_depth
                    .checked_add(1)
                    .ok_or(PdfError::Malformed)?
            }
            "EX" => {
                compatibility_depth = compatibility_depth
                    .checked_sub(1)
                    .ok_or(PdfError::Malformed)?
            }
            "BT" if !in_text => in_text = true,
            "ET" if in_text => in_text = false,
            "BT" | "ET" => return Err(PdfError::Malformed),
            _ => {}
        }
    }
    if depth != 0 || marked_depth != 0 || compatibility_depth != 0 || in_text {
        return Err(PdfError::Malformed);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ariad_core::ocr::{OcrBox, OcrLine, OcrParagraph, OcrWord};
    #[test]
    fn partitions_pages_with_many_distinct_scalars() {
        let bbox = OcrBox {
            x0: 0,
            y0: 0,
            x1: 10,
            y1: 10,
        };
        let words = (0x40000..0x50020)
            .step_by(2)
            .map(|code| OcrWord {
                bbox: bbox.clone(),
                text: [
                    char::from_u32(code).unwrap(),
                    char::from_u32(code + 1).unwrap(),
                ]
                .iter()
                .collect(),
                confidence: 100.0,
            })
            .collect();
        let page = OcrPage {
            width_px: 10,
            height_px: 10,
            dpi: 300,
            rotation_cw: 0,
            languages: vec!["eng".into()],
            paragraphs: vec![OcrParagraph {
                bbox: bbox.clone(),
                lines: vec![OcrLine {
                    bbox,
                    baseline: None,
                    words,
                }],
            }],
        };
        let encoding = TextEncoding::new(&page).unwrap();
        assert_eq!(encoding.maps.len(), 2);
        assert_eq!(encoding.word_fonts.last(), Some(&1));
    }

    #[test]
    fn maps_vietnamese_and_supplementary_characters() {
        let bbox = OcrBox {
            x0: 0,
            y0: 0,
            x1: 10,
            y1: 10,
        };
        let page = OcrPage {
            width_px: 10,
            height_px: 10,
            dpi: 300,
            rotation_cw: 0,
            languages: vec!["vie".into()],
            paragraphs: vec![OcrParagraph {
                bbox: bbox.clone(),
                lines: vec![OcrLine {
                    bbox: bbox.clone(),
                    baseline: None,
                    words: vec![OcrWord {
                        bbox,
                        text: "Việt😀".into(),
                        confidence: 99.0,
                    }],
                }],
            }],
        };
        let encoding = TextEncoding::new(&page).unwrap();
        assert_eq!(encoding.encode(0, "Việt😀").unwrap().len(), 10);
        assert!(
            String::from_utf8(encoding.to_unicode_cmap(0).unwrap())
                .unwrap()
                .contains("D83DDE00")
        );
        assert_eq!(&encoding.cid_to_gid_map(0).unwrap()[..4], &[0, 0, 0, 1]);
        assert!(encoding.encode(0, "x").is_err());
    }
}
