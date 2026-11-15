//! Minimal OpenType face reader over `read-fonts`.
//!
//! It exposes the cmap lookup, metrics and naming data that font embedding
//! needs. Lookups follow the conventions the PDF writers were built against:
//! only Unicode cmap subtables are consulted, in table order, and vertical
//! metrics prefer OS/2 typographic values only when `USE_TYPO_METRICS` is set.

use read_fonts::{
    FontRef, TableProvider,
    tables::{
        cmap::{CmapSubtable, PlatformId},
        head::Head,
        hhea::Hhea,
        hmtx::Hmtx,
        os2::{Os2, SelectionFlags},
        post::Post,
    },
    types::{GlyphId, GlyphId16, NameId},
};

#[cfg(feature = "codecs")]
use read_fonts::types::Tag;

/// Font bounding box in font units, taken from the `head` table.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct BoundingBox {
    pub x_min: i16,
    pub y_min: i16,
    pub x_max: i16,
    pub y_max: i16,
}

/// A parsed face with its required tables resolved up front.
#[derive(Clone)]
pub(crate) struct Face<'a> {
    font: FontRef<'a>,
    head: Head<'a>,
    hhea: Hhea<'a>,
    num_glyphs: u16,
    hmtx: Option<Hmtx<'a>>,
    os2: Option<Os2<'a>>,
    post: Option<Post<'a>>,
}

impl<'a> Face<'a> {
    /// Parse the first face of a font file or collection.
    ///
    /// Fails when `head`, `hhea` or `maxp` is missing or malformed, or when
    /// `unitsPerEm` lies outside 16..=16384. Callers divide by `unitsPerEm`,
    /// so a zero or absurd value must never produce a face.
    pub(crate) fn parse(bytes: &'a [u8]) -> Option<Self> {
        let font = FontRef::from_index(bytes, 0).ok()?;
        let head = font.head().ok()?;
        if !(16..=16384).contains(&head.units_per_em()) {
            return None;
        }
        let hhea = font.hhea().ok()?;
        let num_glyphs = font.maxp().ok()?.num_glyphs();
        Some(Self {
            hmtx: font.hmtx().ok(),
            os2: font.os2().ok(),
            post: font.post().ok(),
            font,
            head,
            hhea,
            num_glyphs,
        })
    }

    /// Map a character through the first Unicode cmap subtable that covers it.
    ///
    /// A mapping to a glyph ID outside the face (beyond `maxp.numGlyphs` or
    /// wider than 16 bits) counts as unmapped, and the next subtable is tried.
    pub(crate) fn glyph_index(&self, ch: char) -> Option<u16> {
        let cmap = self.font.cmap().ok()?;
        let data = cmap.offset_data();
        for record in cmap.encoding_records() {
            let Ok(subtable) = record.subtable(data) else {
                continue;
            };
            if !is_unicode_subtable(record.platform_id(), record.encoding_id(), &subtable) {
                continue;
            }
            if let Some(gid) = subtable
                .map_codepoint(ch)
                .and_then(|gid| u16::try_from(gid.to_u32()).ok())
                .filter(|gid| *gid < self.num_glyphs)
            {
                return Some(gid);
            }
        }
        None
    }

    /// Horizontal advance in font units; `None` for glyphs outside the face.
    pub(crate) fn glyph_hor_advance(&self, glyph: u16) -> Option<u16> {
        if glyph >= self.num_glyphs {
            return None;
        }
        self.hmtx.as_ref()?.advance(GlyphId::new(u32::from(glyph)))
    }

    pub(crate) fn units_per_em(&self) -> u16 {
        self.head.units_per_em()
    }

    pub(crate) fn global_bounding_box(&self) -> BoundingBox {
        BoundingBox {
            x_min: self.head.x_min(),
            y_min: self.head.y_min(),
            x_max: self.head.x_max(),
            y_max: self.head.y_max(),
        }
    }

