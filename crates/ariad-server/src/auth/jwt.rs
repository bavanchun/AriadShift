//! Better Auth JWTs, verified against a cached JWKS.
//!
//! Better Auth's JWT plugin signs with EdDSA (Ed25519) by default and serves
//! its public keys at `/api/auth/jwks`; `iss` and `aud` default to its base
//! URL origin and `sub` is the user id. Each key names its algorithm, and a
//! token is only accepted with the algorithm its key declares.

use std::{
    collections::HashSet,
    sync::Arc,
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

/// Maximum trust age since the last successful fetch (TTL plus outage grace).
const JWKS_MAX_AGE: Duration = Duration::from_secs(15 * 60);

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

type FetchJwks =
    dyn Fn() -> futures_util::future::BoxFuture<'static, Result<JwkSet, JwtError>> + Send + Sync;

/// Verifies tokens with keys fetched from a JWKS URL.
pub struct JwtVerifier {
    config: JwtConfig,
    fetch: Arc<FetchJwks>,
    now: Arc<dyn Fn() -> Instant + Send + Sync>,
    cache: RwLock<Option<Cached>>,
    last_fetch: Mutex<Option<Instant>>,
}

impl JwtVerifier {
    #[must_use]
    pub fn new(config: JwtConfig, http: reqwest::Client) -> Self {
        let url = config.jwks_url.clone();
        let fetch = Arc::new(move || {
            let http = http.clone();
            let url = url.clone();
            Box::pin(async move {
                let response = http
                    .get(url)
                    .timeout(Duration::from_secs(10))
                    .send()
                    .await
                    .and_then(reqwest::Response::error_for_status)
                    .map_err(|err| {
                        tracing::warn!(error = %err, "JWKS fetch failed");
                        JwtError::Fetch
                    })?;
                response.json().await.map_err(|err| {
                    tracing::warn!(error = %err, "JWKS response is not a key set");
                    JwtError::Fetch
                })
            }) as futures_util::future::BoxFuture<'static, Result<JwkSet, JwtError>>
        });
        Self {
            config,
            fetch,
            now: Arc::new(Instant::now),
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

    /// Looks up a key, refreshing stale sets and unknown ids. Every return
    /// rechecks the current cache so a failed/throttled refresh cannot extend
    /// trust or resurrect a key removed by a concurrent successful refresh.
    async fn find(&self, kid: &str) -> Result<Option<Jwk>, JwtError> {
        {
            let cache = self.cache.read().await;
            if let Some(cached) = cache.as_ref()
                && (self.now)().saturating_duration_since(cached.fetched_at) < JWKS_TTL
                && let Some(key) = cached.keys.find(kid)
            {
                return Ok(Some(key.clone()));
            }
        }
        let refreshed = self.refresh().await;
        let cache = self.cache.read().await;
        match cache.as_ref() {
            Some(cached)
                if (self.now)().saturating_duration_since(cached.fetched_at) < JWKS_MAX_AGE =>
            {
                let key = cached.keys.find(kid).cloned();
                if key.is_some() || refreshed.is_ok() {
                    Ok(key)
                } else {
                    Err(JwtError::Fetch)
                }
            }
            _ => Err(JwtError::Fetch),
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
            && (self.now)().saturating_duration_since(at) < spacing
        {
            return if has_cache {
                Ok(())
            } else {
                Err(JwtError::Fetch)
            };
        }
        *last = Some((self.now)());
        let keys = (self.fetch)().await?;
        *self.cache.write().await = Some(Cached {
            keys,
            fetched_at: (self.now)(),
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

#[cfg(test)]
#[path = "../../tests/jwks-cache/mod.rs"]
mod tests;
