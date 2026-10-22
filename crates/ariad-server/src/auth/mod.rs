//! Who is calling: Better Auth JWTs, Better Auth API keys, or anonymous
//! callers identified by a keyed hash of their address.

pub mod api_key;
pub mod jwt;

use std::net::{IpAddr, SocketAddr};

use axum::http::{HeaderMap, header};
use chrono::Utc;
use hmac::{KeyInit, Mac};
use sha2::Sha256;
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    config::{AuthConfig, TurnstileConfig},
    db::{self, ANONYMOUS_PLAN, ANONYMOUS_WORKSPACE},
    error::ApiError,
};

/// Header carrying a Better Auth API key.
pub const API_KEY_HEADER: &str = "x-api-key";

/// Header carrying a Cloudflare Turnstile token.
pub const TURNSTILE_HEADER: &str = "cf-turnstile-response";

/// How a caller authenticated.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Credential {
    Jwt,
    ApiKey,
    Anonymous,
}

/// The authenticated caller of one request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Principal {
    pub credential: Credential,
    /// Better Auth user id; `None` for anonymous callers.
    pub user_id: Option<String>,
    pub workspace_id: Uuid,
    pub plan: String,
    /// Key for quotas and rate limits: `ws:<workspace>` or `ip:<hash>`.
    pub subject: String,
    /// Keyed hash of the client address.
    pub client: String,
}

impl Principal {
    #[must_use]
    pub fn is_anonymous(&self) -> bool {
        self.credential == Credential::Anonymous
    }

    /// Whether this caller may see resources of `workspace_id`. Anonymous
    /// resources are reachable by anyone holding their random id.
    #[must_use]
    pub fn can_access(&self, workspace_id: Uuid) -> bool {
        workspace_id == ANONYMOUS_WORKSPACE || workspace_id == self.workspace_id
    }
}

/// Verifies credentials for the API.
pub struct Authenticator {
    config: AuthConfig,
    jwt: Option<jwt::JwtVerifier>,
    secret: Vec<u8>,
    trusted_ip_header: Option<String>,
    http: reqwest::Client,
}

impl Authenticator {
    #[must_use]
    pub fn new(
        config: AuthConfig,
        secret: Vec<u8>,
        trusted_ip_header: Option<String>,
        http: reqwest::Client,
    ) -> Self {
        let jwt = config
            .jwt
            .clone()
            .map(|jwt| jwt::JwtVerifier::new(jwt, http.clone()));
        Self {
            config,
            jwt,
            secret,
            trusted_ip_header,
            http,
        }
    }

    /// Resolves the caller of a request.
    pub async fn authenticate(
        &self,
        pool: &PgPool,
        headers: &HeaderMap,
        peer: Option<SocketAddr>,
    ) -> Result<Principal, ApiError> {
        let client = self.client_hash(headers, peer);
        if let Some(value) = headers.get(header::AUTHORIZATION) {
            let token = value
                .to_str()
                .ok()
                .and_then(|v| v.split_once(' '))
                .filter(|(scheme, _)| scheme.eq_ignore_ascii_case("Bearer"))
                .map(|(_, token)| token)
                .map(str::trim)
                .filter(|t| !t.is_empty())
                .ok_or_else(|| ApiError::unauthorized("expected a Bearer token"))?;
            let verifier = self
                .jwt
                .as_ref()
                .ok_or_else(|| ApiError::unauthorized("bearer tokens are not enabled"))?;
            let user_id = verifier.verify(token).await.map_err(|err| match err {
                jwt::JwtError::Fetch => ApiError::unavailable("cannot reach the identity service"),
                other => ApiError::unauthorized(other.to_string()),
            })?;
            return self.user(pool, user_id, Credential::Jwt, client).await;
        }
        if let Some(value) = headers.get(API_KEY_HEADER) {
            if !self.config.api_keys {
                return Err(ApiError::unauthorized("API keys are not enabled"));
            }
            let raw = value
                .to_str()
                .map_err(|_| ApiError::unauthorized("the API key is invalid"))?;
            let user_id = api_key::verify(
                pool,
                raw.trim(),
                &self.config.api_key_config_ids,
                Utc::now(),
            )
            .await
            .map_err(|err| match err {
                api_key::ApiKeyError::RateLimited { retry_after_s } => {
                    ApiError::rate_limited(retry_after_s)
                }
                api_key::ApiKeyError::Exhausted => ApiError::quota_exceeded(err.to_string()),
                api_key::ApiKeyError::Database => {
                    ApiError::unavailable("the API key service is unavailable")
                }
                other => ApiError::unauthorized(other.to_string()),
            })?;
            return self.user(pool, user_id, Credential::ApiKey, client).await;
        }
        if !self.config.anonymous {
            return Err(ApiError::unauthorized("credentials are required"));
        }
        Ok(Principal {
            credential: Credential::Anonymous,
            user_id: None,
            workspace_id: ANONYMOUS_WORKSPACE,
            plan: ANONYMOUS_PLAN.to_owned(),
            subject: format!("ip:{client}"),
            client,
        })
    }

    async fn user(
        &self,
        pool: &PgPool,
        user_id: String,
        credential: Credential,
        client: String,
    ) -> Result<Principal, ApiError> {
        let (workspace_id, plan) = db::ensure_workspace(pool, &user_id).await?;
        Ok(Principal {
            credential,
            user_id: Some(user_id),
            workspace_id,
            plan,
            subject: format!("ws:{workspace_id}"),
            client,
        })
    }

