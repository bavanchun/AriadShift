//! Parameters for explicit watermark removal.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The exact candidate identifiers selected by the user.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WatermarkRemoveParams {
    /// Nonempty set of detector-issued identifiers; wildcards are never accepted.
    #[schemars(length(min = 1))]
    pub candidates: Vec<String>,
}

impl WatermarkRemoveParams {
    /// Refuse wildcards, empty identifiers, and duplicate selections.
    pub fn validate(&self) -> Result<(), &'static str> {
        let mut seen = std::collections::HashSet::new();
        if self.candidates.is_empty()
            || self.candidates.iter().any(|id| {
                id.trim().is_empty() || matches!(id.as_str(), "all" | "*") || !seen.insert(id)
            })
        {
            return Err("candidates must be unique explicit identifiers");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn selection_must_be_explicit_and_unique() {
        assert!(serde_json::from_value::<WatermarkRemoveParams>(json!({})).is_err());
        for candidates in [vec![], vec!["all"], vec!["*"], vec!["x", "x"]] {
            let params: WatermarkRemoveParams =
                serde_json::from_value(json!({"candidates":candidates})).unwrap();
            assert!(params.validate().is_err());
        }
        let params: WatermarkRemoveParams =
            serde_json::from_value(json!({"candidates":["x"]})).unwrap();
        assert!(params.validate().is_ok());
    }
}
