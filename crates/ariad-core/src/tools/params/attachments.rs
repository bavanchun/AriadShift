//! Parameter types for the `pdf.attachments` tool.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Action performed by `pdf.attachments`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AttachmentsAction {
    /// List embedded file attachments as JSON.
    List,
    /// Embed additional input files into the PDF `/EmbeddedFiles` name tree.
    Add,
    /// Extract embedded attachments as output artifacts.
    Extract,
    /// Remove embedded attachments matching `names` (or all attachments when `names` is omitted).
    Remove,
}

/// Parameters for `pdf.attachments`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AttachmentsParams {
    /// Attachment operation (`"list"`, `"add"`, `"extract"`, or `"remove"`).
    pub action: AttachmentsAction,
    /// Optional attachment filenames to filter on for `"extract"` or `"remove"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub names: Option<Vec<String>>,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn attachments_params_validates_action_and_rejects_unknown_fields() {
        let list: AttachmentsParams = serde_json::from_value(json!({"action": "list"})).unwrap();
        assert_eq!(list.action, AttachmentsAction::List);
        assert!(list.names.is_none());

        let remove: AttachmentsParams = serde_json::from_value(json!({
            "action": "remove",
            "names": ["data.csv"]
        }))
        .unwrap();
        assert_eq!(remove.action, AttachmentsAction::Remove);
        assert_eq!(remove.names, Some(vec!["data.csv".to_owned()]));

        assert!(serde_json::from_value::<AttachmentsParams>(json!({})).is_err());
        assert!(
            serde_json::from_value::<AttachmentsParams>(json!({"action": "list", "extra": 1}))
                .is_err()
        );
        assert!(serde_json::from_value::<AttachmentsParams>(json!({"action": "bogus"})).is_err());
    }
}
