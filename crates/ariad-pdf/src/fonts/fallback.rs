//! Bundled edit-font selection, NFC coverage checks and deterministic Unicode subsets.

use std::{collections::BTreeMap, fmt::Write};

use lopdf::{Document, Object, ObjectId, dictionary};
use ttf_parser::{Face, GlyphId};
use unicode_normalization::UnicodeNormalization;

use super::{FontError, subset_program};
use crate::PdfError;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BundledFontFamily {
    Sans,
    Serif,
    Mono,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BundledFontStyle {
    Regular,
    Bold,
    Italic,
    BoldItalic,
}

/// One of the already-licensed Liberation TrueType faces.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BundledFontChoice {
    pub family: BundledFontFamily,
    pub style: BundledFontStyle,
}

impl BundledFontChoice {
    #[must_use]
    pub const fn postscript_name(&self) -> &'static str {
        use BundledFontFamily::{Mono, Sans, Serif};
        use BundledFontStyle::{Bold, BoldItalic, Italic, Regular};
        match (self.family, self.style) {
            (Sans, Regular) => "LiberationSans",
            (Sans, Bold) => "LiberationSans-Bold",
            (Sans, Italic) => "LiberationSans-Italic",
            (Sans, BoldItalic) => "LiberationSans-BoldItalic",
            (Serif, Regular) => "LiberationSerif",
            (Serif, Bold) => "LiberationSerif-Bold",
            (Serif, Italic) => "LiberationSerif-Italic",
            (Serif, BoldItalic) => "LiberationSerif-BoldItalic",
            (Mono, Regular) => "LiberationMono",
            (Mono, Bold) => "LiberationMono-Bold",
            (Mono, Italic) => "LiberationMono-Italic",
            (Mono, BoldItalic) => "LiberationMono-BoldItalic",
        }
    }

    #[must_use]
    pub const fn bytes(&self) -> &'static [u8] {
        use BundledFontFamily::{Mono, Sans, Serif};
        use BundledFontStyle::{Bold, BoldItalic, Italic, Regular};
        match (self.family, self.style) {
            (Sans, Regular) => include_bytes!("../../assets/liberation/LiberationSans-Regular.ttf"),
            (Sans, Bold) => include_bytes!("../../assets/liberation/LiberationSans-Bold.ttf"),
            (Sans, Italic) => include_bytes!("../../assets/liberation/LiberationSans-Italic.ttf"),
            (Sans, BoldItalic) => {
                include_bytes!("../../assets/liberation/LiberationSans-BoldItalic.ttf")
            }
            (Serif, Regular) => {
                include_bytes!("../../assets/liberation/LiberationSerif-Regular.ttf")
            }
            (Serif, Bold) => include_bytes!("../../assets/liberation/LiberationSerif-Bold.ttf"),
            (Serif, Italic) => include_bytes!("../../assets/liberation/LiberationSerif-Italic.ttf"),
            (Serif, BoldItalic) => {
                include_bytes!("../../assets/liberation/LiberationSerif-BoldItalic.ttf")
            }
            (Mono, Regular) => include_bytes!("../../assets/liberation/LiberationMono-Regular.ttf"),
            (Mono, Bold) => include_bytes!("../../assets/liberation/LiberationMono-Bold.ttf"),
            (Mono, Italic) => include_bytes!("../../assets/liberation/LiberationMono-Italic.ttf"),
            (Mono, BoldItalic) => {
                include_bytes!("../../assets/liberation/LiberationMono-BoldItalic.ttf")
            }
        }
    }
}

/// Match common PDF/system font aliases, retaining the requested weight and slant.
#[must_use]
pub fn match_bundled_font(
    requested_name: Option<&str>,
    bold: bool,
    italic: bool,
) -> BundledFontChoice {
    let name = requested_name.unwrap_or_default();
    // A PDF subset tag describes the subset, not the font family.
    let name = name
        .split_once('+')
        .filter(|(tag, _)| tag.len() == 6 && tag.bytes().all(|b| b.is_ascii_uppercase()))
        .map_or(name, |(_, family)| family);
    let name: String = name
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect();
    let family = if ["courier", "mono", "sourcecode"]
        .iter()
        .any(|s| name.contains(s))
    {
        BundledFontFamily::Mono
    } else if ["helvetica", "arial", "sans"]
        .iter()
        .any(|s| name.contains(s))
    {
        BundledFontFamily::Sans
    } else if ["times", "serif", "libertinus"]
        .iter()
        .any(|s| name.contains(s))
    {
        BundledFontFamily::Serif
    } else {
        BundledFontFamily::Sans
    };
    let style = match (bold, italic) {
        (false, false) => BundledFontStyle::Regular,
        (true, false) => BundledFontStyle::Bold,
        (false, true) => BundledFontStyle::Italic,
        (true, true) => BundledFontStyle::BoldItalic,
    };
    BundledFontChoice { family, style }
}

