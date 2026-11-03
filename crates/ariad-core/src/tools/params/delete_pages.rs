//! Parameter types for the `pdf.delete_pages` tool.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::tools::page_range::PageRange;

/// Parameters for `pdf.delete_pages`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeletePagesParams {
    /// 1-based page range selection to delete (must leave at least 1 page in the document).
    pub pages: PageRange,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn delete_pages_params_requires_pages_and_rejects_unknown_fields() {
        let valid: DeletePagesParams = serde_json::from_value(json!({"pages": "2,4-6"})).unwrap();
        assert_eq!(valid.pages.as_str(), "2,4-6");

        assert!(serde_json::from_value::<DeletePagesParams>(json!({})).is_err());
        assert!(
            serde_json::from_value::<DeletePagesParams>(json!({"pages": "1", "unknown": 1}))
                .is_err()
        );
        assert!(
            serde_json::from_value::<DeletePagesParams>(json!({"pages": "bad-token"})).is_err()
        );
    }
}
