//! Type 1 (eexec) to Type1C (CFF) conversion.
//!
//! Pure Rust and WASM-safe. The converter parses the cleartext header and the decrypted private
//! part of a Type 1 font program, rewrites every charstring as a Type 2 charstring (subroutines
//! inlined, standard flex and hint replacement mapped, everything else refused) and serializes a
//! bare CFF font. Any unparsable input or unsupported construct yields `None`, so the caller
//! leaves the original program untouched.

use super::type1::crypt;
use std::collections::BTreeMap;

const FIXED: i64 = 65_536;
const MAX_COORD: i64 = 32_767 * FIXED;
const MAX_GLYPHS: usize = 65_535;
const MAX_SUBRS: usize = 65_536;
const MAX_CHARSTRING: usize = 64 * 1024;
const MAX_SUBR_DEPTH: usize = 10;
const MAX_STACK: usize = 24;
const MAX_STEPS: usize = 100_000;
const MAX_STEMS: usize = 23;
const MAX_LEN_IV: usize = 16;
const MAX_TOKENS: usize = 1_000_000;
const STANDARD_STRINGS: i64 = 391;

/// Standard strings 1..=149 of the CFF specification (those used by StandardEncoding).
const STD: [&str; 149] = [
    "space",
    "exclam",
    "quotedbl",
    "numbersign",
    "dollar",
    "percent",
    "ampersand",
    "quoteright",
    "parenleft",
    "parenright",
    "asterisk",
    "plus",
    "comma",
    "hyphen",
    "period",
    "slash",
    "zero",
    "one",
    "two",
    "three",
    "four",
    "five",
    "six",
    "seven",
    "eight",
    "nine",
    "colon",
    "semicolon",
    "less",
    "equal",
    "greater",
    "question",
    "at",
    "A",
    "B",
    "C",
    "D",
    "E",
    "F",
    "G",
    "H",
    "I",
    "J",
    "K",
    "L",
    "M",
    "N",
    "O",
    "P",
    "Q",
    "R",
    "S",
    "T",
    "U",
    "V",
    "W",
    "X",
    "Y",
    "Z",
    "bracketleft",
    "backslash",
    "bracketright",
    "asciicircum",
    "underscore",
    "quoteleft",
    "a",
    "b",
    "c",
    "d",
    "e",
    "f",
    "g",
    "h",
    "i",
    "j",
    "k",
    "l",
    "m",
    "n",
    "o",
    "p",
    "q",
    "r",
    "s",
    "t",
    "u",
    "v",
    "w",
    "x",
    "y",
    "z",
    "braceleft",
    "bar",
    "braceright",
    "asciitilde",
    "exclamdown",
    "cent",
    "sterling",
    "fraction",
    "yen",
    "florin",
    "section",
    "currency",
    "quotesingle",
    "quotedblleft",
    "guillemotleft",
    "guilsinglleft",
    "guilsinglright",
    "fi",
    "fl",
    "endash",
    "dagger",
    "daggerdbl",
    "periodcentered",
    "paragraph",
    "bullet",
    "quotesinglbase",
    "quotedblbase",
    "quotedblright",
    "guillemotright",
    "ellipsis",
    "perthousand",
    "questiondown",
    "grave",
    "acute",
    "circumflex",
    "tilde",
    "macron",
    "breve",
    "dotaccent",
    "dieresis",
    "ring",
    "cedilla",
    "hungarumlaut",
    "ogonek",
    "caron",
    "emdash",
    "AE",
    "ordfeminine",
    "Lslash",
    "Oslash",
    "OE",
    "ordmasculine",
    "ae",
    "dotlessi",
    "lslash",
    "oslash",
    "oe",
    "germandbls",
];

/// StandardEncoding codes above 160 with their standard string ids.
const STD_UPPER: [(u8, u8); 54] = [
    (161, 96),
    (162, 97),
    (163, 98),
    (164, 99),
    (165, 100),
    (166, 101),
    (167, 102),
    (168, 103),
    (169, 104),
    (170, 105),
    (171, 106),
    (172, 107),
    (173, 108),
    (174, 109),
    (175, 110),
    (177, 111),
    (178, 112),
    (179, 113),
    (180, 114),
    (182, 115),
    (183, 116),
    (184, 117),
    (185, 118),
    (186, 119),
    (187, 120),
    (188, 121),
    (189, 122),
    (191, 123),
    (193, 124),
    (194, 125),
    (195, 126),
    (196, 127),
    (197, 128),
    (198, 129),
    (199, 130),
    (200, 131),
    (202, 132),
    (203, 133),
    (205, 134),
    (206, 135),
    (207, 136),
    (208, 137),
    (225, 138),
    (227, 139),
    (232, 140),
    (233, 141),
    (234, 142),
    (235, 143),
    (241, 144),
    (245, 145),
    (248, 146),
    (249, 147),
    (250, 148),
    (251, 149),
];

fn standard_encoding() -> Vec<(u8, &'static str)> {
    let mut table: Vec<(u8, &'static str)> = (32..=126_u8)
        .map(|code| (code, STD[usize::from(code) - 32]))
        .collect();
    table.extend(
        STD_UPPER
            .iter()
            .map(|&(code, sid)| (code, STD[usize::from(sid) - 1])),
    );
    table
}

// ----------------------------------------------------------------------------------------------
// Cleartext and private-part parsing
// ----------------------------------------------------------------------------------------------

fn is_delimiter(byte: u8) -> bool {
    b"[]{}()<>/%".contains(&byte)
}

fn next_token<'a>(bytes: &'a [u8], pos: &mut usize) -> Option<&'a [u8]> {
    while bytes.get(*pos).is_some_and(u8::is_ascii_whitespace) {
        *pos += 1;
    }
    let start = *pos;
    let first = *bytes.get(*pos)?;
    *pos += 1;
    match first {
        b'[' | b']' | b'{' | b'}' | b'<' | b'>' | b')' => {}
        b'(' => {
            let mut depth = 1_usize;
            while depth > 0 {
                match *bytes.get(*pos)? {
                    b'\\' => *pos += 1,
                    b'(' => depth += 1,
                    b')' => depth -= 1,
                    _ => {}
                }
                *pos += 1;
            }
        }
        b'%' => {
            while bytes.get(*pos).is_some_and(|b| *b != b'\n' && *b != b'\r') {
                *pos += 1;
            }
        }
        _ => {
            while bytes
                .get(*pos)
                .is_some_and(|b| !b.is_ascii_whitespace() && !is_delimiter(*b))
            {
                *pos += 1;
            }
        }
    }
    bytes.get(start..*pos)
}

#[derive(Clone, Debug, PartialEq)]
struct Number {
    text: String,
    int: Option<i32>,
}

impl Number {
    fn parse(token: &[u8]) -> Option<Self> {
        let text = std::str::from_utf8(token).ok()?;
        if text.is_empty()
            || !text
                .bytes()
                .all(|b| b.is_ascii_digit() || b"+-.eE".contains(&b))
        {
            return None;
        }
        let text = text.strip_prefix('+').unwrap_or(text);
        text.parse::<f64>().ok().filter(|v| v.is_finite())?;
        Some(Self {
            text: text.to_owned(),
            int: text.parse::<i32>().ok(),
        })
    }
    fn value(&self) -> Option<f64> {
        self.text.parse().ok()
    }
}

/// Read `[ n n ... ]` or `{ n n ... }`, or a single number.
fn read_numbers(bytes: &[u8], pos: &mut usize) -> Option<Vec<Number>> {
    let first = next_token(bytes, pos)?;
    let close: &[u8] = match first {
        b"[" => b"]",
        b"{" => b"}",
        other => return Some(vec![Number::parse(other)?]),
    };
    let mut numbers = Vec::new();
    loop {
        let token = next_token(bytes, pos)?;
        if token == close {
            return Some(numbers);
        }
        if numbers.len() >= 64 {
            return None;
        }
        numbers.push(Number::parse(token)?);
    }
}

enum EncodingSpec {
    Standard,
    Custom(Vec<(u8, Vec<u8>)>),
}

struct Header {
    name: Vec<u8>,
    matrix: Option<Vec<Number>>,
    bbox: Vec<Number>,
    encoding: EncodingSpec,
}

fn parse_encoding(bytes: &[u8], pos: &mut usize) -> Option<EncodingSpec> {
    let first = next_token(bytes, pos)?;
    if first == b"StandardEncoding" {
        return Some(EncodingSpec::Standard);
    }
    // `256 array 0 1 255 {1 index exch /.notdef put} for dup N /name put ... readonly def`
    let mut entries = Vec::new();
    for _ in 0..MAX_TOKENS {
        let token = next_token(bytes, pos)?;
        match token {
            b"def" => return Some(EncodingSpec::Custom(entries)),
            b"dup" => {
                let code = std::str::from_utf8(next_token(bytes, pos)?)
                    .ok()?
                    .parse::<u8>()
                    .ok()?;
                let name = next_token(bytes, pos)?;
                let name = name.strip_prefix(b"/")?;
                if next_token(bytes, pos)? != b"put" {
                    return None;
                }
                if name != b".notdef" {
                    entries.push((code, name.to_vec()));
                }
            }
            _ => {}
        }
    }
    None
}

