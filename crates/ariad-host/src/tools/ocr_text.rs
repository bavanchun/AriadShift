//! Text and hOCR sidecars from the validated engine-neutral OCR model.
use ariad_core::ocr::OcrPage;
use std::io::{self, Write};

/// Write recognized paragraphs in reading order.
pub(super) fn text_page(page: &OcrPage, out: &mut impl Write) -> io::Result<()> {
    for paragraph in &page.paragraphs {
        for line in &paragraph.lines {
            for (index, word) in line.words.iter().enumerate() {
                if index != 0 {
                    out.write_all(b" ")?;
                }
                out.write_all(word.text.as_bytes())?;
            }
            out.write_all(b"\n")?;
        }
        out.write_all(b"\n")?;
    }
    Ok(())
}

fn escape(value: &str, out: &mut impl Write) -> io::Result<()> {
    for ch in value.chars() {
        if !matches!(ch, '\t' | '\n' | '\r' | '\u{20}'..='\u{d7ff}' | '\u{e000}'..='\u{fffd}' | '\u{10000}'..='\u{10ffff}')
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "OCR text is not valid XML",
            ));
        }
        match ch {
            '&' => out.write_all(b"&amp;")?,
            '<' => out.write_all(b"&lt;")?,
            '>' => out.write_all(b"&gt;")?,
            '"' => out.write_all(b"&quot;")?,
            '\'' => out.write_all(b"&apos;")?,
            _ => {
                let mut bytes = [0; 4];
                out.write_all(ch.encode_utf8(&mut bytes).as_bytes())?;
            }
        }
    }
    Ok(())
}

/// Write a page fragment with top-left pixel coordinates and word confidences.
pub(super) fn hocr_page(page: &OcrPage, number: usize, out: &mut impl Write) -> io::Result<()> {
    writeln!(
        out,
        "<div class=\"ocr_page\" id=\"page_{number}\" title=\"bbox 0 0 {} {}\">",
        page.width_px, page.height_px
    )?;
    for (p, paragraph) in page.paragraphs.iter().enumerate() {
        let b = &paragraph.bbox;
        writeln!(
            out,
            "<p class=\"ocr_par\" id=\"par_{number}_{}\" title=\"bbox {} {} {} {}\">",
            p + 1,
            b.x0,
            b.y0,
            b.x1,
            b.y1
        )?;
        for (l, line) in paragraph.lines.iter().enumerate() {
            let b = &line.bbox;
            write!(
                out,
                "<span class=\"ocr_line\" id=\"line_{number}_{}_{}\" title=\"bbox {} {} {} {}",
                p + 1,
                l + 1,
                b.x0,
                b.y0,
                b.x1,
                b.y1
            )?;
            if let Some(base) = &line.baseline {
                write!(out, "; baseline {} {}", base.slope, base.offset_px)?;
            }
            writeln!(out, "\">")?;
            for (w, word) in line.words.iter().enumerate() {
                let b = &word.bbox;
                write!(
                    out,
                    "<span class=\"ocrx_word\" id=\"word_{number}_{}_{}_{}\" title=\"bbox {} {} {} {}; x_wconf {}\">",
                    p + 1,
                    l + 1,
                    w + 1,
                    b.x0,
                    b.y0,
                    b.x1,
                    b.y1,
                    word.confidence
                )?;
                escape(&word.text, out)?;
                writeln!(out, "</span>")?;
            }
            writeln!(out, "</span>")?;
        }
        writeln!(out, "</p>")?;
    }
    writeln!(out, "</div>")
}

#[cfg(test)]
mod tests {
    use super::*;
    use ariad_core::ocr::{OcrBox, OcrLine, OcrParagraph, OcrWord};

    #[test]
    fn preserves_unicode_and_escapes_markup() {
        let bounds = OcrBox {
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
                bbox: bounds.clone(),
                lines: vec![OcrLine {
                    bbox: bounds.clone(),
                    baseline: None,
                    words: vec![OcrWord {
                        bbox: bounds,
                        text: "Việt<&".into(),
                        confidence: 99.0,
                    }],
                }],
            }],
        };
        let mut text = Vec::new();
        text_page(&page, &mut text).unwrap();
        assert_eq!(String::from_utf8(text).unwrap(), "Việt<&\n\n");
        let mut hocr = Vec::new();
        hocr_page(&page, 1, &mut hocr).unwrap();
        let actual = String::from_utf8(hocr).unwrap();
        assert!(actual.contains("Việt&lt;&amp;"));
        assert!(actual.contains("bbox 0 0 10 10; x_wconf 99"));
    }
}
