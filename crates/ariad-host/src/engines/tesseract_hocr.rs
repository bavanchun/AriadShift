//! Bounded hOCR parsing for the Tesseract adapter.
use ariad_core::ocr::{
    MAX_OCR_JSON_BYTES, MAX_OCR_WORD_BYTES, MAX_OCR_WORDS_PER_PAGE, OcrBaseline, OcrBox, OcrLine,
    OcrPage, OcrParagraph, OcrWord,
};
use quick_xml::{
    Reader,
    events::{BytesStart, Event},
};
use unicode_normalization::UnicodeNormalization;

#[derive(Debug)]
pub(crate) struct InvalidHocr;

fn attributes(event: &BytesStart<'_>) -> Result<(String, String), InvalidHocr> {
    let mut class = String::new();
    let mut title = String::new();
    for attribute in event.attributes() {
        let attribute = attribute.map_err(|_| InvalidHocr)?;
        let value = attribute.value.as_ref();
        match attribute.key.as_ref() {
            "class" => class = value.to_owned(),
            "title" => title = value.to_owned(),
            _ => {}
        }
    }
    Ok((class, title))
}

fn field<'a>(title: &'a str, name: &str) -> Option<&'a str> {
    title.split(';').map(str::trim).find_map(|part| {
        part.strip_prefix(name)
            .filter(|rest| rest.starts_with(' '))
            .map(str::trim)
    })
}

fn bbox(title: &str) -> Result<OcrBox, InvalidHocr> {
    let mut values = field(title, "bbox").ok_or(InvalidHocr)?.split_whitespace();
    let mut next = || {
        values
            .next()
            .ok_or(InvalidHocr)?
            .parse::<u32>()
            .map_err(|_| InvalidHocr)
    };
    let value = OcrBox {
        x0: next()?,
        y0: next()?,
        x1: next()?,
        y1: next()?,
    };
    if values.next().is_some() {
        return Err(InvalidHocr);
    }
    Ok(value)
}

fn baseline(title: &str) -> Result<Option<OcrBaseline>, InvalidHocr> {
    let Some(value) = field(title, "baseline") else {
        return Ok(None);
    };
    let mut values = value.split_whitespace();
    let slope = values
        .next()
        .ok_or(InvalidHocr)?
        .parse::<f32>()
        .map_err(|_| InvalidHocr)?;
    let offset_px = values
        .next()
        .ok_or(InvalidHocr)?
        .parse::<f32>()
        .map_err(|_| InvalidHocr)?;
    if values.next().is_some() || !slope.is_finite() || !offset_px.is_finite() {
        return Err(InvalidHocr);
    }
    Ok(Some(OcrBaseline { slope, offset_px }))
}