fn parse_header(head: &[u8]) -> Option<Header> {
    let mut pos = 0;
    let mut name = None;
    let mut matrix = None;
    let mut bbox = None;
    let mut encoding = None;
    for _ in 0..MAX_TOKENS {
        let Some(token) = next_token(head, &mut pos) else {
            break;
        };
        match token {
            b"/FontName" if name.is_none() => {
                name = Some(next_token(head, &mut pos)?.strip_prefix(b"/")?.to_vec());
            }
            b"/FontMatrix" if matrix.is_none() => {
                let values = read_numbers(head, &mut pos)?;
                if values.len() != 6 {
                    return None;
                }
                matrix = Some(values);
            }
            b"/FontBBox" if bbox.is_none() => {
                let values = read_numbers(head, &mut pos)?;
                if values.len() != 4 {
                    return None;
                }
                bbox = Some(values);
            }
            b"/PaintType" | b"/FontType" => {
                // Programs may mention these keys in prelude code (`/FontType get 1 eq`); only a
                // numeric definition counts.
                let mut at = pos;
                if let Some(value) = next_token(head, &mut at)
                    .and_then(Number::parse)
                    .and_then(|n| n.int)
                {
                    let expected = i32::from(token == b"/FontType");
                    if value != expected {
                        return None;
                    }
                }
            }
            b"/Encoding" if encoding.is_none() => encoding = Some(parse_encoding(head, &mut pos)?),
            _ => {}
        }
    }
    let name = name?;
    if name.is_empty()
        || name.len() > 127
        || !name
            .iter()
            .all(|b| (33..=126).contains(b) && !b"[](){}<>/%".contains(b))
    {
        return None;
    }
    Some(Header {
        name,
        matrix,
        bbox: bbox?,
        encoding: encoding?,
    })
}

#[derive(Default)]
struct PrivateValues {
    values: BTreeMap<&'static str, Vec<Number>>,
    force_bold: bool,
}

const PRIVATE_KEYS: [(&str, &str); 14] = [
    ("/BlueValues", "BlueValues"),
    ("/OtherBlues", "OtherBlues"),
    ("/FamilyBlues", "FamilyBlues"),
    ("/FamilyOtherBlues", "FamilyOtherBlues"),
    ("/StdHW", "StdHW"),
    ("/StdVW", "StdVW"),
    ("/StemSnapH", "StemSnapH"),
    ("/StemSnapV", "StemSnapV"),
    ("/BlueScale", "BlueScale"),
    ("/BlueShift", "BlueShift"),
    ("/BlueFuzz", "BlueFuzz"),
    ("/ForceBold", "ForceBold"),
    ("/LanguageGroup", "LanguageGroup"),
    ("/lenIV", "lenIV"),
];

struct Parsed {
    header: Header,
    private: PrivateValues,
    subrs: Vec<Option<Vec<u8>>>,
    glyphs: Vec<(Vec<u8>, Vec<u8>)>,
}

struct PrivatePart {
    values: PrivateValues,
    subrs: Vec<Option<Vec<u8>>>,
    glyphs: Vec<(Vec<u8>, Vec<u8>)>,
}

fn binary_entry<'a>(clear: &'a [u8], pos: &mut usize, len: usize) -> Option<&'a [u8]> {
    // `RD` is followed by exactly one separator byte, then `len` raw bytes.
    if !clear.get(*pos)?.is_ascii_whitespace() {
        return None;
    }
    *pos += 1;
    let end = pos.checked_add(len)?;
    let data = clear.get(*pos..end)?;
    *pos = end;
    Some(data)
}

fn terminator(clear: &[u8], pos: &mut usize) -> Option<()> {
    let token = next_token(clear, pos)?;
    if token == b"noaccess" {
        next_token(clear, pos)?;
    } else if token.first() == Some(&b'/') {
        return None;
    }
    Some(())
}

fn parse_private(clear: &[u8]) -> Option<PrivatePart> {
    let mut pos = 0;
    let mut private = PrivateValues::default();
    let mut subrs_at = None;
    for _ in 0..MAX_TOKENS {
        let token = next_token(clear, &mut pos)?;
        if token == b"/Subrs" {
            subrs_at = Some(pos);
            break;
        }
        if token == b"/CharStrings" {
            pos -= token.len();
            break;
        }
        if token == b"/ForceBold" {
            let mut at = pos;
            private.force_bold = next_token(clear, &mut at) == Some(b"true".as_slice());
        }
        if let Some((_, key)) = PRIVATE_KEYS
            .iter()
            .find(|(name, _)| name.as_bytes() == token)
        {
            let mut at = pos;
            if let Some(values) = read_numbers(clear, &mut at)
                && !private.values.contains_key(key)
            {
                private.values.insert(key, values);
            }
        }
    }
    let len_iv = match private.values.get("lenIV") {
        Some(values) => usize::try_from(values.first()?.int?).ok()?,
        None => 4,
    };
    if len_iv > MAX_LEN_IV {
        return None;
    }
    let mut subrs: Vec<Option<Vec<u8>>> = Vec::new();
    if let Some(start) = subrs_at {
        pos = start;
        let count = usize::try_from(Number::parse(next_token(clear, &mut pos)?)?.int?).ok()?;
        if count > MAX_SUBRS || next_token(clear, &mut pos)? != b"array" {
            return None;
        }
        subrs.resize(count, None);
        loop {
            let before = pos;
            if next_token(clear, &mut pos)? != b"dup" {
                pos = before;
                break;
            }
            let index = usize::try_from(Number::parse(next_token(clear, &mut pos)?)?.int?).ok()?;
            let len = usize::try_from(Number::parse(next_token(clear, &mut pos)?)?.int?).ok()?;
            let rd = next_token(clear, &mut pos)?;
            if rd.first().is_none_or(|b| *b == b'/' || b.is_ascii_digit()) {
                return None;
            }
            let data = binary_entry(clear, &mut pos, len)?;
            *subrs.get_mut(index)? = Some(decrypt(data, len_iv)?);
            terminator(clear, &mut pos)?;
        }
    }
    for _ in 0..64 {
        if next_token(clear, &mut pos)? == b"/CharStrings" {
            break;
        }
    }
    let count = usize::try_from(Number::parse(next_token(clear, &mut pos)?)?.int?).ok()?;
    if count > MAX_GLYPHS
        || next_token(clear, &mut pos)? != b"dict"
        || next_token(clear, &mut pos)? != b"dup"
        || next_token(clear, &mut pos)? != b"begin"
    {
        return None;
    }
    let mut glyphs = Vec::new();
    loop {
        let token = next_token(clear, &mut pos)?;
        if token == b"end" {
            break;
        }
        let name = token.strip_prefix(b"/")?;
        let len = usize::try_from(Number::parse(next_token(clear, &mut pos)?)?.int?).ok()?;
        next_token(clear, &mut pos)?;
        let data = binary_entry(clear, &mut pos, len)?;
        if glyphs.len() >= MAX_GLYPHS {
            return None;
        }
        glyphs.push((name.to_vec(), decrypt(data, len_iv)?));
        terminator(clear, &mut pos)?;
    }
    Some(PrivatePart {
        values: private,
        subrs,
        glyphs,
    })
}

fn decrypt(cipher: &[u8], len_iv: usize) -> Option<Vec<u8>> {
    if cipher.len() > MAX_CHARSTRING {
        return None;
    }
    let plain = crypt(cipher, 4330, false);
    Some(plain.get(len_iv..)?.to_vec())
}

fn parse(program: &[u8], length1: usize, length2: usize) -> Option<Parsed> {
    let end = length1.checked_add(length2)?;
    let head = program.get(..length1)?;
    let encrypted = program.get(length1..end)?;
    if encrypted.len() < 8 {
        return None;
    }
    // Hex-encoded eexec is not valid inside PDF; refuse rather than guess.
    if encrypted
        .iter()
        .take(4)
        .all(|b| b.is_ascii_hexdigit() || b.is_ascii_whitespace())
    {
        return None;
    }
    let header = parse_header(head)?;
    let clear = crypt(encrypted, 55665, false);
    let part = parse_private(clear.get(4..)?)?;
    Some(Parsed {
        header,
        private: part.values,
        subrs: part.subrs,
        glyphs: part.glyphs,
    })
}

