//! PostgreSQL access: pool, migrations, rows and queries.
//!
//! Queries are checked at runtime; the integration tests run each of them
//! against PostgreSQL with pgmq.

use std::{fmt, str::FromStr, time::Duration};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{
    PgConnection, PgPool, Postgres, Transaction,
    migrate::Migrator,
    postgres::{PgConnectOptions, PgPoolOptions},
};
use thiserror::Error;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::packs::Pack;

/// Embedded schema migrations.
pub static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

/// Tenant that owns every anonymous upload and job.
pub const ANONYMOUS_WORKSPACE: Uuid = Uuid::nil();

/// Plan given to a new account's workspace.
pub const DEFAULT_PLAN: &str = "free";

/// Entitlement row for callers without credentials.
pub const ANONYMOUS_PLAN: &str = "anonymous";

/// Channel the `job_events` trigger notifies.
pub const JOB_EVENTS_CHANNEL: &str = "ariad_job_events";

#[derive(Debug, Error)]
pub enum SchemaError {
    #[error(transparent)]
    Database(#[from] sqlx::Error),
    #[error("the database schema is behind this build; run `ariad-server migrate`")]
    Pending,
    #[error("the database schema is newer than this build ({0})")]
    Unknown(i64),
}

/// Connects to PostgreSQL.
pub async fn connect(url: &str, max_connections: u32) -> Result<PgPool, sqlx::Error> {
    let options: PgConnectOptions = url.parse()?;
    PgPoolOptions::new()
        .max_connections(max_connections)
        .acquire_timeout(Duration::from_secs(10))
        .connect_with(options.application_name("ariad-server"))
        .await
}

/// Applies pending migrations.
pub async fn migrate(pool: &PgPool) -> Result<(), sqlx::migrate::MigrateError> {
    MIGRATOR.run(pool).await
}

/// Fails unless every embedded migration is applied and none is unknown.
pub async fn check_schema(pool: &PgPool) -> Result<(), SchemaError> {
    let exists: bool = sqlx::query_scalar("SELECT to_regclass('_sqlx_migrations') IS NOT NULL")
        .fetch_one(pool)
        .await?;
    if !exists {
        return Err(SchemaError::Pending);
    }
    let applied: Vec<i64> =
        sqlx::query_scalar("SELECT version FROM _sqlx_migrations WHERE success ORDER BY version")
            .fetch_all(pool)
            .await?;
    if let Some(unknown) = applied
        .iter()
        .find(|v| !MIGRATOR.iter().any(|m| m.version == **v))
    {
        return Err(SchemaError::Unknown(*unknown));
    }
    if MIGRATOR.iter().any(|m| !applied.contains(&m.version)) {
        return Err(SchemaError::Pending);
    }
    Ok(())
}

/// Lifecycle of a job: `queued → running → succeeded | failed | canceled → expired`.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    Canceled,
    Expired,
}

impl JobStatus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Canceled => "canceled",
            Self::Expired => "expired",
        }
    }

    #[must_use]
    pub const fn is_active(self) -> bool {
        matches!(self, Self::Queued | Self::Running)
    }

    #[must_use]
    pub const fn is_final(self) -> bool {
        !self.is_active()
    }
}

impl fmt::Display for JobStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for JobStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "queued" => Ok(Self::Queued),
            "running" => Ok(Self::Running),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "canceled" => Ok(Self::Canceled),
            "expired" => Ok(Self::Expired),
            other => Err(format!("unknown job status '{other}'")),
        }
    }
}

/// Lifecycle of an upload.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum UploadStatus {
    /// Waiting for the client's `PUT` and `complete` call.
    Pending,
    /// Verified and usable as job input.
    Completed,
    /// Removed on request.
    Deleted,
    /// Removed after its retention period.
    Expired,
}

impl UploadStatus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Completed => "completed",
            Self::Deleted => "deleted",
            Self::Expired => "expired",
        }
    }
}

impl FromStr for UploadStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pending" => Ok(Self::Pending),
            "completed" => Ok(Self::Completed),
            "deleted" => Ok(Self::Deleted),
            "expired" => Ok(Self::Expired),
            other => Err(format!("unknown upload status '{other}'")),
        }
    }
}

#[derive(Clone, Debug, sqlx::FromRow)]
pub struct UploadRow {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub subject: String,
    pub filename: String,
    pub size_bytes: i64,
    pub sha256: String,
    pub format: Option<String>,
    pub storage_key: String,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub expires_at: DateTime<Utc>,
}

impl UploadRow {
    #[must_use]
    pub fn status(&self) -> UploadStatus {
        self.status.parse().unwrap_or(UploadStatus::Deleted)
    }
}

