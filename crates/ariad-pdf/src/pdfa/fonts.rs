//! Font embedding and standard-14 font replacement for PDF/A compliance.

use lopdf::{Dictionary, Document, Object, ObjectId, Stream, dictionary};

const SANS_REGULAR: &[u8] = include_bytes!("../../assets/liberation/LiberationSans-Regular.ttf");
const SANS_BOLD: &[u8] = include_bytes!("../../assets/liberation/LiberationSans-Bold.ttf");
const SANS_ITALIC: &[u8] = include_bytes!("../../assets/liberation/LiberationSans-Italic.ttf");
const SANS_BOLD_ITALIC: &[u8] =
    include_bytes!("../../assets/liberation/LiberationSans-BoldItalic.ttf");
const SERIF_REGULAR: &[u8] = include_bytes!("../../assets/liberation/LiberationSerif-Regular.ttf");
const SERIF_BOLD: &[u8] = include_bytes!("../../assets/liberation/LiberationSerif-Bold.ttf");
const SERIF_ITALIC: &[u8] = include_bytes!("../../assets/liberation/LiberationSerif-Italic.ttf");
const SERIF_BOLD_ITALIC: &[u8] =
    include_bytes!("../../assets/liberation/LiberationSerif-BoldItalic.ttf");
const MONO_REGULAR: &[u8] = include_bytes!("../../assets/liberation/LiberationMono-Regular.ttf");
const MONO_BOLD: &[u8] = include_bytes!("../../assets/liberation/LiberationMono-Bold.ttf");
const MONO_ITALIC: &[u8] = include_bytes!("../../assets/liberation/LiberationMono-Italic.ttf");
const MONO_BOLD_ITALIC: &[u8] =
    include_bytes!("../../assets/liberation/LiberationMono-BoldItalic.ttf");

use crate::error::PdfError;

fn replacement(name: &[u8]) -> Option<&'static [u8]> {
    Some(match name {
        b"Helvetica" => SANS_REGULAR,
        b"Helvetica-Bold" => SANS_BOLD,
        b"Helvetica-Oblique" => SANS_ITALIC,
        b"Helvetica-BoldOblique" => SANS_BOLD_ITALIC,
        b"Times-Roman" => SERIF_REGULAR,
        b"Times-Bold" => SERIF_BOLD,
        b"Times-Italic" => SERIF_ITALIC,
        b"Times-BoldItalic" => SERIF_BOLD_ITALIC,
        b"Courier" => MONO_REGULAR,
        b"Courier-Bold" => MONO_BOLD,
        b"Courier-Oblique" => MONO_ITALIC,
        b"Courier-BoldOblique" => MONO_BOLD_ITALIC,
        _ => return None,
    })
}

/// Embed only known metric-compatible fonts. Unknown Unicode mappings are never invented.
pub fn ensure_fonts_embedded(doc: &mut Document, require_to_unicode: bool) -> Result<(), PdfError> {
    // Resource dictionaries may contain direct font dictionaries. Normalize them first.
    let mut promoted = Vec::new();
    let mut next_id = doc.max_id;
    for object in doc.objects.values_mut() {
        promote_fonts(object, &mut next_id, &mut promoted);
    }
    doc.max_id = next_id;
    doc.objects.extend(promoted);
    let ids: Vec<_> = doc
        .objects
        .iter()
        .filter_map(|(id, obj)| {
            obj.as_dict()
                .ok()
                .filter(|dict| dict.get(b"Type").and_then(Object::as_name).ok() == Some(b"Font"))
                .map(|_| *id)
        })
        .collect();
    for id in ids {
        process_font(doc, id, require_to_unicode)?;
    }
    Ok(())
}

fn promote_fonts(object: &mut Object, next_id: &mut u32, promoted: &mut Vec<(ObjectId, Object)>) {
    let children: Vec<&mut Object> = match object {
        Object::Dictionary(dict) => dict.iter_mut().map(|(_, value)| value).collect(),
        Object::Stream(stream) => stream.dict.iter_mut().map(|(_, value)| value).collect(),
        Object::Array(array) => array.iter_mut().collect(),
        _ => Vec::new(),
    };
    for child in children {
        promote_fonts(child, next_id, promoted);
        if child
            .as_dict()
            .ok()
            .is_some_and(|dict| dict.get(b"Type").and_then(Object::as_name).ok() == Some(b"Font"))
        {
            *next_id += 1;
            let id = (*next_id, 0);
            promoted.push((id, std::mem::replace(child, Object::Reference(id))));
        }
    }
}