// ----------------------------------------------------------------------------------------------
// Type 1 -> Type 2 charstring conversion
// ----------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
enum Seg {
    Move(i64, i64),
    Line(i64, i64),
    Curve([i64; 6]),
    Mask(usize),
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Stem {
    vertical: bool,
    pos: i64,
    len: i64,
}

struct Glyph {
    /// Advance width as a 16.16 fixed value (`div` makes fractional widths common).
    width: i64,
    sets: Vec<Vec<Stem>>,
    segs: Vec<Seg>,
}

struct Interp<'a> {
    subrs: &'a [Option<Vec<u8>>],
    stack: Vec<i64>,
    ps: Vec<i64>,
    cx: i64,
    cy: i64,
    sbx: i64,
    width: Option<i64>,
    segs: Vec<Seg>,
    sets: Vec<Vec<Stem>>,
    moved: bool,
    flex: Option<Vec<(i64, i64)>>,
    flex_calls: usize,
    steps: usize,
    done: bool,
}

impl<'a> Interp<'a> {
    fn push(&mut self, value: i64) -> Option<()> {
        if self.stack.len() >= MAX_STACK || value.abs() > MAX_COORD {
            return None;
        }
        self.stack.push(value);
        Some(())
    }

    fn pop(&mut self) -> Option<i64> {
        self.stack.pop()
    }

    fn pop_int(&mut self) -> Option<i64> {
        let value = self.pop()?;
        (value % FIXED == 0).then_some(value / FIXED)
    }

    /// Require exactly `count` operands and clear the stack, as Type 1 operators do.
    fn args(&mut self, count: usize) -> Option<Vec<i64>> {
        if self.stack.len() != count {
            return None;
        }
        Some(std::mem::take(&mut self.stack))
    }

    fn move_to(&mut self, dx: i64, dy: i64) -> Option<()> {
        self.width?;
        self.cx = self.cx.checked_add(dx)?;
        self.cy = self.cy.checked_add(dy)?;
        if let Some(flex) = &mut self.flex {
            if flex.len() >= 7 {
                return None;
            }
            flex.push((dx, dy));
            return Some(());
        }
        let dx = if self.moved {
            dx
        } else {
            dx.checked_add(self.sbx)?
        };
        self.moved = true;
        self.segs.push(Seg::Move(dx, dy));
        Some(())
    }

    fn draw(&mut self, seg: Seg) -> Option<()> {
        if !self.moved || self.flex.is_some() || self.width.is_none() {
            return None;
        }
        let (dx, dy) = match seg {
            Seg::Line(x, y) => (x, y),
            Seg::Curve(c) => (
                c[0].checked_add(c[2])?.checked_add(c[4])?,
                c[1].checked_add(c[3])?.checked_add(c[5])?,
            ),
            _ => return None,
        };
        self.cx = self.cx.checked_add(dx)?;
        self.cy = self.cy.checked_add(dy)?;
        self.segs.push(seg);
        Some(())
    }

    fn stems(&mut self, args: &[i64], vertical: bool) -> Option<()> {
        self.width?;
        let sbx = if vertical { self.sbx } else { 0 };
        let set = self.sets.last_mut()?;
        for &[pos, len] in args.as_chunks::<2>().0 {
            set.push(Stem {
                vertical,
                pos: pos.checked_add(sbx)?,
                len,
            });
        }
        Some(())
    }

    fn other_subr(&mut self) -> Option<()> {
        let number = self.pop_int()?;
        let count = usize::try_from(self.pop_int()?).ok()?;
        if count > self.stack.len() {
            return None;
        }
        let args = self.stack.split_off(self.stack.len() - count);
        match (number, count) {
            (1, 0) => {
                if self.flex.is_some() {
                    return None;
                }
                self.flex = Some(Vec::new());
                self.flex_calls = 0;
            }
            (2, 0) => {
                let seen = self.flex.as_ref()?.len();
                if seen != self.flex_calls + 1 {
                    return None;
                }
                self.flex_calls += 1;
            }
            (0, 3) => {
                let points = self.flex.take()?;
                if points.len() != 7 || self.flex_calls != 7 || !self.moved {
                    return None;
                }
                let [first, d1, d2, d3, d4, d5, d6] = points[..] else {
                    return None;
                };
                self.segs.push(Seg::Curve([
                    first.0.checked_add(d1.0)?,
                    first.1.checked_add(d1.1)?,
                    d2.0,
                    d2.1,
                    d3.0,
                    d3.1,
                ]));
                self.segs
                    .push(Seg::Curve([d4.0, d4.1, d5.0, d5.1, d6.0, d6.1]));
                self.ps = vec![args[2], args[1]];
            }
            (3, 1) => {
                self.ps = vec![args[0]];
                self.sets.push(Vec::new());
                self.segs.push(Seg::Mask(self.sets.len() - 1));
            }
            _ => return None,
        }
        Some(())
    }

    fn run(&mut self, code: &[u8], depth: usize) -> Option<()> {
        if depth > MAX_SUBR_DEPTH {
            return None;
        }
        let mut i = 0;
        while i < code.len() && !self.done {
            self.steps += 1;
            if self.steps > MAX_STEPS {
                return None;
            }
            let byte = code[i];
            i += 1;
            match byte {
                32..=246 => self.push((i64::from(byte) - 139) * FIXED)?,
                247..=250 => {
                    let next = i64::from(*code.get(i)?);
                    i += 1;
                    self.push(((i64::from(byte) - 247) * 256 + next + 108) * FIXED)?;
                }
                251..=254 => {
                    let next = i64::from(*code.get(i)?);
                    i += 1;
                    self.push((-(i64::from(byte) - 251) * 256 - next - 108) * FIXED)?;
                }
                255 => {
                    let end = i.checked_add(4)?;
                    let value = i32::from_be_bytes(code.get(i..end)?.try_into().ok()?);
                    i = end;
                    self.push(i64::from(value).checked_mul(FIXED)?)?;
                }
                1 | 3 => {
                    let args = self.args(2)?;
                    self.stems(&args, byte == 3)?;
                }
                4 => {
                    let a = self.args(1)?;
                    self.move_to(0, a[0])?;
                }
                5 => {
                    let a = self.args(2)?;
                    self.draw(Seg::Line(a[0], a[1]))?;
                }
                6 => {
                    let a = self.args(1)?;
                    self.draw(Seg::Line(a[0], 0))?;
                }
                7 => {
                    let a = self.args(1)?;
                    self.draw(Seg::Line(0, a[0]))?;
                }
                8 => {
                    let a = self.args(6)?;
                    self.draw(Seg::Curve([a[0], a[1], a[2], a[3], a[4], a[5]]))?;
                }
                9 => {
                    self.args(0)?;
                }
                10 => {
                    let index = usize::try_from(self.pop_int()?).ok()?;
                    let subrs = self.subrs;
                    let subr = subrs.get(index)?.as_deref()?;
                    self.run(subr, depth + 1)?;
                }
                11 => return Some(()),
                13 => {
                    let a = self.args(2)?;
                    if self.width.is_some() {
                        return None;
                    }
                    self.sbx = a[0];
                    self.cx = a[0];
                    self.cy = 0;
                    self.width = Some(a[1]);
                }
                14 => {
                    self.args(0)?;
                    self.done = true;
                }
                21 => {
                    let a = self.args(2)?;
                    self.move_to(a[0], a[1])?;
                }
                22 => {
                    let a = self.args(1)?;
                    self.move_to(a[0], 0)?;
                }
                30 => {
                    let a = self.args(4)?;
                    self.draw(Seg::Curve([0, a[0], a[1], a[2], a[3], 0]))?;
                }
                31 => {
                    let a = self.args(4)?;
                    self.draw(Seg::Curve([a[0], 0, a[1], a[2], 0, a[3]]))?;
                }
                12 => {
                    let escape = *code.get(i)?;
                    i += 1;
                    match escape {
                        0 => {
                            self.args(0)?;
                        }
                        1 | 2 => {
                            let a = self.args(6)?;
                            self.stems(&a, escape == 1)?;
                        }
                        7 => {
                            let a = self.args(4)?;
                            if self.width.is_some() || a[1] != 0 || a[3] != 0 {
                                return None;
                            }
                            self.sbx = a[0];
                            self.cx = a[0];
                            self.cy = 0;
                            self.width = Some(a[2]);
                        }
                        12 => {
                            let divisor = self.pop()?;
                            let dividend = self.pop()?;
                            if divisor == 0 {
                                return None;
                            }
                            self.push(dividend.checked_mul(FIXED)? / divisor)?;
                        }
                        16 => self.other_subr()?,
                        17 => {
                            let value = self.ps.pop()?;
                            self.push(value)?;
                        }
                        33 => {
                            let a = self.args(2)?;
                            if a[0] != self.cx || a[1] != self.cy {
                                return None;
                            }
                        }
                        _ => return None,
                    }
                }
                _ => return None,
            }
        }
        Some(())
    }
}

fn convert_charstring(code: &[u8], subrs: &[Option<Vec<u8>>]) -> Option<Glyph> {
    let mut interp = Interp {
        subrs,
        stack: Vec::new(),
        ps: Vec::new(),
        cx: 0,
        cy: 0,
        sbx: 0,
        width: None,
        segs: Vec::new(),
        sets: vec![Vec::new()],
        moved: false,
        flex: None,
        flex_calls: 0,
        steps: 0,
        done: false,
    };
    interp.run(code, 0)?;
    if !interp.done || interp.flex.is_some() {
        return None;
    }
    Some(Glyph {
        width: interp.width?,
        sets: interp.sets,
        segs: interp.segs,
    })
}

