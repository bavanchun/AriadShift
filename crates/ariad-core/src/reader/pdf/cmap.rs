//! Bounded parser for CMap programs: codespace ranges and `bfchar`/`bfrange` Unicode mappings.
//!
//! `lopdf` has a ToUnicode parser, but it expands every range into a reverse map, so a single
//! `<00000000> <FFFFFFFF>` range costs billions of steps. This parser keeps ranges compact and
//! caps the number of entries it accepts.

use std::collections::HashMap;

use super::lexer::{Token, next_token};

/// Most mapping entries (single codes, ranges and range array elements) one CMap may define.
const MAX_ENTRIES: usize = 262_144;

/// Most UTF-16 code units one mapping target may hold.
const MAX_TARGET_UNITS: usize = 32;

#[derive(Clone, Copy, Debug)]
struct CodespaceRange {
    len: u8,
    low: [u8; 4],
    high: [u8; 4],
}

#[derive(Debug)]
enum RangeTarget {
    /// The target of the first code; later codes increment its last UTF-16 unit.
    Increment(Vec<u16>),
    /// One target per code, starting at the range's first code.
    List(Vec<Vec<u16>>),
}

#[derive(Debug)]
struct Range {
    len: u8,
    low: u32,
    high: u32,
    target: RangeTarget,
}

/// A parsed CMap.
#[derive(Debug, Default)]
pub(super) struct CMap {
    codespace: Vec<CodespaceRange>,
    singles: HashMap<(u8, u32), Vec<u16>>,
    /// Sorted by `(len, low)`.
    ranges: Vec<Range>,
    strict_invalid: bool,
}

impl CMap {
    /// Parses a CMap program, keeping whatever was readable before any malformed section.
    pub(super) fn parse(data: &[u8]) -> Self {
        let mut cmap = Self::default();
        let mut entries = 0_usize;
        let mut pos = 0;
        while let Some(token) = next_token(data, &mut pos) {
            let Token::Keyword(keyword) = token else {
                continue;
            };
            match keyword {
                b"begincodespacerange" => cmap.parse_codespace(data, &mut pos),
                b"beginbfchar" => cmap.parse_bfchar(data, &mut pos, &mut entries),
                b"beginbfrange" => cmap.parse_bfrange(data, &mut pos, &mut entries),
                _ => {}
            }
        }
        cmap.ranges.sort_by_key(|range| (range.len, range.low));
        if cmap
            .ranges
            .windows(2)
            .any(|pair| pair[0].len == pair[1].len && pair[1].low <= pair[0].high)
        {
            cmap.strict_invalid = true;
        }
        cmap
    }

    fn parse_codespace(&mut self, data: &[u8], pos: &mut usize) {
        let mut low: Option<Vec<u8>> = None;
        let mut ended = false;
        while let Some(token) = next_token(data, pos) {
            match token {
                Token::String(bytes) => match low.take() {
                    None => low = Some(bytes),
                    Some(first) => {
                        if (1..=4).contains(&first.len())
                            && first.len() == bytes.len()
                            && self.codespace.len() < 256
                        {
                            let mut range = CodespaceRange {
                                len: first.len() as u8,
                                low: [0; 4],
                                high: [0; 4],
                            };
                            range.low[..first.len()].copy_from_slice(&first);
                            range.high[..bytes.len()].copy_from_slice(&bytes);
                            self.codespace.push(range);
                        } else {
                            self.strict_invalid = true;
                        }
                    }
                },
                Token::Keyword(keyword) => {
                    self.strict_invalid |= keyword != b"endcodespacerange" || low.is_some();
                    ended = true;
                    break;
                }
                _ => self.strict_invalid = true,
            }
        }
        self.strict_invalid |= !ended;
    }

    fn parse_bfchar(&mut self, data: &[u8], pos: &mut usize, entries: &mut usize) {
        let mut source: Option<Vec<u8>> = None;
        let mut ended = false;
        while let Some(token) = next_token(data, pos) {
            match token {
                Token::String(bytes) => match source.take() {
                    None => source = Some(bytes),
                    Some(code) => {
                        self.strict_invalid |= !valid_target_bytes(&bytes);
                        if let Some((len, code)) = code_value(&code)
                            && *entries < MAX_ENTRIES
                        {
                            *entries += 1;
                            if self
                                .singles
                                .insert((len, code), utf16_units(&bytes))
                                .is_some()
                            {
                                self.strict_invalid = true;
                            }
                        } else {
                            self.strict_invalid = true;
                        }
                    }
                },
                // A glyph name as target is not resolvable here; drop the pair.
                Token::Name(_) => {
                    self.strict_invalid = true;
                    source = None;
                }
                Token::Keyword(keyword) => {
                    self.strict_invalid |= keyword != b"endbfchar" || source.is_some();
                    ended = true;
                    break;
                }
                _ => self.strict_invalid = true,
            }
        }
        self.strict_invalid |= !ended;
    }