fn process_font(
    doc: &mut Document,
    id: ObjectId,
    require_to_unicode: bool,
) -> Result<(), PdfError> {
    let font = doc
        .get_dictionary(id)
        .map_err(|_| PdfError::Malformed)?
        .clone();
    let subtype = font
        .get(b"Subtype")
        .and_then(Object::as_name)
        .unwrap_or_default();
    let descriptor_font = if subtype == b"Type0" {
        font.get_deref(b"DescendantFonts", doc)
            .and_then(Object::as_array)
            .ok()
            .and_then(|fonts| fonts.first())
            .and_then(|font| doc.dereference(font).ok().map(|(_, font)| font))
            .and_then(|font| font.as_dict().ok())
            .ok_or(PdfError::Malformed)?
    } else {
        &font
    };
    let embedded = descriptor_font
        .get_deref(b"FontDescriptor", doc)
        .and_then(Object::as_dict)
        .ok()
        .is_some_and(|desc| {
            [b"FontFile".as_slice(), b"FontFile2", b"FontFile3"]
                .iter()
                .any(|key| desc.get_deref(key, doc).and_then(Object::as_stream).is_ok())
        });
    if embedded {
        if require_to_unicode
            && !matches!(subtype, b"CIDFontType0" | b"CIDFontType2")
            && font
                .get_deref(b"ToUnicode", doc)
                .and_then(Object::as_stream)
                .is_err()
        {
            return Err(PdfError::InvalidParams("PDF/A-2u requires an existing /ToUnicode mapping for embedded fonts; cannot be conservatively fixed".into()));
        }
        return Ok(());
    }
    let name = font
        .get(b"BaseFont")
        .and_then(Object::as_name)
        .unwrap_or_default();
    let bytes = replacement(name)
        .filter(|_| matches!(subtype, b"Type1" | b"TrueType"))
        .ok_or_else(|| {
            PdfError::InvalidParams(format!(
                "unembedded font '{}' cannot be conservatively fixed",
                String::from_utf8_lossy(name)
            ))
        })?;
    // Only named, unambiguous base encodings are eligible. Custom Differences need a separate review.
    let encoding_name = match font.get_deref(b"Encoding", doc) {
        Ok(Object::Name(name))
            if matches!(
                name.as_slice(),
                b"WinAnsiEncoding" | b"MacRomanEncoding" | b"StandardEncoding"
            ) =>
        {
            name.as_slice()
        }
        Err(_) if !font.has(b"Encoding") => b"StandardEncoding",
        _ => {
            return Err(PdfError::InvalidParams(
                "font encoding cannot be conservatively fixed".into(),
            ));
        }
    };
    let mut encoding_font = font.clone();
    encoding_font.set("Encoding", Object::Name(encoding_name.to_vec()));
    let encoding = encoding_font
        .get_font_encoding(doc)
        .map_err(|_| PdfError::Malformed)?;
    let face = ttf_parser::Face::parse(bytes, 0).map_err(|_| PdfError::Malformed)?;
    let units = f64::from(face.units_per_em());
    let scale = |value: i16| (f64::from(value) * 1000.0 / units).round() as i64;
    let postscript_name = face
        .names()
        .into_iter()
        .filter(|name| name.name_id == ttf_parser::name_id::POST_SCRIPT_NAME)
        .find_map(|name| name.to_string())
        .ok_or(PdfError::Malformed)?;
    let mut widths = Vec::new();
    let mut differences = vec![Object::Integer(0)];
    let mut mappings = Vec::new();
    for code in 0..=255_u8 {
        let text = encoding
            .bytes_to_string(&[code])
            .map_err(|_| PdfError::Malformed)?;
        let mut chars = text.chars();
        let character = chars.next().filter(|_| chars.next().is_none());
        let glyph = character.and_then(|ch| face.glyph_index(ch));
        let width = glyph.and_then(|id| face.glyph_hor_advance(id)).unwrap_or(0);
        widths.push(Object::Integer(
            (f64::from(width) * 1000.0 / units).round() as i64
        ));
        let glyph_name = glyph
            .and_then(|id| face.glyph_name(id))
            .unwrap_or(".notdef");
        differences.push(Object::Name(glyph_name.as_bytes().to_vec()));
        if let Some(ch) = character.filter(|_| glyph.is_some()) {
            mappings.push((code, ch));
        }
    }
    let (first_char, last_char, widths) = if font.has(b"Widths") {
        let invalid_metrics = || {
            PdfError::InvalidParams(
                "font widths cannot be conservatively replaced with Liberation metrics".into(),
            )
        };
        let first = font
            .get(b"FirstChar")
            .and_then(Object::as_i64)
            .map_err(|_| invalid_metrics())?;
        let last = font
            .get(b"LastChar")
            .and_then(Object::as_i64)
            .map_err(|_| invalid_metrics())?;
        let original = font
            .get_deref(b"Widths", doc)
            .and_then(Object::as_array)
            .map_err(|_| invalid_metrics())?;
        if !(0..=255).contains(&first)
            || !(first..=255).contains(&last)
            || original.len() != (last - first + 1) as usize
        {
            return Err(invalid_metrics());
        }
        for (index, original_width) in original.iter().enumerate() {
            let original_width = original_width.as_float().map_err(|_| invalid_metrics())?;
            let replacement_width = widths[first as usize + index]
                .as_float()
                .map_err(|_| invalid_metrics())?;
            if !original_width.is_finite() || (original_width - replacement_width).abs() > 1.0 {
                return Err(invalid_metrics());
            }
        }
        // Keep the original advances, including fractional values, so text positions do not shift.
        (first, last, original.clone())
    } else {
        (0, 255, widths)
    };
    let file = doc.add_object(Stream::new(
        lopdf::dictionary! { "Length1" => bytes.len() as i64 },
        bytes.to_vec(),
    ));
    let bbox = face.global_bounding_box();
    let descriptor = doc.add_object(lopdf::dictionary! {
        "Type" => "FontDescriptor", "FontName" => Object::Name(postscript_name.as_bytes().to_vec()),
        "Flags" => 32_i64 + if face.is_monospaced() { 1 } else { 0 } + if face.is_italic() { 64 } else { 0 },
        "FontBBox" => vec![scale(bbox.x_min).into(), scale(bbox.y_min).into(), scale(bbox.x_max).into(), scale(bbox.y_max).into()],
        "ItalicAngle" => Object::Real(face.italic_angle()), "Ascent" => scale(face.ascender()),
        "Descent" => scale(face.descender()), "CapHeight" => scale(face.capital_height().unwrap_or(face.ascender())),
        "StemV" => 80_i64, "FontFile2" => file,
    });
    let cmap = if require_to_unicode && !font.has(b"ToUnicode") {
        Some(doc.add_object(unicode_cmap(&mappings)))
    } else {
        None
    };
    let font = doc
        .get_object_mut(id)
        .and_then(Object::as_dict_mut)
        .map_err(|_| PdfError::Malformed)?;
    font.set("Subtype", "TrueType");
    font.set("BaseFont", Object::Name(postscript_name.into_bytes()));
    font.set("FontDescriptor", descriptor);
    font.set("FirstChar", first_char);
    font.set("LastChar", last_char);
    font.set("Widths", widths);
    font.set(
        "Encoding",
        lopdf::dictionary! { "Type" => "Encoding", "BaseEncoding" => "WinAnsiEncoding", "Differences" => differences },
    );
    if let Some(cmap) = cmap {
        font.set("ToUnicode", cmap);
    }
    Ok(())
}

fn unicode_cmap(mappings: &[(u8, char)]) -> Stream {
    use std::fmt::Write;
    let mut cmap = String::from(
        "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n/CMapName /Liberation-ToUnicode def\n/CMapType 2 def\n1 begincodespacerange\n<00> <FF>\nendcodespacerange\n",
    );
    for chunk in mappings.chunks(100) {
        writeln!(cmap, "{} beginbfchar", chunk.len()).unwrap();
        for (code, ch) in chunk {
            let mut utf16 = [0; 2];
            write!(cmap, "<{code:02X}> <").unwrap();
            for unit in ch.encode_utf16(&mut utf16) {
                write!(cmap, "{unit:04X}").unwrap();
            }
            cmap.push_str(">\n");
        }
        cmap.push_str("endbfchar\n");
    }
    cmap.push_str("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
    Stream::new(Dictionary::new(), cmap.into_bytes())
}