    fn uses_typo_metrics(&self) -> Option<&Os2<'a>> {
        self.os2.as_ref().filter(|os2| {
            os2.version() >= 4
                && os2
                    .fs_selection()
                    .contains(SelectionFlags::USE_TYPO_METRICS)
        })
    }

    /// Ascender, falling back to OS/2 values when `hhea` reports zero.
    pub(crate) fn ascender(&self) -> i16 {
        if let Some(os2) = self.uses_typo_metrics() {
            return os2.s_typo_ascender();
        }
        match (self.hhea.ascender().to_i16(), &self.os2) {
            (0, Some(os2)) => match os2.s_typo_ascender() {
                0 => os2.us_win_ascent() as i16,
                value => value,
            },
            (value, _) => value,
        }
    }

    /// Descender (negative below the baseline), with the same fallbacks.
    pub(crate) fn descender(&self) -> i16 {
        if let Some(os2) = self.uses_typo_metrics() {
            return os2.s_typo_descender();
        }
        match (self.hhea.descender().to_i16(), &self.os2) {
            (0, Some(os2)) => match os2.s_typo_descender() {
                0 => (os2.us_win_descent() as i16).wrapping_neg(),
                value => value,
            },
            (value, _) => value,
        }
    }

    /// OS/2 capital height; `None` before OS/2 version 2.
    pub(crate) fn capital_height(&self) -> Option<i16> {
        self.os2
            .as_ref()
            .filter(|os2| os2.version() >= 2)
            .and_then(Os2::s_cap_height)
    }

    /// Italic angle in degrees from `post`, or zero without that table.
    pub(crate) fn italic_angle(&self) -> f32 {
        self.post
            .as_ref()
            .map_or(0.0, |post| post.italic_angle().to_f32())
    }
}

impl Face<'_> {
    pub(crate) fn is_monospaced(&self) -> bool {
        self.post
            .as_ref()
            .is_some_and(|post| post.is_fixed_pitch() != 0)
    }

    /// OS/2 italic selection flag or a nonzero italic angle.
    pub(crate) fn is_italic(&self) -> bool {
        self.os2
            .as_ref()
            .is_some_and(|os2| os2.fs_selection().contains(SelectionFlags::ITALIC))
            || self.italic_angle() != 0.0
    }

    /// Glyph name from the `post` table.
    pub(crate) fn glyph_name(&self, glyph: u16) -> Option<&str> {
        self.post.as_ref()?.glyph_name(GlyphId16::new(glyph))
    }

    /// First PostScript name record stored as UTF-16BE (Unicode or Windows platform).
    pub(crate) fn postscript_name(&self) -> Option<String> {
        let name = self.font.name().ok()?;
        let storage = name.string_data().as_bytes();
        name.name_record()
            .iter()
            .filter(|record| record.name_id() == NameId::POSTSCRIPT_NAME)
            .filter(|record| {
                matches!(
                    (record.platform_id(), record.encoding_id()),
                    (0, _) | (3, 0 | 1)
                )
            })
            .find_map(|record| {
                let start = record.string_offset().to_u32() as usize;
                let bytes = storage.get(start..start.checked_add(usize::from(record.length()))?)?;
                let units: Vec<u16> = bytes
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|pair| u16::from_be_bytes(*pair))
                    .collect();
                String::from_utf16(&units).ok()
            })
    }
}

/// Raw bytes of one table from the first face, without requiring any other table.
#[cfg(feature = "codecs")]
pub(crate) fn raw_table<'a>(bytes: &'a [u8], tag: &[u8; 4]) -> Option<&'a [u8]> {
    FontRef::from_index(bytes, 0)
        .ok()?
        .table_data(Tag::new(tag))
        .map(|data| data.as_bytes())
}

