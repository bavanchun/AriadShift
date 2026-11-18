//! List markers at the start of a line: bullets, dashes, and numbered or lettered labels.
//!
//! Running text and the cells of a table read two sets of labels. In running text a label such as
//! `[1]` is a citation and `A.` an initial, so only bullets, dashes and the labels `1.`, `a)`,
//! `(iv)` start an item. A table cell that holds nothing but a label, or starts with one, sets it
//! apart from the text beside it, so [`Labels::Cell`] also reads `[1]`, `(A)` and `A)`.

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

/// Which labels start a list item.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Labels {
    /// Running text.
    Running,
    /// The cell of a table: also bracketed numbers and capital letters in parentheses.
    Cell,
}

const BULLETS: &[char] = &[
    '•', '◦', '▪', '▫', '●', '○', '■', '□', '►', '▸', '‣', '⁃', '∙', '·', '➢', '➤', '✓', '✔', '❖',
    '◆', '♦',
];

const DASHES: &[char] = &['-', '–', '*', '+'];

/// Parses a marker at the start of `text` that is followed by item text.
pub(super) fn parse(text: &str) -> Option<Marker> {
    parse_as(text, Labels::Running)
}

/// Like [`parse`], for the text of a table cell.
pub(super) fn parse_cell(text: &str) -> Option<Marker> {
    parse_as(text, Labels::Cell)
}

fn parse_as(text: &str, labels: Labels) -> Option<Marker> {
    let (marker, rest) = split(text, labels)?;
    (!rest.trim().is_empty()).then_some(marker)
}

/// The strength of a marker at the start of `text`, whether or not text follows it.
pub(super) fn marker_strength(text: &str) -> MarkerStrength {
    split(text, Labels::Running).map_or(MarkerStrength::None, |(marker, _)| marker.strength)
}

/// Whether `text` is nothing but a list marker.
pub(super) fn is_marker_only(text: &str) -> bool {
    marker_only(text, Labels::Running)
}

/// Like [`is_marker_only`], for the text of a table cell.
pub(super) fn is_cell_marker_only(text: &str) -> bool {
    marker_only(text, Labels::Cell)
}

fn marker_only(text: &str, labels: Labels) -> bool {
    split(text.trim(), labels).is_some_and(|(_, rest)| rest.trim().is_empty())
}

fn split(text: &str, labels: Labels) -> Option<(Marker, &str)> {
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
    ordered(text, labels)
}

/// `1.`, `1)`, `(1)`, `a)`, `a.`, `(a)`, `iv.` and similar labels followed by whitespace. In a
/// cell also `[1]`, `(A)` and `A)`; a capital letter with a dot is an initial and never a label.
fn ordered(text: &str, labels: Labels) -> Option<(Marker, &str)> {
    let cell = labels == Labels::Cell;
    let (opening, body) = match text.chars().next() {
        Some(open @ '(') => (Some(open), &text[1..]),
        Some(open @ '[') if cell => (Some(open), &text[1..]),
        _ => (None, text),
    };
    let token_len = body.bytes().take_while(u8::is_ascii_alphanumeric).count();
    if token_len == 0 {
        return None;
    }
    let (token, after) = body.split_at(token_len);
    let mut chars = after.chars();
    let close = chars.next()?;
    let valid_close = match opening {
        Some('(') => close == ')',
        Some(_) => close == ']',
        None => close == '.' || close == ')',
    };
    if !valid_close {
        return None;
    }
    // Brackets hold a number: `[a]` and `[Smith]` are not labels.
    let bracketed = opening == Some('[');
    let capital = token.len() == 1 && token.bytes().all(|byte| byte.is_ascii_uppercase());
    if (bracketed && !token.bytes().all(|byte| byte.is_ascii_digit()))
        || (capital && (!cell || close != ')'))
    {
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

/// The value of a label, or `None` when `token` is not one. A capital letter counts like its
/// small form; whether capitals are labels at all is decided by [`ordered`].
fn label_value(token: &str) -> Option<u64> {
    if token.len() <= 3 && token.bytes().all(|byte| byte.is_ascii_digit()) {
        return token.parse().ok();
    }
    if token == "i" || token == "I" {
        return Some(1);
    }
    if let [letter] = token.as_bytes()
        && letter.is_ascii_alphabetic()
    {
        return Some(u64::from(letter.to_ascii_lowercase() - b'a') + 1);
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
    use super::{
        MarkerStrength, is_cell_marker_only, is_marker_only, marker_strength, parse, parse_cell,
    };

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

    #[test]
    fn a_cell_reads_bracketed_numbers_and_capital_letters_as_labels() {
        for (text, number) in [
            ("[1] Smith", 1),
            ("[12] Garcia", 12),
            ("(A) 14", 1),
            ("(D) 17", 4),
            ("B) 15", 2),
            ("(I) One", 1),
        ] {
            let marker = parse_cell(text).unwrap_or_else(|| panic!("{text}"));
            assert!(marker.ordered, "{text}");
            assert_eq!(marker.number, Some(number), "{text}");
            assert_eq!(marker.strength, MarkerStrength::Weak, "{text}");
        }
        assert_eq!(parse_cell("[3] Item").unwrap().len, 4);
        assert!(is_cell_marker_only("[3]"));
        assert!(is_cell_marker_only(" (C) "));
        assert!(is_cell_marker_only("D)"));
        assert!(!is_cell_marker_only("[3] Item"));
    }

    #[test]
    fn running_text_keeps_the_narrower_set_of_labels() {
        for text in ["[1] Smith", "(A) 14", "B) 15", "[12] Garcia"] {
            assert!(parse(text).is_none(), "{text}");
            assert_eq!(marker_strength(text), MarkerStrength::None, "{text}");
        }
        assert!(!is_marker_only("[3]"));
        assert!(!is_marker_only("(C)"));
    }

    #[test]
    fn a_cell_does_not_read_initials_or_other_brackets_as_labels() {
        for text in [
            // A capital letter with a dot is an initial.
            "A. Smith",
            "K. Lee",
            // Brackets hold a number of at most three digits.
            "[a] Smith",
            "[Smith] 2019",
            "[1234] Smith",
            "[1)] Smith",
            // A label is followed by a space.
            "[1]Smith",
            "(A)14",
            // Mixed brackets, and two capital letters.
            "(A] 14",
            "(AB) 14",
            "[]",
        ] {
            assert!(parse_cell(text).is_none(), "{text}");
        }
        assert!(!is_cell_marker_only("A."));
    }

    #[test]
    fn a_cell_still_reads_the_labels_of_running_text() {
        for text in [
            "1. One",
            "12) Twelve",
            "(3) Three",
            "b) Bee",
            "iv. Four",
            "• Item",
        ] {
            assert_eq!(
                parse_cell(text).map(|marker| (marker.ordered, marker.number)),
                parse(text).map(|marker| (marker.ordered, marker.number)),
                "{text}"
            );
            assert!(parse_cell(text).is_some(), "{text}");
        }
    }
}
