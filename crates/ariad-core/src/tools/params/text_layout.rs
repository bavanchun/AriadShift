//! Parameter type for the `pdf.text_layout` tool.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::tools::PageRange;

/// Parameters for `pdf.text_layout`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TextLayoutParams {
    /// Pages to analyse (1-based selector).
    #[serde(default)]
    pub pages: PageRange,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn defaults_to_all_pages() {
        let params: TextLayoutParams = serde_json::from_value(json!({})).expect("valid");
        assert_eq!(params.pages, PageRange::all());
    }

    #[test]
    fn accepts_a_valid_example() {
        let params: TextLayoutParams =
            serde_json::from_value(json!({"pages": "1,3-5"})).expect("valid");
        assert_eq!(params.pages.as_str(), "1,3-5");
    }

    #[test]
    fn rejects_invalid_examples() {
        for value in [
            json!({"pages": "0"}),
            json!({"words": true}),
            json!({"pages": 3}),
        ] {
            assert!(serde_json::from_value::<TextLayoutParams>(value).is_err());
        }
    }
}
