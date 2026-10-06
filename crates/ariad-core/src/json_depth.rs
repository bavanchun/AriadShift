//! I/O-free token-level JSON nesting depth pre-scanner.
//!
//! Counts opening `{` and `[` characters outside string literals in a single pass
//! without recursion. Rejects inputs that exceed a depth budget before deserializing,
//! preventing call-stack overflows when building or dropping deep trees.

use thiserror::Error;

/// Error returned when JSON nesting depth exceeds the allowed budget.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum JsonDepthError {
    #[error("JSON nesting depth {depth} exceeds limit of {max_depth}")]
    DepthExceeded { depth: usize, max_depth: usize },
}

/// Derives a JSON depth budget from the configured IR `max_nesting_depth`.
///
/// Each IR nesting level produces 2 to 5 levels of JSON nesting (objects and arrays,
/// e.g. nested lists or tables in table cells).
/// A margin is added to accommodate document root structures and metadata.
#[must_use]
pub const fn json_depth_budget_for_nesting(max_nesting_depth: u16) -> usize {
    (max_nesting_depth as usize) * 8 + 64
}

/// Incremental state machine for scanning JSON nesting depth without recursion.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JsonDepthScanner {
    in_string: bool,
    escaped: bool,
    current_depth: usize,
    max_observed_depth: usize,
    max_depth: usize,
}

impl JsonDepthScanner {
    /// Creates a new scanner with the specified maximum nesting depth limit.
    #[must_use]
    pub const fn new(max_depth: usize) -> Self {
        Self {
            in_string: false,
            escaped: false,
            current_depth: 0,
            max_observed_depth: 0,
            max_depth,
        }
    }

    /// Feeds a chunk of bytes to the scanner, updating the current depth.
    ///
    /// Returns [`JsonDepthError::DepthExceeded`] if `current_depth` exceeds `max_depth`.
    pub fn feed(&mut self, bytes: &[u8]) -> Result<(), JsonDepthError> {
        for &byte in bytes {
            if self.in_string {
                if self.escaped {
                    self.escaped = false;
                } else if byte == b'\\' {
                    self.escaped = true;
                } else if byte == b'"' {
                    self.in_string = false;
                }
            } else {
                match byte {
                    b'"' => self.in_string = true,
                    b'{' | b'[' => {
                        self.current_depth = self.current_depth.saturating_add(1);
                        if self.current_depth > self.max_depth {
                            return Err(JsonDepthError::DepthExceeded {
                                depth: self.current_depth,
                                max_depth: self.max_depth,
                            });
                        }
                        if self.current_depth > self.max_observed_depth {
                            self.max_observed_depth = self.current_depth;
                        }
                    }
                    b'}' | b']' => {
                        self.current_depth = self.current_depth.saturating_sub(1);
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }

    /// Returns the maximum depth observed so far.
    #[must_use]
    pub const fn max_observed_depth(&self) -> usize {
        self.max_observed_depth
    }

    /// Returns the current nesting depth.
    #[must_use]
    pub const fn current_depth(&self) -> usize {
        self.current_depth
    }
}

/// Scans `bytes` for JSON nesting depth in a single pass without recursion.
///
/// Returns the maximum observed depth, or [`JsonDepthError::DepthExceeded`] if
/// depth exceeds `max_depth` at any point.
pub fn prescan_json_depth(bytes: &[u8], max_depth: usize) -> Result<usize, JsonDepthError> {
    let mut scanner = JsonDepthScanner::new(max_depth);
    scanner.feed(bytes)?;
    Ok(scanner.max_observed_depth())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_flat_and_moderate_json() {
        assert_eq!(prescan_json_depth(b"{}", 10).unwrap(), 1);
        assert_eq!(prescan_json_depth(b"[]", 10).unwrap(), 1);
        assert_eq!(
            prescan_json_depth(br#"{"key": [1, 2, {"nested": true}]}"#, 10).unwrap(),
            3
        );
    }

    #[test]
    fn ignores_brackets_inside_strings() {
        let json = br#"{"key": "{[{\"still_string\": true}]}"}"#;
        assert_eq!(prescan_json_depth(json, 10).unwrap(), 1);

        let json_with_escaped_quotes = br#"{"key": "quote \" {[ and escaped backslash \\"}"#;
        assert_eq!(prescan_json_depth(json_with_escaped_quotes, 10).unwrap(), 1);
    }

    #[test]
    fn rejects_depth_exceeding_budget() {
        let nested = b"[[[[[[[[[[ ]]]]]]]]]]"; // depth 10
        assert_eq!(
            prescan_json_depth(nested, 5),
            Err(JsonDepthError::DepthExceeded {
                depth: 6,
                max_depth: 5,
            })
        );
    }

    #[test]
    fn budget_derivation_provides_margin() {
        let budget = json_depth_budget_for_nesting(64);
        assert!(budget >= 256);
        assert_eq!(budget, 64 * 8 + 64);
    }
}
