//! Parameter types for the `pdf.alternate_mix` tool.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Parameters for `pdf.alternate_mix`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AlternateMixParams {
    /// Whether pages from the second input PDF should be taken in reverse order (`false` by default).
    #[serde(default)]
    pub reverse_second: bool,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn alternate_mix_params_defaults_unknown_field_and_valid_invalid_json() {
        let defaults: AlternateMixParams = serde_json::from_value(json!({})).unwrap();
        assert!(!defaults.reverse_second);

        let valid: AlternateMixParams =
            serde_json::from_value(json!({"reverse_second": true})).unwrap();
        assert!(valid.reverse_second);

        assert!(serde_json::from_value::<AlternateMixParams>(json!({"unknown": 1})).is_err());
        assert!(
            serde_json::from_value::<AlternateMixParams>(json!({"reverse_second": "yes"})).is_err()
        );
    }
}
