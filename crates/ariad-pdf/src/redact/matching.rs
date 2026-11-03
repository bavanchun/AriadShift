//! Bounded text matching and preset validation.

use ariad_core::tools::params::redact::{RedactionPreset, RedactionTarget};
use regex::{Regex, RegexBuilder};
use regex_syntax::hir::{Hir, HirKind};
use unicode_normalization::UnicodeNormalization;

use super::normalize::Normalized;
use crate::ToolCtx;

const MAX_TEXT: usize = 16 * 1024 * 1024;
const MAX_MATCHES: usize = 100_000;
const MAX_WORK: usize = 256 * 1024 * 1024;

/// The match offsets refer to UTF-8 byte offsets in the supplied text.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Match {
    pub target: usize,
    pub start: usize,
    pub end: usize,
}

pub(crate) struct Matcher {
    target: usize,
    pattern: Pattern,
    preset: Option<RedactionPreset>,
}

enum Pattern {
    Literal { bytes: Vec<u8>, prefix: Vec<usize> },
    Regex(Regex),
}

/// Compiled patterns are kept private: errors never contain target values.
pub struct Matchers(pub(crate) Vec<Matcher>);

/// Shared across all searchable carriers in one request.
#[derive(Clone, Default)]
pub struct MatchBudget {
    text: usize,
    matches: usize,
    work: usize,
    ctx: Option<ToolCtx>,
}

impl MatchBudget {
    pub fn set_context(&mut self, ctx: &ToolCtx) {
        self.ctx = Some(ctx.clone());
    }

    pub(crate) fn with_context(ctx: &ToolCtx) -> Self {
        Self {
            ctx: Some(ctx.clone()),
            ..Self::default()
        }
    }

    fn check(&self) -> Result<(), &'static str> {
        if self.ctx.as_ref().is_some_and(|ctx| ctx.check().is_err()) {
            Err("redaction matching interrupted")
        } else {
            Ok(())
        }
    }
}

