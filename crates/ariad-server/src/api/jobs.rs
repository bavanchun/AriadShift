//! Job creation, status, outputs and deletion.

use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde_json::json;
use uuid::Uuid;

use super::{ApiJson, AppState, Caller, parse_id};
use crate::{
    api::dto::{CreateJobRequest, Job, JobWarning, OutputFile, Outputs, Profile, RouteStep},
    db::{self, JobRow, JobStatus, NewJob, UploadStatus, UsageEvent},
    error::{ApiError, ApiResult, ErrorBody, ErrorDetail},
    packs::Pack,
    queue::JobMessage,
    routing, telemetry,
};

pub(crate) async fn job_dto(state: &AppState, row: &JobRow) -> ApiResult<Job> {
    let steps = db::job_steps(&state.db, row.id).await?;
    let warnings: Vec<JobWarning> =
        serde_json::from_value(row.warnings.0.clone()).unwrap_or_default();
    Ok(Job {
        id: row.id,
        status: row.status(),
        upload_id: row.upload_id,
        from: row.source_format.clone(),
        to: row.target_format.clone(),
        profile: Profile::parse(&row.profile).unwrap_or_default(),
        pack: row.pack(),
        route: steps
            .into_iter()
            .map(|s| RouteStep {
                from: s.from_format,
                to: s.to_format,
                engine: s.engine,
            })
            .collect(),
        warnings,
        error: row.error_code.as_ref().map(|code| ErrorDetail {
            code: code.clone(),
            message: row.error_message.clone().unwrap_or_default(),
        }),
        attempts: u32::try_from(row.attempts).unwrap_or(0),
        created_at: row.created_at,
        started_at: row.started_at,
        finished_at: row.finished_at,
        expires_at: row.expires_at,
    })
}

/// Loads a job the caller may see.
pub(crate) async fn visible_job(
    state: &AppState,
    caller: &Caller,
    raw_id: &str,
) -> ApiResult<JobRow> {
    let id = parse_id(raw_id, "job")?;
    db::get_job(&state.db, id)
        .await?
        .filter(|row| caller.principal.can_access(row.workspace_id))
        .ok_or_else(|| ApiError::not_found("job"))
}