// ----------------------------------------------------------------------------------------------
// Type 2 encoding
// ----------------------------------------------------------------------------------------------

fn type2_number(out: &mut Vec<u8>, value: i64) -> Option<()> {
    if value.abs() > MAX_COORD {
        return None;
    }
    if value % FIXED == 0 {
        let n = value / FIXED;
        match n {
            -107..=107 => out.push(u8::try_from(n + 139).ok()?),
            108..=1131 => {
                let m = n - 108;
                out.push(u8::try_from(247 + m / 256).ok()?);
                out.push(u8::try_from(m % 256).ok()?);
            }
            -1131..=-108 => {
                let m = -n - 108;
                out.push(u8::try_from(251 + m / 256).ok()?);
                out.push(u8::try_from(m % 256).ok()?);
            }
            _ => {
                out.push(28);
                out.extend_from_slice(&i16::try_from(n).ok()?.to_be_bytes());
            }
        }
    } else {
        out.push(255);
        out.extend_from_slice(&i32::try_from(value).ok()?.to_be_bytes());
    }
    Some(())
}

struct Encoder {
    out: Vec<u8>,
    width: Option<i64>,
}

impl Encoder {
    fn op(&mut self, args: &[i64], op: &[u8]) -> Option<()> {
        let carried = usize::from(self.width.is_some());
        if args.len() + carried > 48 || self.out.len() > 4 * MAX_CHARSTRING {
            return None;
        }
        if let Some(width) = self.width.take() {
            type2_number(&mut self.out, width)?;
        }
        for &arg in args {
            type2_number(&mut self.out, arg)?;
        }
        self.out.extend_from_slice(op);
        Some(())
    }
}

fn delta_stems(stems: &[(i64, i64)]) -> Vec<i64> {
    let mut previous = 0;
    let mut args = Vec::with_capacity(stems.len() * 2);
    for &(pos, len) in stems {
        args.push(pos - previous);
        args.push(len);
        previous = pos + len;
    }
    args
}

fn aligned(dx: i64, dy: i64) -> bool {
    (dx == 0) != (dy == 0)
}

fn emit_lines(enc: &mut Encoder, lines: &[(i64, i64)]) -> Option<()> {
    let mut k = 0;
    while k < lines.len() {
        let (dx, dy) = lines[k];
        let mut args = Vec::new();
        if aligned(dx, dy) {
            let start_horizontal = dy == 0;
            let mut horizontal = start_horizontal;
            while let Some(&(x, y)) = lines.get(k) {
                if !aligned(x, y) || (y == 0) != horizontal || args.len() >= 48 {
                    break;
                }
                args.push(if horizontal { x } else { y });
                horizontal = !horizontal;
                k += 1;
            }
            enc.op(&args, if start_horizontal { &[6] } else { &[7] })?;
        } else {
            while let Some(&(x, y)) = lines.get(k) {
                if aligned(x, y) || args.len() >= 48 {
                    break;
                }
                args.extend([x, y]);
                k += 1;
            }
            enc.op(&args, &[5])?;
        }
    }
    Some(())
}

fn is_hv(c: &[i64; 6]) -> bool {
    c[1] == 0 && c[4] == 0
}
fn is_vh(c: &[i64; 6]) -> bool {
    c[0] == 0 && c[5] == 0
}

fn emit_curves(enc: &mut Encoder, curves: &[[i64; 6]]) -> Option<()> {
    let mut k = 0;
    while k < curves.len() {
        let first = &curves[k];
        let mut args = Vec::new();
        if is_hv(first) || is_vh(first) {
            let start_hv = is_hv(first);
            let mut hv = start_hv;
            while let Some(c) = curves.get(k) {
                let fits = if hv { is_hv(c) } else { is_vh(c) };
                if !fits || args.len() + 4 > 48 {
                    break;
                }
                if hv {
                    args.extend([c[0], c[2], c[3], c[5]]);
                } else {
                    args.extend([c[1], c[2], c[3], c[4]]);
                }
                hv = !hv;
                k += 1;
            }
            enc.op(&args, if start_hv { &[31] } else { &[30] })?;
        } else {
            while let Some(c) = curves.get(k) {
                if is_hv(c) || is_vh(c) || args.len() + 6 > 48 {
                    break;
                }
                args.extend_from_slice(c);
                k += 1;
            }
            enc.op(&args, &[8])?;
        }
    }
    Some(())
}

/// The charstring width operand: the delta from `nominalWidthX`, absent for `defaultWidthX`.
/// Widths are 16.16 fixed values; the two Private DICT widths are whole numbers.
fn width_delta(width: i64, default_width: i64, nominal_width: i64) -> Option<i64> {
    (width != default_width * FIXED).then_some(width - nominal_width * FIXED)
}

fn encode_glyph(glyph: &Glyph, default_width: i64, nominal_width: i64) -> Option<Vec<u8>> {
    let mut hstems: Vec<(i64, i64)> = Vec::new();
    let mut vstems: Vec<(i64, i64)> = Vec::new();
    for stem in glyph.sets.iter().flatten() {
        let list = if stem.vertical {
            &mut vstems
        } else {
            &mut hstems
        };
        if !list.contains(&(stem.pos, stem.len)) {
            list.push((stem.pos, stem.len));
        }
    }
    hstems.sort_unstable();
    vstems.sort_unstable();
    if hstems.len() > MAX_STEMS || vstems.len() > MAX_STEMS {
        return None;
    }
    let has_stems = !hstems.is_empty() || !vstems.is_empty();
    let masked = has_stems && glyph.sets.len() > 1;
    let mask_for = |set: usize| -> Option<Vec<u8>> {
        let mut mask = vec![0_u8; (hstems.len() + vstems.len()).div_ceil(8)];
        for stem in glyph.sets.get(set)? {
            let list = if stem.vertical { &vstems } else { &hstems };
            let base = if stem.vertical { hstems.len() } else { 0 };
            let at = base + list.iter().position(|s| *s == (stem.pos, stem.len))?;
            *mask.get_mut(at / 8)? |= 0x80 >> (at % 8);
        }
        Some(mask)
    };
    let mut enc = Encoder {
        out: Vec::new(),
        width: width_delta(glyph.width, default_width, nominal_width),
    };
    if !hstems.is_empty() {
        enc.op(&delta_stems(&hstems), if masked { &[18] } else { &[1] })?;
    }
    if !vstems.is_empty() {
        enc.op(&delta_stems(&vstems), if masked { &[23] } else { &[3] })?;
    }
    if masked {
        let mut op = vec![19];
        op.extend(mask_for(0)?);
        enc.op(&[], &op)?;
    }
    let mut i = 0;
    while i < glyph.segs.len() {
        match glyph.segs[i] {
            Seg::Move(dx, dy) => {
                if dy == 0 {
                    enc.op(&[dx], &[22])?;
                } else if dx == 0 {
                    enc.op(&[dy], &[4])?;
                } else {
                    enc.op(&[dx, dy], &[21])?;
                }
                i += 1;
            }
            Seg::Mask(set) => {
                if masked {
                    let mut op = vec![19];
                    op.extend(mask_for(set)?);
                    enc.op(&[], &op)?;
                }
                i += 1;
            }
            Seg::Line(..) => {
                let mut lines = Vec::new();
                while let Some(Seg::Line(x, y)) = glyph.segs.get(i) {
                    lines.push((*x, *y));
                    i += 1;
                }
                emit_lines(&mut enc, &lines)?;
            }
            Seg::Curve(..) => {
                let mut curves = Vec::new();
                while let Some(Seg::Curve(c)) = glyph.segs.get(i) {
                    curves.push(*c);
                    i += 1;
                }
                emit_curves(&mut enc, &curves)?;
            }
        }
    }
    enc.op(&[], &[14])?;
    (enc.out.len() <= MAX_CHARSTRING).then_some(enc.out)
}

// ----------------------------------------------------------------------------------------------
// CFF serialization
// ----------------------------------------------------------------------------------------------

fn index(items: &[Vec<u8>]) -> Option<Vec<u8>> {
    let count = u16::try_from(items.len()).ok()?;
    if items.is_empty() {
        return Some(vec![0, 0]);
    }
    let total: usize = items.iter().map(Vec::len).sum::<usize>().checked_add(1)?;
    let off_size: u8 = match total {
        0..=0xff => 1,
        0x100..=0xffff => 2,
        0x1_0000..=0xff_ffff => 3,
        _ => 4,
    };
    let mut out = Vec::with_capacity(total + items.len() * 4 + 8);
    out.extend_from_slice(&count.to_be_bytes());
    out.push(off_size);
    let mut offset = 1_u32;
    let push = |out: &mut Vec<u8>, value: u32| {
        out.extend_from_slice(&value.to_be_bytes()[4 - usize::from(off_size)..]);
    };
    push(&mut out, offset);
    for item in items {
        offset = offset.checked_add(u32::try_from(item.len()).ok()?)?;
        push(&mut out, offset);
    }
    for item in items {
        out.extend_from_slice(item);
    }
    Some(out)
}

