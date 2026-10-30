//! The caller's job list, newest first, one page at a time.

use std::collections::HashMap;

use axum::{
    Json,
    extract::{Query, State},
    http::Uri,
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use super::{AppState, Caller, jobs::job_from_row};
use crate::{
    api::dto::Job,
    db::{self, JobListQuery, JobStatus, StepRow},
    error::{ApiError, ApiResult, ErrorBody},
};

/// Jobs per page when `limit` is not given.
pub const DEFAULT_PAGE_SIZE: u32 = 50;

/// Largest accepted `limit`.
pub const MAX_PAGE_SIZE: u32 = 100;

/// Longest cursor this server issues, with room to spare.
const MAX_CURSOR_LEN: usize = 128;

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ListJobsParams {
    /// Jobs per page, from 1 to 100.
    #[param(minimum = 1, maximum = 100, default = json!(50))]
    pub limit: Option<u32>,
    /// The previous page's `next_cursor`, to read the page after it.
    pub cursor: Option<String>,
    /// Only jobs with this status.
    pub status: Option<JobStatus>,
}

/// A job of the caller's list: every `Job` property plus the input's name.
#[derive(Debug, Serialize, ToSchema)]
pub struct JobListItem {
    #[serde(flatten)]
    pub job: Job,
    /// The input's original file name. Absent once the input is deleted or
    /// expired, since the service forgets the name with the file.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(nullable = false, example = "report.docx")]
    pub filename: Option<String>,
}

/// One page of the caller's jobs, newest first.
#[derive(Debug, Serialize, ToSchema)]
pub struct JobList {
    #[schema(max_items = 100)]
    pub jobs: Vec<JobListItem>,
    /// Pass as `cursor` to read the next page; `null` on the last page.
    #[schema(required = true)]
    pub next_cursor: Option<String>,
}

/// Where a page ends: the last job's creation time and id. Encoded as
/// URL-safe base64 so clients treat it as opaque.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Cursor {
    created_at: DateTime<Utc>,
    id: Uuid,
}

impl Cursor {
    fn encode(self) -> String {
        URL_SAFE_NO_PAD.encode(format!(
            "{}.{}",
            self.created_at.timestamp_micros(),
            self.id
        ))
    }

    fn decode(raw: &str) -> Result<Self, ApiError> {
        let invalid = || ApiError::invalid("the cursor is invalid; pass a `next_cursor` unchanged");
        if raw.len() > MAX_CURSOR_LEN {
            return Err(invalid());
        }
        let bytes = URL_SAFE_NO_PAD.decode(raw).map_err(|_| invalid())?;
        let text = std::str::from_utf8(&bytes).map_err(|_| invalid())?;
        let (micros, id) = text.split_once('.').ok_or_else(invalid)?;
        let created_at = micros
            .parse::<i64>()
            .ok()
            .and_then(DateTime::from_timestamp_micros)
            .ok_or_else(invalid)?;
        let id = Uuid::parse_str(id).map_err(|_| invalid())?;
        Ok(Self { created_at, id })
    }
}

