//! `POST /v1/uploads` and `POST /v1/uploads/{id}/complete`.

use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
};
use uuid::Uuid;

use super::{ApiJson, AppState, Caller, parse_id};
use crate::{
    api::dto::{CreateUploadRequest, PresignedRequest, Upload},
    db::{self, NewUpload, UploadRow, UploadStatus},
    error::{ApiError, ApiResult, ErrorBody},
    routing,
    storage::{Direction, Presigned, StorageError, UPLOAD_URL_TTL},
    telemetry::name_hash,
};

/// Longest accepted file name, in characters.
const MAX_FILENAME_CHARS: usize = 255;

/// Keeps the last path component and rejects control characters.
fn clean_filename(raw: &str) -> Result<String, ApiError> {
    let name = raw
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
        .trim()
        .to_owned();
    if name.is_empty() || name == "." || name == ".." {
        return Err(ApiError::invalid("filename must name a file"));
    }
    if name.chars().count() > MAX_FILENAME_CHARS {
        return Err(ApiError::invalid(format!(
            "filename is longer than {MAX_FILENAME_CHARS} characters"
        )));
    }
    if name.chars().any(char::is_control) {
        return Err(ApiError::invalid("filename contains control characters"));
    }
    Ok(name)
}

fn clean_sha256(raw: &str) -> Result<String, ApiError> {
    let sha = raw.trim().to_ascii_lowercase();
    if sha.len() == 64 && sha.bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(sha)
    } else {
        Err(ApiError::invalid(
            "sha256 must be 64 hexadecimal characters",
        ))
    }
}

pub(crate) fn upload_dto(row: &UploadRow, presigned: Option<Presigned>) -> Upload {
    Upload {
        id: row.id,
        status: row.status(),
        filename: row.filename.clone(),
        size: u64::try_from(row.size_bytes).unwrap_or(0),
        sha256: row.sha256.clone(),
        format: row.format.clone(),
        created_at: row.created_at,
        expires_at: row.expires_at,
        upload: presigned.map(|p| PresignedRequest {
            method: p.method.to_owned(),
            url: p.url.to_string(),
            headers: p.headers,
            expires_at: p.expires_at,
        }),
    }
}

/// Start an upload.
///
/// Returns one presigned `PUT` request. Send the file body with the returned
/// headers, then call `complete`. Anonymous callers may need a Turnstile token.
#[utoipa::path(
    post,
    path = "/v1/uploads",
    tag = "uploads",
    request_body = CreateUploadRequest,
    params(
        ("cf-turnstile-response" = Option<String>, Header,
         description = "Cloudflare Turnstile token; required for anonymous callers when the deployment enables Turnstile"),
    ),
    responses(
        (status = 201, description = "Upload created", body = Upload),
        (status = 401, description = "Credentials rejected", body = ErrorBody),
        (status = 403, description = "Turnstile check failed", body = ErrorBody),
        (status = 413, description = "File exceeds the plan's input limit", body = ErrorBody),
        (status = 422, description = "Invalid request", body = ErrorBody),
        (status = 429, description = "Rate limited or upload quota exceeded", body = ErrorBody),
        (status = 500, description = "Internal service error", body = ErrorBody),
        (status = 503, description = "Identity verification is unavailable", body = ErrorBody),
    ),
    security((), ("bearer" = []), ("api_key" = [])),
)]
pub async fn create_upload(
    State(state): State<AppState>,
    caller: Caller,
    headers: HeaderMap,
    ApiJson(request): ApiJson<CreateUploadRequest>,
) -> ApiResult<(StatusCode, Json<Upload>)> {
    let filename = clean_filename(&request.filename)?;
    let sha256 = clean_sha256(&request.sha256)?;
    if request.size == 0 {
        return Err(ApiError::invalid("size must be greater than zero"));
    }
    if let Some(max) = caller.entitlement.limits.max_input_bytes
        && request.size > max
    {
        return Err(ApiError::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "limit_exceeded",
            format!("the plan accepts files up to {max} bytes"),
        ));
    }
    let size = i64::try_from(request.size).map_err(|_| ApiError::too_large("size is too large"))?;
    if caller.principal.is_anonymous() {
        state.auth.check_turnstile(&headers, caller.peer).await?;
    }

    let id = Uuid::new_v4();
    let key = state
        .storage
        .key(caller.principal.workspace_id, id, Direction::In, &sha256);
    let presigned = state
        .storage
        .presign_put(&key, request.size, &sha256)
        .await
        .map_err(|err| ApiError::internal(&err))?;
    let mut tx = state.db.begin().await?;
    db::lock_subject(&mut tx, &caller.principal.subject).await?;
    crate::entitlements::admit_upload(
        &mut tx,
        &caller.principal.subject,
        &caller.entitlement,
        request.size,
    )
    .await?;
    let retention_s = i64::try_from(UPLOAD_URL_TTL.as_secs() + 300).unwrap_or(i64::MAX);
    let row = db::insert_upload(
        &mut tx,
        &NewUpload {
            id,
            workspace_id: caller.principal.workspace_id,
            subject: &caller.principal.subject,
            filename: &filename,
            size_bytes: size,
            sha256: &sha256,
            storage_key: &key,
            retention_s,
        },
    )
    .await?;
    tx.commit().await?;
    tracing::info!(upload_id = %id, name = %name_hash(&filename), size, "upload created");
    Ok((StatusCode::CREATED, Json(upload_dto(&row, Some(presigned)))))
}