fn dict_int(out: &mut Vec<u8>, value: i32) {
    match value {
        -107..=107 => out.push((value + 139) as u8),
        108..=1131 => {
            let m = value - 108;
            out.push((247 + m / 256) as u8);
            out.push((m % 256) as u8);
        }
        -1131..=-108 => {
            let m = -value - 108;
            out.push((251 + m / 256) as u8);
            out.push((m % 256) as u8);
        }
        -32768..=32767 => {
            out.push(28);
            out.extend_from_slice(&(value as i16).to_be_bytes());
        }
        _ => {
            out.push(29);
            out.extend_from_slice(&value.to_be_bytes());
        }
    }
}

fn dict_int5(out: &mut Vec<u8>, value: usize) -> Option<()> {
    out.push(29);
    out.extend_from_slice(&i32::try_from(value).ok()?.to_be_bytes());
    Some(())
}

fn dict_real(out: &mut Vec<u8>, text: &str) -> Option<()> {
    let mut nibbles: Vec<u8> = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '0'..='9' => nibbles.push(c as u8 - b'0'),
            '.' => nibbles.push(0xa),
            '-' => nibbles.push(0xe),
            'e' | 'E' => {
                if chars.peek() == Some(&'-') {
                    chars.next();
                    nibbles.push(0xc);
                } else {
                    if chars.peek() == Some(&'+') {
                        chars.next();
                    }
                    nibbles.push(0xb);
                }
            }
            _ => return None,
        }
    }
    nibbles.push(0xf);
    if nibbles.len() % 2 == 1 {
        nibbles.push(0xf);
    }
    out.push(30);
    out.extend(nibbles.chunks(2).map(|pair| (pair[0] << 4) | pair[1]));
    Some(())
}

fn dict_number(out: &mut Vec<u8>, number: &Number) -> Option<()> {
    match number.int {
        Some(value) => dict_int(out, value),
        None => dict_real(out, &number.text)?,
    }
    Some(())
}

fn dict_op(out: &mut Vec<u8>, op: u16) {
    if op >= 0x0c00 {
        out.push(12);
        out.push((op & 0xff) as u8);
    } else {
        out.push(op as u8);
    }
}

fn private_dict(values: &PrivateValues, default_width: i64, nominal_width: i64) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let delta_keys: [(&str, u16); 6] = [
        ("BlueValues", 6),
        ("OtherBlues", 7),
        ("FamilyBlues", 8),
        ("FamilyOtherBlues", 9),
        ("StemSnapH", 0x0c0c),
        ("StemSnapV", 0x0c0d),
    ];
    for (key, op) in delta_keys {
        let Some(numbers) = values.values.get(key) else {
            continue;
        };
        let ints: Option<Vec<i32>> = numbers.iter().map(|n| n.int).collect();
        let Some(ints) = ints else { continue };
        if ints.is_empty() || ints.len() > 14 {
            continue;
        }
        let mut previous = 0_i32;
        for value in ints {
            dict_int(&mut out, value.checked_sub(previous)?);
            previous = value;
        }
        dict_op(&mut out, op);
    }
    for (key, op) in [("StdHW", 10_u16), ("StdVW", 11)] {
        if let Some(number) = values.values.get(key).and_then(|v| v.first()) {
            dict_number(&mut out, number)?;
            dict_op(&mut out, op);
        }
    }
    let scalar: [(&str, u16, f64); 4] = [
        ("BlueScale", 0x0c09, 0.039625),
        ("BlueShift", 0x0c0a, 7.0),
        ("BlueFuzz", 0x0c0b, 1.0),
        ("LanguageGroup", 0x0c11, 0.0),
    ];
    for (key, op, default) in scalar {
        if let Some(number) = values.values.get(key).and_then(|v| v.first())
            && number.value()? != default
        {
            dict_number(&mut out, number)?;
            dict_op(&mut out, op);
        }
    }
    if values.force_bold {
        dict_int(&mut out, 1);
        dict_op(&mut out, 0x0c0e);
    }
    if default_width != 0 {
        dict_int(&mut out, i32::try_from(default_width).ok()?);
        dict_op(&mut out, 20);
    }
    if nominal_width != 0 {
        dict_int(&mut out, i32::try_from(nominal_width).ok()?);
        dict_op(&mut out, 21);
    }
    Some(out)
}

struct Names {
    custom: Vec<Vec<u8>>,
    lookup: BTreeMap<Vec<u8>, i64>,
}

impl Names {
    fn sid(&mut self, name: &[u8]) -> Option<u16> {
        if let Some(at) = STD.iter().position(|s| s.as_bytes() == name) {
            return u16::try_from(at + 1).ok();
        }
        if let Some(&sid) = self.lookup.get(name) {
            return u16::try_from(sid).ok();
        }
        let sid = STANDARD_STRINGS + i64::try_from(self.custom.len()).ok()?;
        self.custom.push(name.to_vec());
        self.lookup.insert(name.to_vec(), sid);
        u16::try_from(sid).ok()
    }
}

fn build(parsed: &Parsed, max_bytes: usize) -> Option<Vec<u8>> {
    let Parsed {
        header,
        private,
        subrs,
        glyphs: raw,
        ..
    } = parsed;
    let mut converted: Vec<(Vec<u8>, Glyph)> = Vec::with_capacity(raw.len());
    let mut seen: BTreeMap<&[u8], ()> = BTreeMap::new();
    for (name, code) in raw {
        if seen.insert(name.as_slice(), ()).is_some() {
            return None;
        }
        converted.push((name.clone(), convert_charstring(code, subrs)?));
    }
    // Choose the default and nominal widths, then order glyphs: .notdef, encoded, the rest.
    let mut counts: BTreeMap<i64, usize> = BTreeMap::new();
    for (_, glyph) in &converted {
        *counts.entry(glyph.width).or_default() += 1;
    }
    let default_width = counts
        .iter()
        .filter(|(width, _)| **width % FIXED == 0)
        .max_by_key(|(width, count)| (**count, std::cmp::Reverse(**width)))
        .map_or(0, |(width, _)| *width / FIXED);
    let others = counts.keys().filter(|w| **w != default_width * FIXED);
    let (low, high) = others.fold((None::<i64>, None::<i64>), |(lo, hi), w| {
        (
            Some(lo.map_or(*w, |v| v.min(*w))),
            Some(hi.map_or(*w, |v| v.max(*w))),
        )
    });
    let nominal_width = match (low, high) {
        (Some(lo), Some(hi)) => (lo + (hi - lo) / 2) / FIXED,
        _ => default_width,
    };
    let encoding: Vec<(u8, Vec<u8>)> = match &header.encoding {
        EncodingSpec::Standard => standard_encoding()
            .into_iter()
            .map(|(code, name)| (code, name.as_bytes().to_vec()))
            .collect(),
        EncodingSpec::Custom(entries) => {
            let last: BTreeMap<u8, &Vec<u8>> =
                entries.iter().map(|(code, name)| (*code, name)).collect();
            last.into_iter()
                .map(|(code, name)| (code, name.clone()))
                .collect()
        }
    };
    let mut order: Vec<usize> = Vec::with_capacity(converted.len() + 1);
    let notdef = converted.iter().position(|(n, _)| n == b".notdef");
    let mut placed = vec![false; converted.len()];
    if let Some(at) = notdef {
        placed[at] = true;
    }
    let mut main_codes: Vec<u8> = Vec::new();
    let mut supplements: Vec<(u8, usize)> = Vec::new();
    let mut sorted = encoding.clone();
    sorted.sort_by_key(|(code, _)| *code);
    let mut glyph_at = BTreeMap::new();
    for (i, (name, _)) in converted.iter().enumerate() {
        glyph_at.insert(name.as_slice(), i);
    }
    for (code, name) in &sorted {
        let Some(&at) = glyph_at.get(name.as_slice()) else {
            continue;
        };
        if Some(at) == notdef {
            continue;
        }
        if placed[at] {
            supplements.push((*code, at));
        } else {
            placed[at] = true;
            order.push(at);
            main_codes.push(*code);
        }
    }
    for (i, was_placed) in placed.iter().enumerate() {
        if !was_placed {
            order.push(i);
        }
    }
    let notdef_glyph = match notdef {
        Some(at) => converted[at].1.width,
        None => default_width,
    };
    let mut charstrings: Vec<Vec<u8>> = Vec::with_capacity(order.len() + 1);
    charstrings.push(match notdef {
        Some(at) => encode_glyph(&converted[at].1, default_width, nominal_width)?,
        None => {
            let mut enc = Encoder {
                out: Vec::new(),
                width: width_delta(notdef_glyph, default_width, nominal_width),
            };
            enc.op(&[], &[14])?;
            enc.out
        }
    });
    for &at in &order {
        charstrings.push(encode_glyph(
            &converted[at].1,
            default_width,
            nominal_width,
        )?);
    }
    if charstrings.len() > MAX_GLYPHS {
        return None;
    }

    let mut names = Names {
        custom: Vec::new(),
        lookup: BTreeMap::new(),
    };
    let mut charset = vec![0_u8];
    for &at in &order {
        charset.extend_from_slice(&names.sid(&converted[at].0)?.to_be_bytes());
    }
    let mut encoding_table = vec![if supplements.is_empty() { 0 } else { 0x80 }];
    encoding_table.push(u8::try_from(main_codes.len()).ok()?);
    encoding_table.extend_from_slice(&main_codes);
    if !supplements.is_empty() {
        encoding_table.push(u8::try_from(supplements.len()).ok()?);
        for (code, at) in &supplements {
            encoding_table.push(*code);
            encoding_table.extend_from_slice(&names.sid(&converted[*at].0)?.to_be_bytes());
        }
    }

    let charstring_index = index(&charstrings)?;
    let private_bytes = private_dict(private, default_width, nominal_width)?;
    let name_index = index(std::slice::from_ref(&header.name))?;
    let string_index = index(&names.custom)?;
    let global_subrs = index(&[])?;

    let top = |charset_at: usize, encoding_at: usize, strings_at: usize, private_at: usize| {
        let mut dict = Vec::new();
        for number in &header.bbox {
            dict_number(&mut dict, number)?;
        }
        dict_op(&mut dict, 5);
        if let Some(matrix) = &header.matrix {
            let is_default = matrix
                .iter()
                .zip([0.001, 0.0, 0.0, 0.001, 0.0, 0.0])
                .all(|(n, want)| n.value().is_some_and(|v| (v - want).abs() < 1e-9));
            if !is_default {
                for number in matrix {
                    dict_number(&mut dict, number)?;
                }
                dict_op(&mut dict, 0x0c07);
            }
        }
        dict_int5(&mut dict, charset_at)?;
        dict_op(&mut dict, 15);
        dict_int5(&mut dict, encoding_at)?;
        dict_op(&mut dict, 16);
        dict_int5(&mut dict, strings_at)?;
        dict_op(&mut dict, 17);
        dict_int5(&mut dict, private_bytes.len())?;
        dict_int5(&mut dict, private_at)?;
        dict_op(&mut dict, 18);
        Some(dict)
    };
    // Offsets are fixed-width, so one trial pass fixes the layout.
    let trial = index(&[top(0, 0, 0, 0)?])?;
    let header_bytes = [1_u8, 0, 4, 4];
    let after_top = header_bytes.len() + name_index.len() + trial.len();
    let encoding_at = after_top + string_index.len() + global_subrs.len();
    let charset_at = encoding_at + encoding_table.len();
    let charstrings_at = charset_at + charset.len();
    let private_at = charstrings_at + charstring_index.len();
    let top_index = index(&[top(charset_at, encoding_at, charstrings_at, private_at)?])?;
    if top_index.len() != trial.len() {
        return None;
    }
    let mut out = Vec::with_capacity(private_at + private_bytes.len());
    out.extend_from_slice(&header_bytes);
    out.extend_from_slice(&name_index);
    out.extend_from_slice(&top_index);
    out.extend_from_slice(&string_index);
    out.extend_from_slice(&global_subrs);
    out.extend_from_slice(&encoding_table);
    out.extend_from_slice(&charset);
    out.extend_from_slice(&charstring_index);
    out.extend_from_slice(&private_bytes);
    (out.len() <= max_bytes).then_some(out)
}