/// List the caller's jobs.
///
/// Returns the jobs of the caller's account, newest first, including failed,
/// canceled and expired jobs whose record still exists. Pages are stable:
/// jobs created after the first page never shift later pages. Anonymous
/// jobs are not listed; their ids are their only handle.
#[utoipa::path(
    get,
    path = "/v1/jobs",
    tag = "jobs",
    params(ListJobsParams),
    responses(
        (status = 200, description = "A page of the caller's jobs", body = JobList),
        (status = 401, description = "Credentials missing or rejected", body = ErrorBody),
        (status = 422, description = "Invalid `limit`, `cursor` or `status`", body = ErrorBody),
        (status = 429, description = "Rate limited", body = ErrorBody,
         headers(("retry-after" = u64, description = "Seconds to wait before retrying; sent with the `rate_limited` code"))),
        (status = 500, description = "Internal service error", body = ErrorBody),
        (status = 503, description = "Identity verification is unavailable", body = ErrorBody),
    ),
    security(("bearer" = []), ("api_key" = [])),
)]
pub async fn list_jobs(
    State(state): State<AppState>,
    caller: Caller,
    uri: Uri,
) -> ApiResult<Json<JobList>> {
    if caller.principal.is_anonymous() {
        return Err(ApiError::unauthorized(
            "listing jobs requires an account's bearer token or API key",
        ));
    }
    let Query(params) = Query::<ListJobsParams>::try_from_uri(&uri)
        .map_err(|rejection| ApiError::invalid(rejection.body_text()))?;
    let limit = params.limit.unwrap_or(DEFAULT_PAGE_SIZE);
    if !(1..=MAX_PAGE_SIZE).contains(&limit) {
        return Err(ApiError::invalid(format!(
            "limit must be between 1 and {MAX_PAGE_SIZE}"
        )));
    }
    let after = params.cursor.as_deref().map(Cursor::decode).transpose()?;
    let page_size = limit as usize;

    // One extra row tells whether another page follows.
    let mut rows = db::list_jobs(
        &state.db,
        &JobListQuery {
            workspace_id: caller.principal.workspace_id,
            status: params.status,
            after: after.map(|cursor| (cursor.created_at, cursor.id)),
            limit: i64::from(limit) + 1,
        },
    )
    .await?;
    let next_cursor = if rows.len() > page_size {
        rows.truncate(page_size);
        rows.last().map(|row| {
            Cursor {
                created_at: row.job.created_at,
                id: row.job.id,
            }
            .encode()
        })
    } else {
        None
    };

    let ids: Vec<Uuid> = rows.iter().map(|row| row.job.id).collect();
    let mut routes: HashMap<Uuid, Vec<StepRow>> = HashMap::new();
    for step in db::steps_of_jobs(&state.db, &ids).await? {
        routes.entry(step.job_id).or_default().push(step.step);
    }
    let jobs = rows
        .into_iter()
        .map(|row| JobListItem {
            job: job_from_row(&row.job, routes.remove(&row.job.id).unwrap_or_default()),
            filename: row.filename,
        })
        .collect();
    Ok(Json(JobList { jobs, next_cursor }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cursor() -> Cursor {
        Cursor {
            created_at: DateTime::from_timestamp_micros(1_792_934_400_123_456).expect("time"),
            id: Uuid::parse_str("0192f5c4-1a2b-7c3d-8e4f-5a6b7c8d9e0f").expect("uuid"),
        }
    }

    #[test]
    fn cursors_round_trip_with_microsecond_precision() {
        let encoded = cursor().encode();
        assert!(encoded.len() <= MAX_CURSOR_LEN);
        assert!(
            encoded
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'),
            "URL-safe: {encoded}"
        );
        assert_eq!(Cursor::decode(&encoded).expect("decode"), cursor());
    }

    #[test]
    fn malformed_cursors_are_invalid_requests() {
        let too_long = "A".repeat(MAX_CURSOR_LEN + 1);
        let no_separator = URL_SAFE_NO_PAD.encode("123");
        let bad_time = URL_SAFE_NO_PAD.encode("soon.0192f5c4-1a2b-7c3d-8e4f-5a6b7c8d9e0f");
        let bad_id = URL_SAFE_NO_PAD.encode("1792934400123456.not-a-uuid");
        let out_of_range =
            URL_SAFE_NO_PAD.encode(format!("{}.0192f5c4-1a2b-7c3d-8e4f-5a6b7c8d9e0f", i64::MAX));
        for raw in [
            "",
            "not base64!",
            too_long.as_str(),
            no_separator.as_str(),
            bad_time.as_str(),
            bad_id.as_str(),
            out_of_range.as_str(),
        ] {
            let error = Cursor::decode(raw).expect_err(raw);
            assert_eq!(error.code(), "invalid_request", "{raw}");
        }
    }

    #[test]
    fn listed_jobs_omit_a_forgotten_filename() {
        let item = |filename: Option<&str>| JobListItem {
            job: Job {
                id: Uuid::nil(),
                status: JobStatus::Expired,
                upload_id: Uuid::nil(),
                from: "markdown".to_owned(),
                to: "html".to_owned(),
                profile: crate::api::dto::Profile::Editable,
                pack: crate::packs::Pack::Core,
                route: Vec::new(),
                warnings: Vec::new(),
                error: None,
                attempts: 1,
                created_at: cursor().created_at,
                started_at: None,
                finished_at: None,
                expires_at: cursor().created_at,
            },
            filename: filename.map(str::to_owned),
        };
        let named = serde_json::to_value(item(Some("report.md"))).expect("serialize");
        assert_eq!(named["filename"], "report.md");
        assert_eq!(named["status"], "expired", "job properties are flattened");
        let forgotten = serde_json::to_value(item(None)).expect("serialize");
        assert!(forgotten.get("filename").is_none());
    }

    #[test]
    fn the_list_is_documented_for_accounts_only() {
        let doc = crate::api::openapi();
        let operation = doc.paths.paths["/v1/jobs"]
            .get
            .as_ref()
            .expect("GET /v1/jobs");
        assert_eq!(operation.operation_id.as_deref(), Some("list_jobs"));
        let security = operation.security.as_ref().expect("security");
        assert_eq!(security.len(), 2, "bearer or API key, never anonymous");
        let names: Vec<&str> = operation
            .parameters
            .as_ref()
            .expect("parameters")
            .iter()
            .filter_map(|parameter| match parameter {
                utoipa::openapi::RefOr::T(parameter) => Some(parameter.name.as_str()),
                utoipa::openapi::RefOr::Ref(_) => None,
            })
            .collect();
        assert_eq!(names, ["limit", "cursor", "status"]);
    }
}
