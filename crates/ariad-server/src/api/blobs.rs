//! `PUT` and `GET /v1/blobs/{token}`: presigned object URLs for the local
//! store, which cannot sign URLs itself. The token is an HMAC-signed claim
//! naming the key, method, expiry, and for uploads the exact size and hash.
//! Clients never build these URLs: they follow the `url` an upload or the
//! outputs list returned. With S3 storage those URLs point at the object
//! store instead, so these routes are documented as the local-store shape.

use axum::{
    body::Body,
    extract::{Path, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use chrono::Utc;

use super::AppState;
use crate::{
    api::dto::FileBytes,
    error::{ApiError, ApiResult, ErrorBody},
    storage::BlobClaims,
};

fn claims(state: &AppState, token: &str, method: &str) -> ApiResult<BlobClaims> {
    state
        .storage
        .verify_blob_token(token, Utc::now())
        .filter(|claims| claims.method == method)
        .ok_or_else(|| ApiError::forbidden("forbidden", "the URL is invalid or has expired"))
}

/// Upload a file through a presigned URL.
///
/// The URL comes from `POST /v1/uploads`; send exactly the declared bytes.
/// The signed token in the path is the only credential.
#[utoipa::path(
    put,
    path = "/v1/blobs/{token}",
    tag = "blobs",
    params(("token" = String, Path, description = "Signed token issued by the API")),
    request_body(
        content = inline(FileBytes),
        content_type = "application/octet-stream",
        description = "The file: exactly the declared size, with the declared SHA-256"
    ),
    responses(
        (status = 200, description = "Stored"),
        (status = 403, description = "The URL is invalid or has expired", body = ErrorBody),
        (status = 422, description = "The body does not match the declared size or SHA-256", body = ErrorBody),
        (status = 500, description = "Internal service error", body = ErrorBody),
    ),
)]
pub async fn put_blob(
    State(state): State<AppState>,
    Path(token): Path<String>,
    headers: HeaderMap,
    body: Body,
) -> ApiResult<StatusCode> {
    let claims = claims(&state, &token, "PUT")?;
    let (Some(size), Some(sha256)) = (claims.size, claims.sha256.as_deref()) else {
        return Err(ApiError::forbidden("forbidden", "the URL is invalid"));
    };
    if let Some(length) = headers
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        && length != size
    {
        return Err(ApiError::invalid(
            "content-length does not match the declared size",
        ));
    }
    let stored = state
        .storage
        .put_stream(&claims.key, body.into_data_stream(), size, sha256)
        .await
        .map_err(|err| ApiError::internal(&err))?;
    if stored {
        Ok(StatusCode::OK)
    } else {
        Err(ApiError::invalid(
            "the body does not match the declared size and SHA-256",
        ))
    }
}

/// Download a file through a presigned URL.
///
/// The URL comes from `GET /v1/jobs/{id}/outputs`. The signed token in the
/// path is the only credential.
#[utoipa::path(
    get,
    path = "/v1/blobs/{token}",
    tag = "blobs",
    params(("token" = String, Path, description = "Signed token issued by the API")),
    responses(
        (status = 200, description = "The file", body = inline(FileBytes),
         content_type = "application/octet-stream",
         headers(("content-length" = u64, description = "File size in bytes"))),
        (status = 403, description = "The URL is invalid or has expired", body = ErrorBody),
        (status = 404, description = "The file was deleted", body = ErrorBody),
        (status = 500, description = "Internal service error", body = ErrorBody),
    ),
)]
pub async fn get_blob(
    State(state): State<AppState>,
    Path(token): Path<String>,
) -> ApiResult<Response> {
    let claims = claims(&state, &token, "GET")?;
    let (size, stream) = match state.storage.get_stream(&claims.key).await {
        Ok(found) => found,
        Err(err) if err.is_not_found() => return Err(ApiError::not_found("object")),
        Err(err) => return Err(ApiError::internal(&err)),
    };
    let mut response = Body::from_stream(stream).into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    if let Ok(length) = HeaderValue::from_str(&size.to_string()) {
        headers.insert(header::CONTENT_LENGTH, length);
    }
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(response)
}
