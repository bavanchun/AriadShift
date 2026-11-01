//! Parameter type for the `pdf.detect_scanned` tool.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Parameters for `pdf.detect_scanned`.
///
/// The tool takes no parameters (`{}`) and rejects any unknown fields.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DetectScannedParams {}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn accepts_the_empty_object() {
        let params: DetectScannedParams = serde_json::from_value(json!({})).expect("valid");
        assert_eq!(params, DetectScannedParams::default());
    }

    #[test]
    fn rejects_any_field() {
        assert!(serde_json::from_value::<DetectScannedParams>(json!({"pages": "all"})).is_err());
        assert!(serde_json::from_value::<DetectScannedParams>(json!(3)).is_err());
    }
}
