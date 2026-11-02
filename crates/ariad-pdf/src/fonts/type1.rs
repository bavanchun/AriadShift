//! Conservative pruning of binary Type 1 charstrings.

use std::collections::BTreeSet;

fn crypt(input: &[u8], key: u16, encrypt: bool) -> Vec<u8> {
    let mut state = key;
    input
        .iter()
        .map(|&byte| {
            let value = byte ^ (state >> 8) as u8;
            let cipher = if encrypt { value } else { byte };
            state = (u32::from(cipher)
                .wrapping_add(u32::from(state))
                .wrapping_mul(52845)
                .wrapping_add(22719)
                & 65535) as u16;
            value
        })
        .collect()
}

fn token<'a>(bytes: &'a [u8], pos: &mut usize) -> Option<&'a [u8]> {
    while bytes.get(*pos).is_some_and(u8::is_ascii_whitespace) {
        *pos += 1;
    }
    let start = *pos;
    while bytes
        .get(*pos)
        .is_some_and(|b| !b.is_ascii_whitespace() && !b"[]{}()<>/".contains(b))
    {
        *pos += 1;
    }
    if *pos == start && bytes.get(*pos) == Some(&b'/') {
        *pos += 1;
        while bytes
            .get(*pos)
            .is_some_and(|b| !b.is_ascii_whitespace() && !b"[]{}()<>/".contains(b))
        {
            *pos += 1;
        }
    }
    (start < *pos).then(|| &bytes[start..*pos])
}
fn number(bytes: &[u8]) -> Option<usize> {
    std::str::from_utf8(bytes).ok()?.parse().ok()
}
fn find(bytes: &[u8], needle: &[u8]) -> Option<usize> {
    let mut matches = bytes
        .windows(needle.len())
        .enumerate()
        .filter(|(_, w)| *w == needle);
    let first = matches.next()?.0;
    if matches.next().is_some() {
        None
    } else {
        Some(first)
    }
}

// Read a charstring; an unknown or malformed instruction makes the font ineligible.
fn has_seac(cipher: &[u8], len_iv: usize) -> Option<bool> {
    let bytes = crypt(cipher, 4330, false);
    let bytes = bytes.get(len_iv..)?;
    let mut i = 0;
    while i < bytes.len() {
        let byte = bytes[i];
        i += 1;
        match byte {
            12 => {
                let op = *bytes.get(i)?;
                i += 1;
                if op == 6 {
                    return Some(true);
                }
            }
            247..=254 => i = i.checked_add(1)?,
            255 => i = i.checked_add(4)?,
            _ => {}
        }
        if i > bytes.len() {
            return None;
        }
    }
    Some(false)
}

fn seac_components(cipher: &[u8]) -> Option<Vec<Vec<u8>>> {
    let bytes = crypt(cipher, 4330, false);
    let bytes = bytes.get(4..)?;
    let mut stack = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        i += 1;
        let number = match b {
            32..=246 => Some(i32::from(b) - 139),
            247..=250 => {
                let v = *bytes.get(i)?;
                i += 1;
                Some((i32::from(b) - 247) * 256 + i32::from(v) + 108)
            }
            251..=254 => {
                let v = *bytes.get(i)?;
                i += 1;
                Some(-((i32::from(b) - 251) * 256) - i32::from(v) - 108)
            }
            255 => {
                let value = i32::from_be_bytes(bytes.get(i..i.checked_add(4)?)?.try_into().ok()?);
                i += 4;
                Some(value)
            }
            _ => None,
        };
        if let Some(number) = number {
            // Type 1 charstrings have a bounded operand stack.
            if stack.len() >= 24 {
                return None;
            }
            stack.push(number);
            continue;
        }
        if b == 12 {
            let op = *bytes.get(i)?;
            i += 1;
            if op == 6 {
                if stack.len() != 5 {
                    return None;
                }
                return Some(vec![
                    super::standard_glyph_name(u8::try_from(stack[3]).ok()?)?,
                    super::standard_glyph_name(u8::try_from(stack[4]).ok()?)?,
                ]);
            }
        }
        stack.clear();
    }
    Some(Vec::new())
}

