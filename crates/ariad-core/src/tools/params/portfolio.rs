//! Parameter types for the `pdf.portfolio` tool.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Action performed by `pdf.portfolio`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PortfolioAction {
    /// Package multiple input files into a PDF collection (`/Collection`) with a cover page.
    Create,
    /// List embedded files in a PDF portfolio as JSON.
    List,
    /// Extract all files from a PDF portfolio.
    Extract,
}

/// Initial collection view mode (`/Collection /View`) for `pdf.portfolio`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum PortfolioView {
    /// Multi-column details table view (`"details"`, default; PDF `/D`).
    #[default]
    Details,
    /// Tile icon view (`"tile"`; PDF `/T`).
    Tile,
}

/// Parameters for `pdf.portfolio`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PortfolioParams {
    /// Portfolio operation (`"create"`, `"list"`, or `"extract"`).
    pub action: PortfolioAction,
    /// Initial collection view when `action == "create"` (`"details"` by default, or `"tile"`).
    #[serde(default)]
    pub view: PortfolioView,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn portfolio_params_defaults_unknown_field_and_valid_invalid_json() {
        let create_default: PortfolioParams =
            serde_json::from_value(json!({"action": "create"})).unwrap();
        assert_eq!(create_default.action, PortfolioAction::Create);
        assert_eq!(create_default.view, PortfolioView::Details);

        let tile: PortfolioParams = serde_json::from_value(json!({
            "action": "create",
            "view": "tile"
        }))
        .unwrap();
        assert_eq!(tile.view, PortfolioView::Tile);

        assert!(serde_json::from_value::<PortfolioParams>(json!({})).is_err());
        assert!(
            serde_json::from_value::<PortfolioParams>(json!({"action": "list", "unknown": 1}))
                .is_err()
        );
        assert!(
            serde_json::from_value::<PortfolioParams>(json!({"action": "create", "view": "bad"}))
                .is_err()
        );
    }
}