impl Matchers {
    pub fn new(targets: &[RedactionTarget]) -> Result<Self, &'static str> {
        ariad_core::tools::params::redact::validate_targets(targets)
            .map_err(|_| "invalid redaction targets")?;
        let mut patterns = Vec::new();
        for (target, value) in targets.iter().enumerate() {
            if let RedactionTarget::Exact { text } = value {
                let bytes = text.nfc().collect::<String>().into_bytes();
                let mut prefix = vec![0; bytes.len()];
                let mut length = 0;
                for i in 1..bytes.len() {
                    while length > 0 && bytes[length] != bytes[i] {
                        length = prefix[length - 1];
                    }
                    if bytes[length] == bytes[i] {
                        length += 1;
                    }
                    prefix[i] = length;
                }
                patterns.push(Matcher {
                    target,
                    pattern: Pattern::Literal { bytes, prefix },
                    preset: None,
                });
                continue;
            }
            let (pattern, preset) = match value {
                RedactionTarget::Regex { pattern } => (normalize_regex(pattern)?, None),
                RedactionTarget::Preset { preset } => {
                    (preset_pattern(*preset).to_owned(), Some(*preset))
                }
                _ => continue,
            };
            let regex = RegexBuilder::new(&pattern)
                .size_limit(1024 * 1024)
                .dfa_size_limit(1024 * 1024)
                .build()
                .map_err(|_| "invalid or oversized redaction pattern")?;
            if regex.is_match("") {
                return Err("redaction pattern may not match empty text");
            }
            patterns.push(Matcher {
                target,
                pattern: Pattern::Regex(regex),
                preset,
            });
        }
        Ok(Self(patterns))
    }

    pub(crate) fn find(
        &self,
        text: &str,
        budget: &mut MatchBudget,
    ) -> Result<Vec<Match>, &'static str> {
        budget.check()?;
        budget.text = budget
            .text
            .checked_add(text.len())
            .ok_or("searchable text limit exceeded")?;
        if budget.text > MAX_TEXT {
            return Err("searchable text limit exceeded");
        }
        let normalized = Normalized::new(text)?;
        budget.check()?;
        let text = normalized.text.as_str();
        let mut result = Vec::new();
        for matcher in &self.0 {
            let regex = match &matcher.pattern {
                Pattern::Literal { bytes, prefix } => {
                    let work = text
                        .len()
                        .checked_mul(2)
                        .and_then(|n| n.checked_add(bytes.len()))
                        .ok_or("matching work limit exceeded")?;
                    budget.work = budget
                        .work
                        .checked_add(work)
                        .ok_or("matching work limit exceeded")?;
                    if budget.work > MAX_WORK {
                        return Err("matching work limit exceeded");
                    }
                    let mut length = 0;
                    for (index, byte) in text.bytes().enumerate() {
                        if index % 4096 == 0 {
                            budget.check()?;
                        }
                        while length > 0 && bytes[length] != byte {
                            length = prefix[length - 1];
                        }
                        if bytes[length] == byte {
                            length += 1;
                        }
                        if length == bytes.len() {
                            if budget.matches == MAX_MATCHES {
                                return Err("redaction match limit exceeded");
                            }
                            budget.matches += 1;
                            let (start, end) =
                                normalized.original_range(index + 1 - length, index + 1)?;
                            result.push(Match {
                                target: matcher.target,
                                start,
                                end,
                            });
                            length = prefix[length - 1];
                        }
                    }
                    continue;
                }
                Pattern::Regex(regex) => regex,
            };
            let mut offset = 0;
            while offset <= text.len() {
                budget.check()?;
                budget.work = budget
                    .work
                    .checked_add(text.len() - offset + 1)
                    .ok_or("matching work limit exceeded")?;
                if budget.work > MAX_WORK {
                    return Err("matching work limit exceeded");
                }
                let Some(found) = regex.find_at(text, offset) else {
                    budget.check()?;
                    break;
                };
                budget.check()?;
                if found.is_empty() {
                    return Err("redaction pattern matched empty text");
                }
                if let Some(
                    preset @ (RedactionPreset::Iban
                    | RedactionPreset::Cccd
                    | RedactionPreset::Cmnd
                    | RedactionPreset::CreditCard),
                ) = matcher.preset
                {
                    let mut compact_len = 0;
                    for (i, byte) in found.as_str().bytes().enumerate() {
                        if byte.is_ascii_alphanumeric() {
                            compact_len += 1;
                        }
                        let length_valid = match preset {
                            RedactionPreset::Iban => (15..=34).contains(&compact_len),
                            RedactionPreset::Cccd => compact_len == 12,
                            RedactionPreset::Cmnd => matches!(compact_len, 9 | 12),
                            RedactionPreset::CreditCard => (13..=19).contains(&compact_len),
                            _ => false,
                        };
                        if !length_valid || !byte.is_ascii_alphanumeric() {
                            continue;
                        }
                        let end = found.start() + i + 1;
                        let candidate = &text[found.start()..end];
                        if preset_valid(preset, candidate, text, found.start(), end) {
                            if budget.matches == MAX_MATCHES {
                                return Err("redaction match limit exceeded");
                            }
                            budget.matches += 1;
                            let (start, end) = normalized.original_range(found.start(), end)?;
                            result.push(Match {
                                target: matcher.target,
                                start,
                                end,
                            });
                        }
                    }
                } else if matcher.preset.is_none_or(|p| {
                    preset_valid(p, found.as_str(), text, found.start(), found.end())
                }) {
                    if budget.matches == MAX_MATCHES {
                        return Err("redaction match limit exceeded");
                    }
                    budget.matches += 1;
                    let (start, end) = normalized.original_range(found.start(), found.end())?;
                    result.push(Match {
                        target: matcher.target,
                        start,
                        end,
                    });
                }
                offset = found.start()
                    + text[found.start()..]
                        .chars()
                        .next()
                        .map_or(1, |c| c.len_utf8());
            }
        }
        Ok(result)
    }
}

/// Normalize literal runs without changing the meaning of character classes or operators.
fn normalize_regex(pattern: &str) -> Result<String, &'static str> {
    let parsed = regex_syntax::Parser::new()
        .parse(pattern)
        .map_err(|_| "invalid redaction pattern")?;
    if parsed.properties().minimum_len() == Some(0) {
        return Err("redaction pattern may not match empty text");
    }
    fn normalize(hir: Hir) -> Hir {
        match hir.into_kind() {
            HirKind::Empty => Hir::empty(),
            HirKind::Literal(literal) => match std::str::from_utf8(&literal.0) {
                Ok(text) => Hir::literal(text.nfc().collect::<String>().into_bytes()),
                Err(_) => Hir::literal(literal.0),
            },
            HirKind::Class(class) => Hir::class(class),
            HirKind::Look(look) => Hir::look(look),
            HirKind::Repetition(mut repeat) => {
                repeat.sub = Box::new(normalize(*repeat.sub));
                Hir::repetition(repeat)
            }
            HirKind::Capture(mut capture) => {
                capture.sub = Box::new(normalize(*capture.sub));
                Hir::capture(capture)
            }
            HirKind::Concat(parts) => Hir::concat(parts.into_iter().map(normalize).collect()),
            HirKind::Alternation(parts) => {
                Hir::alternation(parts.into_iter().map(normalize).collect())
            }
        }
    }
    struct BoundedPattern(String);
    impl std::fmt::Write for BoundedPattern {
        fn write_str(&mut self, value: &str) -> std::fmt::Result {
            if value.len() > (1024 * 1024usize).saturating_sub(self.0.len()) {
                return Err(std::fmt::Error);
            }
            self.0.push_str(value);
            Ok(())
        }
    }
    let mut output = BoundedPattern(String::new());
    std::fmt::write(&mut output, format_args!("{}", normalize(parsed)))
        .map_err(|_| "invalid or oversized redaction pattern")?;
    Ok(output.0)
}