    fn parse_bfrange(&mut self, data: &[u8], pos: &mut usize, entries: &mut usize) {
        let mut codes: Vec<Vec<u8>> = Vec::with_capacity(2);
        let mut ended = false;
        while let Some(token) = next_token(data, pos) {
            match token {
                Token::String(bytes) if codes.len() < 2 => codes.push(bytes),
                Token::String(bytes) => {
                    self.strict_invalid |= !valid_target_bytes(&bytes);
                    let target = RangeTarget::Increment(utf16_units(&bytes));
                    self.push_range(&codes, target, entries);
                    codes.clear();
                }
                Token::ArrayStart => {
                    let (list, invalid) = target_list(data, pos);
                    self.strict_invalid |= invalid;
                    if codes.len() == 2 {
                        *entries = entries.saturating_add(list.len());
                        self.push_range(&codes, RangeTarget::List(list), entries);
                    } else {
                        self.strict_invalid = true;
                    }
                    codes.clear();
                }
                Token::Keyword(keyword) => {
                    self.strict_invalid |= keyword != b"endbfrange" || !codes.is_empty();
                    ended = true;
                    break;
                }
                _ => {
                    self.strict_invalid = true;
                    codes.clear();
                }
            }
        }
        self.strict_invalid |= !ended;
    }

    fn push_range(&mut self, codes: &[Vec<u8>], target: RangeTarget, entries: &mut usize) {
        let [low, high] = codes else {
            self.strict_invalid = true;
            return;
        };
        let (Some((len, low)), Some((high_len, high))) = (code_value(low), code_value(high)) else {
            self.strict_invalid = true;
            return;
        };
        if len != high_len || high < low || *entries >= MAX_ENTRIES {
            self.strict_invalid = true;
            return;
        }
        *entries += 1;
        self.ranges.push(Range {
            len,
            low,
            high,
            target,
        });
    }

    /// Whether the CMap declares codespace ranges.
    pub(super) fn has_codespace(&self) -> bool {
        !self.codespace.is_empty()
    }

    /// Approximate heap retained by codespace ranges, mappings and their UTF-16 targets.
    pub(super) fn heap_bytes(&self) -> usize {
        let unit = std::mem::size_of::<u16>();
        let target = std::mem::size_of::<Vec<u16>>();
        let singles = self.singles.capacity() * (std::mem::size_of::<(u8, u32)>() + target)
            + self.singles.values().map(Vec::capacity).sum::<usize>() * unit;
        let ranges = self.ranges.capacity() * std::mem::size_of::<Range>()
            + self
                .ranges
                .iter()
                .map(|range| match &range.target {
                    RangeTarget::Increment(units) => units.capacity() * unit,
                    RangeTarget::List(targets) => {
                        targets.capacity() * target
                            + targets.iter().map(Vec::capacity).sum::<usize>() * unit
                    }
                })
                .sum::<usize>();
        self.codespace.capacity() * std::mem::size_of::<CodespaceRange>() + singles + ranges
    }

    /// Splits the next character code off `bytes` by the codespace ranges.
    ///
    /// Returns the code and its byte length, or `None` when the CMap has no codespace. Bytes
    /// matching no range consume the shortest declared code length.
    pub(super) fn next_code(&self, bytes: &[u8]) -> Option<(u32, usize)> {
        if bytes.is_empty() {
            return None;
        }
        let shortest = self.codespace.iter().map(|range| range.len).min()?;
        if let Some(code) = self.match_code(bytes, false) {
            return Some(code);
        }
        let len = usize::from(shortest).min(bytes.len()).max(1);
        code_value(&bytes[..len]).map(|(_, code)| (code, len))
    }

    /// Splits only codes actually covered by a declared codespace range.
    pub(super) fn next_code_strict(&self, bytes: &[u8]) -> Option<(u32, usize)> {
        if self.strict_invalid {
            return None;
        }
        self.match_code(bytes, true)
    }

