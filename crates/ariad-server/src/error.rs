//! The API's error type and its JSON shape.

use axum::{
    Json,
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::Serialize;
use utoipa::ToSchema;

/// Body of every error response.
#[derive(Debug, Serialize, ToSchema)]
pub struct ErrorBody {
    pub error: ErrorDetail,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ErrorDetail {
    /// Stable machine-readable code, for example `not_found` or `limit_exceeded`.
    pub code: String,
    /// Human-readable explanation. Never contains document content.
    pub message: String,
}

#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: String,
    retry_after_s: Option<u64>,
}

pub type ApiResult<T> = Result<T, ApiError>;

impl ApiError {
    #[must_use]
    pub fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
            retry_after_s: None,
        }
    }

    #[must_use]
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(StatusCode::UNPROCESSABLE_ENTITY, "invalid_request", message)
    }

    #[must_use]
    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::new(StatusCode::UNAUTHORIZED, "unauthorized", message)
    }

    #[must_use]
    pub fn forbidden(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::FORBIDDEN, code, message)
    }

    #[must_use]
    pub fn not_found(what: &str) -> Self {
        Self::new(
            StatusCode::NOT_FOUND,
            "not_found",
            format!("{what} not found"),
        )
    }

    #[must_use]
    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, "conflict", message)
    }

    #[must_use]
    pub fn too_large(message: impl Into<String>) -> Self {
        Self::new(StatusCode::PAYLOAD_TOO_LARGE, "payload_too_large", message)
    }

    #[must_use]
    pub fn rate_limited(retry_after_s: u64) -> Self {
        Self {
            retry_after_s: Some(retry_after_s.max(1)),
            ..Self::new(
                StatusCode::TOO_MANY_REQUESTS,
                "rate_limited",
                "too many requests; retry later",
            )
        }
    }

    #[must_use]
    pub fn quota_exceeded(message: impl Into<String>) -> Self {
        Self::new(StatusCode::TOO_MANY_REQUESTS, "quota_exceeded", message)
    }

    #[must_use]
    pub fn unavailable(message: impl Into<String>) -> Self {
        Self::new(StatusCode::SERVICE_UNAVAILABLE, "unavailable", message)
    }

    /// An unexpected failure. The cause goes to the log, never to the client.
    #[must_use]
    pub fn internal(cause: &dyn std::fmt::Display) -> Self {
        tracing::error!(error = %cause, "request failed");
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            "internal server error",
        )
    }

    #[must_use]
    pub const fn status(&self) -> StatusCode {
        self.status
    }

    #[must_use]
    pub const fn code(&self) -> &'static str {
        self.code
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = ErrorBody {
            error: ErrorDetail {
                code: self.code.to_owned(),
                message: self.message,
            },
        };
        let mut response = (self.status, Json(body)).into_response();
        if let Some(secs) = self.retry_after_s
            && let Ok(value) = HeaderValue::from_str(&secs.to_string())
        {
            response.headers_mut().insert(header::RETRY_AFTER, value);
        }
        response
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(err: sqlx::Error) -> Self {
        Self::internal(&err)
    }
}

impl From<object_store::Error> for ApiError {
    fn from(err: object_store::Error) -> Self {
        Self::internal(&err)
    }
}