fn preset_pattern(preset: RedactionPreset) -> &'static str {
    match preset {
        RedactionPreset::Email => {
            r"(?i)[a-z0-9.!#$%&'*+/=?^_`{|}~-]+@[a-z0-9](?:[a-z0-9-]*[a-z0-9])?(?:\.[a-z0-9](?:[a-z0-9-]*[a-z0-9])?)+"
        }
        RedactionPreset::Phone => r"(?:\+[1-9][0-9]{1,14}|0[0-9]{9,10})",
        RedactionPreset::Cccd => r"[0-9](?:[ .-]?[0-9]){11}",
        RedactionPreset::Cmnd => r"(?:[0-9](?:[ .-]?[0-9]){11}|[0-9](?:[ .-]?[0-9]){8})",
        RedactionPreset::CreditCard => r"[0-9](?:[ -]?[0-9]){12,18}",
        RedactionPreset::Iban => r"(?i)[a-z]{2}[0-9]{2}(?: ?[a-z0-9]){11,30}",
    }
}

fn preset_valid(
    preset: RedactionPreset,
    candidate: &str,
    text: &str,
    start: usize,
    end: usize,
) -> bool {
    let before = text[..start].chars().next_back();
    let after = text[end..].chars().next();
    let boundary =
        !before.is_some_and(|c| c.is_ascii_digit()) && !after.is_some_and(|c| c.is_ascii_digit());
    match preset {
        RedactionPreset::Cccd | RedactionPreset::Cmnd | RedactionPreset::Phone => boundary,
        RedactionPreset::CreditCard => boundary && luhn(candidate),
        RedactionPreset::Iban => {
            !before.is_some_and(|c| c.is_ascii_alphanumeric())
                && !after.is_some_and(|c| c.is_ascii_alphanumeric())
                && iban(candidate)
        }
        RedactionPreset::Email => true,
    }
}

fn luhn(candidate: &str) -> bool {
    let mut sum = 0u32;
    for (index, digit) in candidate
        .bytes()
        .filter(u8::is_ascii_digit)
        .rev()
        .enumerate()
    {
        let mut value = u32::from(digit - b'0');
        if index % 2 == 1 {
            value *= 2;
            if value > 9 {
                value -= 9;
            }
        }
        sum += value;
    }
    sum.is_multiple_of(10)
}

