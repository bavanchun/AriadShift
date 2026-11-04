//! Bounded target definitions for PDF redaction.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{CropRect, HexColor, ParamsError};

/// Carriers to inspect and scrub. A report is partial if any carrier is excluded.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct RedactionScope {
    /// Text and text-bearing structures.
    pub text: bool,
    /// Image pixels and masks.
    pub images: bool,
    /// Vector content.
    pub vectors: bool,
    /// Annotations and form fields.
    pub annotations: bool,
    /// Metadata and other document-level text.
    pub metadata: bool,
    /// Embedded files.
    pub attachments: bool,
}

impl Default for RedactionScope {
    fn default() -> Self {
        Self {
            text: true,
            images: true,
            vectors: true,
            annotations: true,
            metadata: true,
            attachments: true,
        }
    }
}

impl RedactionScope {
    /// Whether the request includes every carrier required for full verification.
    #[must_use]
    pub const fn is_complete(self) -> bool {
        self.text
            && self.images
            && self.vectors
            && self.annotations
            && self.metadata
            && self.attachments
    }
}

/// Parameters for true redaction.
#[derive(Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RedactParams {
    /// Content and page regions to remove.
    pub targets: Vec<RedactionTarget>,
    /// Opaque replacement color; black when omitted.
    #[serde(default)]
    pub color: Option<HexColor>,
    /// Optional replacement text.
    #[serde(default)]
    pub overlay_text: Option<String>,
    /// Document carriers to inspect.
    #[serde(default)]
    pub scope: RedactionScope,
    /// Explicitly permit invalidation of signatures by the full rewrite.
    #[serde(default)]
    pub invalidate_signatures: bool,
}

impl std::fmt::Debug for RedactParams {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RedactParams")
            .field("targets", &self.targets)
            .field("color", &self.color)
            .field(
                "overlay_text",
                &self.overlay_text.as_ref().map(|_| "[redacted]"),
            )
            .field("scope", &self.scope)
            .field("invalidate_signatures", &self.invalidate_signatures)
            .finish()
    }
}

impl RedactParams {
    /// Validates the target bounds.
    pub fn validate(&self) -> Result<(), ParamsError> {
        if let Some(overlay) = &self.overlay_text
            && overlay.len() > 256
        {
            return Err(ParamsError::new(
                "overlay_text",
                "must not exceed 256 bytes",
            ));
        }
        validate_targets(&self.targets)
    }
}

/// Parameters for standalone redaction verification.
#[derive(Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RedactVerifyParams {
    /// Content and page regions that should be absent from the document.
    pub targets: Vec<RedactionTarget>,
    /// Document carriers to inspect.
    #[serde(default)]
    pub scope: RedactionScope,
}

impl std::fmt::Debug for RedactVerifyParams {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RedactVerifyParams")
            .field("targets", &self.targets)
            .field("scope", &self.scope)
            .finish()
    }
}

impl RedactVerifyParams {
    /// Validates the target bounds.
    pub fn validate(&self) -> Result<(), ParamsError> {
        validate_targets(&self.targets)
    }
}

/// Public report describing the outcome of all mandatory verification checks.
/// Evidence of modifications across carriers during redaction.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct RedactionCarrierEvidence {
    /// Number of text occurrences removed.
    pub text_removed: usize,
    /// Number of image pixels/masks modified.
    pub images_modified: usize,
    /// Number of vector paths inspected or removed.
    pub vectors_inspected: usize,
    /// Number of annotations/forms removed.
    pub annotations_removed: usize,
    /// Number of metadata strings/streams scrubbed.
    pub metadata_scrubbed: usize,
    /// Number of embedded files/attachments removed.
    pub attachments_removed: usize,
}

/// Never contains target values or matched text.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RedactionVerificationReport {
    /// Overall verification status.
    pub status: super::redact_internal::RedactionVerificationStatus,
    /// Requested carrier scope.
    pub scope: RedactionScope,
    /// Detailed results for each mandatory verification check.
    pub checks: super::redact_internal::RedactionChecks,
    /// Number of targets provided in the request.
    pub target_count: usize,
    /// Number of distinct matched/redacted occurrences found and removed.
    pub match_count: usize,
    /// Conspicuous warnings, such as signature invalidation.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    /// Modifications made across carriers during redaction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub carriers: Option<RedactionCarrierEvidence>,
}

/// A target to remove from the document.
#[derive(Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RedactionTarget {
    /// Exact text match.
    Exact { text: String },
    /// A bounded regular expression.
    Regex { pattern: String },
    /// A built-in detector.
    Preset { preset: RedactionPreset },
    /// Rectangle in PDF points on a one-based page.
    Rectangle { page: u32, rect: CropRect },
    /// Entire one-based page.
    Page { page: u32 },
}