/// Preserve subroutines and the named charstrings and their seac components.
pub(super) fn subset(
    input: &[u8],
    length1: usize,
    length2: usize,
    names: &BTreeSet<Vec<u8>>,
) -> Option<(Vec<u8>, usize)> {
    let end = length1.checked_add(length2)?;
    let encrypted = input.get(length1..end)?;
    let clear = crypt(encrypted, 55665, false);
    let subrs = find(&clear, b"/Subrs ")?;
    let chars = find(&clear, b"/CharStrings ")?;
    if subrs >= chars
        || !clear
            .get(..chars)?
            .windows(b"/RD {string currentfile exch readstring pop}".len())
            .any(|v| v == b"/RD {string currentfile exch readstring pop}")
    {
        return None;
    }
    // Type 1 defaults to four random charstring bytes. Nondefault lenIV is skipped.
    if clear.get(..chars)?.windows(6).any(|v| v == b"/lenIV") {
        return None;
    }
    let mut p = subrs + b"/Subrs ".len();
    let count = number(token(&clear, &mut p)?)?;
    if token(&clear, &mut p)? != b"array" || count > 65536 {
        return None;
    }
    for _ in 0..count {
        if token(&clear, &mut p)? != b"dup" {
            return None;
        }
        number(token(&clear, &mut p)?)?;
        let len = number(token(&clear, &mut p)?)?;
        if token(&clear, &mut p)? != b"RD" || !clear.get(p)?.is_ascii_whitespace() {
            return None;
        }
        p += 1;
        let end = p.checked_add(len)?;
        if has_seac(clear.get(p..end)?, 4)? {
            return None;
        }
        p = end;
        if token(&clear, &mut p)? != b"NP" {
            return None;
        }
    }
    if p > chars {
        return None;
    }
    p = chars + b"/CharStrings ".len();
    let count = number(token(&clear, &mut p)?)?;
    if count > 65536
        || token(&clear, &mut p)? != b"dict"
        || token(&clear, &mut p)? != b"dup"
        || token(&clear, &mut p)? != b"begin"
    {
        return None;
    }
    let header_end = p;
    let mut entries = Vec::with_capacity(count);
    for _ in 0..count {
        let start = p;
        let name = token(&clear, &mut p)?.strip_prefix(b"/")?;
        let len = number(token(&clear, &mut p)?)?;
        if token(&clear, &mut p)? != b"RD" || !clear.get(p)?.is_ascii_whitespace() {
            return None;
        }
        p += 1;
        let end = p.checked_add(len)?;
        let code = clear.get(p..end)?;
        p = end;
        if token(&clear, &mut p)? != b"ND" {
            return None;
        }
        entries.push((name, code, start, p));
    }
    let mut needed = names.clone();
    needed.insert(b".notdef".to_vec());
    let mut pending: Vec<_> = needed.iter().cloned().collect();
    while let Some(name) = pending.pop() {
        let (_, code, _, _) = entries.iter().find(|(entry, _, _, _)| *entry == name)?;
        for component in seac_components(code)? {
            if needed.insert(component.clone()) {
                pending.push(component);
            }
        }
    }
    let mut output = clear.get(..header_end)?.to_vec();
    for (name, _, start, end) in entries {
        if needed.contains(name) {
            output.extend_from_slice(clear.get(start..end)?);
        }
    }
    output.extend_from_slice(clear.get(p..)?);
    let mut result = input.get(..length1)?.to_vec();
    let new_length2 = output.len();
    result.extend_from_slice(&crypt(&output, 55665, true));
    result.extend_from_slice(input.get(end..)?);
    Some((result, new_length2))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oversized_charstring_stack_is_rejected() {
        let mut plain = vec![0; 4];
        plain.extend(std::iter::repeat_n(139, 25));
        assert!(seac_components(&crypt(&plain, 4330, true)).is_none());
    }

    #[test]
    fn seac_keeps_base_and_accent_charstrings() {
        let ordinary = crypt(&[0, 0, 0, 0, 14], 4330, true);
        let accent = crypt(
            &[0, 0, 0, 0, 139, 139, 139, 204, 247, 86, 12, 6],
            4330,
            true,
        );
        let mut clear = b"\0\0\0\0/RD {string currentfile exch readstring pop} /Subrs 0 array\n/CharStrings 5 dict dup begin".to_vec();
        for (name, code) in [
            (b".notdef".as_slice(), &ordinary),
            (b"A", &ordinary),
            (b"acute", &ordinary),
            (b"Aacute", &accent),
            (b"unused", &ordinary),
        ] {
            clear.extend_from_slice(b"\n/");
            clear.extend_from_slice(name);
            clear.extend_from_slice(format!(" {} RD ", code.len()).as_bytes());
            clear.extend_from_slice(code);
            clear.extend_from_slice(b"ND");
        }
        clear.extend_from_slice(b"\nend\n");
        let input = crypt(&clear, 55665, true);
        let (output, _) = subset(
            &input,
            0,
            input.len(),
            &BTreeSet::from([b"Aacute".to_vec()]),
        )
        .unwrap();
        let decoded = crypt(&output, 55665, false);
        assert!(decoded.windows(3).any(|w| w == b"/A "));
        assert!(decoded.windows(7).any(|w| w == b"/acute "));
        assert!(!decoded.windows(8).any(|w| w == b"/unused "));
    }
}