/// Parse one hOCR page; all coordinates are validated against the input image.
pub(crate) fn parse(
    xml: &[u8],
    width_px: u32,
    height_px: u32,
    dpi: u32,
    languages: Vec<String>,
) -> Result<OcrPage, InvalidHocr> {
    if xml.len() > MAX_OCR_JSON_BYTES {
        return Err(InvalidHocr);
    }
    let mut reader = Reader::from_reader(xml);
    let mut page = OcrPage {
        width_px,
        height_px,
        dpi,
        rotation_cw: 0,
        languages,
        paragraphs: Vec::new(),
    };
    let mut paragraph: Option<(usize, OcrParagraph)> = None;
    let mut line: Option<(usize, OcrLine)> = None;
    let mut word: Option<(usize, OcrWord)> = None;
    let mut page_depth = None;
    let mut seen_page = false;
    let mut depth = 0usize;
    let mut words = 0usize;
    loop {
        match reader.read_event().map_err(|_| InvalidHocr)? {
            Event::Start(start) => {
                depth = depth.checked_add(1).ok_or(InvalidHocr)?;
                if depth > 64 {
                    return Err(InvalidHocr);
                }
                let (class, title) = attributes(&start)?;
                let classes: Vec<_> = class.split_whitespace().collect();
                if classes.contains(&"ocr_page") {
                    if seen_page
                        || bbox(&title)?
                            != (OcrBox {
                                x0: 0,
                                y0: 0,
                                x1: width_px,
                                y1: height_px,
                            })
                    {
                        return Err(InvalidHocr);
                    }
                    seen_page = true;
                    page_depth = Some(depth);
                } else if classes.contains(&"ocr_par") {
                    if page_depth.is_none()
                        || paragraph.is_some()
                        || line.is_some()
                        || word.is_some()
                    {
                        return Err(InvalidHocr);
                    }
                    paragraph = Some((
                        depth,
                        OcrParagraph {
                            bbox: bbox(&title)?,
                            lines: Vec::new(),
                        },
                    ));
                } else if [
                    "ocr_line",
                    "ocrx_line",
                    "ocr_header",
                    "ocr_textfloat",
                    "ocr_caption",
                ]
                .iter()
                .any(|name| classes.contains(name))
                {
                    if paragraph.is_none() || line.is_some() || word.is_some() {
                        return Err(InvalidHocr);
                    }
                    line = Some((
                        depth,
                        OcrLine {
                            bbox: bbox(&title)?,
                            baseline: baseline(&title)?,
                            words: Vec::new(),
                        },
                    ));
                } else if classes.contains(&"ocrx_word") {
                    if line.is_none() || word.is_some() {
                        return Err(InvalidHocr);
                    }
                    words += 1;
                    if words > MAX_OCR_WORDS_PER_PAGE {
                        return Err(InvalidHocr);
                    }
                    let confidence = field(&title, "x_wconf")
                        .ok_or(InvalidHocr)?
                        .parse::<f32>()
                        .map_err(|_| InvalidHocr)?;
                    word = Some((
                        depth,
                        OcrWord {
                            bbox: bbox(&title)?,
                            text: String::new(),
                            confidence,
                        },
                    ));
                }
            }
            Event::Text(text) => {
                if let Some((_, current)) = word.as_mut() {
                    let raw = text.as_ref();
                    current
                        .text
                        .push_str(&quick_xml::escape::unescape(raw).map_err(|_| InvalidHocr)?);
                    if current.text.len() > MAX_OCR_WORD_BYTES * 4 {
                        return Err(InvalidHocr);
                    }
                }
            }
            Event::GeneralRef(reference) => {
                if let Some((_, current)) = word.as_mut() {
                    let name = reference.as_ref();
                    let escaped = format!("&{name};");
                    current
                        .text
                        .push_str(&quick_xml::escape::unescape(&escaped).map_err(|_| InvalidHocr)?);
                    if current.text.len() > MAX_OCR_WORD_BYTES * 4 {
                        return Err(InvalidHocr);
                    }
                }
            }
            Event::End(_) => {
                if depth == 0 {
                    return Err(InvalidHocr);
                }
                if word.as_ref().is_some_and(|(d, _)| *d == depth) {
                    let (_, mut current) = word.take().ok_or(InvalidHocr)?;
                    current.text = current.text.trim().nfc().collect();
                    if !current.text.is_empty() {
                        line.as_mut().ok_or(InvalidHocr)?.1.words.push(current);
                    }
                }
                if line.as_ref().is_some_and(|(d, _)| *d == depth) {
                    let (_, current) = line.take().ok_or(InvalidHocr)?;
                    if !current.words.is_empty() {
                        paragraph.as_mut().ok_or(InvalidHocr)?.1.lines.push(current);
                    }
                }
                if paragraph.as_ref().is_some_and(|(d, _)| *d == depth) {
                    let (_, current) = paragraph.take().ok_or(InvalidHocr)?;
                    if !current.lines.is_empty() {
                        page.paragraphs.push(current);
                    }
                }
                if page_depth == Some(depth) {
                    page_depth = None;
                }
                depth -= 1;
            }
            Event::Eof => break,
            Event::CData(_) => return Err(InvalidHocr),
            _ => {}
        }
    }
    if !seen_page
        || depth != 0
        || page_depth.is_some()
        || paragraph.is_some()
        || line.is_some()
        || word.is_some()
    {
        return Err(InvalidHocr);
    }
    page.validate().map_err(|_| InvalidHocr)?;
    Ok(page)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_vietnamese_entities_and_geometry() {
        let xml = "<html><div class='ocr_page' title='bbox 0 0 100 100'><p class='ocr_par' title='bbox 1 2 95 45'><span class='ocr_line' title='bbox 1 2 95 45; baseline 0.02 -3'><span class='ocrx_word' title='bbox 1 2 50 40; x_wconf 87'>Vie\u{323}̂t &amp; Nam</span></span></p></div></html>";
        let page = parse(xml.as_bytes(), 100, 100, 300, vec!["vie".into()]).unwrap();
        assert_eq!(page.paragraphs[0].lines[0].words[0].text, "Việt & Nam");
        assert_eq!(
            page.paragraphs[0].lines[0]
                .baseline
                .as_ref()
                .unwrap()
                .offset_px,
            -3.0
        );
    }
    #[test]
    fn rejects_bad_coordinates_and_unbounded_nesting() {
        let invalid = b"<div class='ocr_page' title='bbox 0 0 100 100'><p class='ocr_par' title='bbox 0 0 101 5'><span class='ocr_line' title='bbox 0 0 2 2'><span class='ocrx_word' title='bbox 0 0 2 2; x_wconf 90'>a</span></span></p></div>";
        assert!(parse(invalid, 100, 100, 300, vec![]).is_err());
        let nested = format!("{}{}", "<a>".repeat(65), "</a>".repeat(65));
        assert!(parse(nested.as_bytes(), 100, 100, 300, vec![]).is_err());
    }
}
