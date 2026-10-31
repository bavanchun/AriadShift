//! Simple-font encodings and glyph names.
//!
//! The standard encoding tables and the Adobe Glyph List live inside `lopdf`, which exposes them
//! only through font-encoding resolution. These helpers ask `lopdf` to resolve small synthetic
//! font dictionaries and read the answer back, so no glyph data is duplicated here. The built-in
//! encodings of the `Symbol` and `ZapfDingbats` fonts, which a PDF never names, come from
//! `symbolic`.

use std::collections::HashMap;

use lopdf::{Dictionary, Document, Encoding, Object};

use super::symbolic;

/// The base encodings a simple font can name.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(super) enum BaseEncoding {
    Standard,
    WinAnsi,
    MacRoman,
    MacExpert,
    /// Built-in encoding of the `Symbol` font; no PDF names it.
    Symbol,
    /// Built-in encoding of the `ZapfDingbats` font; no PDF names it.
    ZapfDingbats,
}

impl BaseEncoding {
    pub(super) fn from_name(name: &[u8]) -> Option<Self> {
        match name {
            b"StandardEncoding" => Some(Self::Standard),
            b"WinAnsiEncoding" => Some(Self::WinAnsi),
            b"MacRomanEncoding" => Some(Self::MacRoman),
            b"MacExpertEncoding" => Some(Self::MacExpert),
            _ => None,
        }
    }

    /// The PDF name of the encoding, for the encodings a font dictionary can name.
    const fn name(self) -> Option<&'static [u8]> {
        match self {
            Self::Standard => Some(b"StandardEncoding"),
            Self::WinAnsi => Some(b"WinAnsiEncoding"),
            Self::MacRoman => Some(b"MacRomanEncoding"),
            Self::MacExpert => Some(b"MacExpertEncoding"),
            Self::Symbol | Self::ZapfDingbats => None,
        }
    }
}

pub(super) type CodeTable = [Option<char>; 256];

/// Most distinct glyph names remembered per document.
const MAX_CACHED_NAMES: usize = 65_536;

/// Per-document cache of encoding tables and glyph-name lookups.
#[derive(Default)]
pub(super) struct Encodings {
    tables: HashMap<BaseEncoding, Box<CodeTable>>,
    names: HashMap<Vec<u8>, Option<Box<str>>>,
}

impl Encodings {
    /// The code-to-character table of a base encoding.
    pub(super) fn table(&mut self, doc: &Document, encoding: BaseEncoding) -> &CodeTable {
        self.tables.entry(encoding).or_insert_with(|| {
            Box::new(match encoding {
                BaseEncoding::Symbol => symbolic_table(symbolic::SYMBOL),
                BaseEncoding::ZapfDingbats => symbolic_table(symbolic::ZAPF_DINGBATS),
                _ => lopdf_table(doc, encoding),
            })
        })
    }

    /// Unicode text for a glyph name, following the Adobe Glyph List conventions for `uniXXXX`,
    /// `uXXXX[XX]`, `.suffix` variants and `_` ligatures.
    pub(super) fn glyph(&mut self, doc: &Document, name: &[u8]) -> Option<&str> {
        if !self.names.contains_key(name) {
            let resolved = resolve_glyph_name(doc, name).map(String::into_boxed_str);
            if self.names.len() >= MAX_CACHED_NAMES {
                self.names.clear();
            }
            self.names.insert(name.to_vec(), resolved);
        }
        self.names.get(name).and_then(|value| value.as_deref())
    }
}

fn symbolic_table(entries: &[(u8, char)]) -> CodeTable {
    let mut table = [None; 256];
    for &(code, ch) in entries {
        table[usize::from(code)] = Some(ch);
    }
    table
}

fn lopdf_table(doc: &Document, encoding: BaseEncoding) -> CodeTable {
    let mut table = [None; 256];
    let Some(name) = encoding.name() else {
        return table;
    };
    let mut font = Dictionary::new();
    font.set("Type", Object::Name(b"Font".to_vec()));
    font.set("Encoding", Object::Name(name.to_vec()));
    if let Ok(Encoding::OneByteEncoding(set)) = font.get_font_encoding(doc) {
        for (slot, glyph) in table.iter_mut().zip(set.iter()) {
            *slot = glyph.and_then(|glyph| char::from_u32(u32::from(glyph.utf16_code_unit())));
        }
    }
    table
}