    fn match_code(&self, bytes: &[u8], reject_ambiguity: bool) -> Option<(u32, usize)> {
        let mut found = None;
        for len in 1..=bytes.len().min(4) {
            let prefix = &bytes[..len];
            let matched = self.codespace.iter().any(|range| {
                usize::from(range.len) == len
                    && prefix
                        .iter()
                        .enumerate()
                        .all(|(index, &byte)| range.low[index] <= byte && byte <= range.high[index])
            });
            if matched {
                if !reject_ambiguity {
                    return Some((code_value(prefix)?.1, len));
                }
                if found.is_some() {
                    return None;
                }
                found = Some((code_value(prefix)?.1, len));
            }
        }
        found
    }

    /// Appends the Unicode text of `code` (of `len` bytes) to `out`; returns whether it mapped.
    pub(super) fn lookup(&self, code: u32, len: u8, out: &mut String) -> bool {
        self.lookup_impl(code, len, out, false)
    }

    /// Looks up a mapping without dropping malformed UTF-16 code units.
    pub(super) fn lookup_strict(&self, code: u32, len: u8, out: &mut String) -> bool {
        if self.strict_invalid {
            return false;
        }
        if self.singles.contains_key(&(len, code)) {
            let index = self
                .ranges
                .partition_point(|range| (range.len, range.low) <= (len, code));
            if index
                .checked_sub(1)
                .and_then(|i| self.ranges.get(i))
                .is_some_and(|range| range.len == len && code <= range.high)
            {
                return false;
            }
        }
        self.lookup_impl(code, len, out, true)
    }

    fn lookup_impl(&self, code: u32, len: u8, out: &mut String, strict: bool) -> bool {
        if let Some(units) = self.singles.get(&(len, code)) {
            return push_utf16(units.iter().copied(), out, strict);
        }
        let index = self
            .ranges
            .partition_point(|range| (range.len, range.low) <= (len, code));
        // Ranges rarely overlap; checking a few predecessors covers the overlaps that occur.
        for range in self.ranges[..index].iter().rev().take(8) {
            if range.len != len {
                break;
            }
            if code > range.high {
                continue;
            }
            let offset = code - range.low;
            return match &range.target {
                RangeTarget::Increment(units) => {
                    let Some((&last, head)) = units.split_last() else {
                        return false;
                    };
                    let last = if strict {
                        let Ok(offset) = u16::try_from(offset) else {
                            return false;
                        };
                        let Some(last) = last.checked_add(offset) else {
                            return false;
                        };
                        last
                    } else {
                        last.wrapping_add(offset as u16)
                    };
                    push_utf16(
                        head.iter().copied().chain(std::iter::once(last)),
                        out,
                        strict,
                    )
                }
                RangeTarget::List(targets) => targets
                    .get(offset as usize)
                    .is_some_and(|units| push_utf16(units.iter().copied(), out, strict)),
            };
        }
        false
    }
}

fn target_list(data: &[u8], pos: &mut usize) -> (Vec<Vec<u16>>, bool) {
    let mut list = Vec::new();
    let mut invalid = false;
    let mut ended = false;
    while let Some(token) = next_token(data, pos) {
        match token {
            Token::String(bytes) => {
                invalid |= !valid_target_bytes(&bytes);
                if list.len() < MAX_ENTRIES {
                    list.push(utf16_units(&bytes));
                } else {
                    invalid = true;
                }
            }
            Token::ArrayEnd => {
                ended = true;
                break;
            }
            Token::Keyword(_) => {
                invalid = true;
                break;
            }
            _ => invalid = true,
        }
    }
    (list, invalid || !ended)
}

/// Big-endian code value of 1 to 4 bytes.
fn code_value(bytes: &[u8]) -> Option<(u8, u32)> {
    if !(1..=4).contains(&bytes.len()) {
        return None;
    }
    let value = bytes
        .iter()
        .fold(0_u32, |acc, &byte| acc << 8 | u32::from(byte));
    Some((bytes.len() as u8, value))
}

fn utf16_units(bytes: &[u8]) -> Vec<u16> {
    bytes
        .chunks(2)
        .take(MAX_TARGET_UNITS)
        .map(|pair| match pair {
            [high, low] => u16::from_be_bytes([*high, *low]),
            [single] => u16::from(*single),
            _ => 0,
        })
        .collect()
}

fn valid_target_bytes(bytes: &[u8]) -> bool {
    !bytes.is_empty() && bytes.len().is_multiple_of(2) && bytes.len() <= MAX_TARGET_UNITS * 2
}