fn parse_font(bytes: &[u8]) -> Result<Face<'_>, FontError> {
    Face::parse(bytes, 0).map_err(|_| FontError::Program("cannot parse font face".into()))
}

fn missing_character(font: &Face<'_>, text: &str) -> Option<char> {
    text.nfc()
        .filter(|ch| *ch != '\n')
        .find(|ch| font.glyph_index(*ch).is_none_or(|gid| gid.0 == 0))
}

/// Check every NFC-normalized scalar except line feeds; `.notdef` is never coverage.
pub fn check_font_coverage(font_bytes: &[u8], text: &str) -> Result<bool, FontError> {
    Ok(missing_character(&parse_font(font_bytes)?, text).is_none())
}

#[derive(Debug, thiserror::Error)]
pub enum FontFallbackError {
    #[error(transparent)]
    Font(#[from] FontError),
    #[error(
        "glyph_unavailable: character U+{codepoint:04X} is not covered by embedded or fallback fonts"
    )]
    GlyphUnavailable { ch: char, codepoint: u32 },
}

/// A substitution carries the warning fields and the exact face to embed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResolvedEditFont {
    Embedded {
        name: String,
    },
    Substituted {
        from: String,
        to: &'static str,
        choice: BundledFontChoice,
    },
}

/// Retain a covering embedded font; otherwise require complete bundled coverage.
///
/// Callers must pass an embedded font only when it can also re-encode the new text.
/// A malformed embedded program is treated as unavailable and triggers substitution.
pub fn resolve_font_for_text(
    embedded_font: Option<(&str, &[u8])>,
    requested_name: Option<&str>,
    bold: bool,
    italic: bool,
    text: &str,
) -> Result<ResolvedEditFont, FontFallbackError> {
    if let Some((name, bytes)) = embedded_font
        && check_font_coverage(bytes, text).unwrap_or(false)
    {
        return Ok(ResolvedEditFont::Embedded { name: name.into() });
    }
    let embedded_name = embedded_font.map(|(name, _)| name);
    let choice = match_bundled_font(requested_name.or(embedded_name), bold, italic);
    if let Some(ch) = missing_character(&parse_font(choice.bytes())?, text) {
        return Err(FontFallbackError::GlyphUnavailable {
            ch,
            codepoint: u32::from(ch),
        });
    }
    Ok(ResolvedEditFont::Substituted {
        from: embedded_name
            .or(requested_name)
            .unwrap_or("unspecified")
            .into(),
        to: choice.postscript_name(),
        choice,
    })
}