/// Unicode platform, Windows BMP, or Windows full repertoire in format 12/13.
fn is_unicode_subtable(platform: PlatformId, encoding: u16, subtable: &CmapSubtable<'_>) -> bool {
    match platform {
        PlatformId::Unicode => true,
        PlatformId::Windows => {
            encoding == 1
                || (encoding == 10
                    && matches!(
                        subtable,
                        CmapSubtable::Format12(_) | CmapSubtable::Format13(_)
                    ))
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::Face;

    const SANS: &[u8] = include_bytes!("../assets/liberation/LiberationSans-Regular.ttf");

    /// Offset and length of a table from the sfnt table directory.
    fn table_record(font: &[u8], tag: &[u8; 4]) -> (usize, usize, usize) {
        let count = usize::from(u16::from_be_bytes([font[4], font[5]]));
        (0..count)
            .map(|index| 12 + index * 16)
            .find(|record| &font[*record..*record + 4] == tag)
            .map(|record| {
                let read =
                    |at: usize| u32::from_be_bytes(font[at..at + 4].try_into().unwrap()) as usize;
                (record, read(record + 8), read(record + 12))
            })
            .unwrap()
    }

    fn patched(tag: &[u8; 4], edit: impl FnOnce(&mut Vec<u8>, usize, usize)) -> Vec<u8> {
        let mut font = SANS.to_vec();
        let (record, offset, length) = table_record(&font, tag);
        edit(&mut font, record, offset);
        assert!(length > 0);
        font
    }

    fn set_u16(font: &mut [u8], at: usize, value: u16) {
        font[at..at + 2].copy_from_slice(&value.to_be_bytes());
    }

    #[test]
    fn units_per_em_outside_the_valid_range_is_rejected() {
        for units in [0, 15, 16_385, u16::MAX] {
            // head.unitsPerEm sits 18 bytes into the table.
            let font = patched(b"head", |font, _, offset| set_u16(font, offset + 18, units));
            assert!(Face::parse(&font).is_none(), "unitsPerEm {units}");
        }
        for units in [16, 2048, 16_384] {
            let font = patched(b"head", |font, _, offset| set_u16(font, offset + 18, units));
            assert_eq!(Face::parse(&font).unwrap().units_per_em(), units);
        }
    }

    #[test]
    fn truncated_hmtx_yields_no_advances_without_panicking() {
        // Shrink the hmtx table record to two bytes.
        let font = patched(b"hmtx", |font, record, _| {
            font[record + 12..record + 16].copy_from_slice(&2_u32.to_be_bytes());
        });
        let face = Face::parse(&font).unwrap();
        let glyph = face.glyph_index('A').unwrap();
        assert_eq!(face.glyph_hor_advance(glyph), None);
        assert_eq!(face.glyph_hor_advance(0), None);
    }

    #[test]
    fn cmap_entries_past_the_glyph_count_are_unmapped() {
        let original = Face::parse(SANS).unwrap();
        let glyph = original.glyph_index('A').unwrap();
        assert!(glyph > 1);
        // maxp.numGlyphs sits 4 bytes into the table; keep only glyphs 0 and 1.
        let font = patched(b"maxp", |font, _, offset| set_u16(font, offset + 4, 2));
        let face = Face::parse(&font).unwrap();
        assert_eq!(face.glyph_index('A'), None);
        assert_eq!(face.glyph_hor_advance(glyph), None);
    }

    #[cfg(feature = "codecs")]
    #[test]
    fn malformed_embedded_fonts_fall_back_to_bundled_faces() {
        use crate::fonts::{ResolvedEditFont, check_font_coverage, resolve_font_for_text};

        let zero_units = patched(b"head", |font, _, offset| set_u16(font, offset + 18, 0));
        let short_maxp = patched(b"maxp", |font, _, offset| set_u16(font, offset + 4, 2));
        let short_hmtx = patched(b"hmtx", |font, record, _| {
            font[record + 12..record + 16].copy_from_slice(&2_u32.to_be_bytes());
        });
        assert!(check_font_coverage(&zero_units, "A").is_err());
        assert!(!check_font_coverage(&short_maxp, "A").unwrap());
        for bytes in [&zero_units, &short_maxp] {
            let resolved =
                resolve_font_for_text(Some(("Embedded", bytes)), None, false, false, "A").unwrap();
            assert!(
                matches!(resolved, ResolvedEditFont::Substituted { .. }),
                "{resolved:?}"
            );
        }
        // Coverage only needs the cmap, so this face stays embedded; it must not panic.
        resolve_font_for_text(Some(("Embedded", &short_hmtx)), None, false, false, "A").unwrap();
    }
}
