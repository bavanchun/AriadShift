//! Liveness and readiness probes.

use axum::{Json, extract::State};

use super::AppState;
use crate::{api::dto::Health, error::ErrorBody};

/// Liveness: the process serves HTTP.
#[utoipa::path(
    get,
    path = "/healthz",
    tag = "health",
    responses((status = 200, description = "The process is alive", body = Health)),
)]
pub async fn healthz() -> Json<Health> {
    Json(Health {
        status: "ok".to_owned(),
    })
}

/// Readiness: the database and object storage answer.
#[utoipa::path(
    get,
    path = "/readyz",
    tag = "health",
    responses(
        (status = 200, description = "Ready to serve", body = Health),
        (status = 503, description = "A dependency is unavailable", body = ErrorBody),
    ),
)]
pub async fn readyz(State(state): State<AppState>) -> Result<Json<Health>, crate::error::ApiError> {
    sqlx::query("SELECT 1")
        .execute(&state.db)
        .await
        .map_err(|err| {
            tracing::warn!(error = %err, "readiness: database unavailable");
            crate::error::ApiError::unavailable("the database is unavailable")
        })?;
    state.storage.probe().await.map_err(|err| {
        tracing::warn!(error = %err, "readiness: storage unavailable");
        crate::error::ApiError::unavailable("object storage is unavailable")
    })?;
    Ok(Json(Health {
        status: "ready".to_owned(),
    }))
}
