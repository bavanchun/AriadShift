//! Parameter types for the `pdf.metadata` tool.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Document metadata fields to set or update (`pdf.metadata`).
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MetadataFields {
    /// Document title (`/Title` and `dc:title`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Document author (`/Author` and `dc:creator`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    /// Document subject (`/Subject` and `dc:description`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    /// Document keywords (`/Keywords` and `pdf:Keywords`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keywords: Option<String>,
    /// Creator application (`/Creator` and `xmp:CreatorTool`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub creator: Option<String>,
    /// Producer application (`/Producer` and `pdf:Producer`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub producer: Option<String>,
    /// Creation timestamp (`/CreationDate` and `xmp:CreateDate`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<String>,
    /// Modification timestamp (`/ModDate` and `xmp:ModifyDate`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modified: Option<String>,
    /// Additional custom `/Info` dictionary entries (`key -> value`).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub custom: BTreeMap<String, String>,
}

/// Parameters for `pdf.metadata`.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MetadataParams {
    /// Metadata fields to set or overwrite in `/Info` and XMP `/Metadata`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub set: Option<MetadataFields>,
    /// Metadata field keys to remove from `/Info` and XMP `/Metadata`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remove: Option<Vec<String>>,
    /// When `true`, strip all `/Info` and XMP `/Metadata` entries before applying `set` (`false` by default).
    #[serde(default)]
    pub strip_all: bool,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn metadata_params_defaults_unknown_field_and_valid_invalid_json() {
        let defaults: MetadataParams = serde_json::from_value(json!({})).unwrap();
        assert_eq!(defaults, MetadataParams::default());
        assert!(!defaults.strip_all);

        let valid: MetadataParams = serde_json::from_value(json!({
            "set": {
                "title": "Specification",
                "author": "Ariadnev",
                "custom": { "Department": "R&D" }
            },
            "remove": ["Subject"],
            "strip_all": false
        }))
        .unwrap();
        assert_eq!(
            valid.set.as_ref().and_then(|s| s.title.as_deref()),
            Some("Specification")
        );
        assert_eq!(valid.remove, Some(vec!["Subject".to_owned()]));

        assert!(serde_json::from_value::<MetadataParams>(json!({"unknown": 1})).is_err());
        assert!(
            serde_json::from_value::<MetadataParams>(json!({"set": {"bad_field": "x"}})).is_err()
        );
    }
}
