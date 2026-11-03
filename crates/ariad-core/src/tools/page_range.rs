//! Shared 1-based page-range parser and expander (`docs/tools.md` §5).
//!
//! Grammar (EBNF):
//! ```ebnf
//! page_range  = token , { "," , token } ;
//! token       = "all"
//!             | "even"
//!             | "odd"
//!             | range_expr
//!             | page_ref ;
//! range_expr  = page_ref , "-" , page_ref
//!             | page_ref , "-"
//!             | "-" , page_ref ;
//! page_ref    = pos_int
//!             | "last"
//!             | "last-" , pos_int ;
//! pos_int     = nonzero_digit , { digit } ;
//! ```

use std::collections::HashSet;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Policy governing whether duplicate 1-based page indices are allowed during expansion.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DuplicatePolicy {
    /// Preserve duplicate page indices in the exact order specified by the user
    /// (used by reorder and extraction tools such as `pdf.extract_pages` and `pdf.reorder`).
    KeepInOrder,
    /// Reject any repeated page index with [`PageRangeError::DuplicatePage`]
    /// (used by in-place modification and deletion tools such as `pdf.rotate` and `pdf.delete_pages`).
    Reject,
}

/// A parsed 1-based page reference endpoint.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum PageRef {
    /// Explicit 1-based page number `N` (`N >= 1`).
    Page(u32),
    /// The final page `P` (`"last"`).
    Last,
    /// Page `P - K` (`"last-K"`, `K >= 1`).
    FromLast(u32),
}

impl PageRef {
    /// Resolves this reference against a document with `page_count` pages (`page_count >= 1`).
    pub fn resolve(self, page_count: u32, token: &str) -> Result<u32, PageRangeError> {
        if page_count == 0 {
            return Err(PageRangeError::ExceedsPageCount {
                token: token.to_owned(),
                page_count: 0,
            });
        }
        match self {
            Self::Page(n) => {
                if n > page_count {
                    Err(PageRangeError::ExceedsPageCount {
                        token: token.to_owned(),
                        page_count,
                    })
                } else {
                    Ok(n)
                }
            }
            Self::Last => Ok(page_count),
            Self::FromLast(k) => {
                if k >= page_count {
                    Err(PageRangeError::UnderflowsPageCount {
                        token: token.to_owned(),
                        page_count,
                    })
                } else {
                    Ok(page_count - k)
                }
            }
        }
    }
}

/// A single parsed comma-separated selector token inside a [`PageRange`].
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct PageToken {
    /// Original trimmed token text used for normative error reporting.
    pub raw: String,
    /// Parsed selector semantics.
    pub kind: PageTokenKind,
}

/// Semantic kind of a [`PageToken`].
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum PageTokenKind {
    /// `"all"` (`1..=P`).
    All,
    /// `"even"` (`2, 4, ...`).
    Even,
    /// `"odd"` (`1, 3, ...`).
    Odd,
    /// Single page reference (`N`, `"last"`, or `"last-K"`).
    Single(PageRef),
    /// Inclusive range (`start..=end` ascending when `start <= end`, descending when `start > end`).
    Range {
        /// Range start reference.
        start: PageRef,
        /// Range end reference.
        end: PageRef,
    },
}

/// Validated 1-based page-range selector (`"all"`, `"1-3, 5, last-1"`, etc.).
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(try_from = "String", into = "String")]
#[schemars(
    with = "String",
    description = "1-based page range selector (`all`, `even`, `odd`, `N`, `N-M`, `N-`, `-M`, `last`, `last-K`)."
)]
pub struct PageRange {
    raw: String,
    tokens: Vec<PageToken>,
}

impl Default for PageRange {
    fn default() -> Self {
        Self::all()
    }
}

