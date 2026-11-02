//! Deterministic embedded subsets of the bundled Libertinus fonts.

use std::collections::{BTreeMap, BTreeSet};

use ariad_core::tools::params::FontFace;
use lopdf::{Document, Object, ObjectId, Stream, dictionary};
use subsetter::GlyphRemapper;
use ttf_parser::{Face, GlyphId, RawFace, Tag};
use unicode_normalization::UnicodeNormalization;

use crate::PdfError;

/// Maximum number of Unicode scalars in a single drawing run.
pub const MAX_TEXT_CHARS: usize = 4096;
/// Maximum number of distinct glyphs in one subset.
pub const MAX_GLYPHS: usize = 16_384;

fn bytes(face: FontFace) -> &'static [u8] {
    match face {
        FontFace::Serif => include_bytes!("../../fonts/LibertinusSerif-Regular.otf"),
        FontFace::SerifBold => include_bytes!("../../fonts/LibertinusSerif-Bold.otf"),
    }
}
fn parsed(face: FontFace) -> Result<Face<'static>, PdfError> {
    Face::parse(bytes(face), 0).map_err(|_| PdfError::Malformed)
}

/// Accumulates all text for a face before embedding it once in a document.
#[derive(Clone, Debug)]
pub struct FontPlan {
    face: FontFace,
    glyphs: BTreeMap<char, u16>,
}

impl FontPlan {
    /// Create an empty subset plan for a bundled font.
    #[must_use]
    pub fn new(face: FontFace) -> Self {
        Self {
            face,
            glyphs: BTreeMap::new(),
        }
    }

    /// Normalize and add a text run, rejecting missing or ambiguous glyphs.
    pub fn add_text(&mut self, text: &str) -> Result<String, PdfError> {
        let normalized: String = text.nfc().collect();
        if normalized.chars().count() > MAX_TEXT_CHARS {
            return Err(PdfError::InvalidParams(
                "text exceeds maximum length".into(),
            ));
        }
        let font = parsed(self.face)?;
        let mut added = self.glyphs.clone();
        let mut owners: BTreeMap<u16, char> = added.iter().map(|(ch, gid)| (*gid, *ch)).collect();
        for ch in normalized.chars().filter(|c| *c != '\n') {
            let gid = font
                .glyph_index(ch)
                .filter(|g| g.0 != 0)
                .ok_or_else(|| {
                    PdfError::InvalidParams(format!(
                        "font has no glyph for U+{:04X}",
                        u32::from(ch)
                    ))
                })?
                .0;
            if owners.get(&gid).is_some_and(|existing| *existing != ch) {
                return Err(PdfError::InvalidParams(
                    "font maps distinct characters to the same glyph".into(),
                ));
            }
            owners.insert(gid, ch);
            added.insert(ch, gid);
            if added.len() > MAX_GLYPHS {
                return Err(PdfError::InvalidParams("too many font glyphs".into()));
            }
        }
        self.glyphs = added;
        Ok(normalized)
    }

    /// Measure one normalized line in points at the requested font size.
    pub fn measure_line(&self, text: &str, size_pt: f64) -> Result<f64, PdfError> {
        if !size_pt.is_finite() || size_pt <= 0.0 {
            return Err(PdfError::InvalidParams("invalid font size".into()));
        }
        let font = parsed(self.face)?;
        let mut advance = 0u64;
        for ch in text.chars() {
            let gid = *self.glyphs.get(&ch).ok_or_else(|| {
                PdfError::InvalidParams("text was not included in font subset".into())
            })?;
            advance += u64::from(
                font.glyph_hor_advance(GlyphId(gid))
                    .ok_or(PdfError::Malformed)?,
            );
        }
        Ok(advance as f64 * size_pt / f64::from(font.units_per_em()))
    }

    /// Ascender and descender in points for the given font size.
    pub fn vertical_metrics(&self, size_pt: f64) -> Result<(f64, f64), PdfError> {
        if !size_pt.is_finite() || size_pt <= 0.0 {
            return Err(PdfError::InvalidParams("invalid font size".into()));
        }
        let font = parsed(self.face)?;
        let scale = size_pt / f64::from(font.units_per_em());
        Ok((
            f64::from(font.ascender()) * scale,
            f64::from(font.descender()) * scale,
        ))
    }