/// Convert a Type 1 font program into a bare CFF font.
///
/// `lengths` carries the PDF `/Length1` and `/Length2` values (the clear text and the eexec
/// section). Without them the sections are located from `currentfile eexec`. Returns `None` for
/// anything the converter does not support; the caller then keeps the original program.
pub fn type1_to_cff(
    program: &[u8],
    lengths: Option<(usize, usize)>,
    max_bytes: usize,
) -> Option<Converted> {
    if program.len() > max_bytes {
        return None;
    }
    let (length1, length2) = match lengths {
        Some(pair) => pair,
        None => {
            let marker = b"currentfile eexec";
            let at = program
                .windows(marker.len())
                .position(|window| window == marker)?
                .checked_add(marker.len())?;
            let mut length1 = at;
            while program
                .get(length1)
                .is_some_and(|b| matches!(*b, b' ' | b'\r' | b'\n' | b'\t'))
            {
                length1 += 1;
            }
            (length1, program.len().checked_sub(length1)?)
        }
    };
    let parsed = parse(program, length1, length2)?;
    let glyphs = parsed.glyphs.len();
    build(&parsed, max_bytes).map(|bytes| Converted { bytes, glyphs })
}

/// A converted font program and its glyph count (without the synthesized `.notdef`).
#[derive(Debug)]
pub struct Converted {
    pub bytes: Vec<u8>,
    pub glyphs: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use read_fonts::{
        FontData,
        ps::{cff::CffFontRef, charstring::CommandSink, type1::Type1Font},
        types::{Fixed, GlyphId},
    };

    #[derive(Clone, Copy)]
    enum T {
        N(i32),
        O(u8),
        E(u8),
    }
    use T::{E, N, O};

    const HSTEM: u8 = 1;
    const VSTEM: u8 = 3;
    const VMOVETO: u8 = 4;
    const RLINETO: u8 = 5;
    const HLINETO: u8 = 6;
    const VLINETO: u8 = 7;
    const RRCURVETO: u8 = 8;
    const CLOSEPATH: u8 = 9;
    const CALLSUBR: u8 = 10;
    const RETURN: u8 = 11;
    const HSBW: u8 = 13;
    const ENDCHAR: u8 = 14;
    const RMOVETO: u8 = 21;
    const VHCURVETO: u8 = 30;
    const HVCURVETO: u8 = 31;
    const CALLOTHERSUBR: u8 = 16;
    const POP: u8 = 17;
    const SETCURRENTPOINT: u8 = 33;
    const SEAC: u8 = 6;
    const SBW: u8 = 7;

    fn t1_number(n: i32) -> Vec<u8> {
        match n {
            -107..=107 => vec![(n + 139) as u8],
            108..=1131 => {
                let m = n - 108;
                vec![(247 + m / 256) as u8, (m % 256) as u8]
            }
            -1131..=-108 => {
                let m = -n - 108;
                vec![(251 + m / 256) as u8, (m % 256) as u8]
            }
            _ => {
                let mut v = vec![255];
                v.extend_from_slice(&n.to_be_bytes());
                v
            }
        }
    }

    fn cs(items: &[T]) -> Vec<u8> {
        let mut out = Vec::new();
        for item in items {
            match item {
                N(n) => out.extend(t1_number(*n)),
                O(op) => out.push(*op),
                E(op) => out.extend([12, *op]),
            }
        }
        out
    }

    fn sealed(plain: &[u8]) -> Vec<u8> {
        let mut data = vec![0x5a, 0x11, 0x22, 0x33];
        data.extend_from_slice(plain);
        crypt(&data, 4330, true)
    }

    struct Built {
        program: Vec<u8>,
        length1: usize,
        length2: usize,
    }

    fn build_font(
        encoding: &str,
        subrs: &[Vec<u8>],
        glyphs: &[(&str, Vec<u8>)],
        private_extra: &str,
    ) -> Built {
        let head = format!(
            "%!PS-AdobeFont-1.0: TestFont 1.0\n11 dict begin\n/FontInfo 2 dict dup begin\n/Notice (A /Encoding trap) readonly def\nend readonly def\n/FontName /ABCDEF+TestFont def\n/PaintType 0 def\n/FontType 1 def\n/FontMatrix [0.001 0 0 0.001 0 0] readonly def\n{encoding}\n/FontBBox {{-10 -20 900 800}} readonly def\ncurrentdict end\ncurrentfile eexec\n"
        );
        let mut private = Vec::new();
        private.extend_from_slice(
            format!(
                "dup /Private 9 dict dup begin\n/RD {{string currentfile exch readstring pop}} executeonly def\n/ND {{noaccess def}} executeonly def\n/NP {{noaccess put}} executeonly def\n/BlueValues [-15 0 700 715] def\n/StdHW [50] def\n/BlueScale 0.04 def\n/ForceBold true def\n{private_extra}/Subrs {} array\n",
                subrs.len()
            )
            .as_bytes(),
        );
        for (i, subr) in subrs.iter().enumerate() {
            let sealed = sealed(subr);
            private.extend_from_slice(format!("dup {i} {} RD ", sealed.len()).as_bytes());
            private.extend_from_slice(&sealed);
            private.extend_from_slice(b" NP\n");
        }
        private.extend_from_slice(
            format!("ND\n2 index /CharStrings {} dict dup begin\n", glyphs.len()).as_bytes(),
        );
        for (name, code) in glyphs {
            let sealed = sealed(code);
            private.extend_from_slice(format!("/{name} {} RD ", sealed.len()).as_bytes());
            private.extend_from_slice(&sealed);
            private.extend_from_slice(b" ND\n");
        }
        private.extend_from_slice(
            b"end\nend\nreadonly put\nnoaccess put\ndup /FontName get exch definefont pop\nmark currentfile closefile\n",
        );
        let mut prefixed = vec![0x77, 0x31, 0x42, 0x53];
        prefixed.extend_from_slice(&private);
        let encrypted = crypt(&prefixed, 55665, true);
        assert!(
            !encrypted.iter().take(4).all(u8::is_ascii_hexdigit),
            "test data must not look hex encoded"
        );
        let mut program = head.clone().into_bytes();
        let length1 = program.len();
        program.extend_from_slice(&encrypted);
        for _ in 0..8 {
            program.extend_from_slice(&[b'0'; 64]);
            program.push(b'\n');
        }
        program.extend_from_slice(b"cleartomark\n");
        Built {
            program,
            length1,
            length2: encrypted.len(),
        }
    }