/// Complete an upload.
///
/// Checks that the stored file has the declared size and SHA-256 and detects
/// its format. A mismatching file is deleted.
#[utoipa::path(
    post,
    path = "/v1/uploads/{id}/complete",
    tag = "uploads",
    params(("id" = Uuid, Path, description = "Upload id")),
    responses(
        (status = 200, description = "Upload verified", body = Upload),
        (status = 401, description = "Credentials rejected", body = ErrorBody),
        (status = 404, description = "No such upload", body = ErrorBody),
        (status = 409, description = "The file is missing, or the upload is no longer live", body = ErrorBody),
        (status = 422, description = "The file does not match the declared size or hash", body = ErrorBody),
        (status = 429, description = "Rate limited", body = ErrorBody),
        (status = 500, description = "Internal service error", body = ErrorBody),
        (status = 503, description = "Identity verification is unavailable", body = ErrorBody),
    ),
    security((), ("bearer" = []), ("api_key" = [])),
)]
pub async fn complete_upload(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<String>,
) -> ApiResult<Json<Upload>> {
    let id = parse_id(&id, "upload")?;
    let row = db::get_upload(&state.db, id)
        .await?
        .filter(|row| caller.principal.can_access(row.workspace_id))
        .ok_or_else(|| ApiError::not_found("upload"))?;
    if row.expires_at <= chrono::Utc::now() {
        return Err(ApiError::conflict("the upload has expired"));
    }
    match row.status() {
        UploadStatus::Pending => {}
        UploadStatus::Completed => return Ok(Json(upload_dto(&row, None))),
        UploadStatus::Deleted | UploadStatus::Expired => {
            return Err(ApiError::conflict("the upload is no longer available"));
        }
    }
    let declared = u64::try_from(row.size_bytes).unwrap_or(0);
    match state.storage.size(&row.storage_key).await {
        Ok(Some(size)) if size == declared => {}
        Ok(Some(_)) => return Err(reject(&state, &row, "the file size does not match").await),
        Ok(None) => return Err(ApiError::conflict("the file has not been uploaded yet")),
        Err(err) => return Err(ApiError::internal(&err)),
    }
    let digested = match state.storage.digest(&row.storage_key, declared).await {
        Ok(digested) => digested,
        Err(StorageError::TooLarge(_)) => {
            return Err(reject(&state, &row, "the file size does not match").await);
        }
        Err(err) => return Err(ApiError::internal(&err)),
    };
    if digested.size != declared || digested.sha256 != row.sha256 {
        return Err(reject(&state, &row, "the file does not match the declared SHA-256").await);
    }
    let format = routing::detect_format(&digested.prefix, &row.filename);
    let retention_s = i64::try_from(caller.entitlement.retention.as_secs()).unwrap_or(i64::MAX);
    let completed = db::complete_upload(&state.db, id, format.map(|f| f.id()), retention_s).await?;
    let row = match completed {
        Some(row) => row,
        // A concurrent call completed it first.
        None => db::get_upload(&state.db, id)
            .await?
            .filter(|row| row.status() == UploadStatus::Completed)
            .ok_or_else(|| ApiError::conflict("the upload is no longer pending"))?,
    };
    tracing::info!(upload_id = %id, format = ?row.format, "upload completed");
    Ok(Json(upload_dto(&row, None)))
}

/// Deletes a mismatching file and retires the upload.
async fn reject(state: &AppState, row: &UploadRow, message: &str) -> ApiError {
    if let Err(err) = state.storage.delete(&row.storage_key).await {
        return ApiError::internal(&err);
    }
    let mut conn = match state.db.acquire().await {
        Ok(conn) => conn,
        Err(err) => return ApiError::internal(&err),
    };
    if let Err(err) = db::retire_upload(&mut conn, row.id, UploadStatus::Deleted).await {
        return ApiError::internal(&err);
    }
    tracing::info!(upload_id = %row.id, "upload rejected: {message}");
    ApiError::invalid(message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filenames_keep_the_last_component() {
        assert_eq!(
            clean_filename("dir/sub\\report.docx").expect("name"),
            "report.docx"
        );
        assert_eq!(clean_filename("  notes.md ").expect("name"), "notes.md");
        assert!(clean_filename("dir/").is_err());
        assert!(clean_filename("..").is_err());
        assert!(clean_filename("a\u{0}b").is_err());
        assert!(clean_filename(&"x".repeat(256)).is_err());
    }

    #[test]
    fn sha256_must_be_hex() {
        assert_eq!(
            clean_sha256(&"AB".repeat(32)).expect("sha"),
            "ab".repeat(32)
        );
        assert!(clean_sha256("xyz").is_err());
        assert!(clean_sha256(&"g".repeat(64)).is_err());
    }
}
