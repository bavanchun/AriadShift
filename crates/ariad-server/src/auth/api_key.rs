//! Better Auth API keys, checked read-only against its `apikey` table.
//!
//! Better Auth (`@better-auth/api-key`) stores `base64url_nopad(sha256(key))`
//! in `"key"`, with camelCase column names. This module never writes the
//! table: usage counters and rate-limit windows that Better Auth maintains
//! are honoured only as a snapshot (an exhausted key or a full window is
//! refused), and the API applies its own per-workspace rate limits.

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use thiserror::Error;

/// Minimum accepted raw key length, including any configured prefix.
const MIN_KEY_LEN: usize = 32;

/// Longest value accepted before hashing.
const MAX_KEY_LEN: usize = 512;

/// The configuration id Better Auth treats as equal to NULL.
const DEFAULT_CONFIG_ID: &str = "default";

#[derive(Debug, Error, PartialEq)]
pub enum ApiKeyError {
    #[error("the API key is invalid")]
    Invalid,
    #[error("the API key is disabled")]
    Disabled,
    #[error("the API key has expired")]
    Expired,
    #[error("the API key's usage is exhausted")]
    Exhausted,
    #[error("the API key's rate limit is reached")]
    RateLimited { retry_after_s: u64 },
    #[error("the API key's owner type is not supported")]
    UnsupportedOwner,
    #[error("API key lookup failed")]
    Database,
}

/// Hash Better Auth stores for a raw key.
#[must_use]
pub fn hash_key(raw: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(raw.as_bytes()))
}

#[derive(Debug, sqlx::FromRow)]
pub struct KeyRow {
    #[sqlx(rename = "referenceId")]
    pub reference_id: String,
    #[sqlx(rename = "configId")]
    pub config_id: Option<String>,
    pub enabled: Option<bool>,
    #[sqlx(rename = "expiresAt")]
    pub expires_at: Option<DateTime<Utc>>,
    pub remaining: Option<i32>,
    #[sqlx(rename = "refillInterval")]
    pub refill_interval: Option<i32>,
    #[sqlx(rename = "refillAmount")]
    pub refill_amount: Option<i32>,
    #[sqlx(rename = "lastRefillAt")]
    pub last_refill_at: Option<DateTime<Utc>>,
    #[sqlx(rename = "createdAt")]
    pub created_at: DateTime<Utc>,
    #[sqlx(rename = "rateLimitEnabled")]
    pub rate_limit_enabled: Option<bool>,
    #[sqlx(rename = "rateLimitTimeWindow")]
    pub rate_limit_time_window: Option<i32>,
    #[sqlx(rename = "rateLimitMax")]
    pub rate_limit_max: Option<i32>,
    #[sqlx(rename = "requestCount")]
    pub request_count: Option<i32>,
    #[sqlx(rename = "lastRequest")]
    pub last_request: Option<DateTime<Utc>>,
}

/// Looks a raw key up and returns the owning user id.
pub async fn verify(
    pool: &PgPool,
    raw: &str,
    user_config_ids: &[String],
    now: DateTime<Utc>,
) -> Result<String, ApiKeyError> {
    if raw.len() < MIN_KEY_LEN || raw.len() > MAX_KEY_LEN {
        return Err(ApiKeyError::Invalid);
    }
    let row: Option<KeyRow> = sqlx::query_as(
        r#"SELECT "referenceId", "configId", "enabled", "expiresAt", "remaining",
                  "refillInterval", "refillAmount", "lastRefillAt", "createdAt",
                  "rateLimitEnabled", "rateLimitTimeWindow", "rateLimitMax",
                  "requestCount", "lastRequest"
           FROM "apikey" WHERE "key" = $1"#,
    )
    .bind(hash_key(raw))
    .fetch_optional(pool)
    .await
    .map_err(|err| {
        match &err {
            sqlx::Error::Database(db) if db.code().as_deref() == Some("42P01") => {
                tracing::warn!("API key presented, but Better Auth's apikey table does not exist");
            }
            _ => tracing::error!(error = %err, "API key lookup failed"),
        }
        ApiKeyError::Database
    })?;
    let row = row.ok_or(ApiKeyError::Invalid)?;
    check(&row, user_config_ids, now)?;
    Ok(row.reference_id)
}

