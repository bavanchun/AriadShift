//! Retention sweeper: deletes inputs and outputs whose retention period has
//! passed and marks their jobs `expired`. Runs inside the API process; an
//! advisory lock keeps it to one replica at a time.

use std::time::Duration;

use sqlx::PgPool;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::{
    api::jobs::{Purge, purge_job},
    db::{self, UploadStatus},
    rate_limit,
    storage::{Storage, UPLOAD_URL_TTL},
};

/// Advisory lock key shared by every API replica.
const SWEEP_LOCK: i64 = 0x0a71_ad5f_0001;

/// Rows handled per pass and table.
const DEFAULT_BATCH: i64 = 200;

/// What one pass removed.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SweepReport {
    pub jobs: usize,
    pub uploads: usize,
}

/// Sweeps every `interval` until `shutdown`.
pub async fn run(
    pool: PgPool,
    storage: Storage,
    interval: Duration,
    batch: i64,
    shutdown: CancellationToken,
) {
    loop {
        tokio::select! {
            () = shutdown.cancelled() => return,
            () = tokio::time::sleep(interval) => {}
        }
        match sweep_locked_with_batch(&pool, &storage, batch).await {
            Ok(Some(report)) if report != SweepReport::default() => {
                tracing::info!(
                    jobs = report.jobs,
                    uploads = report.uploads,
                    "retention sweep"
                );
            }
            Ok(_) => {}
            Err(err) => tracing::warn!(error = %err, "retention sweep failed"),
        }
    }
}

/// Runs one pass if no other replica holds the sweep lock.
pub async fn sweep_locked(
    pool: &PgPool,
    storage: &Storage,
) -> Result<Option<SweepReport>, sqlx::Error> {
    sweep_locked_with_batch(pool, storage, DEFAULT_BATCH).await
}

async fn sweep_locked_with_batch(
    pool: &PgPool,
    storage: &Storage,
    batch: i64,
) -> Result<Option<SweepReport>, sqlx::Error> {
    // Closing a detached session releases the lock even when an unlock query
    // fails or this future is canceled. Never return a locked session to a pool.
    let mut lock_conn = pool.acquire().await?.detach();
    let locked: bool = sqlx::query_scalar("SELECT pg_try_advisory_lock($1)")
        .bind(SWEEP_LOCK)
        .fetch_one(&mut lock_conn)
        .await?;
    if !locked {
        return Ok(None);
    }
    let result = sweep_with_batch(pool, storage, batch).await;
    result.map(Some)
}

/// One pass: expired jobs, then expired uploads, then old rate-limit windows.
pub async fn sweep_once(pool: &PgPool, storage: &Storage) -> Result<SweepReport, sqlx::Error> {
    sweep_with_batch(pool, storage, DEFAULT_BATCH).await
}

async fn sweep_with_batch(
    pool: &PgPool,
    storage: &Storage,
    batch: i64,
) -> Result<SweepReport, sqlx::Error> {
    let mut report = SweepReport::default();
    check_upload_sizes(pool, storage, batch).await?;
    let jobs: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM jobs WHERE status <> 'expired' AND expires_at <= now() \
         ORDER BY expires_at LIMIT $1",
    )
    .bind(batch)
    .fetch_all(pool)
    .await?;
    for job_id in jobs {
        match purge_job(pool, storage, job_id, Purge::Retention).await {
            Ok(()) => report.jobs += 1,
            Err(err) => {
                tracing::warn!(job_id = %job_id, code = err.code(), "could not expire a job")
            }
        }
    }

    let uploads: Vec<(Uuid, String)> = sqlx::query_as(
        "SELECT id, storage_key FROM uploads \
         WHERE status IN ('pending', 'completed', 'deleted') AND expires_at <= now() \
         ORDER BY expires_at LIMIT $1",
    )
    .bind(batch)
    .fetch_all(pool)
    .await?;
    for (upload_id, key) in uploads {
        if let Err(err) = storage.delete(&key).await {
            tracing::warn!(upload_id = %upload_id, error = %err, "could not delete an expired upload");
            continue;
        }
        let mut conn = pool.acquire().await?;
        db::retire_upload(&mut conn, upload_id, UploadStatus::Expired).await?;
        report.uploads += 1;
    }

    rate_limit::prune(pool).await?;
    prune_metadata(pool, batch).await?;
    Ok(report)
}

