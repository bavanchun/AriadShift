//! Better Auth JWTs, verified against a cached JWKS.
//!
//! Better Auth's JWT plugin signs with EdDSA (Ed25519) by default and serves
//! its public keys at `/api/auth/jwks`; `iss` and `aud` default to its base
//! URL origin and `sub` is the user id. Each key names its algorithm, and a
//! token is only accepted with the algorithm its key declares.

use std::{
    collections::HashSet,
    time::{Duration, Instant},
};

use jsonwebtoken::{
    Algorithm, DecodingKey, Validation, decode, decode_header,
    jwk::{Jwk, JwkSet, KeyAlgorithm},
};
use serde::Deserialize;
use thiserror::Error;
use tokio::sync::{Mutex, RwLock};

use crate::config::JwtConfig;

/// How long a fetched JWKS is trusted before a refetch.
const JWKS_TTL: Duration = Duration::from_secs(300);

/// Minimum spacing of refetches triggered by unknown key ids.
const REFRESH_SPACING: Duration = Duration::from_secs(30);

/// Minimum spacing of fetch retries while no key set is cached.
const RETRY_SPACING: Duration = Duration::from_secs(5);

/// Allowed clock skew for `exp`.
const LEEWAY_S: u64 = 30;

#[derive(Debug, Error)]
pub enum JwtError {
    #[error("the token is malformed")]
    Malformed,
    #[error("the token's signing key is unknown")]
    UnknownKey,
    #[error("the token is invalid: {0}")]
    Invalid(String),
    #[error("the key set could not be fetched")]
    Fetch,
}

#[derive(Debug, Deserialize)]
struct Claims {
    sub: String,
}

struct Cached {
    keys: JwkSet,
    fetched_at: Instant,
}

/// Verifies tokens with keys fetched from a JWKS URL.
pub struct JwtVerifier {
    config: JwtConfig,
    http: reqwest::Client,
    cache: RwLock<Option<Cached>>,
    last_fetch: Mutex<Option<Instant>>,
}

impl JwtVerifier {
    #[must_use]
    pub fn new(config: JwtConfig, http: reqwest::Client) -> Self {
        Self {
            config,
            http,
            cache: RwLock::new(None),
            last_fetch: Mutex::new(None),
        }
    }

    /// Returns the token's subject (the Better Auth user id).
    pub async fn verify(&self, token: &str) -> Result<String, JwtError> {
        let header = decode_header(token).map_err(|_| JwtError::Malformed)?;
        let kid = header.kid.as_deref().ok_or(JwtError::Malformed)?;
        if !is_asymmetric(header.alg) {
            return Err(JwtError::Invalid(format!(
                "algorithm {:?} is not accepted",
                header.alg
            )));
        }
        let jwk = self.find(kid).await?.ok_or(JwtError::UnknownKey)?;
        if let Some(key_alg) = jwk.common.key_algorithm
            && algorithm_of(key_alg) != Some(header.alg)
        {
            return Err(JwtError::Invalid(
                "the token's algorithm does not match its key".to_owned(),
            ));
        }
        let key = DecodingKey::from_jwk(&jwk).map_err(|e| JwtError::Invalid(e.to_string()))?;
        let mut validation = Validation::new(header.alg);
        validation.set_issuer(&[&self.config.issuer]);
        validation.set_audience(&[&self.config.audience]);
        validation.leeway = LEEWAY_S;
        validation.required_spec_claims = ["exp", "iss", "aud", "sub"]
            .into_iter()
            .map(str::to_owned)
            .collect::<HashSet<_>>();
        let data = decode::<Claims>(token, &key, &validation)
            .map_err(|e| JwtError::Invalid(e.to_string()))?;
        if data.claims.sub.is_empty() {
            return Err(JwtError::Invalid("empty subject".to_owned()));
        }
        Ok(data.claims.sub)
    }

    /// Looks a key up, refetching the set when it is stale or lacks the key.
    /// A failed refetch falls back to a stale key that matches.
    async fn find(&self, kid: &str) -> Result<Option<Jwk>, JwtError> {
        let (hit, stale) = {
            let cache = self.cache.read().await;
            match cache.as_ref() {
                Some(cached) => (
                    cached.keys.find(kid).cloned(),
                    cached.fetched_at.elapsed() >= JWKS_TTL,
                ),
                None => (None, true),
            }
        };
        if hit.is_some() && !stale {
            return Ok(hit);
        }
        match self.refresh().await {
            Ok(()) => {
                let cache = self.cache.read().await;
                Ok(cache.as_ref().and_then(|c| c.keys.find(kid).cloned()))
            }
            Err(err) => hit.map_or(Err(err), |jwk| Ok(Some(jwk))),
        }
    }

    /// Fetches the key set, at most once per spacing interval.
    async fn refresh(&self) -> Result<(), JwtError> {
        let mut last = self.last_fetch.lock().await;
        let has_cache = self.cache.read().await.is_some();
        let spacing = if has_cache {
            REFRESH_SPACING
        } else {
            RETRY_SPACING
        };
        if let Some(at) = *last
            && at.elapsed() < spacing
        {
            return if has_cache {
                Ok(())
            } else {
                Err(JwtError::Fetch)
            };
        }
        *last = Some(Instant::now());
        let response = self
            .http
            .get(self.config.jwks_url.clone())
            .timeout(Duration::from_secs(10))
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|err| {
                tracing::warn!(error = %err, "JWKS fetch failed");
                JwtError::Fetch
            })?;
        let keys: JwkSet = response.json().await.map_err(|err| {
            tracing::warn!(error = %err, "JWKS response is not a key set");
            JwtError::Fetch
        })?;
        *self.cache.write().await = Some(Cached {
            keys,
            fetched_at: Instant::now(),
        });
        Ok(())
    }
}

const fn is_asymmetric(alg: Algorithm) -> bool {
    !matches!(alg, Algorithm::HS256 | Algorithm::HS384 | Algorithm::HS512)
}

const fn algorithm_of(alg: KeyAlgorithm) -> Option<Algorithm> {
    Some(match alg {
        KeyAlgorithm::EdDSA => Algorithm::EdDSA,
        KeyAlgorithm::ES256 => Algorithm::ES256,
        KeyAlgorithm::ES384 => Algorithm::ES384,
        KeyAlgorithm::RS256 => Algorithm::RS256,
        KeyAlgorithm::RS384 => Algorithm::RS384,
        KeyAlgorithm::RS512 => Algorithm::RS512,
        KeyAlgorithm::PS256 => Algorithm::PS256,
        KeyAlgorithm::PS384 => Algorithm::PS384,
        KeyAlgorithm::PS512 => Algorithm::PS512,
        _ => return None,
    })
}
