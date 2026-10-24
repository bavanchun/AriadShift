//! List markers at the start of a line: bullets, dashes, and numbered or lettered labels.

/// How reliably a leading marker starts a new list item.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum MarkerStrength {
    None,
    /// Dashes and numbers, which also start ordinary wrapped lines now and then.
    Weak,
    /// Bullet characters, which only ever start items.
    Strong,
}

/// A recognized list marker.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Marker {
    pub(super) ordered: bool,
    /// The item number for ordered markers (`a` and `i` count as 1).
    pub(super) number: Option<u64>,
    /// Whether the label is written in digits, as opposed to letters or Roman numerals.
    pub(super) arabic: bool,
    /// Byte length of the marker and the whitespace after it.
    pub(super) len: usize,
    pub(super) strength: MarkerStrength,
}

const BULLETS: &[char] = &[
    '•', '◦', '▪', '▫', '●', '○', '■', '□', '►', '▸', '‣', '⁃', '∙', '·', '➢', '➤', '✓', '✔', '❖',
    '◆', '♦',
];

const DASHES: &[char] = &['-', '–', '*', '+'];

/// Parses a marker at the start of `text` that is followed by item text.
pub(super) fn parse(text: &str) -> Option<Marker> {
    let (marker, rest) = split(text)?;
    (!rest.trim().is_empty()).then_some(marker)
}

/// The strength of a marker at the start of `text`, whether or not text follows it.
pub(super) fn marker_strength(text: &str) -> MarkerStrength {
    split(text).map_or(MarkerStrength::None, |(marker, _)| marker.strength)
}

/// Whether `text` is nothing but a list marker.
pub(super) fn is_marker_only(text: &str) -> bool {
    split(text.trim()).is_some_and(|(_, rest)| rest.trim().is_empty())
}

fn split(text: &str) -> Option<(Marker, &str)> {
    let first = text.chars().next()?;
    if BULLETS.contains(&first) {
        let rest = &text[first.len_utf8()..];
        let trimmed = rest.trim_start();
        return Some((
            Marker {
                ordered: false,
                number: None,
                arabic: false,
                len: text.len() - trimmed.len(),
                strength: MarkerStrength::Strong,
            },
            trimmed,
        ));
    }
    if DASHES.contains(&first) {
        let rest = &text[first.len_utf8()..];
        let trimmed = rest.trim_start();
        if trimmed.len() == rest.len() && !trimmed.is_empty() {
            return None;
        }
        return Some((
            Marker {
                ordered: false,
                number: None,
                arabic: false,
                len: text.len() - trimmed.len(),
                strength: MarkerStrength::Weak,
            },
            trimmed,
        ));
    }
    ordered(text)
}

/// `1.`, `1)`, `(1)`, `a)`, `a.`, `(a)`, `iv.` and similar labels followed by whitespace.
fn ordered(text: &str) -> Option<(Marker, &str)> {
    let (parenthesized, body) = match text.strip_prefix('(') {
        Some(body) => (true, body),
        None => (false, text),
    };
    let token_len = body.bytes().take_while(u8::is_ascii_alphanumeric).count();
    if token_len == 0 {
        return None;
    }
    let (token, after) = body.split_at(token_len);
    let mut chars = after.chars();
    let close = chars.next()?;
    let valid_close = if parenthesized {
        close == ')'
    } else {
        close == '.' || close == ')'
    };
    if !valid_close {
        return None;
    }
    let rest = chars.as_str();
    let trimmed = rest.trim_start();
    if trimmed.len() == rest.len() && !trimmed.is_empty() {
        return None;
    }
    let number = label_value(token)?;
    Some((
        Marker {
            ordered: true,
            number: Some(number),
            arabic: token.bytes().all(|byte| byte.is_ascii_digit()),
            len: text.len() - trimmed.len(),
            strength: MarkerStrength::Weak,
        },
        trimmed,
    ))
}

fn label_value(token: &str) -> Option<u64> {
    if token.len() <= 3 && token.bytes().all(|byte| byte.is_ascii_digit()) {
        return token.parse().ok();
    }
    if token == "i" {
        return Some(1);
    }
    if let [letter] = token.as_bytes()
        && letter.is_ascii_lowercase()
    {
        return Some(u64::from(letter - b'a') + 1);
    }
    roman_value(token)
}

fn roman_value(token: &str) -> Option<u64> {
    if token.is_empty() || token.len() > 6 {
        return None;
    }
    let mut total = 0_i64;
    let mut previous = 0_i64;
    for byte in token.bytes().rev() {
        let value = match byte {
            b'i' => 1,
            b'v' => 5,
            b'x' => 10,
            b'l' => 50,
            b'c' => 100,
            _ => return None,
        };
        if value < previous {
            total -= value;
        } else {
            total += value;
            previous = value;
        }
    }
    u64::try_from(total).ok().filter(|&value| value > 0)
}

#[cfg(test)]
mod tests {
    use super::{MarkerStrength, is_marker_only, marker_strength, parse};

    #[test]
    fn recognizes_bullets_dashes_and_labels() {
        let bullet = parse("• Item").unwrap();
        assert_eq!(
            (bullet.ordered, bullet.len, bullet.strength),
            (false, 4, MarkerStrength::Strong)
        );
        let dash = parse("- Ban Bí thư").unwrap();
        assert_eq!((dash.ordered, dash.len), (false, 2));
        for (text, number) in [
            ("1. One", 1),
            ("12) Twelve", 12),
            ("(3) Three", 3),
            ("b) Bee", 2),
            ("iv. Four", 4),
            ("i. One", 1),
        ] {
            let marker = parse(text).unwrap();
            assert!(marker.ordered, "{text}");
            assert_eq!(marker.number, Some(number), "{text}");
        }
    }

    #[test]
    fn rejects_ordinary_text() {
        for text in [
            "2026. A year",
            "Điều 1. Scope",
            "A. Smith",
            "-5 degrees",
            "1.5 million",
            "word",
            "•",
            "3.",
        ] {
            assert!(parse(text).is_none(), "{text}");
        }
        assert_eq!(marker_strength("3."), MarkerStrength::Weak);
        assert_eq!(marker_strength("Plain text"), MarkerStrength::None);
        assert!(is_marker_only(" 3. "));
        assert!(is_marker_only("•"));
        assert!(!is_marker_only("3. Item"));
    }
}
