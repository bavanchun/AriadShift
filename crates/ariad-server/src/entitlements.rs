//! Plan entitlements: conversion limits and quotas per plan.

use std::time::Duration;

use ariad_core::limits::Limits;
use sqlx::PgPool;
use thiserror::Error;

/// One `plan_entitlements` row, converted.
#[derive(Clone, Debug, PartialEq)]
pub struct Entitlement {
    pub plan: String,
    /// Fills the host's limits for every job of the plan.
    pub limits: Limits,
    pub jobs_per_day: Option<u64>,
    pub input_bytes_per_day: Option<u64>,
    pub concurrent_jobs: Option<u64>,
    pub requests_per_minute: Option<u32>,
    /// How long inputs and outputs are kept.
    pub retention: Duration,
}

#[derive(Debug, Error)]
pub enum EntitlementError {
    #[error(transparent)]
    Database(#[from] sqlx::Error),
    #[error("plan '{0}' has no entitlement row")]
    UnknownPlan(String),
    #[error("plan '{plan}' has an invalid value in {field}")]
    Invalid { plan: String, field: &'static str },
}

#[derive(sqlx::FromRow)]
struct Row {
    plan: String,
    max_input_bytes: Option<i64>,
    max_pages: Option<i32>,
    timeout_s: Option<i32>,
    max_memory_mb: Option<i32>,
    max_asset_bytes: Option<i64>,
    max_nesting_depth: i32,
    max_blocks: i32,
    max_front_matter_bytes: i32,
    max_archive_entries: i32,
    max_decompressed_bytes: i64,
    max_ir_json_bytes: i64,
    jobs_per_day: Option<i32>,
    input_bytes_per_day: Option<i64>,
    concurrent_jobs: Option<i32>,
    requests_per_minute: Option<i32>,
    retention_s: i32,
}

impl Row {
    fn into_entitlement(self) -> Result<Entitlement, EntitlementError> {
        let plan = self.plan;
        let bad = |field: &'static str| EntitlementError::Invalid {
            plan: plan.clone(),
            field,
        };
        fn conv<S: Copy, T: TryFrom<S>>(
            value: Option<S>,
            field: &'static str,
            bad: &dyn Fn(&'static str) -> EntitlementError,
        ) -> Result<Option<T>, EntitlementError> {
            value
                .map(|v| T::try_from(v).map_err(|_| bad(field)))
                .transpose()
        }
        let limits = Limits {
            max_input_bytes: conv(self.max_input_bytes, "max_input_bytes", &bad)?,
            max_pages: conv(self.max_pages, "max_pages", &bad)?,
            timeout_s: conv(self.timeout_s, "timeout_s", &bad)?,
            max_memory_mb: conv(self.max_memory_mb, "max_memory_mb", &bad)?,
            max_asset_bytes: conv(self.max_asset_bytes, "max_asset_bytes", &bad)?,
            max_nesting_depth: u16::try_from(self.max_nesting_depth)
                .map_err(|_| bad("max_nesting_depth"))?,
            max_blocks: u32::try_from(self.max_blocks).map_err(|_| bad("max_blocks"))?,
            max_front_matter_bytes: u32::try_from(self.max_front_matter_bytes)
                .map_err(|_| bad("max_front_matter_bytes"))?,
            max_archive_entries: u32::try_from(self.max_archive_entries)
                .map_err(|_| bad("max_archive_entries"))?,
            max_decompressed_bytes: u64::try_from(self.max_decompressed_bytes)
                .map_err(|_| bad("max_decompressed_bytes"))?,
            max_ir_json_bytes: u64::try_from(self.max_ir_json_bytes)
                .map_err(|_| bad("max_ir_json_bytes"))?,
        };
        limits.validate().map_err(|_| bad("limits"))?;
        Ok(Entitlement {
            limits,
            jobs_per_day: conv(self.jobs_per_day, "jobs_per_day", &bad)?,
            input_bytes_per_day: conv(self.input_bytes_per_day, "input_bytes_per_day", &bad)?,
            concurrent_jobs: conv(self.concurrent_jobs, "concurrent_jobs", &bad)?,
            requests_per_minute: conv(self.requests_per_minute, "requests_per_minute", &bad)?,
            retention: Duration::from_secs(
                u64::try_from(self.retention_s).map_err(|_| bad("retention_s"))?,
            ),
            plan,
        })
    }
}

/// Loads a plan's entitlement.
pub async fn load(pool: &PgPool, plan: &str) -> Result<Entitlement, EntitlementError> {
    let row: Option<Row> = sqlx::query_as(
        "SELECT plan, max_input_bytes, max_pages, timeout_s, max_memory_mb, max_asset_bytes, \
         max_nesting_depth, max_blocks, max_front_matter_bytes, max_archive_entries, \
         max_decompressed_bytes, max_ir_json_bytes, jobs_per_day, input_bytes_per_day, \
         concurrent_jobs, requests_per_minute, retention_s \
         FROM plan_entitlements WHERE plan = $1",
    )
    .bind(plan)
    .fetch_optional(pool)
    .await?;
    row.ok_or_else(|| EntitlementError::UnknownPlan(plan.to_owned()))?
        .into_entitlement()
}

/// Current usage of one quota subject.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Usage {
    pub jobs_last_day: u64,
    pub input_bytes_last_day: u64,
    pub active_jobs: u64,
}

/// Live pending uploads allowed when the plan sets no `concurrent_jobs`.
pub const DEFAULT_PENDING_UPLOADS: u64 = 64;

/// Upload usage of one quota subject.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct UploadUsage {
    /// Uploads created in the last day, whatever their status.
    pub uploads_last_day: u64,
    /// Declared bytes of those uploads.
    pub input_bytes_last_day: u64,
    /// Pending uploads that have not expired.
    pub pending: u64,
}