/// Appends UTF-16 units as text, skipping unpaired surrogates; returns whether anything was added.
fn push_utf16(units: impl Iterator<Item = u16>, out: &mut String, strict: bool) -> bool {
    let before = out.len();
    for item in char::decode_utf16(units) {
        match item {
            Ok(character) => out.push(character),
            Err(_) if strict => {
                out.truncate(before);
                return false;
            }
            Err(_) => {}
        }
    }
    out.len() > before
}

#[cfg(test)]
mod tests {
    use super::CMap;

    const SAMPLE: &[u8] = b"/CIDInit /ProcSet findresource begin 12 dict begin begincmap
        1 begincodespacerange <0000> <FFFF> endcodespacerange
        2 beginbfchar <0003> <0020> <0010> <00660069> endbfchar
        2 beginbfrange <0020> <0022> <0041> <0030> <0031> [<0078> <D835DC00>] endbfrange
        endcmap CMapName currentdict /CMap defineresource pop end end";

    fn text(cmap: &CMap, code: u32, len: u8) -> Option<String> {
        let mut out = String::new();
        cmap.lookup(code, len, &mut out).then_some(out)
    }

    #[test]
    fn maps_chars_ranges_and_arrays() {
        let cmap = CMap::parse(SAMPLE);
        assert_eq!(text(&cmap, 0x0003, 2).as_deref(), Some(" "));
        assert_eq!(text(&cmap, 0x0010, 2).as_deref(), Some("fi"));
        assert_eq!(text(&cmap, 0x0021, 2).as_deref(), Some("B"));
        assert_eq!(text(&cmap, 0x0022, 2).as_deref(), Some("C"));
        assert_eq!(text(&cmap, 0x0031, 2).as_deref(), Some("\u{1D400}"));
        assert_eq!(text(&cmap, 0x0023, 2), None);
        assert_eq!(text(&cmap, 0x21, 1), None);
    }

    #[test]
    fn splits_codes_by_codespace() {
        let cmap = CMap::parse(b"2 begincodespacerange <00> <80> <8140> <9FFC> endcodespacerange");
        assert_eq!(cmap.next_code(&[0x41, 0x81]), Some((0x41, 1)));
        assert_eq!(cmap.next_code(&[0x81, 0x40]), Some((0x8140, 2)));
        assert_eq!(cmap.next_code(&[0xFF, 0x40]), Some((0xFF, 1)));
        assert_eq!(cmap.next_code_strict(&[0xFF, 0x40]), None);
        assert_eq!(cmap.next_code_strict(&[0x81]), None);
        assert_eq!(CMap::parse(b"").next_code(&[1, 2]), None);
    }

    #[test]
    fn huge_ranges_stay_cheap() {
        let cmap = CMap::parse(b"1 beginbfrange <00000000> <FFFFFFFF> <0041> endbfrange");
        assert_eq!(text(&cmap, 0x0000_0002, 4).as_deref(), Some("C"));
        assert_eq!(text(&cmap, 0xFFFF_FFFF, 4).as_deref(), Some("\u{40}"));
    }

    #[test]
    fn strict_lookup_refuses_partial_utf16_and_overflow() {
        let cmap = CMap::parse(
            b"1 beginbfchar <01> <0041D800> endbfchar 1 beginbfrange <02> <03> <FFFF> endbfrange",
        );
        let mut output = String::new();
        assert!(cmap.lookup(1, 1, &mut output));
        assert_eq!(output, "A");
        output.clear();
        assert!(!cmap.lookup_strict(1, 1, &mut output));
        assert!(output.is_empty());
        assert!(!cmap.lookup_strict(3, 1, &mut output));
        let odd = CMap::parse(b"1 beginbfchar <04> <004100> endbfchar");
        assert!(!odd.lookup_strict(4, 1, &mut output));
    }

    #[test]
    fn strict_mapping_rejects_ambiguous_and_truncated_cmaps() {
        let ambiguous =
            CMap::parse(b"2 begincodespacerange <00> <FF> <0000> <FFFF> endcodespacerange");
        assert_eq!(ambiguous.next_code(&[0, 1]), Some((0, 1)));
        assert_eq!(ambiguous.next_code_strict(&[0, 1]), None);
        let overlapping = CMap::parse(
            b"1 beginbfchar <01> <0041> endbfchar 1 beginbfrange <01> <02> <0042> endbfrange",
        );
        assert!(!overlapping.lookup_strict(1, 1, &mut String::new()));
        let truncated = CMap::parse(b"1 beginbfchar <01> <0041>");
        assert!(!truncated.lookup_strict(1, 1, &mut String::new()));
    }
}