fn iban(candidate: &str) -> bool {
    let normalized: Vec<u8> = candidate
        .bytes()
        .filter(|b| *b != b' ')
        .map(|b| b.to_ascii_uppercase())
        .collect();
    if normalized.len() < 15 || normalized.len() > 34 {
        return false;
    }
    let mut remainder = 0u32;
    for &b in normalized[4..].iter().chain(normalized[..4].iter()) {
        let value = if b.is_ascii_digit() {
            u32::from(b - b'0')
        } else {
            u32::from(b - b'A') + 10
        };
        remainder = if value >= 10 {
            (remainder * 100 + value) % 97
        } else {
            (remainder * 10 + value) % 97
        };
    }
    remainder == 1
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn overlapping_patterns_and_checksums() {
        let targets = [
            RedactionTarget::Exact { text: "ana".into() },
            RedactionTarget::Preset {
                preset: RedactionPreset::CreditCard,
            },
            RedactionTarget::Preset {
                preset: RedactionPreset::Iban,
            },
        ];
        let found = Matchers::new(&targets)
            .unwrap()
            .find(
                "banana 4111 1111 1111 1111 GB82 WEST 1234 5698 7654 32",
                &mut MatchBudget::default(),
            )
            .unwrap();
        assert_eq!(found.iter().filter(|m| m.target == 0).count(), 2);
        assert!(found.iter().any(|m| m.target == 1));
        assert!(found.iter().any(|m| m.target == 2));
        assert!(!luhn("4111 1111 1111 1112"));
        assert!(!iban("GB82 WEST 1234 5698 7654 33"));
        let iban_matcher = Matchers::new(&[RedactionTarget::Preset {
            preset: RedactionPreset::Iban,
        }])
        .unwrap();
        assert_eq!(
            iban_matcher
                .find(
                    "GB82 WEST 1234 5698 7654 32 payable",
                    &mut MatchBudget::default()
                )
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn identity_numbers_respect_digit_boundaries() {
        let matcher = Matchers::new(&[RedactionTarget::Preset {
            preset: RedactionPreset::Cccd,
        }])
        .unwrap();
        assert_eq!(
            matcher
                .find("ID: 123 456 789 012.", &mut MatchBudget::default())
                .unwrap()
                .len(),
            1
        );
        assert!(
            matcher
                .find("1123456789012", &mut MatchBudget::default())
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn reports_overlapping_identity_lengths_and_card_before_following_digits() {
        let matcher = Matchers::new(&[
            RedactionTarget::Preset {
                preset: RedactionPreset::Cmnd,
            },
            RedactionTarget::Preset {
                preset: RedactionPreset::CreditCard,
            },
        ])
        .unwrap();
        let source = "123 456 789 012; 4111 1111 1111 1111 123";
        let matches = matcher.find(source, &mut MatchBudget::default()).unwrap();
        assert!(
            matches
                .iter()
                .any(|m| m.target == 0 && &source[m.start..m.end] == "123 456 789")
        );
        assert!(
            matches
                .iter()
                .any(|m| m.target == 0 && &source[m.start..m.end] == "123 456 789 012")
        );
        assert!(
            matches
                .iter()
                .any(|m| m.target == 1 && &source[m.start..m.end] == "4111 1111 1111 1111")
        );
    }

    #[test]
    fn budgets_are_shared_across_searchable_carriers() {
        let matcher = Matchers::new(&[RedactionTarget::Exact { text: "x".into() }]).unwrap();
        let mut budget = MatchBudget {
            text: MAX_TEXT - 1,
            ..Default::default()
        };
        assert_eq!(matcher.find("x", &mut budget).unwrap().len(), 1);
        assert!(matcher.find("x", &mut budget).is_err());
        let mut budget = MatchBudget {
            matches: MAX_MATCHES,
            ..Default::default()
        };
        assert!(matcher.find("x", &mut budget).is_err());
    }

    #[test]
    fn literal_enumeration_handles_many_overlaps_with_linear_work() {
        let matcher = Matchers::new(&[RedactionTarget::Exact { text: "aaa".into() }]).unwrap();
        let mut budget = MatchBudget::default();
        let matches = matcher.find(&"a".repeat(50_000), &mut budget).unwrap();
        assert_eq!(matches.len(), 49_998);
        assert_eq!((matches[1].start, matches[1].end), (1, 4));
        assert!(budget.work <= 100_003);
    }

    #[test]
    fn matching_honors_cancellation() {
        use std::sync::atomic::Ordering;
        let ctx = ToolCtx::new();
        let mut budget = MatchBudget::with_context(&ctx);
        ctx.cancel.store(true, Ordering::Relaxed);
        let matcher = Matchers::new(&[RedactionTarget::Exact {
            text: "secret".into(),
        }])
        .unwrap();
        assert!(matcher.find("secret", &mut budget).is_err());
    }

    #[test]
    fn regex_normalization_keeps_character_classes_and_rejects_empty_matches() {
        let matcher = Matchers::new(&[RedactionTarget::Regex {
            pattern: "[a\u{301}]".into(),
        }])
        .unwrap();
        let matches = matcher.find("a ́", &mut MatchBudget::default()).unwrap();
        assert_eq!(matches.len(), 2);
        let matcher = Matchers::new(&[RedactionTarget::Regex {
            pattern: "a\u{306}\u{301}".into(),
        }])
        .unwrap();
        assert_eq!(
            matcher
                .find("ắ", &mut MatchBudget::default())
                .unwrap()
                .len(),
            1
        );
        assert!(
            Matchers::new(&[RedactionTarget::Regex {
                pattern: r"\b".into()
            }])
            .is_err()
        );
    }

    #[test]
    fn canonical_equivalents_match_original_byte_ranges() {
        let matcher = Matchers::new(&[RedactionTarget::Exact { text: "ắ".into() }]).unwrap();
        let source = "x a\u{306}\u{301} y";
        let found = matcher.find(source, &mut MatchBudget::default()).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(&source[found[0].start..found[0].end], "a\u{306}\u{301}");
    }
}
