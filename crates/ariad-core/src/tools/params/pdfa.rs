//! Parameter types for PDF/A and PDF/UA validation and conversion tools.

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

/// PDF/A target conformance level.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub enum PdfaLevel {
    /// PDF/A-2b: Visual conformance without tagging or Unicode requirement.
    #[default]
    #[serde(rename = "2b")]
    Level2b,
    /// PDF/A-3b: Visual conformance allowing embedded files.
    #[serde(rename = "3b")]
    Level3b,
    /// PDF/A-2u: Visual conformance with Unicode mappings for all characters.
    #[serde(rename = "2u")]
    Level2u,
}

impl PdfaLevel {
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Level2b => "2b",
            Self::Level3b => "3b",
            Self::Level2u => "2u",
        }
    }
}

/// Parameters for `pdf.to_pdfa`.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ToPdfaParams {
    /// Target PDF/A conformance level ("2b", "3b", or "2u"). Defaults to "2b".
    #[serde(default)]
    pub level: PdfaLevel,
}

/// Parameters for `pdf.to_pdfua`.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ToPdfUaParams {
    /// Target PDF/UA conformance level ("1"). Defaults to "1".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(regex(pattern = "^1$"))]
    pub flavour: Option<String>,
}

/// Parameters for `pdf.accessibility_report`.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AccessibilityReportParams {}

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

    #[test]
    fn to_pdfa_params_default() {
        let params: ToPdfaParams = serde_json::from_str("{}").expect("valid default JSON");
        assert_eq!(params.level, PdfaLevel::Level2b);
        assert_eq!(params.level.as_str(), "2b");
    }

    #[test]
    fn to_pdfa_params_levels() {
        let p3b: ToPdfaParams = serde_json::from_str(r#"{"level":"3b"}"#).unwrap();
        assert_eq!(p3b.level, PdfaLevel::Level3b);
        assert_eq!(p3b.level.as_str(), "3b");

        let p2u: ToPdfaParams = serde_json::from_str(r#"{"level":"2u"}"#).unwrap();
        assert_eq!(p2u.level, PdfaLevel::Level2u);
        assert_eq!(p2u.level.as_str(), "2u");
    }

    #[test]
    fn accessibility_report_params_default() {
        let params: AccessibilityReportParams =
            serde_json::from_str("{}").expect("valid default JSON");
        assert_eq!(params, AccessibilityReportParams {});
    }
}
