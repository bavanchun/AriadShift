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
/// Each IR nesting level produces 2 to 4 levels of JSON nesting (objects and arrays).
/// A margin is added to accommodate document root structures and metadata.
#[must_use]
pub const fn json_depth_budget_for_nesting(max_nesting_depth: u16) -> usize {
    (max_nesting_depth as usize) * 8 + 64
}

/// Scans `bytes` for JSON nesting depth in a single pass without recursion.
///
/// Returns the maximum observed depth, or [`JsonDepthError::DepthExceeded`] if
/// depth exceeds `max_depth` at any point.
pub fn prescan_json_depth(bytes: &[u8], max_depth: usize) -> Result<usize, JsonDepthError> {
    let mut in_string = false;
    let mut escaped = false;
    let mut current_depth = 0_usize;
    let mut max_observed_depth = 0_usize;

    for &byte in bytes {
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
        } else {
            match byte {
                b'"' => in_string = true,
                b'{' | b'[' => {
                    current_depth = current_depth.saturating_add(1);
                    if current_depth > max_depth {
                        return Err(JsonDepthError::DepthExceeded {
                            depth: current_depth,
                            max_depth,
                        });
                    }
                    if current_depth > max_observed_depth {
                        max_observed_depth = current_depth;
                    }
                }
                b'}' | b']' => {
                    current_depth = current_depth.saturating_sub(1);
                }
                _ => {}
            }
        }
    }

    Ok(max_observed_depth)
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
