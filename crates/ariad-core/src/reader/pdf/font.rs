//! Font dictionaries: character code splitting, Unicode decoding and glyph widths.

use lopdf::{Dictionary, Document, Object};

use super::{
    ReadError,
    cmap::CMap,
    encoding::{BaseEncoding, CodeTable, Encodings},
    lexer::{Token, next_token},
    objects::{Budget, get, get_array, get_dict, get_name, get_number, number, resolve},
};

/// Most `/W` entries kept for one CID font.
const MAX_CID_WIDTH_ENTRIES: usize = 131_072;

/// Bytes of a Type 1 font program scanned for its built-in encoding.
const MAX_TYPE1_HEADER_BYTES: usize = 64 * 1024;

/// How character codes are split off a string.
#[derive(Debug)]
enum Codes {
    OneByte,
    TwoByte,
    /// Codespace ranges of the font's encoding CMap.
    Encoding(CMap),
    /// Codespace ranges of the font's ToUnicode CMap.
    ToUnicodeCodespace,
}

#[derive(Debug)]
enum Widths {
    Simple {
        first: u32,
        values: Vec<f64>,
    },
    /// Sorted, non-overlapping `(first, last, width)` ranges of CIDs.
    Cid(Vec<(u32, u32, f64)>),
}

/// Approximate metrics for a standard font used without a `/Widths` array.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Family {
    Fixed,
    Sans,
    Serif,
    Unknown,
}

/// A loaded font.
#[derive(Debug)]
pub(super) struct Font {
    codes: Codes,
    to_unicode: Option<CMap>,
    /// Unicode text per code for simple fonts, from the encoding and glyph names.
    simple: Vec<Option<Box<str>>>,
    /// The codes of a composite font are Unicode (UCS-2 or UTF-16 CMaps).
    unicode_codes: bool,
    widths: Widths,
    default_width: f64,
    family: Family,
    /// Text-space units per width unit: 0.001 except for Type 3 fonts.
    width_scale: f64,
    /// Factor from the nominal font size to the size of the glyphs' em square.
    pub(super) size_scale: f64,
    pub(super) bold: bool,
    pub(super) italic: bool,
}

impl Font {
    /// A font with no information, used when a font resource is missing or unreadable.
    pub(super) fn unknown() -> Self {
        Self {
            codes: Codes::OneByte,
            to_unicode: None,
            simple: Vec::new(),
            unicode_codes: false,
            widths: Widths::Simple {
                first: 0,
                values: Vec::new(),
            },
            default_width: 500.0,
            family: Family::Unknown,
            width_scale: 0.001,
            size_scale: 1.0,
            bold: false,
            italic: false,
        }
    }

    /// Retained width/encoding allocations plus cache overhead, charged for every font.
    pub(super) fn work_bytes(&self) -> usize {
        let widths = match &self.widths {
            Widths::Simple { values, .. } => values.capacity() * std::mem::size_of::<f64>(),
            Widths::Cid(ranges) => ranges.capacity() * std::mem::size_of::<(u32, u32, f64)>(),
        };
        16 * 1024
            + widths
            + self.simple.capacity() * std::mem::size_of::<Option<Box<str>>>()
            + self
                .simple
                .iter()
                .flatten()
                .map(|text| text.len())
                .sum::<usize>()
    }

    /// Splits the next character code off non-empty `bytes`; returns it with its byte length.
    pub(super) fn next_code(&self, bytes: &[u8]) -> (u32, usize) {
        let single = (u32::from(bytes[0]), 1);
        match &self.codes {
            Codes::OneByte => single,
            Codes::TwoByte => match bytes {
                [high, low, ..] => (u32::from(u16::from_be_bytes([*high, *low])), 2),
                _ => single,
            },
            Codes::Encoding(cmap) => cmap.next_code(bytes).unwrap_or(single),
            Codes::ToUnicodeCodespace => self
                .to_unicode
                .as_ref()
                .and_then(|cmap| cmap.next_code(bytes))
                .unwrap_or(single),
        }
    }

