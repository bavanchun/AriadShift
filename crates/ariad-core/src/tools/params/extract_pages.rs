//! Parameter types for the `pdf.extract_pages` tool.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::tools::page_range::PageRange;

/// Parameters for `pdf.extract_pages`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExtractPagesParams {
    /// 1-based page range selection to extract (duplicates allowed and kept in order).
    pub pages: PageRange,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn extract_pages_params_requires_pages_and_rejects_unknown_fields() {
        let valid: ExtractPagesParams =
            serde_json::from_value(json!({"pages": "1,3-5,1,last"})).unwrap();
        assert_eq!(valid.pages.as_str(), "1,3-5,1,last");

        assert!(serde_json::from_value::<ExtractPagesParams>(json!({})).is_err());
        assert!(
            serde_json::from_value::<ExtractPagesParams>(json!({"pages": "1-3", "extra": true}))
                .is_err()
        );
        assert!(serde_json::from_value::<ExtractPagesParams>(json!({"pages": "0"})).is_err());
    }
}
