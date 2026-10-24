//! Tokenizer for PDF content streams and CMap programs.
//!
//! Both use PostScript-like syntax. The lexer never fails: bytes that do not start a token are
//! skipped, and unterminated strings end at the end of the data. It works on a byte slice and a
//! caller-owned position so that interpretation can be suspended and resumed across nested form
//! XObjects without recursion.

/// One lexical token.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Token<'a> {
    Number(f64),
    Name(Vec<u8>),
    String(Vec<u8>),
    ArrayStart,
    ArrayEnd,
    DictStart,
    DictEnd,
    /// An operator or another bare keyword such as `true`, `null` or `begincmap`.
    Keyword(&'a [u8]),
}

/// Whether `byte` is PDF whitespace.
pub(super) const fn is_whitespace(byte: u8) -> bool {
    matches!(byte, b'\0' | b'\t' | b'\n' | b'\x0C' | b'\r' | b' ')
}

/// Whether `byte` is a PDF delimiter.
const fn is_delimiter(byte: u8) -> bool {
    matches!(
        byte,
        b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
    )
}

const fn is_regular(byte: u8) -> bool {
    !is_whitespace(byte) && !is_delimiter(byte)
}

const fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// Returns the next token at or after `*pos`, or `None` at the end of the data.
pub(super) fn next_token<'a>(data: &'a [u8], pos: &mut usize) -> Option<Token<'a>> {
    loop {
        let byte = *data.get(*pos)?;
        match byte {
            _ if is_whitespace(byte) => *pos += 1,
            b'%' => {
                while let Some(&next) = data.get(*pos) {
                    if next == b'\n' || next == b'\r' {
                        break;
                    }
                    *pos += 1;
                }
            }
            b'(' => {
                *pos += 1;
                return Some(Token::String(literal_string(data, pos)));
            }
            b'<' => {
                if data.get(*pos + 1) == Some(&b'<') {
                    *pos += 2;
                    return Some(Token::DictStart);
                }
                *pos += 1;
                return Some(Token::String(hex_string(data, pos)));
            }
            b'>' => {
                *pos += 1;
                if data.get(*pos) == Some(&b'>') {
                    *pos += 1;
                    return Some(Token::DictEnd);
                }
                // A stray `>` carries no meaning; skip it.
            }
            b'[' => {
                *pos += 1;
                return Some(Token::ArrayStart);
            }
            b']' => {
                *pos += 1;
                return Some(Token::ArrayEnd);
            }
            b'/' => {
                *pos += 1;
                return Some(Token::Name(name(data, pos)));
            }
            b'{' | b'}' | b')' => *pos += 1,
            b'0'..=b'9' | b'+' | b'-' | b'.' => {
                let start = *pos;
                while data.get(*pos).is_some_and(|&next| is_regular(next)) {
                    *pos += 1;
                }
                return Some(Token::Number(parse_number(&data[start..*pos])));
            }
            _ => {
                let start = *pos;
                while data.get(*pos).is_some_and(|&next| is_regular(next)) {
                    *pos += 1;
                }
                return Some(Token::Keyword(&data[start..*pos]));
            }
        }
    }
}

/// Parses a PDF number leniently: a sign, digits and at most one decimal point. Anything after
/// the first invalid character is ignored, and an empty number reads as zero.
fn parse_number(bytes: &[u8]) -> f64 {
    let mut index = 0;
    let mut negative = false;
    while let Some(&sign) = bytes.get(index) {
        match sign {
            b'-' => negative = !negative,
            b'+' => {}
            _ => break,
        }
        index += 1;
    }
    let mut value = 0.0_f64;
    let mut scale = 0.0_f64;
    for &byte in &bytes[index..] {
        match byte {
            b'0'..=b'9' => {
                let digit = f64::from(byte - b'0');
                if scale == 0.0 {
                    value = value * 10.0 + digit;
                } else {
                    value += digit * scale;
                    scale /= 10.0;
                }
            }
            b'.' if scale == 0.0 => scale = 0.1,
            _ => break,
        }
    }
    let value = if value.is_finite() { value } else { 0.0 };
    if negative { -value } else { value }
}

fn name(data: &[u8], pos: &mut usize) -> Vec<u8> {
    let mut out = Vec::new();
    while let Some(&byte) = data.get(*pos) {
        if !is_regular(byte) {
            break;
        }
        *pos += 1;
        if byte == b'#'
            && let (Some(high), Some(low)) = (
                data.get(*pos).copied().and_then(hex_value),
                data.get(*pos + 1).copied().and_then(hex_value),
            )
        {
            out.push(high << 4 | low);
            *pos += 2;
        } else {
            out.push(byte);
        }
    }
    out
}

