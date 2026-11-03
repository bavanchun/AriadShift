//! Parameter types for the `pdf.outline` tool.

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};

/// Action performed by `pdf.outline`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OutlineAction {
    /// Export the document's bookmark tree (`/Outlines`) as a JSON artifact.
    Export,
    /// Replace the document's `/Outlines` hierarchy with the tree in `json`.
    Import,
    /// Remove `/Outlines` from the document catalog.
    Clear,
    /// Generate a bookmark hierarchy automatically from detected document headings.
    #[serde(alias = "from-headings")]
    FromHeadings,
}

/// A single bookmark node in a `pdf.outline` hierarchy.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OutlineNode {
    /// Visible bookmark title string.
    pub title: String,
    /// 1-based destination page number (`>= 1`).
    #[serde(deserialize_with = "deserialize_nonzero_page")]
    #[schemars(range(min = 1))]
    pub page: u32,
    /// Nested child bookmarks under this node.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<OutlineNode>,
}

fn deserialize_nonzero_page<'de, D>(deserializer: D) -> Result<u32, D::Error>
where
    D: Deserializer<'de>,
{
    let page = u32::deserialize(deserializer)?;
    if page == 0 {
        return Err(serde::de::Error::custom("outline page must be >= 1"));
    }
    Ok(page)
}

/// Parameters for `pdf.outline`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OutlineParams {
    /// Outline operation (`"export"`, `"import"`, `"clear"`, or `"from_headings"`).
    pub action: OutlineAction,
    /// Bookmark tree to import when `action == "import"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub json: Option<Vec<OutlineNode>>,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn outline_params_validates_action_tree_and_rejects_unknown_fields() {
        let export: OutlineParams = serde_json::from_value(json!({"action": "export"})).unwrap();
        assert_eq!(export.action, OutlineAction::Export);
        assert!(export.json.is_none());

        let import: OutlineParams = serde_json::from_value(json!({
            "action": "import",
            "json": [
                {
                    "title": "Chapter 1",
                    "page": 1,
                    "children": [
                        { "title": "Section 1.1", "page": 2 }
                    ]
                }
            ]
        }))
        .unwrap();
        assert_eq!(import.action, OutlineAction::Import);
        assert_eq!(import.json.as_ref().map(Vec::len), Some(1));

        assert!(serde_json::from_value::<OutlineParams>(json!({})).is_err());
        assert!(
            serde_json::from_value::<OutlineParams>(json!({"action": "clear", "extra": 1}))
                .is_err()
        );
        assert!(
            serde_json::from_value::<OutlineParams>(json!({
                "action": "import",
                "json": [{"title": "Bad", "page": 0}]
            }))
            .is_err()
        );
    }
}