/// Normative error returned when parsing or expanding a [`PageRange`].
///
/// Every variant names the exact offending comma-separated token (`docs/tools.md` §5).
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum PageRangeError {
    /// Token failed syntax validation.
    #[error(
        "invalid page range token {token:?}: expected page number, range, \"all\", \"even\", \"odd\", or \"last\""
    )]
    InvalidToken {
        /// The offending token.
        token: String,
    },
    /// Token used 0 as a page number (`pages are 1-based`).
    #[error("invalid page range token {token:?}: pages are 1-based")]
    ZeroIndex {
        /// The offending token.
        token: String,
    },
    /// Explicit page number exceeded the document's total page count.
    #[error("page range token {token:?} exceeds document page count ({page_count})")]
    ExceedsPageCount {
        /// The offending token.
        token: String,
        /// Total pages in the target document.
        page_count: u32,
    },
    /// `"last-K"` offset underflowed the document's total page count (`K >= page_count`).
    #[error("page range token {token:?} underflows document page count ({page_count})")]
    UnderflowsPageCount {
        /// The offending token.
        token: String,
        /// Total pages in the target document.
        page_count: u32,
    },
    /// A duplicate page was selected under [`DuplicatePolicy::Reject`].
    #[error("duplicate page {page} in page range token {token:?} (already selected)")]
    DuplicatePage {
        /// 1-based duplicate page number.
        page: u32,
        /// The offending token.
        token: String,
    },
    /// Selector produced an empty page list (for example `"even"` on a 1-page document).
    #[error("page range token {token:?} selects no pages in a {page_count}-page document")]
    EmptySelection {
        /// The offending token.
        token: String,
        /// Total pages in the target document.
        page_count: u32,
    },
}

impl PageRangeError {
    /// Returns the offending comma-separated token named by this error.
    #[must_use]
    pub fn token(&self) -> &str {
        match self {
            Self::InvalidToken { token }
            | Self::ZeroIndex { token }
            | Self::ExceedsPageCount { token, .. }
            | Self::UnderflowsPageCount { token, .. }
            | Self::DuplicatePage { token, .. }
            | Self::EmptySelection { token, .. } => token,
        }
    }
}

impl PageRange {
    /// Returns the canonical `"all"` page range selector.
    #[must_use]
    pub fn all() -> Self {
        Self {
            raw: "all".to_owned(),
            tokens: vec![PageToken {
                raw: "all".to_owned(),
                kind: PageTokenKind::All,
            }],
        }
    }

    /// Parses a comma-separated page-range expression according to the normative EBNF grammar.
    pub fn parse(input: &str) -> Result<Self, PageRangeError> {
        let trimmed_all = input.trim_matches(|c: char| c.is_ascii_whitespace());
        if trimmed_all.is_empty() {
            return Err(PageRangeError::InvalidToken {
                token: String::new(),
            });
        }

        let mut tokens = Vec::new();
        for part in input.split(',') {
            let raw = part.trim_matches(|c: char| c.is_ascii_whitespace());
            if raw.is_empty() {
                return Err(PageRangeError::InvalidToken {
                    token: String::new(),
                });
            }
            let kind = parse_token_kind(raw)?;
            tokens.push(PageToken {
                raw: raw.to_owned(),
                kind,
            });
        }

        Ok(Self {
            raw: trimmed_all.to_owned(),
            tokens,
        })
    }

    /// Returns the original page-range expression string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.raw
    }

    /// Returns the parsed comma-separated tokens.
    #[must_use]
    pub fn tokens(&self) -> &[PageToken] {
        &self.tokens
    }

    /// Expands this page range against a document with `page_count` pages (`1..=page_count`)
    /// under `policy`, returning 1-based page numbers in selection order.
    pub fn expand(
        &self,
        page_count: u32,
        policy: DuplicatePolicy,
    ) -> Result<Vec<u32>, PageRangeError> {
        if page_count == 0 {
            let first_token = self
                .tokens
                .first()
                .map(|t| t.raw.clone())
                .unwrap_or_default();
            return Err(PageRangeError::ExceedsPageCount {
                token: first_token,
                page_count: 0,
            });
        }

        let mut pages = Vec::new();
        let mut seen = if policy == DuplicatePolicy::Reject {
            Some(HashSet::new())
        } else {
            None
        };

        for token in &self.tokens {
            let mut push_page = |page: u32| -> Result<(), PageRangeError> {
                if let Some(ref mut set) = seen
                    && !set.insert(page)
                {
                    return Err(PageRangeError::DuplicatePage {
                        page,
                        token: token.raw.clone(),
                    });
                }
                pages.push(page);
                Ok(())
            };

            match token.kind {
                PageTokenKind::All => {
                    for p in 1..=page_count {
                        push_page(p)?;
                    }
                }
                PageTokenKind::Even => {
                    if page_count < 2 {
                        return Err(PageRangeError::EmptySelection {
                            token: token.raw.clone(),
                            page_count,
                        });
                    }
                    let mut p = 2;
                    while p <= page_count {
                        push_page(p)?;
                        match p.checked_add(2) {
                            Some(next) => p = next,
                            None => break,
                        }
                    }
                }
                PageTokenKind::Odd => {
                    let mut p = 1;
                    while p <= page_count {
                        push_page(p)?;
                        match p.checked_add(2) {
                            Some(next) => p = next,
                            None => break,
                        }
                    }
                }
                PageTokenKind::Single(page_ref) => {
                    let p = page_ref.resolve(page_count, &token.raw)?;
                    push_page(p)?;
                }
                PageTokenKind::Range { start, end } => {
                    let start_page = start.resolve(page_count, &token.raw)?;
                    let end_page = end.resolve(page_count, &token.raw)?;
                    if start_page <= end_page {
                        for p in start_page..=end_page {
                            push_page(p)?;
                        }
                    } else {
                        for p in (end_page..=start_page).rev() {
                            push_page(p)?;
                        }
                    }
                }
            }
        }

        Ok(pages)
    }

    /// Alias for [`PageRange::expand`].
    pub fn resolve(
        &self,
        page_count: u32,
        policy: DuplicatePolicy,
    ) -> Result<Vec<u32>, PageRangeError> {
        self.expand(page_count, policy)
    }
}