pub struct NewUpload<'a> {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub subject: &'a str,
    pub filename: &'a str,
    pub size_bytes: i64,
    pub sha256: &'a str,
    pub storage_key: &'a str,
    /// Seconds until the stored file is deleted, counted from the database clock.
    pub retention_s: i64,
}

macro_rules! upload_columns {
    () => {
        "id, workspace_id, subject, filename, size_bytes, sha256, format, storage_key, status, \
         created_at, completed_at, expires_at"
    };
}

pub async fn insert_upload(
    conn: &mut PgConnection,
    upload: &NewUpload<'_>,
) -> Result<UploadRow, sqlx::Error> {
    sqlx::query_as(concat!(
        "INSERT INTO uploads (id, workspace_id, subject, filename, size_bytes, sha256, \
         storage_key, status, expires_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, 'pending', now() + make_interval(secs => $8)) RETURNING ",
        upload_columns!()
    ))
    .bind(upload.id)
    .bind(upload.workspace_id)
    .bind(upload.subject)
    .bind(upload.filename)
    .bind(upload.size_bytes)
    .bind(upload.sha256)
    .bind(upload.storage_key)
    .bind(upload.retention_s as f64)
    .fetch_one(conn)
    .await
}

pub async fn get_upload(pool: &PgPool, id: Uuid) -> Result<Option<UploadRow>, sqlx::Error> {
    sqlx::query_as(concat!(
        "SELECT ",
        upload_columns!(),
        " FROM uploads WHERE id = $1"
    ))
    .bind(id)
    .fetch_optional(pool)
    .await
}

/// Marks a pending upload completed. Returns `None` when it was not pending.
pub async fn complete_upload(
    pool: &PgPool,
    id: Uuid,
    format: Option<&str>,
    retention_s: i64,
) -> Result<Option<UploadRow>, sqlx::Error> {
    sqlx::query_as(concat!(
        "UPDATE uploads SET status = 'completed', format = $2, completed_at = now(), \
         expires_at = created_at + make_interval(secs => $3) \
         WHERE id = $1 AND status = 'pending' AND expires_at > now() RETURNING ",
        upload_columns!()
    ))
    .bind(id)
    .bind(format)
    .bind(retention_s as f64)
    .fetch_optional(pool)
    .await
}

/// Moves a live upload to `deleted` or `expired`.
pub async fn retire_upload(
    conn: &mut PgConnection,
    id: Uuid,
    status: UploadStatus,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE uploads SET status = $2, filename = '' WHERE id = $1 AND status IN ('pending', 'completed', 'deleted')",
    )
    .bind(id)
    .bind(status.as_str())
    .execute(conn)
    .await?;
    Ok(())
}

#[derive(Clone, Debug, sqlx::FromRow)]
pub struct JobRow {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub subject: String,
    pub upload_id: Uuid,
    pub source_format: String,
    pub target_format: String,
    pub profile: String,
    pub pack: String,
    pub status: String,
    pub attempts: i32,
    pub limits: sqlx::types::Json<serde_json::Value>,
    pub warnings: sqlx::types::Json<serde_json::Value>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub expires_at: DateTime<Utc>,
}

impl JobRow {
    #[must_use]
    pub fn status(&self) -> JobStatus {
        self.status.parse().unwrap_or(JobStatus::Failed)
    }

    #[must_use]
    pub fn pack(&self) -> Pack {
        self.pack.parse().unwrap_or(Pack::Core)
    }
}

macro_rules! job_columns {
    () => {
        "id, workspace_id, subject, upload_id, source_format, target_format, profile, pack, \
         status, attempts, limits, warnings, error_code, error_message, created_at, started_at, \
         finished_at, expires_at"
    };
}

pub struct NewJob<'a> {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub subject: &'a str,
    pub upload_id: Uuid,
    pub source_format: &'a str,
    pub target_format: &'a str,
    pub profile: &'a str,
    pub pack: Pack,
    pub limits: serde_json::Value,
    /// Seconds until the job's files are deleted, counted from the database clock.
    pub retention_s: i64,
}

pub async fn insert_job(conn: &mut PgConnection, job: &NewJob<'_>) -> Result<JobRow, sqlx::Error> {
    sqlx::query_as(concat!(
        "INSERT INTO jobs (id, workspace_id, subject, upload_id, source_format, target_format, \
         profile, pack, status, limits, expires_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 'queued', $9, now() + make_interval(secs => $10)) RETURNING ",
        job_columns!()
    ))
    .bind(job.id)
    .bind(job.workspace_id)
    .bind(job.subject)
    .bind(job.upload_id)
    .bind(job.source_format)
    .bind(job.target_format)
    .bind(job.profile)
    .bind(job.pack.as_str())
    .bind(sqlx::types::Json(&job.limits))
    .bind(job.retention_s as f64)
    .fetch_one(conn)
    .await
}