    /// Appends the Unicode text of a code to `out`; returns whether the code mapped.
    pub(super) fn decode(&self, code: u32, len: usize, out: &mut String) -> bool {
        let start = out.len();
        if let Some(cmap) = &self.to_unicode {
            if cmap.lookup(code, len as u8, out) && is_useful(&out[start..]) {
                return true;
            }
            out.truncate(start);
        }
        if let Some(Some(text)) = self.simple.get(code as usize)
            && len == 1
        {
            out.push_str(text);
            return true;
        }
        if self.unicode_codes
            && let Some(ch) = char::from_u32(code)
            && !ch.is_control()
        {
            out.push(ch);
            return true;
        }
        false
    }

    /// Horizontal displacement of a code in text-space units per unit of font size.
    pub(super) fn width(&self, code: u32, text: Option<char>) -> f64 {
        let raw = match &self.widths {
            Widths::Simple { first, values } => code
                .checked_sub(*first)
                .and_then(|index| values.get(index as usize).copied()),
            Widths::Cid(ranges) => {
                let index = ranges.partition_point(|&(first, _, _)| first <= code);
                index
                    .checked_sub(1)
                    .map(|index| ranges[index])
                    .filter(|&(_, last, _)| code <= last)
                    .map(|(_, _, width)| width)
            }
        };
        let raw = match raw {
            Some(width) => width,
            None if self.family != Family::Unknown => family_width(self.family, text),
            None => self.default_width,
        };
        raw * self.width_scale
    }

    /// Whether a code is the single-byte space that word spacing applies to.
    pub(super) fn is_word_space(&self, code: u32, len: usize) -> bool {
        code == 32 && len == 1
    }

    /// Loads a font dictionary. Only exhausting the decompression budget is an error; anything
    /// unreadable degrades to a font without that information.
    pub(super) fn load(
        doc: &Document,
        dict: &Dictionary,
        budget: &mut Budget,
        encodings: &mut Encodings,
    ) -> Result<Self, ReadError> {
        let mut font = Self::unknown();
        let subtype = get_name(doc, dict, b"Subtype").unwrap_or_default();
        let base_name = get_name(doc, dict, b"BaseFont").map(strip_subset_tag);
        if let Some(stream) =
            get(doc, dict, b"ToUnicode").and_then(|object| object.as_stream().ok())
            && let Some(data) = budget.decode(stream)?
        {
            font.to_unicode = Some(CMap::parse(&data));
        }

        let descriptor;
        if subtype == b"Type0" {
            let descendant = get_array(doc, dict, b"DescendantFonts")
                .and_then(|fonts| fonts.first())
                .and_then(|object| resolve(doc, object))
                .and_then(|object| object.as_dict().ok());
            descriptor = descendant.and_then(|cid| get_dict(doc, cid, b"FontDescriptor"));
            font.load_composite(doc, dict, descendant, budget)?;
        } else {
            descriptor = get_dict(doc, dict, b"FontDescriptor");
            font.load_simple(doc, dict, subtype, descriptor, base_name, budget, encodings)?;
        }

        if let Some(name) = base_name {
            let (bold, italic) = style_from_name(name);
            font.bold |= bold;
            font.italic |= italic;
        }
        if let Some(descriptor) = descriptor {
            let flags = get_number(doc, descriptor, b"Flags").unwrap_or(0.0) as i64;
            font.italic |= flags & (1 << 6) != 0;
            font.bold |= flags & (1 << 18) != 0;
            font.bold |= get_number(doc, descriptor, b"FontWeight").is_some_and(|w| w >= 600.0);
        }
        Ok(font)
    }