/// Embed a deterministic Identity-H Type0 / CIDFontType2 subset.
///
/// The map encodes **NFC-normalized** text as big-endian, two-byte character codes.
/// Line feeds are layout separators and have no code. Each scalar receives its own
/// CID, including aliases sharing a glyph, so `/ToUnicode` remains reversible.
/// All validation and subsetting finish before adding any objects to the document.
pub fn embed_subset_type0_font(
    doc: &mut Document,
    choice: BundledFontChoice,
    text: &str,
) -> Result<(ObjectId, BTreeMap<char, u16>), PdfError> {
    let font = parse_font(choice.bytes()).map_err(|_| PdfError::Malformed)?;
    let mut glyphs = BTreeMap::new();
    for ch in text.nfc().filter(|ch| *ch != '\n') {
        let gid = font.glyph_index(ch).filter(|g| g.0 != 0).ok_or_else(|| {
            PdfError::InvalidParams(
                FontFallbackError::GlyphUnavailable {
                    ch,
                    codepoint: u32::from(ch),
                }
                .to_string(),
            )
        })?;
        glyphs.insert(ch, gid.0);
        if glyphs.len() >= usize::from(u16::MAX) {
            return Err(PdfError::InvalidParams("too many font characters".into()));
        }
    }
    let (subset, remap) = subset_program(
        choice.bytes(),
        &glyphs.values().copied().collect::<Vec<_>>(),
    )
    .map_err(|_| PdfError::Malformed)?;
    let mut codes = BTreeMap::new();
    // CID 0 stays .notdef. A CIDToGIDMap separates Unicode aliases from subset glyph IDs.
    let mut cid_to_gid = vec![0, 0];
    let mut widths = Vec::new();
    let scale = |value: i16| i64::from(value) * 1000 / i64::from(font.units_per_em());
    let mut hash = 0xcbf29ce484222325_u64;
    for (index, (&ch, &gid)) in glyphs.iter().enumerate() {
        let code = u16::try_from(index + 1).map_err(|_| PdfError::Malformed)?;
        codes.insert(ch, code);
        cid_to_gid.extend_from_slice(&remap.get(&gid).ok_or(PdfError::Malformed)?.to_be_bytes());
        let advance = font
            .glyph_hor_advance(GlyphId(gid))
            .ok_or(PdfError::Malformed)?;
        widths.push(Object::Integer(
            i64::from(advance) * 1000 / i64::from(font.units_per_em()),
        ));
        for byte in u32::from(ch).to_be_bytes() {
            hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
        }
    }
    let mut tag = [b'A'; 6];
    for letter in &mut tag {
        *letter += (hash % 26) as u8;
        hash /= 26;
    }
    let tag = std::str::from_utf8(&tag).map_err(|_| PdfError::Malformed)?;
    let name = format!("{tag}+{}", choice.postscript_name());
    let program = crate::image_to_pdf::build_flate_stream(
        dictionary! { "Length1" => subset.len() as i64 },
        subset,
    )?;
    let cid_map = crate::image_to_pdf::build_flate_stream(dictionary! {}, cid_to_gid)?;
    let unicode =
        crate::image_to_pdf::build_flate_stream(dictionary! {}, to_unicode(&codes).into_bytes())?;
    let program_id = doc.add_object(program);
    let bbox = font.global_bounding_box();
    let mut flags = 32; // Nonsymbolic Unicode font.
    if choice.family == BundledFontFamily::Mono {
        flags |= 1;
    }
    if choice.family == BundledFontFamily::Serif {
        flags |= 2;
    }
    if matches!(
        choice.style,
        BundledFontStyle::Italic | BundledFontStyle::BoldItalic
    ) {
        flags |= 64;
    }
    let descriptor = doc.add_object(dictionary! {
        "Type" => "FontDescriptor", "FontName" => Object::Name(name.as_bytes().to_vec()),
        "Flags" => flags,
        "FontBBox" => vec![scale(bbox.x_min).into(), scale(bbox.y_min).into(), scale(bbox.x_max).into(), scale(bbox.y_max).into()],
        "ItalicAngle" => Object::Real(font.italic_angle()),
        "Ascent" => scale(font.ascender()), "Descent" => scale(font.descender()),
        "CapHeight" => scale(font.capital_height().unwrap_or(font.ascender())),
        "StemV" => if matches!(choice.style, BundledFontStyle::Bold | BundledFontStyle::BoldItalic) { 120 } else { 80 },
        "FontFile2" => program_id,
    });
    let cid_map_id = doc.add_object(cid_map);
    let mut descendant = dictionary! {
        "Type" => "Font", "Subtype" => "CIDFontType2", "BaseFont" => Object::Name(name.as_bytes().to_vec()),
        "CIDSystemInfo" => dictionary! { "Registry" => Object::string_literal("Adobe"), "Ordering" => Object::string_literal("Identity"), "Supplement" => 0 },
        "FontDescriptor" => descriptor, "CIDToGIDMap" => cid_map_id,
        "DW" => 1000,
    };
    if !widths.is_empty() {
        descendant.set("W", vec![1.into(), Object::Array(widths)]);
    }
    let descendant = doc.add_object(descendant);
    let unicode_id = doc.add_object(unicode);
    let id = doc.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type0", "BaseFont" => Object::Name(name.into_bytes()),
        "Encoding" => "Identity-H", "DescendantFonts" => vec![Object::Reference(descendant)], "ToUnicode" => unicode_id,
    });
    Ok((id, codes))
}

fn to_unicode(codes: &BTreeMap<char, u16>) -> String {
    let mut out = String::from(
        "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n/CMapName /AriadShiftEditUnicode def\n/CMapType 2 def\n1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n",
    );
    let entries: Vec<_> = codes.iter().collect();
    for chunk in entries.chunks(100) {
        let _ = writeln!(out, "{} beginbfchar", chunk.len());
        for (ch, code) in chunk {
            let mut utf16 = [0; 2];
            let _ = write!(out, "<{code:04X}> <");
            for unit in ch.encode_utf16(&mut utf16) {
                let _ = write!(out, "{unit:04X}");
            }
            out.push_str(">\n");
        }
        out.push_str("endbfchar\n");
    }
    out.push_str("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
    out
}