pub async fn get_job(pool: &PgPool, id: Uuid) -> Result<Option<JobRow>, sqlx::Error> {
    sqlx::query_as(concat!(
        "SELECT ",
        job_columns!(),
        " FROM jobs WHERE id = $1"
    ))
    .bind(id)
    .fetch_optional(pool)
    .await
}

/// A job of a workspace's list, with its input's original file name while
/// the upload keeps it (deleted and expired uploads forget the name).
#[derive(Clone, Debug, sqlx::FromRow)]
pub struct ListedJobRow {
    #[sqlx(flatten)]
    pub job: JobRow,
    pub filename: Option<String>,
}

/// Selects one page of a workspace's jobs, newest first.
pub struct JobListQuery {
    pub workspace_id: Uuid,
    pub status: Option<JobStatus>,
    /// Only jobs ordered after this `(created_at, id)` position.
    pub after: Option<(DateTime<Utc>, Uuid)>,
    pub limit: i64,
}

macro_rules! list_jobs_sql {
    ($after:literal) => {
        concat!(
            "SELECT ",
            job_columns!(),
            ", NULLIF((SELECT u.filename FROM uploads u WHERE u.id = jobs.upload_id), '') \
             AS filename FROM jobs WHERE workspace_id = $1 AND ($2::text IS NULL OR status = $2)",
            $after,
            " ORDER BY created_at DESC, id DESC LIMIT $3"
        )
    };
}

/// Lists a workspace's jobs by `(created_at, id)` descending. Keyset paging
/// keeps pages stable while new jobs are created.
pub async fn list_jobs(
    pool: &PgPool,
    query: &JobListQuery,
) -> Result<Vec<ListedJobRow>, sqlx::Error> {
    let status = query.status.map(JobStatus::as_str);
    match query.after {
        None => {
            sqlx::query_as(list_jobs_sql!(""))
                .bind(query.workspace_id)
                .bind(status)
                .bind(query.limit)
                .fetch_all(pool)
                .await
        }
        Some((created_at, id)) => {
            sqlx::query_as(list_jobs_sql!(" AND (created_at, id) < ($4, $5)"))
                .bind(query.workspace_id)
                .bind(status)
                .bind(query.limit)
                .bind(created_at)
                .bind(id)
                .fetch_all(pool)
                .await
        }
    }
}

pub async fn job_status(pool: &PgPool, id: Uuid) -> Result<Option<JobStatus>, sqlx::Error> {
    let status: Option<String> = sqlx::query_scalar("SELECT status FROM jobs WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(status.and_then(|s| s.parse().ok()))
}

/// Claims a job for a worker attempt. Returns `None` unless it was queued, or
/// running from an attempt whose worker disappeared.
pub async fn start_job(pool: &PgPool, id: Uuid) -> Result<Option<JobRow>, sqlx::Error> {
    sqlx::query_as(concat!(
        "UPDATE jobs SET status = 'running', attempts = attempts + 1, \
         started_at = COALESCE(started_at, now()) \
         WHERE id = $1 AND status IN ('queued', 'running') RETURNING ",
        job_columns!()
    ))
    .bind(id)
    .fetch_optional(pool)
    .await
}

/// Returns a running job to the queue. With `refund`, the attempt does not
/// count toward the retry limit (the worker stopped; the job did not fail).
pub async fn requeue_job(
    conn: &mut PgConnection,
    id: Uuid,
    refund: bool,
) -> Result<bool, sqlx::Error> {
    let done = sqlx::query(
        "UPDATE jobs SET status = 'queued', \
         attempts = CASE WHEN $2 THEN GREATEST(attempts - 1, 0) ELSE attempts END \
         WHERE id = $1 AND status = 'running'",
    )
    .bind(id)
    .bind(refund)
    .execute(conn)
    .await?;
    Ok(done.rows_affected() == 1)
}

pub struct JobOutcome<'a> {
    pub status: JobStatus,
    pub error_code: Option<&'a str>,
    pub error_message: Option<&'a str>,
    pub warnings: serde_json::Value,
}

