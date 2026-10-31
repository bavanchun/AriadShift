//! `GET /v1/limits`.

use axum::{
    Json,
    extract::State,
    http::{HeaderValue, header},
    response::{IntoResponse, Response},
};

use super::{AppState, Caller};
use crate::{
    api::dto::{ConversionLimits, PlanLimits, Quota, Quotas},
    entitlements::{self, Entitlement, UploadUsage, Usage},
    error::{ApiResult, ErrorBody},
};

/// Get the caller's plan limits, quotas and retention.
///
/// Lets a client show what it may upload before it uploads. The quotas are
/// the ones `POST /v1/uploads` and `POST /v1/jobs` enforce for this caller:
/// per workspace with credentials, per client address without.
#[utoipa::path(
    get,
    path = "/v1/limits",
    tag = "planning",
    responses(
        (status = 200, description = "The caller's limits", body = PlanLimits),
        (status = 401, description = "Credentials rejected", body = ErrorBody),
        (status = 429, description = "Rate limited", body = ErrorBody,
         headers(("retry-after" = u64, description = "Seconds to wait before retrying; sent with the `rate_limited` code"))),
        (status = 500, description = "Internal service error", body = ErrorBody),
        (status = 503, description = "Identity verification is unavailable", body = ErrorBody),
    ),
    security((), ("bearer" = []), ("api_key" = [])),
)]
pub async fn get_limits(State(state): State<AppState>, caller: Caller) -> ApiResult<Response> {
    let mut conn = state.db.acquire().await?;
    let jobs = entitlements::usage(&mut conn, &caller.principal.subject).await?;
    let uploads = entitlements::upload_usage(&mut conn, &caller.principal.subject).await?;
    drop(conn);
    let body = plan_limits(
        &caller.entitlement,
        !caller.principal.is_anonymous(),
        jobs,
        uploads,
    );
    let mut response = Json(body).into_response();
    // The body describes one caller's usage.
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    Ok(response)
}

/// Maps a plan and its usage to the response; each `remaining` is what the
/// matching admission check still lets through.
fn plan_limits(
    entitlement: &Entitlement,
    authenticated: bool,
    jobs: Usage,
    uploads: UploadUsage,
) -> PlanLimits {
    let limits = &entitlement.limits;
    PlanLimits {
        plan: entitlement.plan.clone(),
        authenticated,
        retention_s: entitlement.retention.as_secs(),
        requests_per_minute: entitlement.requests_per_minute,
        limits: ConversionLimits {
            max_input_bytes: limits.max_input_bytes,
            max_pages: limits.max_pages,
            timeout_s: limits.timeout_s,
            max_memory_mb: limits.max_memory_mb,
        },
        quotas: Quotas {
            uploads_per_day: Quota::new(entitlement.jobs_per_day, uploads.uploads_last_day),
            jobs_per_day: Quota::new(entitlement.jobs_per_day, jobs.jobs_last_day),
            input_bytes_per_day: Quota::new(
                entitlement.input_bytes_per_day,
                uploads.input_bytes_last_day.max(jobs.input_bytes_last_day),
            ),
            pending_uploads: Quota::new(Some(entitlement.pending_upload_cap()), uploads.pending),
            concurrent_jobs: Quota::new(entitlement.concurrent_jobs, jobs.active_jobs),
        },
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use ariad_core::limits::Limits;

    use super::*;

    fn anonymous() -> Entitlement {
        Entitlement {
            plan: "anonymous".to_owned(),
            limits: Limits {
                max_input_bytes: Some(10_485_760),
                max_pages: Some(50),
                timeout_s: Some(120),
                max_memory_mb: Some(1024),
                ..Limits::cloud()
            },
            jobs_per_day: Some(20),
            input_bytes_per_day: Some(1_000),
            concurrent_jobs: Some(1),
            requests_per_minute: Some(30),
            retention: Duration::from_secs(3600),
        }
    }

    #[test]
    fn quotas_report_what_is_left() {
        assert_eq!(
            Quota::new(Some(20), 3),
            Quota {
                limit: Some(20),
                used: 3,
                remaining: Some(17)
            }
        );
        assert_eq!(Quota::new(Some(2), 5).remaining, Some(0));
        assert_eq!(Quota::new(None, 5).remaining, None);
    }

    #[test]
    fn limits_follow_the_plan_and_both_meters() {
        let body = plan_limits(
            &anonymous(),
            false,
            Usage {
                jobs_last_day: 2,
                input_bytes_last_day: 700,
                active_jobs: 1,
            },
            UploadUsage {
                uploads_last_day: 4,
                input_bytes_last_day: 300,
                pending: 0,
            },
        );
        assert_eq!(body.plan, "anonymous");
        assert!(!body.authenticated);
        assert_eq!(body.retention_s, 3600);
        assert_eq!(body.requests_per_minute, Some(30));
        assert_eq!(body.limits.max_input_bytes, Some(10_485_760));
        assert_eq!(body.limits.max_pages, Some(50));
        assert_eq!(body.limits.timeout_s, Some(120));
        assert_eq!(body.limits.max_memory_mb, Some(1024));
        assert_eq!(body.quotas.uploads_per_day.remaining, Some(16));
        assert_eq!(body.quotas.jobs_per_day.remaining, Some(18));
        // Both upload and job admission check the byte quota.
        assert_eq!(
            body.quotas.input_bytes_per_day,
            Quota::new(Some(1_000), 700)
        );
        assert_eq!(body.quotas.pending_uploads, Quota::new(Some(1), 0));
        assert_eq!(body.quotas.concurrent_jobs.remaining, Some(0));
    }

    #[test]
    fn unlimited_plans_report_null_caps() {
        let plan = Entitlement {
            jobs_per_day: None,
            input_bytes_per_day: None,
            concurrent_jobs: None,
            requests_per_minute: None,
            ..anonymous()
        };
        let body = plan_limits(&plan, true, Usage::default(), UploadUsage::default());
        assert_eq!(body.quotas.jobs_per_day.limit, None);
        assert_eq!(body.quotas.input_bytes_per_day.remaining, None);
        assert_eq!(body.quotas.concurrent_jobs.limit, None);
        // Upload admission still bounds pending uploads.
        assert_eq!(
            body.quotas.pending_uploads.limit,
            Some(entitlements::DEFAULT_PENDING_UPLOADS)
        );
        assert_eq!(body.requests_per_minute, None);
    }
}
