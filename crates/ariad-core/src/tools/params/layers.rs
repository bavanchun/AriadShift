//! Parameter types for the `pdf.layers` tool.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Action performed on PDF optional content groups (`/OCProperties`) by `pdf.layers`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LayersAction {
    /// List optional content groups (layers) and their default visibility as JSON.
    List,
    /// Set the default visibility (`ON` or `OFF`) of the layers named in `names`.
    #[serde(alias = "set-default-visibility")]
    SetDefaultVisibility,
    /// Remove the layers named in `names` and their marked content from the PDF.
    Remove,
}

/// Parameters for `pdf.layers`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LayersParams {
    /// Layer operation (`"list"`, `"set_default_visibility"`, or `"remove"`).
    pub action: LayersAction,
    /// Target layer names for `"set_default_visibility"` or `"remove"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub names: Option<Vec<String>>,
    /// Target default visibility state when `action == "set_default_visibility"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible: Option<bool>,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn layers_params_validates_action_and_rejects_unknown_fields() {
        let list: LayersParams = serde_json::from_value(json!({"action": "list"})).unwrap();
        assert_eq!(list.action, LayersAction::List);
        assert!(list.names.is_none());
        assert!(list.visible.is_none());

        let set_vis: LayersParams = serde_json::from_value(json!({
            "action": "set_default_visibility",
            "names": ["Watermark"],
            "visible": false
        }))
        .unwrap();
        assert_eq!(set_vis.action, LayersAction::SetDefaultVisibility);
        assert_eq!(set_vis.names, Some(vec!["Watermark".to_owned()]));
        assert_eq!(set_vis.visible, Some(false));

        assert!(serde_json::from_value::<LayersParams>(json!({})).is_err());
        assert!(
            serde_json::from_value::<LayersParams>(json!({"action": "list", "extra": 1})).is_err()
        );
        assert!(serde_json::from_value::<LayersParams>(json!({"action": "flatten"})).is_err());
    }
}
