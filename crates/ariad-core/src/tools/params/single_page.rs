//! Parameter types for the `pdf.single_page` tool.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Parameters for `pdf.single_page`.
///
/// `pdf.single_page` takes no parameters (`{}`) and rejects any unknown fields.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SinglePageParams {}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn single_page_params_defaults_and_rejects_unknown_fields() {
        let parsed: SinglePageParams = serde_json::from_value(json!({})).unwrap();
        assert_eq!(parsed, SinglePageParams::default());

        assert!(serde_json::from_value::<SinglePageParams>(json!({"extra": 1})).is_err());
        assert!(serde_json::from_value::<SinglePageParams>(json!("not-an-object")).is_err());
    }
}
