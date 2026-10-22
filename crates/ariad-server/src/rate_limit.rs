//! Fixed one-minute request windows stored in PostgreSQL, so every API
//! replica shares the same counters.

use sqlx::PgPool;

/// Outcome of counting one request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Decision {
    Allowed,
    /// Over the limit; retry after this many seconds.
    Limited {
        retry_after_s: u64,
    },
}

fn limited(remaining_s: f64) -> Decision {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let retry_after_s = remaining_s.ceil().max(1.0) as u64;
    Decision::Limited { retry_after_s }
}

/// Checks a window without counting another request.
pub async fn check(pool: &PgPool, bucket: &str, limit: u32) -> Result<Decision, sqlx::Error> {
    let remaining_s: Option<f64> = sqlx::query_scalar(
        "SELECT extract(epoch FROM (window_start + interval '1 minute' - now()))::float8 \
         FROM rate_limit_counters WHERE bucket = $1 \
         AND window_start = date_trunc('minute', now()) AND count >= $2",
    )
    .bind(bucket)
    .bind(i64::from(limit))
    .fetch_optional(pool)
    .await?;
    Ok(remaining_s.map_or(Decision::Allowed, limited))
}

/// Counts one request in `bucket` against `limit` requests per minute.
pub async fn hit(pool: &PgPool, bucket: &str, limit: u32) -> Result<Decision, sqlx::Error> {
    let (count, remaining_s): (i32, f64) = sqlx::query_as(
        "INSERT INTO rate_limit_counters (bucket, window_start, count) \
         VALUES ($1, date_trunc('minute', now()), 1) \
         ON CONFLICT (bucket, window_start) \
         DO UPDATE SET count = rate_limit_counters.count + 1 \
         RETURNING count, \
         extract(epoch FROM (window_start + interval '1 minute' - now()))::float8",
    )
    .bind(bucket)
    .fetch_one(pool)
    .await?;
    if u32::try_from(count).unwrap_or(u32::MAX) > limit {
        Ok(limited(remaining_s))
    } else {
        Ok(Decision::Allowed)
    }
}

/// Drops windows older than ten minutes.
pub async fn prune(pool: &PgPool) -> Result<u64, sqlx::Error> {
    let done = sqlx::query(
        "DELETE FROM rate_limit_counters WHERE window_start < now() - interval '10 minutes'",
    )
    .execute(pool)
    .await?;
    Ok(done.rows_affected())
}