/// Create a conversion job.
///
/// Plans the route for the upload's format, checks the plan's quotas, and
/// queues the job for the worker pack that runs the route.
#[utoipa::path(
    post,
    path = "/v1/jobs",
    tag = "jobs",
    request_body = CreateJobRequest,
    responses(
        (status = 202, description = "Job queued", body = Job,
         headers(("location" = String, description = "URL of the job"))),
        (status = 401, description = "Credentials rejected", body = ErrorBody),
        (status = 404, description = "No such upload", body = ErrorBody),
        (status = 409, description = "The upload is not complete or no longer available", body = ErrorBody),
        (status = 413, description = "Input exceeds the conversion limit", body = ErrorBody),
        (status = 422, description = "Invalid request or unsupported route (`unsupported_route`)", body = ErrorBody),
        (status = 429, description = "Rate limited or quota exceeded", body = ErrorBody),
        (status = 500, description = "Internal service error", body = ErrorBody),
        (status = 503, description = "Identity verification is unavailable", body = ErrorBody),
    ),
    security((), ("bearer" = []), ("api_key" = [])),
)]
pub async fn create_job(
    State(state): State<AppState>,
    caller: Caller,
    ApiJson(request): ApiJson<CreateJobRequest>,
) -> ApiResult<Response> {
    let target = routing::resolve_format(&request.to).ok_or_else(|| {
        ApiError::invalid(format!("unknown target format '{}'", request.to.trim()))
    })?;
    let upload = db::get_upload(&state.db, request.upload_id)
        .await?
        .filter(|row| row.workspace_id == caller.principal.workspace_id)
        .ok_or_else(|| ApiError::not_found("upload"))?;
    match upload.status() {
        UploadStatus::Completed => {}
        UploadStatus::Pending => return Err(ApiError::conflict("the upload is not complete")),
        UploadStatus::Deleted | UploadStatus::Expired => {
            return Err(ApiError::conflict("the upload is no longer available"));
        }
    }
    let source = upload
        .format
        .as_deref()
        .and_then(routing::resolve_format)
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                "unsupported_route",
                "the upload's format is not recognized",
            )
        })?;
    let plan =
        routing::plan(source, target, request.profile.into(), &state.packs).map_err(|detail| {
            ApiError::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                "unsupported_route",
                detail,
            )
        })?;
    let pack = Pack::for_route(&plan.steps);
    let input_bytes = u64::try_from(upload.size_bytes).unwrap_or(0);
    if let Some(max) = caller.entitlement.limits.max_input_bytes
        && input_bytes > max
    {
        return Err(ApiError::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "limit_exceeded",
            format!("the plan accepts files up to {max} bytes"),
        ));
    }
    if matches!(
        source,
        ariad_core::format::Format::Markdown | ariad_core::format::Format::Html
    ) && input_bytes
        > ariad_host::convert::isolated::native_input_cap(&caller.entitlement.limits)
    {
        return Err(ApiError::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "limit_exceeded",
            "the native input exceeds the text size limit",
        ));
    }

    let id = Uuid::new_v4();
    let subject = caller.principal.subject.clone();
    let limits =
        serde_json::to_value(&caller.entitlement.limits).map_err(|err| ApiError::internal(&err))?;
    let retention_s = i64::try_from(caller.entitlement.retention.as_secs()).unwrap_or(i64::MAX);
    let steps: Vec<(String, String, String)> = plan
        .steps
        .iter()
        .map(|s| {
            (
                s.from.id().to_owned(),
                s.to.id().to_owned(),
                s.engine.clone(),
            )
        })
        .collect();

    let mut tx = state.db.begin().await?;
    db::lock_subject(&mut tx, &subject).await?;
    let usage = crate::entitlements::usage(&mut tx, &subject).await?;
    if let Err(violation) = caller.entitlement.admit(usage, input_bytes) {
        return Err(ApiError::quota_exceeded(violation.to_string()));
    }
    let row = db::insert_job(
        &mut tx,
        &NewJob {
            id,
            workspace_id: caller.principal.workspace_id,
            subject: &subject,
            upload_id: upload.id,
            source_format: source.id(),
            target_format: target.id(),
            profile: request.profile.as_str(),
            pack,
            limits,
            retention_s,
        },
    )
    .await?;
    db::set_job_steps(&mut tx, id, &steps).await?;
    db::add_event(&mut tx, id, "queued", json!({ "pack": pack })).await?;
    db::record_usage(
        &mut tx,
        &UsageEvent {
            event_key: format!("job:{id}:submitted"),
            workspace_id: caller.principal.workspace_id,
            subject: &subject,
            job_id: Some(id),
            kind: "job_submitted",
            input_bytes: upload.size_bytes,
            output_bytes: 0,
            pages: 0,
            engine_ms: 0,
        },
    )
    .await?;
    state
        .queue
        .send(
            &mut tx,
            pack,
            &JobMessage { job_id: id },
            &telemetry::inject_current(),
        )
        .await?;
    tx.commit().await?;
    tracing::info!(job_id = %id, upload_id = %upload.id, from = source.id(), to = target.id(), %pack, "job queued");

    let body = job_dto(&state, &row).await?;
    let mut response = (StatusCode::ACCEPTED, Json(body)).into_response();
    if let Ok(location) = HeaderValue::from_str(&format!("/v1/jobs/{id}")) {
        response.headers_mut().insert(header::LOCATION, location);
    }
    Ok(response)
}

/// Get a job.
#[utoipa::path(
    get,
    path = "/v1/jobs/{id}",
    tag = "jobs",
    params(("id" = Uuid, Path, description = "Job id")),
    responses(
        (status = 200, description = "The job", body = Job),
        (status = 401, description = "Credentials rejected", body = ErrorBody),
        (status = 404, description = "No such job", body = ErrorBody),
        (status = 429, description = "Rate limited", body = ErrorBody),
        (status = 500, description = "Internal service error", body = ErrorBody),
        (status = 503, description = "Identity verification is unavailable", body = ErrorBody),
    ),
    security((), ("bearer" = []), ("api_key" = [])),
)]
pub async fn get_job(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<String>,
) -> ApiResult<Json<Job>> {
    let row = visible_job(&state, &caller, &id).await?;
    Ok(Json(job_dto(&state, &row).await?))
}