/// Records a final status for a running job. Returns `false` when the job was
/// no longer running, for example because it was canceled meanwhile.
pub async fn finish_job(
    conn: &mut PgConnection,
    id: Uuid,
    outcome: &JobOutcome<'_>,
) -> Result<bool, sqlx::Error> {
    let done = sqlx::query(
        "UPDATE jobs SET status = $2, error_code = $3, error_message = $4, warnings = $5, \
         finished_at = now() WHERE id = $1 AND status = 'running'",
    )
    .bind(id)
    .bind(outcome.status.as_str())
    .bind(outcome.error_code)
    .bind(outcome.error_message)
    .bind(sqlx::types::Json(&outcome.warnings))
    .execute(conn)
    .await?;
    Ok(done.rows_affected() == 1)
}

/// Cancels a queued or running job. Returns `false` when it was already final.
pub async fn cancel_job(conn: &mut PgConnection, id: Uuid) -> Result<bool, sqlx::Error> {
    let done = sqlx::query(
        "UPDATE jobs SET status = 'canceled', finished_at = now() \
         WHERE id = $1 AND status IN ('queued', 'running')",
    )
    .bind(id)
    .execute(conn)
    .await?;
    Ok(done.rows_affected() == 1)
}

/// Marks a final job expired after its objects are gone.
pub async fn expire_job(conn: &mut PgConnection, id: Uuid) -> Result<bool, sqlx::Error> {
    let done = sqlx::query(
        "UPDATE jobs SET status = 'expired', expires_at = LEAST(expires_at, now()), \
         warnings = '[]', error_message = NULL \
         WHERE id = $1 AND status IN ('succeeded', 'failed', 'canceled')",
    )
    .bind(id)
    .execute(conn)
    .await?;
    Ok(done.rows_affected() == 1)
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, sqlx::FromRow)]
pub struct StepRow {
    pub position: i32,
    pub from_format: String,
    pub to_format: String,
    pub engine: String,
}

/// Replaces a job's route.
pub async fn set_job_steps(
    conn: &mut PgConnection,
    job_id: Uuid,
    steps: &[(String, String, String)],
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM job_steps WHERE job_id = $1")
        .bind(job_id)
        .execute(&mut *conn)
        .await?;
    for (position, (from, to, engine)) in steps.iter().enumerate() {
        sqlx::query(
            "INSERT INTO job_steps (job_id, position, from_format, to_format, engine) \
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(job_id)
        .bind(i32::try_from(position).unwrap_or(i32::MAX))
        .bind(from)
        .bind(to)
        .bind(engine)
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

pub async fn job_steps(pool: &PgPool, job_id: Uuid) -> Result<Vec<StepRow>, sqlx::Error> {
    sqlx::query_as(
        "SELECT position, from_format, to_format, engine FROM job_steps \
         WHERE job_id = $1 ORDER BY position",
    )
    .bind(job_id)
    .fetch_all(pool)
    .await
}

#[derive(Clone, Debug, sqlx::FromRow)]
pub struct JobStepRow {
    pub job_id: Uuid,
    #[sqlx(flatten)]
    pub step: StepRow,
}

/// The routes of several jobs in one query, each in step order.
pub async fn steps_of_jobs(
    pool: &PgPool,
    job_ids: &[Uuid],
) -> Result<Vec<JobStepRow>, sqlx::Error> {
    if job_ids.is_empty() {
        return Ok(Vec::new());
    }
    sqlx::query_as(
        "SELECT job_id, position, from_format, to_format, engine FROM job_steps \
         WHERE job_id = ANY($1) ORDER BY job_id, position",
    )
    .bind(job_ids)
    .fetch_all(pool)
    .await
}

#[derive(Clone, Debug, sqlx::FromRow)]
pub struct EventRow {
    pub id: i64,
    pub job_id: Uuid,
    pub kind: String,
    pub data: sqlx::types::Json<serde_json::Value>,
    pub created_at: DateTime<Utc>,
}

/// Appends a job event; the trigger notifies listeners on commit.
pub async fn add_event(
    conn: &mut PgConnection,
    job_id: Uuid,
    kind: &str,
    data: serde_json::Value,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO job_events (job_id, kind, data) VALUES ($1, $2, $3) RETURNING id",
    )
    .bind(job_id)
    .bind(kind)
    .bind(sqlx::types::Json(data))
    .fetch_one(conn)
    .await
}

pub async fn events_after(
    pool: &PgPool,
    job_id: Uuid,
    after_id: i64,
) -> Result<Vec<EventRow>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, job_id, kind, data, created_at FROM job_events \
         WHERE job_id = $1 AND id > $2 ORDER BY id LIMIT 500",
    )
    .bind(job_id)
    .bind(after_id)
    .fetch_all(pool)
    .await
}

#[derive(Clone, Debug, sqlx::FromRow)]
pub struct ArtifactRow {
    pub id: Uuid,
    pub job_id: Uuid,
    pub format: String,
    pub filename: String,
    pub media_type: String,
    pub size_bytes: i64,
    pub sha256: String,
    pub storage_key: String,
    pub created_at: DateTime<Utc>,
}

