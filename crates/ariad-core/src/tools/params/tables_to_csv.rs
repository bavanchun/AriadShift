//! Parameter types for the `pdf.tables_to_csv` tool.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Parameters for `pdf.tables_to_csv`.
///
/// `pdf.tables_to_csv` extracts every table in the input PDF into its own CSV file.
/// It takes no mandatory parameters (`{}`) and rejects unknown fields.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TablesToCsvParams {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserializes_empty_params() {
        let params: TablesToCsvParams = serde_json::from_str("{}").expect("valid params");
        assert_eq!(params, TablesToCsvParams {});
        assert!(serde_json::from_str::<TablesToCsvParams>(r#"{"unknown":1}"#).is_err());
    }
}