/// Applies Better Auth's validation rules that need no writes.
pub fn check(
    row: &KeyRow,
    user_config_ids: &[String],
    now: DateTime<Utc>,
) -> Result<(), ApiKeyError> {
    let config_id = row.config_id.as_deref().unwrap_or(DEFAULT_CONFIG_ID);
    if !user_config_ids.iter().any(|id| id == config_id) {
        return Err(ApiKeyError::UnsupportedOwner);
    }
    if row.enabled == Some(false) {
        return Err(ApiKeyError::Disabled);
    }
    if row.expires_at.is_some_and(|at| now > at) {
        return Err(ApiKeyError::Expired);
    }
    if let Some(remaining) = row.remaining
        && remaining <= 0
    {
        let refill_due = match (row.refill_interval, row.refill_amount) {
            (Some(interval_ms), Some(_)) => {
                let since = row.last_refill_at.unwrap_or(row.created_at);
                (now - since).num_milliseconds() > i64::from(interval_ms)
            }
            _ => false,
        };
        if !refill_due {
            return Err(ApiKeyError::Exhausted);
        }
    }
    if row.rate_limit_enabled != Some(false)
        && let (Some(window_ms), Some(max), Some(count), Some(last)) = (
            row.rate_limit_time_window,
            row.rate_limit_max,
            row.request_count,
            row.last_request,
        )
    {
        let elapsed_ms = (now - last).num_milliseconds();
        if elapsed_ms <= i64::from(window_ms) && count >= max {
            let retry_ms = i64::from(window_ms) - elapsed_ms;
            let retry_after_s = u64::try_from(retry_ms.max(0)).unwrap_or(0).div_ceil(1000);
            return Err(ApiKeyError::RateLimited { retry_after_s });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use chrono::Duration;

    use super::*;

    fn row() -> KeyRow {
        KeyRow {
            reference_id: "user-1".to_owned(),
            config_id: Some("default".to_owned()),
            enabled: Some(true),
            expires_at: None,
            remaining: None,
            refill_interval: None,
            refill_amount: None,
            last_refill_at: None,
            created_at: Utc::now() - Duration::days(1),
            rate_limit_enabled: Some(true),
            rate_limit_time_window: Some(86_400_000),
            rate_limit_max: Some(10),
            request_count: Some(0),
            last_request: None,
        }
    }

    fn ids() -> Vec<String> {
        vec!["default".to_owned()]
    }

    #[test]
    fn hash_matches_better_auth_encoding() {
        // base64url without padding of SHA-256("abc").
        assert_eq!(
            hash_key("abc"),
            "ungWv48Bz-pBQUDeXa4iI7ADYaOWF3qctBD_YfIAFa0"
        );
    }

    #[test]
    fn default_key_passes() {
        assert_eq!(check(&row(), &ids(), Utc::now()), Ok(()));
        let null_config = KeyRow {
            config_id: None,
            enabled: None,
            ..row()
        };
        assert_eq!(check(&null_config, &ids(), Utc::now()), Ok(()));
    }

    #[test]
    fn disabled_expired_and_foreign_keys_fail() {
        let now = Utc::now();
        let disabled = KeyRow {
            enabled: Some(false),
            ..row()
        };
        assert_eq!(check(&disabled, &ids(), now), Err(ApiKeyError::Disabled));
        let expired = KeyRow {
            expires_at: Some(now - Duration::seconds(1)),
            ..row()
        };
        assert_eq!(check(&expired, &ids(), now), Err(ApiKeyError::Expired));
        let org = KeyRow {
            config_id: Some("org-keys".to_owned()),
            ..row()
        };
        assert_eq!(check(&org, &ids(), now), Err(ApiKeyError::UnsupportedOwner));
    }

    #[test]
    fn exhausted_keys_fail_unless_a_refill_is_due() {
        let now = Utc::now();
        let exhausted = KeyRow {
            remaining: Some(0),
            ..row()
        };
        assert_eq!(check(&exhausted, &ids(), now), Err(ApiKeyError::Exhausted));
        let refill_due = KeyRow {
            remaining: Some(0),
            refill_interval: Some(1000),
            refill_amount: Some(5),
            last_refill_at: Some(now - Duration::seconds(5)),
            ..row()
        };
        assert_eq!(check(&refill_due, &ids(), now), Ok(()));
    }

    #[test]
    fn full_rate_limit_window_fails() {
        let now = Utc::now();
        let full = KeyRow {
            request_count: Some(10),
            last_request: Some(now - Duration::seconds(10)),
            ..row()
        };
        assert!(matches!(
            check(&full, &ids(), now),
            Err(ApiKeyError::RateLimited { retry_after_s }) if retry_after_s > 86_000
        ));
        let window_over = KeyRow {
            request_count: Some(10),
            last_request: Some(now - Duration::days(2)),
            ..row()
        };
        assert_eq!(check(&window_over, &ids(), now), Ok(()));
        let limit_off = KeyRow {
            rate_limit_enabled: Some(false),
            ..full
        };
        assert_eq!(check(&limit_off, &ids(), now), Ok(()));
    }
}