fn parse_token_kind(raw: &str) -> Result<PageTokenKind, PageRangeError> {
    match raw {
        "all" => return Ok(PageTokenKind::All),
        "even" => return Ok(PageTokenKind::Even),
        "odd" => return Ok(PageTokenKind::Odd),
        "last" => return Ok(PageTokenKind::Single(PageRef::Last)),
        _ => {}
    }

    // Open-start range: `-<page_ref>` (e.g. `-3`, `-last`, `-last-1`)
    if let Some(rest) = raw.strip_prefix('-') {
        let end = parse_page_ref(rest, raw)?;
        return Ok(PageTokenKind::Range {
            start: PageRef::Page(1),
            end,
        });
    }

    // Open-end range: `<page_ref>-` (e.g. `8-`, `last-`, `last-2-`)
    if let Some(prefix) = raw.strip_suffix('-') {
        let start = parse_page_ref(prefix, raw)?;
        return Ok(PageTokenKind::Range {
            start,
            end: PageRef::Last,
        });
    }

    // Check if the entire token is a single `page_ref` (`pos_int`, `last`, or `last-<pos_int>`)
    if !raw.contains('-') {
        let page_ref = parse_page_ref(raw, raw)?;
        return Ok(PageTokenKind::Single(page_ref));
    }

    if let Some(after_last) = raw.strip_prefix("last-") {
        // Could be `last-<pos_int>` (single PageRef::FromLast),
        // `last-last` (Range { start: Last, end: Last }),
        // or `last-<pos_int>-<page_ref>` (Range { start: FromLast(k), end: page_ref }).
        if !after_last.contains('-') {
            if after_last == "last" {
                return Ok(PageTokenKind::Range {
                    start: PageRef::Last,
                    end: PageRef::Last,
                });
            }
            let k = parse_pos_int_for_last_offset(after_last, raw)?;
            return Ok(PageTokenKind::Single(PageRef::FromLast(k)));
        }

        // Split at the first `-` in `after_last`: `<k_str>-<right_str>`
        if let Some((k_str, right_str)) = after_last.split_once('-') {
            if k_str == "last" {
                // `last-last-<pos_int>` -> start = Last, end = FromLast(pos_int)
                let end = parse_page_ref(&format!("last-{right_str}"), raw)?;
                return Ok(PageTokenKind::Range {
                    start: PageRef::Last,
                    end,
                });
            }
            let k = parse_pos_int_for_last_offset(k_str, raw)?;
            let end = parse_page_ref(right_str, raw)?;
            return Ok(PageTokenKind::Range {
                start: PageRef::FromLast(k),
                end,
            });
        }
    }

    // Otherwise the left side must be `<pos_int>-<right_str>`
    if let Some((left_str, right_str)) = raw.split_once('-') {
        let start = PageRef::Page(parse_pos_int_page(left_str, raw)?);
        let end = parse_page_ref(right_str, raw)?;
        return Ok(PageTokenKind::Range { start, end });
    }

    Err(PageRangeError::InvalidToken {
        token: raw.to_owned(),
    })
}

fn parse_page_ref(part: &str, full_token: &str) -> Result<PageRef, PageRangeError> {
    if part == "last" {
        return Ok(PageRef::Last);
    }
    if let Some(k_str) = part.strip_prefix("last-") {
        let k = parse_pos_int_for_last_offset(k_str, full_token)?;
        return Ok(PageRef::FromLast(k));
    }
    let page = parse_pos_int_page(part, full_token)?;
    Ok(PageRef::Page(page))
}

