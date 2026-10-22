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
    storage::Storage,
};

/// Advisory lock key shared by every API replica.
const SWEEP_LOCK: i64 = 0x0a71_ad5f_0001;

/// Rows handled per pass and table.
const BATCH: i64 = 200;

/// What one pass removed.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SweepReport {
    pub jobs: usize,
    pub uploads: usize,
}

/// Sweeps every `interval` until `shutdown`.
pub async fn run(pool: PgPool, storage: Storage, interval: Duration, shutdown: CancellationToken) {
    loop {
        tokio::select! {
            () = shutdown.cancelled() => return,
            () = tokio::time::sleep(interval) => {}
        }
        match sweep_locked(&pool, &storage).await {
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
    let mut lock_conn = pool.acquire().await?;
    let locked: bool = sqlx::query_scalar("SELECT pg_try_advisory_lock($1)")
        .bind(SWEEP_LOCK)
        .fetch_one(&mut *lock_conn)
        .await?;
    if !locked {
        return Ok(None);
    }
    let result = sweep_once(pool, storage).await;
    sqlx::query("SELECT pg_advisory_unlock($1)")
        .bind(SWEEP_LOCK)
        .execute(&mut *lock_conn)
        .await?;
    result.map(Some)
}

/// One pass: expired jobs, then expired uploads, then old rate-limit windows.
pub async fn sweep_once(pool: &PgPool, storage: &Storage) -> Result<SweepReport, sqlx::Error> {
    let mut report = SweepReport::default();
    let jobs: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM jobs WHERE status <> 'expired' AND expires_at <= now() \
         ORDER BY expires_at LIMIT $1",
    )
    .bind(BATCH)
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
         WHERE status IN ('pending', 'completed') AND expires_at <= now() \
         ORDER BY expires_at LIMIT $1",
    )
    .bind(BATCH)
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
    Ok(report)
}