    fn load_composite(
        &mut self,
        doc: &Document,
        dict: &Dictionary,
        descendant: Option<&Dictionary>,
        budget: &mut Budget,
    ) -> Result<(), ReadError> {
        self.codes = Codes::TwoByte;
        match get(doc, dict, b"Encoding") {
            Some(Object::Name(name)) => {
                let identity = name.starts_with(b"Identity");
                self.unicode_codes = contains(name, b"UCS2") || contains(name, b"UTF16");
                if !identity
                    && !self.unicode_codes
                    && self.to_unicode.as_ref().is_some_and(CMap::has_codespace)
                {
                    self.codes = Codes::ToUnicodeCodespace;
                }
            }
            Some(Object::Stream(stream)) => {
                if let Some(data) = budget.decode(stream)? {
                    let cmap = CMap::parse(&data);
                    if cmap.has_codespace() {
                        self.codes = Codes::Encoding(cmap);
                    }
                }
                self.unicode_codes = get_name(doc, &stream.dict, b"CMapName")
                    .is_some_and(|name| contains(name, b"UCS2") || contains(name, b"UTF16"));
            }
            _ => {}
        }
        self.default_width = 1000.0;
        let Some(descendant) = descendant else {
            return Ok(());
        };
        if let Some(width) = get_number(doc, descendant, b"DW") {
            self.default_width = width;
        }
        if let Some(array) = get_array(doc, descendant, b"W") {
            self.widths = Widths::Cid(cid_widths(doc, array));
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn load_simple(
        &mut self,
        doc: &Document,
        dict: &Dictionary,
        subtype: &[u8],
        descriptor: Option<&Dictionary>,
        base_name: Option<&[u8]>,
        budget: &mut Budget,
        encodings: &mut Encodings,
    ) -> Result<(), ReadError> {
        if subtype == b"Type3" {
            if let Some(matrix) = get_array(doc, dict, b"FontMatrix")
                && let [a, _, _, d, ..] = matrix
                && let (Some(a), Some(d)) = (
                    resolve(doc, a).and_then(number),
                    resolve(doc, d).and_then(number),
                )
                && a != 0.0
                && d != 0.0
            {
                self.width_scale = a;
                self.size_scale = (d.abs() * 1000.0).clamp(1e-3, 1e4);
            }
        } else if let Some(name) = base_name {
            self.family = standard_family(name);
        }

        let first = get_number(doc, dict, b"FirstChar").unwrap_or(0.0);
        if let Some(values) = get_array(doc, dict, b"Widths")
            && (0.0..=255.0).contains(&first)
        {
            let values = values
                .iter()
                .take(256)
                .map(|object| resolve(doc, object).and_then(number).unwrap_or(0.0))
                .collect();
            self.widths = Widths::Simple {
                first: first as u32,
                values,
            };
            self.family = Family::Unknown;
        }
        if let Some(width) = descriptor.and_then(|d| get_number(doc, d, b"MissingWidth"))
            && width > 0.0
        {
            self.default_width = width;
        }

        let embedded_type1 = descriptor.and_then(|d| get(doc, d, b"FontFile"));
        let encoding = get(doc, dict, b"Encoding");
        let (encoding_name, differences) = match encoding {
            Some(Object::Name(name)) => (Some(name.as_slice()), None),
            Some(Object::Dictionary(encoding)) => (
                get_name(doc, encoding, b"BaseEncoding"),
                get_array(doc, encoding, b"Differences"),
            ),
            _ => (None, None),
        };

        let mut table: CodeTable = [None; 256];
        let mut builtin_names: Vec<(u8, Vec<u8>)> = Vec::new();
        match encoding_name.and_then(BaseEncoding::from_name) {
            Some(base) => table = *encodings.table(doc, base),
            None => {
                if let Some(Object::Stream(program)) = embedded_type1
                    && let Some(data) = budget.decode(program)?
                {
                    builtin_names = type1_builtin_encoding(&data);
                }
                let fallback = match base_name {
                    Some(b"Symbol") => BaseEncoding::Symbol,
                    Some(b"ZapfDingbats") => BaseEncoding::ZapfDingbats,
                    _ if subtype == b"TrueType" => BaseEncoding::WinAnsi,
                    _ => BaseEncoding::Standard,
                };
                if builtin_names.is_empty() {
                    table = *encodings.table(doc, fallback);
                    if fallback == BaseEncoding::Standard {
                        // StandardEncoding leaves many codes undefined; broken files use them
                        // as WinAnsi.
                        let win = *encodings.table(doc, BaseEncoding::WinAnsi);
                        for (slot, alternative) in table.iter_mut().zip(win) {
                            if slot.is_none() {
                                *slot = alternative;
                            }
                        }
                    }
                }
            }
        }

        let mut simple: Vec<Option<Box<str>>> = table
            .iter()
            .map(|ch| ch.map(|ch| ch.to_string().into_boxed_str()))
            .collect();
        for (code, name) in builtin_names {
            simple[usize::from(code)] = encodings.glyph(doc, &name).map(Box::from);
        }
        if let Some(differences) = differences {
            let mut code = 0_usize;
            for object in differences {
                match resolve(doc, object) {
                    Some(Object::Integer(value)) => code = (*value).clamp(0, 256) as usize,
                    Some(Object::Name(name)) if code < 256 => {
                        simple[code] = encodings.glyph(doc, name).map(Box::from);
                        code += 1;
                    }
                    _ => {}
                }
            }
        }
        self.simple = simple;
        Ok(())
    }
}

/// Whether mapped text carries content; CMaps sometimes map codes to control characters.
fn is_useful(text: &str) -> bool {
    text.chars().any(|ch| !ch.is_control())
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

/// Removes the six-letter subset tag (`ABCDEF+`) from a font name.
fn strip_subset_tag(name: &[u8]) -> &[u8] {
    if name.len() > 7 && name[6] == b'+' && name[..6].iter().all(u8::is_ascii_uppercase) {
        &name[7..]
    } else {
        name
    }
}

fn style_from_name(name: &[u8]) -> (bool, bool) {
    let lower = name.to_ascii_lowercase();
    let has = |needle: &[u8]| contains(&lower, needle);
    let bold = has(b"bold")
        || has(b"black")
        || has(b"heavy")
        || has(b"semibold")
        || has(b"demi")
        || (has(b"-medi") && !has(b"medium"))
        || lower.starts_with(b"cmbx");
    let italic = has(b"italic")
        || has(b"oblique")
        || lower.ends_with(b"ital")
        || lower.starts_with(b"cmti")
        || lower.starts_with(b"cmsl");
    (bold, italic)
}

fn standard_family(name: &[u8]) -> Family {
    let lower = name.to_ascii_lowercase();
    if lower.starts_with(b"courier") {
        Family::Fixed
    } else if lower.starts_with(b"helvetica") || lower.starts_with(b"arial") {
        Family::Sans
    } else if lower.starts_with(b"times") {
        Family::Serif
    } else {
        Family::Unknown
    }
}

/// Approximate advance widths, in thousandths of an em, for standard fonts without `/Widths`.
fn family_width(family: Family, ch: Option<char>) -> f64 {
    let serif = family == Family::Serif;
    let pick = |serif_width: f64, sans_width: f64| if serif { serif_width } else { sans_width };
    match (family, ch) {
        (Family::Fixed, _) => 600.0,
        (_, None) => 500.0,
        (_, Some(' ')) => pick(250.0, 278.0),
        (_, Some(ch)) if "ijl.,:;'!|`".contains(ch) => pick(278.0, 222.0),
        (_, Some(ch)) if "ftrI()[]-/\"".contains(ch) => 333.0,
        (_, Some('m')) => pick(778.0, 833.0),
        (_, Some('w')) => 722.0,
        (_, Some('M')) => pick(889.0, 833.0),
        (_, Some('W')) => 944.0,
        (_, Some(ch)) if ch.is_ascii_digit() => pick(500.0, 556.0),
        (_, Some(ch)) if ch.is_uppercase() => pick(690.0, 700.0),
        (_, Some(_)) => pick(480.0, 540.0),
    }
}

/// Parses a CID font `/W` array into sorted ranges.
fn cid_widths(doc: &Document, array: &[Object]) -> Vec<(u32, u32, f64)> {
    let mut ranges = Vec::new();
    let mut index = 0;
    while index < array.len() && ranges.len() < MAX_CID_WIDTH_ENTRIES {
        let Some(first) = resolve(doc, &array[index]).and_then(number) else {
            index += 1;
            continue;
        };
        let first = first.clamp(0.0, f64::from(u32::MAX)) as u32;
        match array.get(index + 1).and_then(|object| resolve(doc, object)) {
            Some(Object::Array(widths)) => {
                for (offset, width) in widths.iter().enumerate() {
                    if ranges.len() >= MAX_CID_WIDTH_ENTRIES {
                        break;
                    }
                    let Some(cid) = u32::try_from(offset)
                        .ok()
                        .and_then(|offset| first.checked_add(offset))
                    else {
                        break;
                    };
                    if let Some(width) = resolve(doc, width).and_then(number) {
                        ranges.push((cid, cid, width));
                    }
                }
                index += 2;
            }
            Some(object) => {
                let last = number(object).map(|last| last.clamp(0.0, f64::from(u32::MAX)) as u32);
                let width = array
                    .get(index + 2)
                    .and_then(|object| resolve(doc, object))
                    .and_then(number);
                if let (Some(last), Some(width)) = (last, width)
                    && last >= first
                {
                    ranges.push((first, last, width));
                }
                index += 3;
            }
            None => break,
        }
    }
    ranges.sort_by_key(|&(first, _, _)| first);
    // Keep the first definition where ranges overlap so lookups stay a binary search.
    let mut merged: Vec<(u32, u32, f64)> = Vec::with_capacity(ranges.len());
    for range in ranges {
        match merged.last() {
            Some(&(_, last, _)) if range.0 <= last => {
                if range.1 > last {
                    merged.push((last + 1, range.1, range.2));
                }
            }
            _ => merged.push(range),
        }
    }
    merged
}

/// Reads `dup <code> /<name> put` entries from the cleartext part of a Type 1 font program.
fn type1_builtin_encoding(program: &[u8]) -> Vec<(u8, Vec<u8>)> {
    let header = &program[..program.len().min(MAX_TYPE1_HEADER_BYTES)];
    let header = match find(header, b"eexec") {
        Some(end) => &header[..end],
        None => header,
    };
    let Some(start) = find(header, b"/Encoding") else {
        return Vec::new();
    };
    let mut entries = Vec::new();
    let mut pos = start + b"/Encoding".len();
    let mut armed = false;
    let mut code: Option<f64> = None;
    let mut name: Option<Vec<u8>> = None;
    while let Some(token) = next_token(header, &mut pos) {
        match token {
            Token::Keyword(b"StandardEncoding") => return Vec::new(),
            Token::Keyword(b"dup") => {
                armed = true;
                code = None;
                name = None;
            }
            Token::Number(value) => code = Some(value),
            Token::Name(value) => name = Some(value),
            Token::Keyword(b"put") => {
                if let (true, Some(code), Some(name)) = (armed, code.take(), name.take())
                    && (0.0..256.0).contains(&code)
                    && name != b".notdef"
                {
                    entries.push((code as u8, name));
                }
                armed = false;
            }
            Token::Keyword(b"readonly" | b"def") if !entries.is_empty() => break,
            _ => {}
        }
    }
    entries
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// Appends decoded glyph text to `out`, expanding typographic ligatures, mapping private-use
/// bullets from symbol fonts, and dropping control characters.
pub(super) fn normalize_into(text: &str, out: &mut String) {
    for ch in text.chars() {
        match ch {
            '\u{FB00}' => out.push_str("ff"),
            '\u{FB01}' => out.push_str("fi"),
            '\u{FB02}' => out.push_str("fl"),
            '\u{FB03}' => out.push_str("ffi"),
            '\u{FB04}' => out.push_str("ffl"),
            '\u{FB05}' | '\u{FB06}' => out.push_str("st"),
            '\u{F0B7}' | '\u{F06C}' => out.push('•'),
            '\u{F0A7}' | '\u{F06E}' => out.push('▪'),
            '\u{F0D8}' => out.push('➢'),
            '\u{F0FC}' => out.push('✓'),
            '\t' | '\n' | '\r' => out.push(' '),
            ch if ch.is_control() || ch == '\u{FFFD}' => {}
            ch => out.push(ch),
        }
    }
}

#[cfg(test)]
mod tests {
    use lopdf::{Dictionary, Document, Object, Stream, dictionary};

    use super::{Font, normalize_into, strip_subset_tag, style_from_name, type1_builtin_encoding};
    use crate::reader::pdf::{encoding::Encodings, objects::Budget};

    fn load(doc: &Document, dict: &Dictionary) -> Font {
        Font::load(
            doc,
            dict,
            &mut Budget::new(1 << 20, 1 << 20),
            &mut Encodings::default(),
        )
        .unwrap()
    }

    fn decode(font: &Font, bytes: &[u8]) -> String {
        let mut out = String::new();
        let mut rest = bytes;
        while !rest.is_empty() {
            let (code, len) = font.next_code(rest);
            font.decode(code, len, &mut out);
            rest = &rest[len..];
        }
        out
    }

    #[test]
    fn simple_font_uses_differences_over_base_encoding() {
        let doc = Document::new();
        let font = load(
            &doc,
            &dictionary! {
                "Type" => "Font",
                "Subtype" => "Type1",
                "BaseFont" => "ABCDEF+Times-Bold",
                "FirstChar" => 65,
                "Widths" => vec![Object::Integer(722), Object::Integer(667)],
                "Encoding" => dictionary! {
                    "BaseEncoding" => "WinAnsiEncoding",
                    "Differences" => vec![Object::Integer(66), Object::Name(b"Adotbelow".to_vec()), Object::Name(b"uni01A1".to_vec())],
                },
            },
        );
        assert_eq!(decode(&font, b"ABC\xe9"), "AẠơé");
        assert!(font.bold);
        assert!(!font.italic);
        assert!((font.width(65, Some('A')) - 0.722).abs() < 1e-9);
        assert!((font.width(90, Some('Z')) - 0.5).abs() < 1e-9);
    }

    #[test]
    fn composite_font_prefers_to_unicode_and_cid_widths() {
        let mut doc = Document::new();
        let cmap = doc.add_object(Stream::new(
            Dictionary::new(),
            b"1 begincodespacerange <0000> <FFFF> endcodespacerange 1 beginbfchar <0005> <1EE6> endbfchar".to_vec(),
        ));
        let font = load(
            &doc,
            &dictionary! {
                "Type" => "Font",
                "Subtype" => "Type0",
                "Encoding" => "Identity-H",
                "ToUnicode" => cmap,
                "DescendantFonts" => vec![Object::Dictionary(dictionary! {
                    "Type" => "Font",
                    "Subtype" => "CIDFontType2",
                    "DW" => 900,
                    "W" => vec![Object::Integer(5), Object::Array(vec![Object::Integer(700)]), Object::Integer(10), Object::Integer(20), Object::Integer(300)],
                })],
            },
        );
        assert_eq!(font.next_code(&[0, 5, 0]), (5, 2));
        assert_eq!(decode(&font, &[0, 5, 0, 6]), "Ủ");
        assert!((font.width(5, None) - 0.7).abs() < 1e-9);
        assert!((font.width(15, None) - 0.3).abs() < 1e-9);
        assert!((font.width(30, None) - 0.9).abs() < 1e-9);
    }

    #[test]
    fn standard_fonts_without_widths_use_family_estimates() {
        let doc = Document::new();
        let font = load(
            &doc,
            &dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Courier" },
        );
        assert!((font.width(105, Some('i')) - 0.6).abs() < 1e-9);
        assert_eq!(decode(&font, b"Hi"), "Hi");
    }

    #[test]
    fn reads_type1_builtin_encoding() {
        let program = b"%!PS-AdobeFont-1.0: CMR10\n/Encoding 256 array\n0 1 255 {1 index exch /.notdef put} for\ndup 11 /ff put\ndup 65 /A put\nreadonly def\ncurrentfile eexec\x80\x81";
        assert_eq!(
            type1_builtin_encoding(program),
            vec![(11, b"ff".to_vec()), (65, b"A".to_vec())]
        );
        assert!(type1_builtin_encoding(b"/Encoding StandardEncoding def").is_empty());
    }

    #[test]
    fn names_and_text_cleanup() {
        assert_eq!(strip_subset_tag(b"ABCDEF+Arial-BoldMT"), b"Arial-BoldMT");
        assert_eq!(strip_subset_tag(b"Arial"), b"Arial");
        assert_eq!(style_from_name(b"NimbusRomNo9L-MediItal"), (true, true));
        assert_eq!(style_from_name(b"CMR10"), (false, false));
        let mut out = String::new();
        normalize_into("\u{FB01}x\u{0}\u{F0B7}\u{FFFD}\t", &mut out);
        assert_eq!(out, "fix• ");
    }
}