fn parse_pos_int_page(s: &str, full_token: &str) -> Result<u32, PageRangeError> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return Err(PageRangeError::InvalidToken {
            token: full_token.to_owned(),
        });
    }
    if s.bytes().all(|b| b == b'0') {
        return Err(PageRangeError::ZeroIndex {
            token: full_token.to_owned(),
        });
    }
    if s.starts_with('0') {
        return Err(PageRangeError::InvalidToken {
            token: full_token.to_owned(),
        });
    }
    s.parse::<u32>().map_err(|_| PageRangeError::InvalidToken {
        token: full_token.to_owned(),
    })
}

fn parse_pos_int_for_last_offset(s: &str, full_token: &str) -> Result<u32, PageRangeError> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) || s.starts_with('0') {
        return Err(PageRangeError::InvalidToken {
            token: full_token.to_owned(),
        });
    }
    s.parse::<u32>().map_err(|_| PageRangeError::InvalidToken {
        token: full_token.to_owned(),
    })
}

impl std::fmt::Display for PageRange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.raw)
    }
}

impl std::str::FromStr for PageRange {
    type Err = PageRangeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

impl TryFrom<String> for PageRange {
    type Error = PageRangeError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

impl TryFrom<&str> for PageRange {
    type Error = PageRangeError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::parse(value)
    }
}

impl From<PageRange> for String {
    fn from(range: PageRange) -> Self {
        range.raw
    }
}

impl AsRef<str> for PageRange {
    fn as_ref(&self) -> &str {
        &self.raw
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_normative_examples_from_spec() {
        let check = |expr: &str, count: u32, policy: DuplicatePolicy, expected: &[u32]| {
            let range = PageRange::parse(expr).unwrap_or_else(|e| panic!("parse {expr}: {e}"));
            let actual = range
                .expand(count, policy)
                .unwrap_or_else(|e| panic!("expand {expr}: {e}"));
            assert_eq!(actual, expected, "expression {expr}");
        };

        check(
            "all",
            10,
            DuplicatePolicy::Reject,
            &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
        );
        check("even", 10, DuplicatePolicy::Reject, &[2, 4, 6, 8, 10]);
        check("odd", 10, DuplicatePolicy::Reject, &[1, 3, 5, 7, 9]);
        check("5", 10, DuplicatePolicy::Reject, &[5]);
        check("2-5", 10, DuplicatePolicy::Reject, &[2, 3, 4, 5]);
        check("5-2", 10, DuplicatePolicy::Reject, &[5, 4, 3, 2]);
        check("8-", 10, DuplicatePolicy::Reject, &[8, 9, 10]);
        check("-3", 10, DuplicatePolicy::Reject, &[1, 2, 3]);
        check("last", 10, DuplicatePolicy::Reject, &[10]);
        check("last-1", 10, DuplicatePolicy::Reject, &[9]);
        check("last-2-last", 10, DuplicatePolicy::Reject, &[8, 9, 10]);
        check(
            "1-3, 5, last-1",
            10,
            DuplicatePolicy::Reject,
            &[1, 2, 3, 5, 9],
        );
        check(
            "1,1,5-3,last",
            10,
            DuplicatePolicy::KeepInOrder,
            &[1, 1, 5, 4, 3, 10],
        );
    }

    #[test]
    fn reports_normative_error_messages_naming_offending_token() {
        let err = PageRange::parse("1..3").unwrap_err();
        assert_eq!(
            err.to_string(),
            r#"invalid page range token "1..3": expected page number, range, "all", "even", "odd", or "last""#
        );

        let err = PageRange::parse("0").unwrap_err();
        assert_eq!(
            err.to_string(),
            r#"invalid page range token "0": pages are 1-based"#
        );

        let err = PageRange::parse("12-15")
            .unwrap()
            .expand(10, DuplicatePolicy::Reject)
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            r#"page range token "12-15" exceeds document page count (10)"#
        );

        let err = PageRange::parse("last-10")
            .unwrap()
            .expand(10, DuplicatePolicy::Reject)
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            r#"page range token "last-10" underflows document page count (10)"#
        );

        let err = PageRange::parse("1-4,3-5")
            .unwrap()
            .expand(10, DuplicatePolicy::Reject)
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            r#"duplicate page 3 in page range token "3-5" (already selected)"#
        );
    }
}