    /// The client address: the trusted proxy header when configured, else the peer.
    #[must_use]
    pub fn client_ip(&self, headers: &HeaderMap, peer: Option<SocketAddr>) -> Option<IpAddr> {
        if let Some(name) = &self.trusted_ip_header
            && let Some(ip) = headers
                .get(name.as_str())
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.split(',').next())
                .and_then(|v| v.trim().parse::<IpAddr>().ok())
        {
            return Some(ip);
        }
        peer.map(|addr| addr.ip())
    }

    /// Keyed hash of the client address. IPv6 addresses are grouped by /64,
    /// the smallest block a user commonly controls.
    pub fn client_hash(&self, headers: &HeaderMap, peer: Option<SocketAddr>) -> String {
        let label = match self.client_ip(headers, peer) {
            Some(IpAddr::V4(v4)) => v4.to_string(),
            Some(IpAddr::V6(v6)) => match v6.to_ipv4_mapped() {
                Some(v4) => v4.to_string(),
                None => {
                    let s = v6.segments();
                    format!("{:x}:{:x}:{:x}:{:x}::/64", s[0], s[1], s[2], s[3])
                }
            },
            None => "unknown".to_owned(),
        };
        let mut mac = hmac::Hmac::<Sha256>::new_from_slice(&self.secret)
            .expect("HMAC accepts keys of any length");
        mac.update(b"client-ip:");
        mac.update(label.as_bytes());
        hex::encode(&mac.finalize().into_bytes()[..16])
    }

    /// Checks a Turnstile token when Turnstile is configured.
    pub async fn check_turnstile(
        &self,
        headers: &HeaderMap,
        peer: Option<SocketAddr>,
    ) -> Result<(), ApiError> {
        let Some(turnstile) = &self.config.turnstile else {
            return Ok(());
        };
        let token = headers
            .get(TURNSTILE_HEADER)
            .and_then(|v| v.to_str().ok())
            .map(str::trim)
            .filter(|t| !t.is_empty() && t.len() <= 2048)
            .ok_or_else(|| {
                ApiError::forbidden("turnstile_required", "a Turnstile token is required")
            })?;
        let remote_ip = self.client_ip(headers, peer).map(|ip| ip.to_string());
        if verify_turnstile(&self.http, turnstile, token, remote_ip.as_deref()).await {
            Ok(())
        } else {
            Err(ApiError::forbidden(
                "turnstile_failed",
                "the Turnstile check failed",
            ))
        }
    }
}

async fn verify_turnstile(
    http: &reqwest::Client,
    config: &TurnstileConfig,
    token: &str,
    remote_ip: Option<&str>,
) -> bool {
    #[derive(serde::Deserialize)]
    struct Outcome {
        success: bool,
    }
    let mut form = vec![("secret", config.secret.as_str()), ("response", token)];
    if let Some(ip) = remote_ip {
        form.push(("remoteip", ip));
    }
    let result = http
        .post(config.verify_url.clone())
        .timeout(std::time::Duration::from_secs(10))
        .form(&form)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status);
    match result {
        Ok(response) => response
            .json::<Outcome>()
            .await
            .map(|o| o.success)
            .unwrap_or(false),
        Err(err) => {
            tracing::warn!(error = %err, "Turnstile verification request failed");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn authenticator(header: Option<&str>) -> Authenticator {
        Authenticator::new(
            AuthConfig {
                jwt: None,
                api_keys: true,
                api_key_config_ids: vec!["default".to_owned()],
                anonymous: true,
                turnstile: None,
            },
            b"0123456789abcdef0123456789abcdef".to_vec(),
            header.map(str::to_owned),
            crate::http_client(),
        )
    }

    #[test]
    fn trusted_header_overrides_the_peer() {
        let auth = authenticator(Some("cf-connecting-ip"));
        let mut headers = HeaderMap::new();
        headers.insert("cf-connecting-ip", "203.0.113.9".parse().expect("value"));
        let peer = Some(SocketAddr::from(([10, 0, 0, 1], 4000)));
        assert_eq!(
            auth.client_ip(&headers, peer),
            Some("203.0.113.9".parse().expect("ip"))
        );
        let untrusted = authenticator(None);
        assert_eq!(
            untrusted.client_ip(&headers, peer),
            Some("10.0.0.1".parse().expect("ip"))
        );
    }

    #[test]
    fn ipv6_clients_are_grouped_by_prefix() {
        let auth = authenticator(None);
        let headers = HeaderMap::new();
        let a: SocketAddr = "[2001:db8:1:2::1]:80".parse().expect("addr");
        let b: SocketAddr = "[2001:db8:1:2:ffff::9]:80".parse().expect("addr");
        let c: SocketAddr = "[2001:db8:1:3::1]:80".parse().expect("addr");
        assert_eq!(
            auth.client_hash(&headers, Some(a)),
            auth.client_hash(&headers, Some(b))
        );
        assert_ne!(
            auth.client_hash(&headers, Some(a)),
            auth.client_hash(&headers, Some(c))
        );
        assert_eq!(auth.client_hash(&headers, Some(a)).len(), 32);
    }

    #[test]
    fn anonymous_resources_are_shared_but_workspaces_are_not() {
        let principal = Principal {
            credential: Credential::Jwt,
            user_id: Some("u".to_owned()),
            workspace_id: Uuid::from_u128(1),
            plan: "free".to_owned(),
            subject: "ws:1".to_owned(),
            client: String::new(),
        };
        assert!(principal.can_access(Uuid::from_u128(1)));
        assert!(principal.can_access(ANONYMOUS_WORKSPACE));
        assert!(!principal.can_access(Uuid::from_u128(2)));
    }
}