pub struct NewArtifact<'a> {
    pub id: Uuid,
    pub job_id: Uuid,
    pub format: &'a str,
    pub filename: &'a str,
    pub media_type: &'a str,
    pub size_bytes: i64,
    pub sha256: &'a str,
    pub storage_key: &'a str,
}

pub async fn insert_artifact(
    conn: &mut PgConnection,
    artifact: &NewArtifact<'_>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO artifacts (id, job_id, format, filename, media_type, size_bytes, sha256, \
         storage_key) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
    )
    .bind(artifact.id)
    .bind(artifact.job_id)
    .bind(artifact.format)
    .bind(artifact.filename)
    .bind(artifact.media_type)
    .bind(artifact.size_bytes)
    .bind(artifact.sha256)
    .bind(artifact.storage_key)
    .execute(conn)
    .await?;
    Ok(())
}

pub async fn live_artifacts(pool: &PgPool, job_id: Uuid) -> Result<Vec<ArtifactRow>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, job_id, format, filename, media_type, size_bytes, sha256, storage_key, \
         created_at FROM artifacts WHERE job_id = $1 AND deleted_at IS NULL ORDER BY created_at",
    )
    .bind(job_id)
    .fetch_all(pool)
    .await
}

pub async fn mark_artifacts_deleted(
    conn: &mut PgConnection,
    job_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE artifacts SET deleted_at = now(), filename = '' WHERE job_id = $1 AND deleted_at IS NULL")
        .bind(job_id)
        .execute(conn)
        .await?;
    Ok(())
}

/// One metering record. Writing the same `event_key` twice is a no-op.
pub struct UsageEvent<'a> {
    pub event_key: String,
    pub workspace_id: Uuid,
    pub subject: &'a str,
    pub job_id: Option<Uuid>,
    pub kind: &'static str,
    pub input_bytes: i64,
    pub output_bytes: i64,
    pub pages: i32,
    pub engine_ms: i64,
}

pub async fn record_usage(
    conn: &mut PgConnection,
    event: &UsageEvent<'_>,
) -> Result<bool, sqlx::Error> {
    let done = sqlx::query(
        "INSERT INTO usage_events (event_key, workspace_id, subject, job_id, kind, input_bytes, \
         output_bytes, pages, engine_ms) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) \
         ON CONFLICT (event_key) DO NOTHING",
    )
    .bind(&event.event_key)
    .bind(event.workspace_id)
    .bind(event.subject)
    .bind(event.job_id)
    .bind(event.kind)
    .bind(event.input_bytes)
    .bind(event.output_bytes)
    .bind(event.pages)
    .bind(event.engine_ms)
    .execute(conn)
    .await?;
    Ok(done.rows_affected() == 1)
}

/// Finds or creates the workspace of an account. Returns its id and plan.
pub async fn ensure_workspace(pool: &PgPool, user_id: &str) -> Result<(Uuid, String), sqlx::Error> {
    let select = "SELECT id, plan FROM workspaces WHERE owner_user_id = $1";
    if let Some(row) = sqlx::query_as(select)
        .bind(user_id)
        .fetch_optional(pool)
        .await?
    {
        return Ok(row);
    }
    sqlx::query(
        "INSERT INTO workspaces (id, owner_user_id, plan) VALUES ($1, $2, $3) \
         ON CONFLICT (owner_user_id) DO NOTHING",
    )
    .bind(Uuid::new_v4())
    .bind(user_id)
    .bind(DEFAULT_PLAN)
    .execute(pool)
    .await?;
    sqlx::query_as(select).bind(user_id).fetch_one(pool).await
}

/// Serializes quota checks and inserts for one subject within a transaction.
pub async fn lock_subject(
    tx: &mut Transaction<'_, Postgres>,
    subject: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(subject)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// Serialize fleet admission before taking a quota subject's lock. Count and
/// all job/message/metering writes remain in the caller's transaction.
pub async fn admit_global_job(
    tx: &mut Transaction<'_, Postgres>,
    ceiling: u32,
) -> Result<bool, sqlx::Error> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('ariad-global-admission', 0))")
        .execute(&mut **tx)
        .await?;
    let active: i64 =
        sqlx::query_scalar("SELECT count(*) FROM (SELECT 1 FROM jobs WHERE status IN ('queued', 'running') LIMIT $1) active")
            .bind(i64::from(ceiling))
            .fetch_one(&mut **tx)
            .await?;
    Ok(active < i64::from(ceiling))
}
