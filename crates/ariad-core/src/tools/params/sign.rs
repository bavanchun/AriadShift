//! Parameters for PDF digital signing, verification, and removal.

use std::fmt;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::ParamsError;

/// PAdES digital signature conformance level.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum SignLevel {
    /// PAdES B-B: basic digital signature.
    #[default]
    #[serde(rename = "B-B", alias = "b-b")]
    BB,
    /// PAdES B-T: signature with a trusted RFC 3161 timestamp.
    #[serde(rename = "B-T", alias = "b-t")]
    BT,
    /// PAdES B-LT: signature with Long-Term validation data (certificates and revocation info).
    #[serde(rename = "B-LT", alias = "b-lt")]
    BLT,
    /// PAdES B-LTA: signature with Long-Term validation data and archival timestamp.
    #[serde(rename = "B-LTA", alias = "b-lta")]
    BLTA,
}

impl SignLevel {
    /// Returns the canonical string representation of the level.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::BB => "B-B",
            Self::BT => "B-T",
            Self::BLT => "B-LT",
            Self::BLTA => "B-LTA",
        }
    }
}

/// Visual appearance settings for a digital signature.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VisibleSignature {
    /// 1-based page number where the signature will appear.
    pub page: u32,
    /// Bounding rectangle `[x1, y1, x2, y2]` in PDF user space units (points).
    pub rect: [i64; 4],
    /// Optional appearance text or template to render inside the signature box.
    #[serde(default, skip_serializing_if = "Option::is_none", alias = "text")]
    pub appearance: Option<String>,
}

/// Caller-supplied offline evidence files (D7).
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OfflineEvidence {
    /// Optional path to a pre-fetched RFC 3161 timestamp response token file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp_token: Option<String>,
    /// Optional paths to pre-fetched OCSP response files.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ocsp_responses: Vec<String>,
    /// Optional paths to pre-fetched CRL (Certificate Revocation List) files.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub crl_files: Vec<String>,
}

/// PKCS#11 hardware token backend interface (D6).
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TokenParams {
    /// Path to the PKCS#11 module dynamic library (`.so` / `.dylib` / `.dll`).
    pub module: String,
    /// Optional slot number or token label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slot: Option<u32>,
    /// Optional token label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_label: Option<String>,
    /// Optional key label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_label: Option<String>,
    /// Token user PIN (secret).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pin: Option<String>,
}

impl fmt::Debug for TokenParams {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TokenParams")
            .field("module", &self.module)
            .field("slot", &self.slot)
            .field("token_label", &self.token_label)
            .field("key_label", &self.key_label)
            .field("pin", &self.pin.as_ref().map(|_| "[REDACTED]"))
            .finish()
    }
}

/// macOS Keychain backend interface (D6).
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct KeychainParams {
    /// Common name or SHA-1 fingerprint of the signing certificate in the Keychain.
    pub identity: String,
    /// Optional keychain name or path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keychain_path: Option<String>,
}

/// Parameters for `pdf.sign`.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SignParams {
    /// PKCS#12 certificate file path, handle, or base64 data (secret).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub certificate: Option<String>,
    /// Passphrase to decrypt the PKCS#12 certificate container (secret).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    /// Passphrase to unlock the private key if separate from container password (secret).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_password: Option<String>,
    /// Conformance level (B-B, B-T, B-LT, B-LTA). Default is `B-B`.
    #[serde(default)]
    pub level: SignLevel,
    /// RFC 3161 Timestamp Authority (TSA) URL. Required for B-T, B-LT, B-LTA.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tsa_url: Option<String>,
    /// Reason for signing the document.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Location where the document was signed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    /// Contact info of the signer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contact: Option<String>,
    /// Signature field name (default: "Signature1").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field_name: Option<String>,
    /// Visible signature visual appearance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible: Option<VisibleSignature>,
    /// Caller-supplied offline evidence (D7).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offline_evidence: Option<OfflineEvidence>,
    /// PKCS#11 hardware token backend parameters (D6).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<TokenParams>,
    /// macOS Keychain backend parameters (D6).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keychain: Option<KeychainParams>,
}

impl SignParams {
    /// Validates parameter invariants.
    pub fn validate(&self) -> Result<(), ParamsError> {
        if self.certificate.is_none() && self.token.is_none() && self.keychain.is_none() {
            return Err(ParamsError::new(
                "certificate",
                "must supply certificate, token, or keychain configuration",
            ));
        }
        if matches!(self.level, SignLevel::BT | SignLevel::BLT | SignLevel::BLTA)
            && self.tsa_url.is_none()
            && self
                .offline_evidence
                .as_ref()
                .and_then(|e| e.timestamp_token.as_ref())
                .is_none()
        {
            return Err(ParamsError::new(
                "tsa_url",
                "TSA URL or offline timestamp token is required for B-T, B-LT, and B-LTA levels",
            ));
        }
        if let Some(visible) = &self.visible {
            if visible.page == 0 {
                return Err(ParamsError::new(
                    "visible.page",
                    "must be 1-based (page >= 1)",
                ));
            }
            if visible.rect[2] <= visible.rect[0] || visible.rect[3] <= visible.rect[1] {
                return Err(ParamsError::new(
                    "visible.rect",
                    "invalid rectangle coordinates: width and height must be positive",
                ));
            }
        }
        Ok(())
    }
}

impl fmt::Debug for SignParams {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SignParams")
            .field(
                "certificate",
                &self.certificate.as_ref().map(|_| "[REDACTED]"),
            )
            .field("password", &self.password.as_ref().map(|_| "[REDACTED]"))
            .field(
                "key_password",
                &self.key_password.as_ref().map(|_| "[REDACTED]"),
            )
            .field("level", &self.level)
            .field("tsa_url", &self.tsa_url)
            .field("reason", &self.reason)
            .field("location", &self.location)
            .field("contact", &self.contact)
            .field("field_name", &self.field_name)
            .field("visible", &self.visible)
            .field("offline_evidence", &self.offline_evidence)
            .field("token", &self.token)
            .field("keychain", &self.keychain)
            .finish()
    }
}

/// Parameters for `pdf.sign_verify`.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SignVerifyParams {
    /// Whether to fetch live revocation status (OCSP/CRL) over the network (D7).
    /// Defaults to `false` (offline validation by default).
    #[serde(default)]
    pub fetch_revocation: bool,
    /// Optional custom TSA URL to verify timestamps.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tsa_url: Option<String>,
    /// Optional paths to trusted root CA certificates (PEM/DER format).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub trust_anchors: Vec<String>,
    /// Optional caller-supplied offline evidence file paths.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub offline_evidence: Vec<String>,
}

impl SignVerifyParams {
    /// Validates parameter invariants.
    pub fn validate(&self) -> Result<(), ParamsError> {
        Ok(())
    }
}

/// Parameters for `pdf.sign_remove`.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SignRemoveParams {
    /// Optional signature field name to remove. If `None`, removes all signature fields.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field_name: Option<String>,
    /// Confirmation that signature removal invalidates document cryptographic integrity (D8).
    /// Must be set to `true` to proceed.
    pub confirm_invalidation: bool,
}

impl SignRemoveParams {
    /// Validates parameter invariants.
    pub fn validate(&self) -> Result<(), ParamsError> {
        if !self.confirm_invalidation {
            return Err(ParamsError::new(
                "confirm_invalidation",
                "must be true to acknowledge signature removal and document invalidation",
            ));
        }
        Ok(())
    }
}