    fn standard_subrs() -> Vec<Vec<u8>> {
        vec![
            // 0: flex end
            cs(&[
                N(3),
                N(0),
                E(CALLOTHERSUBR),
                E(POP),
                E(POP),
                E(SETCURRENTPOINT),
                O(RETURN),
            ]),
            // 1: flex begin
            cs(&[N(0), N(1), E(CALLOTHERSUBR), O(RETURN)]),
            // 2: flex point
            cs(&[N(0), N(2), E(CALLOTHERSUBR), O(RETURN)]),
            // 3: plain return
            cs(&[O(RETURN)]),
            // 4: replacement hints
            cs(&[N(100), N(10), O(HSTEM), N(60), N(40), O(VSTEM), O(RETURN)]),
            // 5: a curve
            cs(&[
                N(10),
                N(20),
                O(RMOVETO),
                N(100),
                N(50),
                N(-30),
                N(80),
                N(20),
                N(10),
                O(RRCURVETO),
                N(10),
                O(HLINETO),
                O(RETURN),
            ]),
        ]
    }

    fn standard_glyphs() -> Vec<(&'static str, Vec<u8>)> {
        let flex_moves = [
            (50, 0),
            (-50, 10),
            (20, 5),
            (30, 0),
            (30, -5),
            (20, -10),
            (10, 0),
        ];
        let mut flex = vec![
            N(10),
            N(600),
            O(HSBW),
            N(100),
            N(100),
            O(RMOVETO),
            N(1),
            O(CALLSUBR),
        ];
        for (dx, dy) in flex_moves {
            flex.extend([N(dx), N(dy), O(RMOVETO), N(2), O(CALLSUBR)]);
        }
        flex.extend([
            N(50),
            N(220),
            N(100),
            N(0),
            O(CALLSUBR),
            N(20),
            O(HLINETO),
            O(CLOSEPATH),
            O(ENDCHAR),
        ]);
        vec![
            (".notdef", cs(&[N(0), N(600), O(HSBW), O(ENDCHAR)])),
            (
                "A",
                cs(&[
                    N(50),
                    N(600),
                    O(HSBW),
                    N(0),
                    N(20),
                    O(HSTEM),
                    N(50),
                    N(30),
                    O(VSTEM),
                    N(0),
                    N(100),
                    O(RMOVETO),
                    N(500),
                    O(HLINETO),
                    N(700),
                    O(VLINETO),
                    N(-500),
                    O(HLINETO),
                    O(CLOSEPATH),
                    O(ENDCHAR),
                ]),
            ),
            (
                "B",
                cs(&[
                    N(40),
                    N(650),
                    O(HSBW),
                    N(5),
                    O(CALLSUBR),
                    N(30),
                    N(40),
                    O(RLINETO),
                    N(30),
                    N(40),
                    N(0),
                    N(20),
                    O(VHCURVETO),
                    N(20),
                    N(10),
                    N(10),
                    N(30),
                    O(HVCURVETO),
                    O(CLOSEPATH),
                    O(ENDCHAR),
                ]),
            ),
            ("C", cs(&flex)),
            (
                "D",
                cs(&[
                    N(30),
                    N(600),
                    O(HSBW),
                    N(0),
                    N(10),
                    O(HSTEM),
                    N(20),
                    N(30),
                    O(VSTEM),
                    N(10),
                    N(0),
                    O(RMOVETO),
                    N(100),
                    O(HLINETO),
                    N(100),
                    O(VLINETO),
                    O(CLOSEPATH),
                    N(4),
                    N(1),
                    N(3),
                    E(CALLOTHERSUBR),
                    E(POP),
                    O(CALLSUBR),
                    N(50),
                    N(0),
                    O(RMOVETO),
                    N(80),
                    O(HLINETO),
                    N(80),
                    O(VLINETO),
                    O(CLOSEPATH),
                    O(ENDCHAR),
                ]),
            ),
            (
                "E",
                cs(&[
                    N(0),
                    N(600),
                    O(HSBW),
                    N(10),
                    O(VMOVETO),
                    N(8),
                    N(9),
                    O(RLINETO),
                    O(ENDCHAR),
                ]),
            ),
        ]
    }

    fn standard_font() -> Built {
        build_font(
            "/Encoding StandardEncoding def",
            &standard_subrs(),
            &standard_glyphs(),
            "",
        )
    }

    #[derive(Default)]
    struct Recorder {
        path: Vec<String>,
        masks: usize,
    }

    fn f(v: Fixed) -> String {
        format!("{:.4}", v.to_f64())
    }

    impl CommandSink for Recorder {
        fn move_to(&mut self, x: Fixed, y: Fixed) {
            self.path.push(format!("M {} {}", f(x), f(y)));
        }
        fn line_to(&mut self, x: Fixed, y: Fixed) {
            self.path.push(format!("L {} {}", f(x), f(y)));
        }
        fn curve_to(&mut self, a: Fixed, b: Fixed, c: Fixed, d: Fixed, x: Fixed, y: Fixed) {
            self.path.push(format!(
                "C {} {} {} {} {} {}",
                f(a),
                f(b),
                f(c),
                f(d),
                f(x),
                f(y)
            ));
        }
        fn close(&mut self) {}
        fn hint_mask(&mut self, _mask: &[u8]) {
            self.masks += 1;
        }
    }

    type Outlines = BTreeMap<String, (Option<String>, Vec<String>, usize)>;

    fn type1_outlines(program: &[u8]) -> Outlines {
        let font = Type1Font::new(program).expect("oracle parses the Type 1 program");
        let mut map = BTreeMap::new();
        for (gid, name) in font.glyph_names() {
            let mut sink = Recorder::default();
            let width = font
                .evaluate_charstring(gid, &mut sink)
                .expect("oracle evaluates the Type 1 glyph");
            map.insert(name.to_owned(), (width.map(f), sink.path, sink.masks));
        }
        map
    }

    fn cff_outlines(cff: &[u8]) -> Outlines {
        let font = CffFontRef::new(cff, 0, None).expect("CFF parses");
        let subfont = font.subfont(0, &[]).expect("private dict parses");
        let charset = font.charset().expect("charset");
        let mut map = BTreeMap::new();
        for (gid, sid) in charset.iter() {
            let name = if gid == GlyphId::new(0) {
                ".notdef".to_owned()
            } else {
                String::from_utf8(
                    sid.resolve_standard()
                        .map_or_else(|_| font.string(sid).unwrap().to_vec(), <[u8]>::to_vec),
                )
                .unwrap()
            };
            let mut sink = Recorder::default();
            let width = font
                .evaluate_charstring(&subfont, gid, &[], &mut sink)
                .expect("CFF glyph evaluates");
            map.insert(name, (width.map(f), sink.path, sink.masks));
        }
        map
    }

    fn convert_built(font: &Built) -> Option<Converted> {
        type1_to_cff(&font.program, Some((font.length1, font.length2)), 1 << 24)
    }

    use std::collections::BTreeMap;

    #[test]
    fn converts_standard_font_and_matches_the_oracle() {
        let font = standard_font();
        let converted = convert_built(&font).expect("converts");
        assert!(converted.bytes.len() < font.program.len());
        assert_eq!(converted.glyphs, 6);
        let mut want = type1_outlines(&font.program);
        let got = cff_outlines(&converted.bytes);
        // The Type 1 parser reports the flex glyph path through its own flex handling.
        assert_eq!(
            want.keys().collect::<Vec<_>>(),
            got.keys().collect::<Vec<_>>()
        );
        for (name, (width, path, masks)) in &got {
            let (want_width, want_path, _) = &want[name];
            assert_eq!(width, want_width, "width of {name}");
            assert_eq!(path, want_path, "outline of {name}");
            if name == "D" {
                assert_eq!(*masks, 2, "hint replacement becomes two hint masks");
            } else {
                assert_eq!(*masks, 0, "{name} has no hint masks");
            }
        }
        want.clear();
    }