    /// Embed a subset and return its Type0 font object and character-to-CID map.
    pub fn embed(&self, doc: &mut Document) -> Result<EmbeddedFont, PdfError> {
        let font = parsed(self.face)?;
        let glyphs: BTreeSet<u16> = self
            .glyphs
            .values()
            .copied()
            .chain(std::iter::once(0))
            .collect();
        let remap = GlyphRemapper::new_from_glyphs_sorted(&glyphs.into_iter().collect::<Vec<_>>());
        let subset =
            subsetter::subset(bytes(self.face), 0, &remap).map_err(|_| PdfError::Malformed)?;
        let raw = RawFace::parse(&subset, 0).map_err(|_| PdfError::Malformed)?;
        let cff = raw
            .table(Tag::from_bytes(b"CFF "))
            .ok_or(PdfError::Malformed)?;
        let cids: BTreeMap<char, u16> = self
            .glyphs
            .iter()
            .map(|(ch, gid)| {
                remap
                    .get(*gid)
                    .map(|cid| (*ch, cid))
                    .ok_or(PdfError::Malformed)
            })
            .collect::<Result<_, _>>()?;
        let mut hash = 0xcbf29ce484222325_u64;
        for (ch, gid) in &self.glyphs {
            for byte in u32::from(*ch)
                .to_be_bytes()
                .into_iter()
                .chain(gid.to_be_bytes())
            {
                hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
            }
        }
        let mut tag = [b'A'; 6];
        for ch in &mut tag {
            *ch += (hash % 26) as u8;
            hash /= 26;
        }
        let tag = std::str::from_utf8(&tag).map_err(|_| PdfError::Malformed)?;
        let name = format!(
            "{tag}+{}",
            match self.face {
                FontFace::Serif => "LibertinusSerif-Regular",
                FontFace::SerifBold => "LibertinusSerif-Bold",
            }
        );
        let mut program = Stream::new(dictionary! { "Subtype" => "CIDFontType0C" }, cff.to_vec());
        let _ = program.compress();
        let program_id = doc.add_object(program);
        let units = i64::from(font.units_per_em());
        let scale = |v: i16| i64::from(v) * 1000 / units;
        let bbox = font.global_bounding_box();
        let descriptor = doc.add_object(dictionary! {
            "Type" => "FontDescriptor", "FontName" => Object::Name(name.as_bytes().to_vec()),
            "Flags" => 4, "FontBBox" => vec![scale(bbox.x_min).into(), scale(bbox.y_min).into(), scale(bbox.x_max).into(), scale(bbox.y_max).into()],
            "ItalicAngle" => 0, "Ascent" => scale(font.ascender()), "Descent" => scale(font.descender()),
            "CapHeight" => scale(font.capital_height().unwrap_or(font.ascender())), "StemV" => 80,
            "FontFile3" => program_id,
        });
        let mut widths = BTreeMap::new();
        for (ch, cid) in &cids {
            let gid = self.glyphs[ch];
            let advance = i64::from(
                font.glyph_hor_advance(GlyphId(gid))
                    .ok_or(PdfError::Malformed)?,
            ) * 1000
                / units;
            widths.insert(*cid, advance);
        }
        let w: Vec<Object> = widths
            .into_iter()
            .flat_map(|(cid, width)| {
                [
                    Object::Integer(i64::from(cid)),
                    Object::Array(vec![Object::Integer(width)]),
                ]
            })
            .collect();
        let descendant = doc.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "CIDFontType0", "BaseFont" => Object::Name(name.as_bytes().to_vec()),
            "CIDSystemInfo" => dictionary! { "Registry" => Object::string_literal("Adobe"), "Ordering" => Object::string_literal("Identity"), "Supplement" => 0 },
            "FontDescriptor" => descriptor, "W" => w,
        });
        let unicode = make_to_unicode(&cids);
        let mut unicode_stream = Stream::new(dictionary! {}, unicode.into_bytes());
        let _ = unicode_stream.compress();
        let unicode_id = doc.add_object(unicode_stream);
        let id = doc.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "Type0", "BaseFont" => Object::Name(name.into_bytes()),
            "Encoding" => "Identity-H", "DescendantFonts" => vec![Object::Reference(descendant)], "ToUnicode" => unicode_id,
        });
        Ok(EmbeddedFont {
            id,
            cids,
            subset_size: subset.len(),
        })
    }
}

/// Embedded Type0 font with a map for encoding previously collected text.
#[derive(Clone, Debug)]
pub struct EmbeddedFont {
    /// Object ID of the Type0 font dictionary.
    pub id: ObjectId,
    cids: BTreeMap<char, u16>,
    /// Size of the OpenType subset before extracting its CFF program.
    pub subset_size: usize,
}
impl EmbeddedFont {
    /// Encode a collected line as two-byte CIDs.
    pub fn encode(&self, text: &str) -> Result<Vec<u8>, PdfError> {
        let mut output = Vec::with_capacity(text.len().saturating_mul(2));
        for ch in text.chars() {
            let cid = self.cids.get(&ch).ok_or_else(|| {
                PdfError::InvalidParams("text was not included in font subset".into())
            })?;
            output.extend_from_slice(&cid.to_be_bytes());
        }
        Ok(output)
    }
}

fn make_to_unicode(cids: &BTreeMap<char, u16>) -> String {
    use std::fmt::Write;
    let mut out = String::from(
        "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n/CMapName /AriadShiftUnicode def\n/CMapType 2 def\n1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n",
    );
    let entries: Vec<_> = cids.iter().collect();
    for chunk in entries.chunks(100) {
        let _ = writeln!(out, "{} beginbfchar", chunk.len());
        for (ch, cid) in chunk {
            let mut utf16 = [0u16; 2];
            let encoded = ch.encode_utf16(&mut utf16);
            let _ = write!(out, "<{cid:04X}> <");
            for unit in encoded {
                let _ = write!(out, "{unit:04X}");
            }
            out.push_str(">\n");
        }
        out.push_str("endbfchar\n");
    }
    out.push_str("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn subset_is_smaller_and_unicode_is_preserved() {
        for face in [FontFace::Serif, FontFace::SerifBold] {
            let mut plan = FontPlan::new(face);
            let text = plan
                .add_text("Bản nháp — Tài liệu mật: Đường Hưng Đạo, số 1, ₫")
                .unwrap();
            let mut doc = Document::with_version("1.7");
            let subset = plan.embed(&mut doc).unwrap();
            assert!(subset.subset_size < bytes(face).len());
            assert_eq!(
                subset.encode(&text).unwrap().len(),
                text.chars().count() * 2
            );
            assert!(plan.measure_line(&text, 12.0).unwrap() > 0.0);
            assert!(plan.add_text("中文").is_err());
        }
    }
}