/// Checks stored bytes without trusting the client's completion request. A
/// signed PUT remains usable until URL expiry. Completed and retired inputs
/// are checked during that window and once successfully after it closes;
/// retention cleanup removes them once more.
async fn check_upload_sizes(
    pool: &PgPool,
    storage: &Storage,
    batch: i64,
) -> Result<(), sqlx::Error> {
    let uploads: Vec<(Uuid, String, i64, String, chrono::DateTime<chrono::Utc>)> = sqlx::query_as(
        "SELECT id, storage_key, size_bytes, status, now() FROM uploads \
           WHERE status IN ('pending', 'completed', 'deleted') \
           AND ((status = 'pending' AND expires_at > now()) OR \
                (status IN ('completed', 'deleted') AND \
                 (created_at + $2 * interval '1 second' > now() OR size_checked_at IS NULL OR \
                  size_checked_at < created_at + $2 * interval '1 second'))) \
           AND created_at <= now() - interval '2 minutes' \
           ORDER BY size_checked_at NULLS FIRST, created_at LIMIT $1",
    )
    .bind(batch)
    .bind(UPLOAD_URL_TTL.as_secs_f64())
    .fetch_all(pool)
    .await?;
    for (id, key, declared, status, checked_at) in uploads {
        match storage.size(&key).await {
            Ok(Some(size))
                if status == "deleted" || size != u64::try_from(declared).unwrap_or(0) =>
            {
                if let Err(err) = storage.delete(&key).await {
                    tracing::warn!(upload_id = %id, error = %err, "could not remove an invalid upload");
                    continue;
                }
                let mut conn = pool.acquire().await?;
                db::retire_upload(&mut conn, id, UploadStatus::Deleted).await?;
            }
            Ok(_) => {}
            Err(err) => {
                tracing::warn!(upload_id = %id, error = %err, "upload size check failed");
                continue;
            }
        }
        // Failed HEAD/deletion attempts must remain eligible for the final
        // post-expiry check. Timestamp before HEAD so a check straddling URL
        // expiry cannot hide a last-second replay. The API lock serializes passes.
        sqlx::query("UPDATE uploads SET size_checked_at = $2 WHERE id = $1")
            .bind(id)
            .bind(checked_at)
            .execute(pool)
            .await?;
    }
    Ok(())
}

/// Keep quota admission rows for at least a day and diagnostic metadata for
/// thirty days. The immutable, filename-free usage ledger remains untouched.
async fn prune_metadata(pool: &PgPool, batch: i64) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM event_stream_leases WHERE id IN (SELECT id FROM event_stream_leases WHERE expires_at <= now() ORDER BY expires_at LIMIT $1)")
        .bind(batch)
        .execute(&mut *tx)
        .await?;
    let ids: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM jobs WHERE status = 'expired' AND expires_at < now() - interval '30 days' \
         ORDER BY expires_at LIMIT $1 FOR UPDATE",
    )
    .bind(batch)
    .fetch_all(&mut *tx)
    .await?;
    for query in [
        "DELETE FROM job_events WHERE job_id = ANY($1)",
        "DELETE FROM job_steps WHERE job_id = ANY($1)",
        "DELETE FROM artifacts WHERE job_id = ANY($1)",
    ] {
        sqlx::query(query).bind(&ids).execute(&mut *tx).await?;
    }
    sqlx::query("DELETE FROM jobs WHERE id = ANY($1)")
        .bind(&ids)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "DELETE FROM uploads WHERE id IN (SELECT id FROM uploads u \
         WHERE status IN ('deleted', 'expired') AND expires_at < now() - interval '30 days' \
         AND NOT EXISTS (SELECT 1 FROM jobs WHERE upload_id = u.id) ORDER BY expires_at LIMIT $1)",
    )
    .bind(batch)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "DELETE FROM job_events WHERE id IN (SELECT e.id FROM job_events e JOIN jobs j ON j.id = e.job_id \
         WHERE j.status IN ('succeeded', 'failed', 'canceled', 'expired') \
         AND e.created_at < now() - interval '30 days' ORDER BY e.id LIMIT $1)",
    ).bind(batch).execute(&mut *tx).await?;
    tx.commit().await
}