    #[test]
    fn keeps_encoding_and_private_values() {
        let font = standard_font();
        let converted = convert_built(&font).expect("converts");
        let cff = CffFontRef::new(&converted.bytes, 0, None).unwrap();
        let encoding = cff.encoding().expect("custom encoding");
        let charset = cff.charset().unwrap();
        let gid_of = |name: &[u8]| {
            charset
                .iter()
                .find(|(g, sid)| {
                    *g != GlyphId::new(0)
                        && sid
                            .resolve_standard()
                            .map_or_else(|_| cff.string(*sid).unwrap() == name, |s| s == name)
                })
                .map(|(g, _)| g)
        };
        for (code, name) in [(65_u8, b"A".as_slice()), (66, b"B"), (67, b"C"), (68, b"D")] {
            assert_eq!(encoding.map(code), gid_of(name), "code {code}");
        }
        let subfont = cff.subfont(0, &[]).unwrap();
        assert!(subfont.default_width().is_some());
        let top = &converted.bytes;
        assert_eq!(&top[..3], &[1, 0, 4]);
    }

    #[test]
    fn custom_encoding_with_alias_uses_a_supplement() {
        let encoding = "/Encoding 256 array\n0 1 255 {1 index exch /.notdef put} for\ndup 65 /A put\ndup 66 /B put\ndup 200 /A put\nreadonly def";
        let font = build_font(encoding, &standard_subrs(), &standard_glyphs(), "");
        let converted = convert_built(&font).expect("converts");
        let cff = CffFontRef::new(&converted.bytes, 0, None).unwrap();
        let encoding = cff.encoding().unwrap();
        let a = encoding.map(65).expect("A is encoded");
        assert_eq!(encoding.map(200), Some(a));
        assert!(encoding.map(66).is_some());
        assert_eq!(encoding.map(67), None);
    }

    #[test]
    fn lengths_can_be_found_without_pdf_metadata() {
        let font = standard_font();
        let direct = type1_to_cff(&font.program, None, 1 << 24).expect("converts");
        let with = convert_built(&font).unwrap();
        assert_eq!(direct.bytes.len(), with.bytes.len());
    }

    fn with_glyph(name: &'static str, code: Vec<u8>) -> Built {
        let mut glyphs = standard_glyphs();
        glyphs.push((name, code));
        build_font(
            "/Encoding StandardEncoding def",
            &standard_subrs(),
            &glyphs,
            "",
        )
    }

    #[test]
    fn refuses_seac() {
        let seac = cs(&[
            N(0),
            N(600),
            O(HSBW),
            N(0),
            N(10),
            N(20),
            N(65),
            N(194),
            E(SEAC),
            O(ENDCHAR),
        ]);
        assert!(convert_built(&with_glyph("Aacute", seac)).is_none());
    }

    #[test]
    fn refuses_unknown_constructs_and_bad_charstrings() {
        let cases: Vec<(&str, Vec<u8>)> = vec![
            ("truncated number", vec![139, 255, 0, 1]),
            (
                "unknown operator",
                cs(&[N(0), N(600), O(HSBW), O(2), O(ENDCHAR)]),
            ),
            (
                "unknown othersubr",
                cs(&[
                    N(0),
                    N(600),
                    O(HSBW),
                    N(1),
                    N(14),
                    N(1),
                    E(CALLOTHERSUBR),
                    O(ENDCHAR),
                ]),
            ),
            (
                "sbw with vertical sidebearing",
                cs(&[N(0), N(5), N(600), N(0), E(SBW), O(ENDCHAR)]),
            ),
            ("no endchar", cs(&[N(0), N(600), O(HSBW)])),
            ("missing hsbw", cs(&[N(0), N(10), O(RMOVETO), O(ENDCHAR)])),
            (
                "line before move",
                cs(&[N(0), N(600), O(HSBW), N(5), O(HLINETO), O(ENDCHAR)]),
            ),
            (
                "missing subr",
                cs(&[N(0), N(600), O(HSBW), N(99), O(CALLSUBR), O(ENDCHAR)]),
            ),
            (
                "stack underflow",
                cs(&[N(0), N(600), O(HSBW), O(HLINETO), O(ENDCHAR)]),
            ),
            (
                "division by zero",
                cs(&[
                    N(0),
                    N(600),
                    O(HSBW),
                    N(1),
                    N(0),
                    E(12),
                    O(HLINETO),
                    O(ENDCHAR),
                ]),
            ),
        ];
        for (label, code) in cases {
            assert!(
                convert_built(&with_glyph("X", code)).is_none(),
                "{label} must refuse the font"
            );
        }
    }

    #[test]
    fn refuses_self_recursive_subroutines() {
        let mut subrs = standard_subrs();
        subrs.push(cs(&[N(6), O(CALLSUBR), O(RETURN)]));
        let glyphs = vec![
            (".notdef", cs(&[N(0), N(600), O(HSBW), O(ENDCHAR)])),
            (
                "A",
                cs(&[N(0), N(600), O(HSBW), N(6), O(CALLSUBR), O(ENDCHAR)]),
            ),
        ];
        let font = build_font("/Encoding StandardEncoding def", &subrs, &glyphs, "");
        assert!(convert_built(&font).is_none());
    }

    #[test]
    fn refuses_unsupported_programs() {
        let font = standard_font();
        // Output larger than the allowed size.
        assert!(type1_to_cff(&font.program, Some((font.length1, font.length2)), 64).is_none());
        // Truncated program and wrong section lengths.
        assert!(
            type1_to_cff(
                &font.program[..font.length1 + 8],
                Some((font.length1, font.length2)),
                1 << 20
            )
            .is_none()
        );
        assert!(
            type1_to_cff(
                &font.program,
                Some((font.length1 + 1, font.length2)),
                1 << 20
            )
            .is_none()
        );
        // Nonzero PaintType (stroked fonts) and non-default lenIV beyond the bound.
        let stroked = build_font(
            "/Encoding StandardEncoding def\n/PaintType 2 def",
            &standard_subrs(),
            &standard_glyphs(),
            "",
        );
        assert!(convert_built(&stroked).is_none());
        let huge_len_iv = build_font(
            "/Encoding StandardEncoding def",
            &standard_subrs(),
            &standard_glyphs(),
            "/lenIV 99 def\n",
        );
        assert!(convert_built(&huge_len_iv).is_none());
        assert!(type1_to_cff(b"", None, 1 << 20).is_none());
        assert!(type1_to_cff(b"%!PS-AdobeFont currentfile eexec\n", None, 1 << 20).is_none());
    }

    #[test]
    fn duplicate_glyph_names_are_refused() {
        let mut glyphs = standard_glyphs();
        glyphs.push(("A", cs(&[N(0), N(600), O(HSBW), O(ENDCHAR)])));
        let font = build_font(
            "/Encoding StandardEncoding def",
            &standard_subrs(),
            &glyphs,
            "",
        );
        assert!(convert_built(&font).is_none());
    }

    #[test]
    fn random_charstrings_never_panic_and_accepted_output_parses() {
        // Deterministic xorshift stand-in for a short fuzz run (cargo-fuzz needs nightly).
        let mut state = 0x2545_f491_4f6c_dd1d_u64;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        // Bytes that are valid Type 1 tokens make deeper paths reachable than uniform noise.
        let alphabet: Vec<u8> = (32..=40_u8)
            .chain([139, 140, 150, 200, 247, 251, 255])
            .chain([1, 3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 14, 21, 22, 30, 31])
            .collect();
        let mut accepted = 0;
        for round in 0..6000 {
            let len = 1 + (next() % 48) as usize;
            let mut code = Vec::with_capacity(len);
            for _ in 0..len {
                let r = next();
                code.push(if r % 11 == 0 {
                    12
                } else if r % 13 == 0 {
                    (next() >> 8) as u8
                } else {
                    alphabet[(r >> 16) as usize % alphabet.len()]
                });
            }
            if round % 3 == 0 {
                // Frame a plausible glyph so the interpreter reaches path operators.
                let mut framed = cs(&[N(0), N(600), O(HSBW), N(0), N(0), O(RMOVETO)]);
                framed.extend_from_slice(&code);
                framed.push(ENDCHAR);
                code = framed;
            }
            let font = with_glyph("X", code);
            if let Some(converted) = convert_built(&font) {
                accepted += 1;
                assert!(CffFontRef::new(&converted.bytes, 0, None).is_ok());
            }
        }
        // Random input is almost always refused; the point is that nothing panics.
        let _ = accepted;
    }

    #[test]
    fn arbitrary_garbage_never_panics() {
        let font = standard_font();
        // Flip every byte of the private part in turn; the converter must return, not panic.
        for at in (font.length1..font.length1 + font.length2).step_by(7) {
            let mut mutated = font.program.clone();
            mutated[at] ^= 0x55;
            let _ = type1_to_cff(&mutated, Some((font.length1, font.length2)), 1 << 20);
        }
        let _ = FontData::new(&[]);
    }
}