pub async fn upload_usage(
    conn: &mut sqlx::PgConnection,
    subject: &str,
) -> Result<UploadUsage, sqlx::Error> {
    let (uploads, bytes, pending): (i64, i64, i64) = sqlx::query_as(
        "SELECT count(*) FILTER (WHERE created_at > now() - interval '1 day'), \
         COALESCE(sum(size_bytes) FILTER (WHERE created_at > now() - interval '1 day'), 0)::bigint, \
         (SELECT count(*) FROM uploads WHERE subject = $1 AND status = 'pending' AND expires_at > now()) \
         FROM uploads WHERE subject = $1 AND created_at > now() - interval '1 day' "
    ).bind(subject).fetch_one(conn).await?;
    Ok(UploadUsage {
        uploads_last_day: u64::try_from(uploads).unwrap_or(0),
        input_bytes_last_day: u64::try_from(bytes).unwrap_or(0),
        pending: u64::try_from(pending).unwrap_or(0),
    })
}

/// Upload admission is independent of conversion usage: abandoned uploads
/// consume the daily allocation too, and live pending uploads reserve slots.
pub async fn admit_upload(
    conn: &mut sqlx::PgConnection,
    subject: &str,
    entitlement: &Entitlement,
    input_bytes: u64,
) -> Result<(), crate::error::ApiError> {
    let usage = upload_usage(conn, subject).await?;
    let exceeded = entitlement
        .jobs_per_day
        .is_some_and(|cap| usage.uploads_last_day >= cap)
        || entitlement
            .input_bytes_per_day
            .is_some_and(|cap| usage.input_bytes_last_day.saturating_add(input_bytes) > cap)
        || usage.pending >= entitlement.pending_upload_cap();
    if exceeded {
        return Err(crate::error::ApiError::quota_exceeded(
            "the plan's upload allowance is exhausted",
        ));
    }
    Ok(())
}