fn literal_string(data: &[u8], pos: &mut usize) -> Vec<u8> {
    let mut out = Vec::new();
    let mut depth = 1_usize;
    while let Some(&byte) = data.get(*pos) {
        *pos += 1;
        match byte {
            b'(' => {
                depth += 1;
                out.push(byte);
            }
            b')' => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
                out.push(byte);
            }
            b'\\' => {
                let Some(&escaped) = data.get(*pos) else {
                    break;
                };
                *pos += 1;
                match escaped {
                    b'n' => out.push(b'\n'),
                    b'r' => out.push(b'\r'),
                    b't' => out.push(b'\t'),
                    b'b' => out.push(0x08),
                    b'f' => out.push(0x0C),
                    b'0'..=b'7' => {
                        let mut value = u32::from(escaped - b'0');
                        for _ in 0..2 {
                            match data.get(*pos) {
                                Some(&digit @ b'0'..=b'7') => {
                                    value = value * 8 + u32::from(digit - b'0');
                                    *pos += 1;
                                }
                                _ => break,
                            }
                        }
                        out.push((value & 0xFF) as u8);
                    }
                    b'\r' => {
                        if data.get(*pos) == Some(&b'\n') {
                            *pos += 1;
                        }
                    }
                    b'\n' => {}
                    other => out.push(other),
                }
            }
            _ => out.push(byte),
        }
    }
    out
}

fn hex_string(data: &[u8], pos: &mut usize) -> Vec<u8> {
    let mut out = Vec::new();
    let mut high: Option<u8> = None;
    while let Some(&byte) = data.get(*pos) {
        *pos += 1;
        if byte == b'>' {
            break;
        }
        let Some(value) = hex_value(byte) else {
            continue;
        };
        match high.take() {
            Some(high) => out.push(high << 4 | value),
            None => high = Some(value),
        }
    }
    if let Some(high) = high {
        out.push(high << 4);
    }
    out
}

/// Skips the parameters and data of an inline image that starts right after a `BI` operator.
///
/// Leaves `*pos` after the closing `EI`, or at the end of the data when no end marker exists.
pub(super) fn skip_inline_image(data: &[u8], pos: &mut usize) {
    let mut params = InlineImageParams::default();
    let mut key: Option<Vec<u8>> = None;
    loop {
        let Some(token) = next_token(data, pos) else {
            return;
        };
        match token {
            Token::Keyword(b"ID") => break,
            Token::Keyword(b"EI") => return,
            Token::Name(name) => match key.take() {
                Some(previous) => {
                    params.set_name(&previous, &name);
                }
                None => key = Some(name),
            },
            Token::Number(value) => {
                if let Some(previous) = key.take() {
                    params.set_number(&previous, value);
                }
            }
            Token::ArrayStart => {
                // Array values (filters, decode arrays, indexed colour spaces) are skipped; an
                // indexed colour space still has one component per sample.
                if let Some(previous) = key.take()
                    && matches!(previous.as_slice(), b"CS" | b"ColorSpace")
                {
                    params.components = Some(1);
                }
                skip_array(data, pos);
            }
            Token::Keyword(b"true") => {
                if let Some(previous) = key.take()
                    && matches!(previous.as_slice(), b"IM" | b"ImageMask")
                {
                    params.image_mask = true;
                }
            }
            _ => key = None,
        }
    }
    // A single whitespace byte separates `ID` from the image data.
    if data.get(*pos).is_some_and(|&byte| is_whitespace(byte)) {
        *pos += 1;
    }
    let start = *pos;
    if let Some(length) = params.data_length()
        && let Some(end) = start.checked_add(length)
        && end <= data.len()
    {
        let mut after = end;
        while data.get(after).is_some_and(|&byte| is_whitespace(byte)) {
            after += 1;
        }
        if is_end_marker(data, after) {
            *pos = after + 2;
            return;
        }
    }
    // Fall back to the first `EI` that stands alone between whitespace or delimiters.
    let mut index = start;
    while index + 1 < data.len() {
        if data[index] == b'E'
            && data[index + 1] == b'I'
            && (index == start || is_whitespace(data[index - 1]))
            && is_end_marker(data, index)
        {
            *pos = index + 2;
            return;
        }
        index += 1;
    }
    *pos = data.len();
}

fn is_end_marker(data: &[u8], index: usize) -> bool {
    data.get(index) == Some(&b'E')
        && data.get(index + 1) == Some(&b'I')
        && data
            .get(index + 2)
            .is_none_or(|&byte| is_whitespace(byte) || is_delimiter(byte))
}

/// Skips tokens up to and including the `]` that closes an array whose `[` was already read.
pub(super) fn skip_array(data: &[u8], pos: &mut usize) {
    let mut depth = 1_usize;
    while let Some(token) = next_token(data, pos) {
        match token {
            Token::ArrayStart => depth += 1,
            Token::ArrayEnd => {
                depth -= 1;
                if depth == 0 {
                    return;
                }
            }
            _ => {}
        }
    }
}

/// Skips tokens up to and including the `>>` that closes a dictionary whose `<<` was already read.
pub(super) fn skip_dict(data: &[u8], pos: &mut usize) {
    let mut depth = 1_usize;
    while let Some(token) = next_token(data, pos) {
        match token {
            Token::DictStart => depth += 1,
            Token::DictEnd => {
                depth -= 1;
                if depth == 0 {
                    return;
                }
            }
            _ => {}
        }
    }
}

