//! Parameter types for the `pdf.decompress` tool.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::tools::page_range::PageRange;

/// How compressed object streams (`/Type /ObjStm`) are handled by `pdf.decompress`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ObjectStreamsMode {
    /// Keep object stream grouping if present.
    Keep,
    /// Expand all compressed object streams into top-level indirect objects (`"expand"`, default).
    #[default]
    Expand,
}

/// Parameters for `pdf.decompress`.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DecompressParams {
    /// Optional 1-based page selection whose streams should be decompressed (all streams when omitted).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pages: Option<PageRange>,
    /// Whether to expand `/ObjStm` object streams (`"expand"` by default).
    #[serde(default)]
    pub object_streams: ObjectStreamsMode,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn decompress_params_defaults_unknown_field_and_valid_invalid_json() {
        let defaults: DecompressParams = serde_json::from_value(json!({})).unwrap();
        assert_eq!(defaults, DecompressParams::default());
        assert!(defaults.pages.is_none());
        assert_eq!(defaults.object_streams, ObjectStreamsMode::Expand);

        let valid: DecompressParams = serde_json::from_value(json!({
            "pages": "1-5",
            "object_streams": "keep"
        }))
        .unwrap();
        assert_eq!(valid.pages.as_ref().map(PageRange::as_str), Some("1-5"));
        assert_eq!(valid.object_streams, ObjectStreamsMode::Keep);

        assert!(serde_json::from_value::<DecompressParams>(json!({"unknown": 1})).is_err());
        assert!(
            serde_json::from_value::<DecompressParams>(json!({"object_streams": "bad"})).is_err()
        );
    }
}