// Targets can contain confidential document text and must not appear in debug output.
impl std::fmt::Debug for RedactionTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RedactionTarget([redacted])")
    }
}

/// Built-in text detectors.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RedactionPreset {
    /// Email address.
    Email,
    /// International or Vietnamese phone number.
    Phone,
    /// Vietnamese citizen identification number.
    Cccd,
    /// Vietnamese legacy identity number.
    Cmnd,
    /// International bank account number.
    Iban,
    /// Luhn-valid payment card number.
    CreditCard,
}

/// Validates target bounds before any document processing.
pub fn validate_targets(targets: &[RedactionTarget]) -> Result<(), ParamsError> {
    if targets.is_empty() || targets.len() > 100_000 {
        return Err(ParamsError::new(
            "targets",
            "must contain 1 to 100000 targets",
        ));
    }
    let mut pattern_count = 0usize;
    for target in targets {
        match target {
            RedactionTarget::Exact { text } => {
                pattern_count += 1;
                if text.is_empty() || text.len() > 1024 {
                    return Err(ParamsError::new(
                        "targets",
                        "patterns must contain 1 to 1024 bytes",
                    ));
                }
            }
            RedactionTarget::Regex { pattern: text } => {
                pattern_count += 1;
                if text.is_empty() || text.len() > 1024 {
                    return Err(ParamsError::new(
                        "targets",
                        "patterns must contain 1 to 1024 bytes",
                    ));
                }
            }
            RedactionTarget::Preset { .. } => {
                pattern_count += 1;
            }
            RedactionTarget::Page { page } | RedactionTarget::Rectangle { page, .. }
                if *page == 0 =>
            {
                return Err(ParamsError::new("targets", "page numbers start at 1"));
            }
            RedactionTarget::Rectangle { rect, .. }
                if ![rect.left, rect.bottom, rect.right, rect.top]
                    .iter()
                    .all(|v| v.is_finite() && v.abs() <= 1_000_000.0)
                    || rect.left >= rect.right
                    || rect.bottom >= rect.top =>
            {
                return Err(ParamsError::new(
                    "targets",
                    "rectangle must have finite ordered coordinates",
                ));
            }
            _ => {}
        }
    }
    if pattern_count > 100 {
        return Err(ParamsError::new(
            "targets",
            "patterns capped at 100 per request",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_targets_without_disclosing_text() {
        let target = RedactionTarget::Exact {
            text: "sensitive-example".into(),
        };
        assert!(!format!("{target:?}").contains("sensitive-example"));
        assert!(validate_targets(&[target]).is_ok());
        assert!(validate_targets(&[]).is_err());
        assert!(validate_targets(&[RedactionTarget::Page { page: 0 }]).is_err());
        assert!(validate_targets(&vec![RedactionTarget::Page { page: 1 }; 100_000]).is_ok());
        assert!(validate_targets(&vec![RedactionTarget::Page { page: 1 }; 100_001]).is_err());
        assert!(validate_targets(&vec![RedactionTarget::Exact { text: "a".into() }; 100]).is_ok());
        assert!(validate_targets(&vec![RedactionTarget::Exact { text: "a".into() }; 101]).is_err());
        assert!(
            validate_targets(&[RedactionTarget::Regex {
                pattern: "x".repeat(1025)
            }])
            .is_err()
        );
        assert!(
            validate_targets(&[RedactionTarget::Rectangle {
                page: 1,
                rect: CropRect {
                    left: 1.0,
                    bottom: 0.0,
                    right: 0.0,
                    top: 1.0
                },
            }])
            .is_err()
        );
    }

    #[test]
    fn scope_defaults_to_all_carriers_and_exclusions_are_partial() {
        let params: RedactParams = serde_json::from_value(serde_json::json!({
            "targets": [{"kind": "page", "page": 1}],
            "scope": {"metadata": false}
        }))
        .unwrap();
        assert!(!params.scope.is_complete());
        assert!(params.scope.text && params.scope.attachments);
        assert!(!params.invalidate_signatures);
        assert!(RedactionScope::default().is_complete());
    }

    #[test]
    fn overlay_text_must_not_exceed_cap() {
        let ok_params = RedactParams {
            targets: vec![RedactionTarget::Page { page: 1 }],
            color: None,
            overlay_text: Some("x".repeat(256)),
            scope: RedactionScope::default(),
            invalidate_signatures: false,
        };
        assert!(ok_params.validate().is_ok());

        let oversized_params = RedactParams {
            targets: vec![RedactionTarget::Page { page: 1 }],
            color: None,
            overlay_text: Some("x".repeat(257)),
            scope: RedactionScope::default(),
            invalidate_signatures: false,
        };
        assert!(oversized_params.validate().is_err());
    }
}
