//! Parameter types for PDF/A and PDF/UA validation tools.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Parameters for `pdf.validate_pdfa`.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ValidatePdfaParams {
    /// PDF/A flavour to validate against (e.g., "1a", "1b", "2a", "2b", "2u", "3a", "3b", "3u", "4", "4f", "4e").
    /// If omitted or null, the flavour is auto-detected from document metadata.
    #[serde(default, alias = "profile", skip_serializing_if = "Option::is_none")]
    pub flavour: Option<String>,
}

/// Parameters for `pdf.validate_pdfua`.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ValidatePdfUaParams {
    /// PDF/UA flavour to validate against (e.g., "1", "2", "ua1", "ua2").
    /// Defaults to "1" (PDF/UA-1) if omitted or null.
    #[serde(default, alias = "profile", skip_serializing_if = "Option::is_none")]
    pub flavour: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_pdfa_params_default() {
        let params: ValidatePdfaParams = serde_json::from_str("{}").expect("valid default JSON");
        assert_eq!(params.flavour, None);
    }

    #[test]
    fn validate_pdfa_params_flavour_and_profile_alias() {
        let from_flavour: ValidatePdfaParams =
            serde_json::from_str(r#"{"flavour":"2b"}"#).expect("valid flavour");
        assert_eq!(from_flavour.flavour.as_deref(), Some("2b"));

        let from_profile: ValidatePdfaParams =
            serde_json::from_str(r#"{"profile":"2b"}"#).expect("valid profile alias");
        assert_eq!(from_profile.flavour.as_deref(), Some("2b"));
    }

    #[test]
    fn validate_pdfa_rejects_unknown_fields() {
        assert!(serde_json::from_str::<ValidatePdfaParams>(r#"{"unknown":true}"#).is_err());
    }

    #[test]
    fn validate_pdfua_params_default() {
        let params: ValidatePdfUaParams = serde_json::from_str("{}").expect("valid default JSON");
        assert_eq!(params.flavour, None);
    }

    #[test]
    fn validate_pdfua_rejects_unknown_fields() {
        assert!(serde_json::from_str::<ValidatePdfUaParams>(r#"{"unknown":123}"#).is_err());
    }
}