/// Looks a name up in `lopdf`'s copy of the Adobe Glyph List.
fn lopdf_glyph(doc: &Document, name: &[u8]) -> Option<char> {
    let mut encoding = Dictionary::new();
    encoding.set(
        "Differences",
        Object::Array(vec![Object::Integer(0), Object::Name(name.to_vec())]),
    );
    let mut font = Dictionary::new();
    font.set("Type", Object::Name(b"Font".to_vec()));
    font.set("Encoding", Object::Dictionary(encoding));
    match font.get_font_encoding(doc) {
        Ok(Encoding::Differences(differences)) => differences
            .map
            .get(&0_u8)
            .and_then(|glyph| char::from_u32(u32::from(glyph.utf16_code_unit()))),
        _ => None,
    }
}

fn resolve_glyph_name(doc: &Document, name: &[u8]) -> Option<String> {
    // Variants such as `a.sc` or `one.oldstyle` map like their base name.
    let base = name.split(|&byte| byte == b'.').next().unwrap_or(name);
    if base.is_empty() || name == b".notdef" {
        return None;
    }
    if let Some(text) = single_glyph(doc, base) {
        return Some(text);
    }
    if base.contains(&b'_') {
        let mut out = String::new();
        for part in base.split(|&byte| byte == b'_') {
            out.push_str(&single_glyph(doc, part)?);
        }
        return (!out.is_empty()).then_some(out);
    }
    None
}

fn single_glyph(doc: &Document, name: &[u8]) -> Option<String> {
    if name.is_empty() {
        return None;
    }
    if let Some(ch) = lopdf_glyph(doc, name) {
        return Some(ch.to_string());
    }
    if let Some(hex) = name.strip_prefix(b"uni")
        && !hex.is_empty()
        && hex.len() % 4 == 0
    {
        let units: Option<Vec<u16>> = hex.chunks(4).map(parse_hex_u16).collect();
        let text: String = char::decode_utf16(units?)
            .collect::<Result<String, _>>()
            .ok()?;
        return (!text.is_empty()).then_some(text);
    }
    if let Some(hex) = name.strip_prefix(b"u")
        && (4..=6).contains(&hex.len())
    {
        let value = std::str::from_utf8(hex).ok()?;
        let code = u32::from_str_radix(value, 16).ok()?;
        return char::from_u32(code).map(String::from);
    }
    None
}

fn parse_hex_u16(hex: &[u8]) -> Option<u16> {
    let value = std::str::from_utf8(hex).ok()?;
    if !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    u16::from_str_radix(value, 16).ok()
}

#[cfg(test)]
mod tests {
    use lopdf::Document;

    use super::{BaseEncoding, Encodings};

    #[test]
    fn resolves_glyph_names() {
        let doc = Document::new();
        let mut encodings = Encodings::default();
        assert_eq!(encodings.glyph(&doc, b"eacute"), Some("é"));
        assert_eq!(encodings.glyph(&doc, b"Adotbelow"), Some("\u{1EA0}"));
        assert_eq!(encodings.glyph(&doc, b"uni1EA1"), Some("\u{1EA1}"));
        assert_eq!(encodings.glyph(&doc, b"u1F600"), Some("\u{1F600}"));
        assert_eq!(encodings.glyph(&doc, b"a.sc"), Some("a"));
        assert_eq!(encodings.glyph(&doc, b"T_h"), Some("Th"));
        assert_eq!(encodings.glyph(&doc, b"g123"), None);
        assert_eq!(encodings.glyph(&doc, b".notdef"), None);
        assert_eq!(encodings.glyph(&doc, b"uniD800"), None);
    }

    #[test]
    fn reads_base_encoding_tables() {
        let doc = Document::new();
        let mut encodings = Encodings::default();
        let win = *encodings.table(&doc, BaseEncoding::WinAnsi);
        assert_eq!(win[usize::from(b'A')], Some('A'));
        assert_eq!(win[0xE9], Some('é'));
        assert_eq!(win[0x80], Some('€'));
        let standard = *encodings.table(&doc, BaseEncoding::Standard);
        assert_eq!(standard[0x27], Some('\u{2019}'));
        assert_eq!(standard[0xAE], Some('\u{FB01}'));
    }
}