#[derive(Default)]
struct InlineImageParams {
    width: Option<f64>,
    height: Option<f64>,
    bits: Option<f64>,
    components: Option<u64>,
    filtered: bool,
    image_mask: bool,
    length: Option<f64>,
}

impl InlineImageParams {
    fn set_name(&mut self, key: &[u8], value: &[u8]) {
        match key {
            b"CS" | b"ColorSpace" => {
                self.components = Some(match value {
                    b"RGB" | b"DeviceRGB" | b"CalRGB" | b"Lab" => 3,
                    b"CMYK" | b"DeviceCMYK" => 4,
                    _ => 1,
                });
            }
            b"F" | b"Filter" => self.filtered = true,
            _ => {}
        }
    }

    fn set_number(&mut self, key: &[u8], value: f64) {
        match key {
            b"W" | b"Width" => self.width = Some(value),
            b"H" | b"Height" => self.height = Some(value),
            b"BPC" | b"BitsPerComponent" => self.bits = Some(value),
            b"L" | b"Length" => self.length = Some(value),
            _ => {}
        }
    }

    /// The exact data length when it can be known without decoding.
    fn data_length(&self) -> Option<usize> {
        if let Some(length) = self.length {
            return as_count(length).and_then(|length| usize::try_from(length).ok());
        }
        if self.filtered {
            return None;
        }
        let width = as_count(self.width?)?;
        let height = as_count(self.height?)?;
        let (bits, components) = if self.image_mask {
            (1, 1)
        } else {
            (as_count(self.bits?)?, self.components.unwrap_or(1))
        };
        let row_bits = width.checked_mul(bits)?.checked_mul(components)?;
        let row_bytes = row_bits.div_ceil(8);
        usize::try_from(row_bytes.checked_mul(height)?).ok()
    }
}

fn as_count(value: f64) -> Option<u64> {
    (value.is_finite() && (0.0..=1e12).contains(&value)).then_some(value as u64)
}

#[cfg(test)]
mod tests {
    use super::{Token, next_token, skip_inline_image};

    fn tokens(data: &[u8]) -> Vec<Token<'_>> {
        let mut pos = 0;
        std::iter::from_fn(|| next_token(data, &mut pos)).collect()
    }

    #[test]
    fn reads_operands_and_operators() {
        assert_eq!(
            tokens(b"BT /F1 12 Tf 1 0 0 1 -.5 +3. Tm [(A) -250 <0041>] TJ ET"),
            vec![
                Token::Keyword(b"BT"),
                Token::Name(b"F1".to_vec()),
                Token::Number(12.0),
                Token::Keyword(b"Tf"),
                Token::Number(1.0),
                Token::Number(0.0),
                Token::Number(0.0),
                Token::Number(1.0),
                Token::Number(-0.5),
                Token::Number(3.0),
                Token::Keyword(b"Tm"),
                Token::ArrayStart,
                Token::String(b"A".to_vec()),
                Token::Number(-250.0),
                Token::String(vec![0x00, 0x41]),
                Token::ArrayEnd,
                Token::Keyword(b"TJ"),
                Token::Keyword(b"ET"),
            ]
        );
    }

    #[test]
    fn decodes_string_escapes_and_names() {
        assert_eq!(
            tokens(b"(a\\(b\\)c\\101\\\n(d)) /A#20B <4 1 4>"),
            vec![
                Token::String(b"a(b)cA(d)".to_vec()),
                Token::Name(b"A B".to_vec()),
                Token::String(vec![0x41, 0x40]),
            ]
        );
    }

    #[test]
    fn tolerates_garbage() {
        let data = b"\xff\xfe)) >> -- 1.2.3 % comment\n(unterminated";
        let all = tokens(data);
        assert_eq!(all[0], Token::Keyword(b"\xff\xfe"));
        assert_eq!(all[1], Token::DictEnd);
        assert_eq!(all[2], Token::Number(0.0));
        assert_eq!(all[3], Token::Number(1.2));
        assert_eq!(all[4], Token::String(b"unterminated".to_vec()));
    }

    #[test]
    fn skips_inline_images_by_length_and_by_marker() {
        let data = b"BI /W 2 /H 2 /BPC 8 /CS /G ID \x00EI\x01 EI Q";
        let mut pos = 2;
        skip_inline_image(data, &mut pos);
        assert_eq!(next_token(data, &mut pos), Some(Token::Keyword(b"Q")));

        let data = b"BI /W 9 /H 9 /F /DCT ID abc EIx EI Q";
        let mut pos = 2;
        skip_inline_image(data, &mut pos);
        assert_eq!(next_token(data, &mut pos), Some(Token::Keyword(b"Q")));

        let data = b"BI /W 9 ID never ends";
        let mut pos = 2;
        skip_inline_image(data, &mut pos);
        assert_eq!(pos, data.len());
    }
}