pub async fn usage(conn: &mut sqlx::PgConnection, subject: &str) -> Result<Usage, sqlx::Error> {
    let (jobs, bytes): (i64, i64) = sqlx::query_as(
        "SELECT count(*), COALESCE(sum(input_bytes), 0)::bigint FROM usage_events \
         WHERE subject = $1 AND kind = 'job_submitted' AND created_at > now() - interval '1 day'",
    )
    .bind(subject)
    .fetch_one(&mut *conn)
    .await?;
    let active: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM jobs WHERE subject = $1 AND status IN ('queued', 'running')",
    )
    .bind(subject)
    .fetch_one(&mut *conn)
    .await?;
    Ok(Usage {
        jobs_last_day: u64::try_from(jobs).unwrap_or(0),
        input_bytes_last_day: u64::try_from(bytes).unwrap_or(0),
        active_jobs: u64::try_from(active).unwrap_or(0),
    })
}

/// Why a new job would exceed the plan.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum QuotaViolation {
    JobsPerDay(u64),
    InputBytesPerDay(u64),
    ConcurrentJobs(u64),
}

impl std::fmt::Display for QuotaViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::JobsPerDay(cap) => write!(f, "the plan allows {cap} jobs per day"),
            Self::InputBytesPerDay(cap) => {
                write!(f, "the plan allows {cap} input bytes per day")
            }
            Self::ConcurrentJobs(cap) => {
                write!(f, "the plan allows {cap} queued or running jobs at a time")
            }
        }
    }
}

impl Entitlement {
    /// How many live pending uploads upload admission allows.
    #[must_use]
    pub fn pending_upload_cap(&self) -> u64 {
        self.concurrent_jobs.unwrap_or(DEFAULT_PENDING_UPLOADS)
    }

    /// Checks whether one more job of `input_bytes` fits the quotas.
    pub fn admit(&self, usage: Usage, input_bytes: u64) -> Result<(), QuotaViolation> {
        if let Some(cap) = self.concurrent_jobs
            && usage.active_jobs >= cap
        {
            return Err(QuotaViolation::ConcurrentJobs(cap));
        }
        if let Some(cap) = self.jobs_per_day
            && usage.jobs_last_day >= cap
        {
            return Err(QuotaViolation::JobsPerDay(cap));
        }
        if let Some(cap) = self.input_bytes_per_day
            && usage.input_bytes_last_day.saturating_add(input_bytes) > cap
        {
            return Err(QuotaViolation::InputBytesPerDay(cap));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entitlement() -> Entitlement {
        Entitlement {
            plan: "test".to_owned(),
            limits: Limits::cloud(),
            jobs_per_day: Some(2),
            input_bytes_per_day: Some(100),
            concurrent_jobs: Some(1),
            requests_per_minute: Some(10),
            retention: Duration::from_secs(3600),
        }
    }

    #[test]
    fn admit_enforces_each_quota() {
        let e = entitlement();
        assert_eq!(e.admit(Usage::default(), 100), Ok(()));
        assert_eq!(
            e.admit(
                Usage {
                    active_jobs: 1,
                    ..Usage::default()
                },
                1
            ),
            Err(QuotaViolation::ConcurrentJobs(1))
        );
        assert_eq!(
            e.admit(
                Usage {
                    jobs_last_day: 2,
                    ..Usage::default()
                },
                1
            ),
            Err(QuotaViolation::JobsPerDay(2))
        );
        assert_eq!(
            e.admit(
                Usage {
                    input_bytes_last_day: 60,
                    ..Usage::default()
                },
                41
            ),
            Err(QuotaViolation::InputBytesPerDay(100))
        );
    }

    #[test]
    fn pending_uploads_fall_back_to_a_fixed_cap() {
        assert_eq!(entitlement().pending_upload_cap(), 1);
        let unlimited = Entitlement {
            concurrent_jobs: None,
            ..entitlement()
        };
        assert_eq!(unlimited.pending_upload_cap(), DEFAULT_PENDING_UPLOADS);
    }

    #[test]
    fn unlimited_quotas_admit_everything() {
        let e = Entitlement {
            jobs_per_day: None,
            input_bytes_per_day: None,
            concurrent_jobs: None,
            ..entitlement()
        };
        let busy = Usage {
            jobs_last_day: u64::MAX,
            input_bytes_last_day: u64::MAX,
            active_jobs: u64::MAX,
        };
        assert_eq!(e.admit(busy, u64::MAX), Ok(()));
    }
}
