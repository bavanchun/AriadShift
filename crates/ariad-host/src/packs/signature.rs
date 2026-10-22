//! Minisign verification of pack manifests against compiled-in public keys.

use base64::{Engine as _, engine::general_purpose::STANDARD};
use minisign_verify::{PublicKey, Signature};
use thiserror::Error;

include!(concat!(env!("OUT_DIR"), "/pack_keys.rs"));

/// Upper bound for a detached signature file.
pub const MAX_SIGNATURE_BYTES: u64 = 4096;

/// A signature check failure. Every variant makes an install fail with exit code 1.
#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub enum SignatureError {
    #[error("this build has no trusted pack signing keys")]
    NoTrustedKeys,
    #[error("the signature file is malformed")]
    Malformed,
    #[error("the signature uses the legacy non-prehashed minisign format")]
    Legacy,
    #[error("the manifest is signed by key {key_id}, which this build does not trust")]
    UnknownKey { key_id: String },
    #[error("the signature does not match the manifest (key {key_id})")]
    Invalid { key_id: String },
    #[error("the trusted comment names '{found}' but the manifest is '{expected}'")]
    TrustedComment { expected: String, found: String },
}

/// A public key trusted to sign pack manifests.
#[derive(Clone, Debug)]
pub struct TrustedKey {
    /// Minisign key id as 16 upper-case hex digits.
    pub id: String,
    /// Where the key came from (file name).
    pub source: String,
    key: PublicKey,
}

impl TrustedKey {
    /// Parses a minisign public key in `minisign.pub` format or as a bare base64 line.
    pub fn parse(source: &str, text: &str) -> Result<Self, SignatureError> {
        let line = text
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty() && !line.starts_with("untrusted comment:"))
            .ok_or(SignatureError::Malformed)?;
        let key = PublicKey::from_base64(line).map_err(|_| SignatureError::Malformed)?;
        let id = key_id(line).ok_or(SignatureError::Malformed)?;
        Ok(Self {
            id,
            source: source.to_owned(),
            key,
        })
    }
}

/// The set of keys a pack signature may come from.
#[derive(Clone, Debug, Default)]
pub struct TrustedKeys {
    keys: Vec<TrustedKey>,
}

impl TrustedKeys {
    /// Keys compiled into this build from `crates/ariad-host/data/pack-keys/*.pub`.
    #[must_use]
    pub fn compiled() -> Self {
        Self {
            keys: COMPILED_PACK_KEYS
                .iter()
                .filter_map(|(source, text)| TrustedKey::parse(source, text).ok())
                .collect(),
        }
    }

    /// A key set built from explicit keys.
    #[must_use]
    pub fn new(keys: Vec<TrustedKey>) -> Self {
        Self { keys }
    }

    /// Adds a key.
    pub fn push(&mut self, key: TrustedKey) {
        self.keys.push(key);
    }

    /// Key ids in trust order.
    #[must_use]
    pub fn ids(&self) -> Vec<String> {
        self.keys.iter().map(|key| key.id.clone()).collect()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// Verifies `signature_text` over `manifest` and checks that the trusted comment names
    /// `pack:<name>` and `version:<version>`. Returns the id of the signing key.
    pub fn verify(
        &self,
        manifest: &[u8],
        signature_text: &str,
        name: &str,
        version: &str,
    ) -> Result<String, SignatureError> {
        if self.keys.is_empty() {
            return Err(SignatureError::NoTrustedKeys);
        }
        let signature = Signature::decode(signature_text).map_err(|error| match error {
            minisign_verify::Error::UnsupportedAlgorithm => SignatureError::Legacy,
            _ => SignatureError::Malformed,
        })?;
        let signer = signature_text
            .lines()
            .nth(1)
            .and_then(key_id)
            .ok_or(SignatureError::Malformed)?;
        let key = self
            .keys
            .iter()
            .find(|key| key.id == signer)
            .ok_or_else(|| SignatureError::UnknownKey {
                key_id: signer.clone(),
            })?;
        key.key
            .verify(manifest, &signature, false)
            .map_err(|error| match error {
                minisign_verify::Error::UnexpectedAlgorithm => SignatureError::Legacy,
                _ => SignatureError::Invalid {
                    key_id: signer.clone(),
                },
            })?;
        check_trusted_comment(signature.trusted_comment(), name, version)?;
        Ok(key.id.clone())
    }
}

/// The trusted comment must contain the tokens `pack:<name>` and `version:<version>`.
fn check_trusted_comment(comment: &str, name: &str, version: &str) -> Result<(), SignatureError> {
    let mut pack = None;
    let mut signed_version = None;
    for token in comment.split_whitespace() {
        if let Some(value) = token.strip_prefix("pack:") {
            pack = Some(value);
        } else if let Some(value) = token.strip_prefix("version:") {
            signed_version = Some(value);
        }
    }
    if pack == Some(name) && signed_version == Some(version) {
        Ok(())
    } else {
        Err(SignatureError::TrustedComment {
            expected: format!("pack:{name} version:{version}"),
            found: format!(
                "pack:{} version:{}",
                pack.unwrap_or("-"),
                signed_version.unwrap_or("-")
            ),
        })
    }
}

/// Minisign key id of a base64 key or signature line, printed the way `minisign` prints it.
fn key_id(base64_line: &str) -> Option<String> {
    let bytes = STANDARD.decode(base64_line.trim()).ok()?;
    let id: [u8; 8] = bytes.get(2..10)?.try_into().ok()?;
    Some(format!("{:016X}", u64::from_le_bytes(id)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiled_keys_all_parse() {
        assert_eq!(
            TrustedKeys::compiled().keys.len(),
            COMPILED_PACK_KEYS.len(),
            "every file in data/pack-keys must be a valid minisign public key"
        );
    }

    #[test]
    fn trusted_comment_needs_name_and_version() {
        assert!(check_trusted_comment("pack:docling version:1.0.0", "docling", "1.0.0").is_ok());
        assert!(
            check_trusted_comment(
                "timestamp:1 pack:docling\tversion:1.0.0",
                "docling",
                "1.0.0"
            )
            .is_ok()
        );
        assert!(check_trusted_comment("pack:docling version:0.9.0", "docling", "1.0.0").is_err());
        assert!(check_trusted_comment("pack:other version:1.0.0", "docling", "1.0.0").is_err());
        assert!(check_trusted_comment("timestamp:1", "docling", "1.0.0").is_err());
    }

    #[test]
    fn empty_key_set_refuses_everything() {
        assert_eq!(
            TrustedKeys::default().verify(b"x", "", "a", "1.0.0"),
            Err(SignatureError::NoTrustedKeys)
        );
    }
}