/// Delete a job's files now.
///
/// Cancels the job if it is still queued or running, and deletes its input
/// upload and outputs at once. Other jobs created from the same upload lose
/// their input.
#[utoipa::path(
    delete,
    path = "/v1/jobs/{id}",
    tag = "jobs",
    params(("id" = Uuid, Path, description = "Job id")),
    responses(
        (status = 204, description = "Files deleted"),
        (status = 401, description = "Credentials rejected", body = ErrorBody),
        (status = 404, description = "No such job", body = ErrorBody),
        (status = 429, description = "Rate limited", body = ErrorBody),
        (status = 500, description = "Internal service error", body = ErrorBody),
        (status = 503, description = "Identity verification is unavailable", body = ErrorBody),
    ),
    security((), ("bearer" = []), ("api_key" = [])),
)]
pub async fn delete_job(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    let row = visible_job(&state, &caller, &id).await?;
    purge_job(&state.db, &state.storage, row.id, Purge::OnRequest).await?;
    tracing::info!(job_id = %row.id, "job deleted on request");
    Ok(StatusCode::NO_CONTENT)
}

/// Why a job's files are removed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Purge {
    /// `DELETE /v1/jobs/{id}`: the input upload goes too.
    OnRequest,
    /// The retention period passed; uploads expire on their own schedule.
    Retention,
}

/// Cancels an active job, deletes its outputs (and, on request, its input),
/// and marks it expired. Shared by `DELETE /v1/jobs/{id}` and the sweeper.
pub async fn purge_job(
    pool: &sqlx::PgPool,
    storage: &crate::storage::Storage,
    job_id: Uuid,
    purge: Purge,
) -> ApiResult<()> {
    let reason = match purge {
        Purge::OnRequest => "deleted",
        Purge::Retention => "retention",
    };
    let mut tx = pool.begin().await?;
    if db::cancel_job(&mut tx, job_id).await? {
        db::add_event(&mut tx, job_id, "canceled", json!({ "reason": reason })).await?;
    }
    tx.commit().await?;

    let Some(job) = db::get_job(pool, job_id).await? else {
        return Ok(());
    };
    if purge == Purge::OnRequest
        && let Some(upload) = db::get_upload(pool, job.upload_id).await?
    {
        storage
            .delete(&upload.storage_key)
            .await
            .map_err(|err| ApiError::internal(&err))?;
        let mut conn = pool.acquire().await?;
        db::retire_upload(&mut conn, upload.id, UploadStatus::Deleted).await?;
    }
    for artifact in db::live_artifacts(pool, job_id).await? {
        storage
            .delete(&artifact.storage_key)
            .await
            .map_err(|err| ApiError::internal(&err))?;
    }
    let mut tx = pool.begin().await?;
    db::mark_artifacts_deleted(&mut tx, job_id).await?;
    if db::expire_job(&mut tx, job_id).await? {
        db::add_event(&mut tx, job_id, "expired", json!({ "reason": reason })).await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Get download URLs for a job's outputs.
///
/// Each URL is valid for five minutes; call again for fresh URLs.
#[utoipa::path(
    get,
    path = "/v1/jobs/{id}/outputs",
    tag = "jobs",
    params(("id" = Uuid, Path, description = "Job id")),
    responses(
        (status = 200, description = "Output files", body = Outputs),
        (status = 401, description = "Credentials rejected", body = ErrorBody),
        (status = 404, description = "No such job", body = ErrorBody),
        (status = 409, description = "The job has not succeeded", body = ErrorBody),
        (status = 410, description = "The outputs were deleted", body = ErrorBody),
        (status = 429, description = "Rate limited", body = ErrorBody),
        (status = 500, description = "Internal service error", body = ErrorBody),
        (status = 503, description = "Identity verification is unavailable", body = ErrorBody),
    ),
    security((), ("bearer" = []), ("api_key" = [])),
)]
pub async fn job_outputs(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<String>,
) -> ApiResult<Json<Outputs>> {
    let row = visible_job(&state, &caller, &id).await?;
    match row.status() {
        JobStatus::Succeeded => {}
        JobStatus::Expired => {
            return Err(ApiError::new(
                StatusCode::GONE,
                "expired",
                "the job's files were deleted",
            ));
        }
        status => {
            return Err(ApiError::conflict(format!(
                "the job is {status}; outputs exist only for succeeded jobs"
            )));
        }
    }
    let mut outputs = Vec::new();
    for artifact in db::live_artifacts(&state.db, row.id).await? {
        let (url, expires_at) = state
            .storage
            .presign_get(&artifact.storage_key)
            .await
            .map_err(|err| ApiError::internal(&err))?;
        outputs.push(OutputFile {
            id: artifact.id,
            filename: artifact.filename,
            format: artifact.format,
            media_type: artifact.media_type,
            size: u64::try_from(artifact.size_bytes).unwrap_or(0),
            sha256: artifact.sha256,
            url: url.to_string(),
            expires_at,
        });
    }
    Ok(Json(Outputs {
        job_id: row.id,
        outputs,
    }))
}
