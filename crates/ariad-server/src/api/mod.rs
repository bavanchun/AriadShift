//! The `/v1` HTTP API.

pub mod blobs;
pub mod dto;
pub mod events;
pub mod health;
pub mod jobs;
pub mod plan;
pub mod uploads;

use std::{
    net::SocketAddr,
    ops::Deref,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use axum::{
    Router,
    extract::{ConnectInfo, DefaultBodyLimit, FromRequest, FromRequestParts, Request},
    http::{HeaderName, HeaderValue, Method, StatusCode, request::Parts},
};
use serde::de::DeserializeOwned;
use sqlx::PgPool;
use tower_http::{
    compression::CompressionLayer,
    cors::{AllowOrigin, CorsLayer},
    limit::RequestBodyLimitLayer,
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    timeout::TimeoutLayer,
    trace::TraceLayer,
};
use utoipa::{
    Modify, OpenApi,
    openapi::security::{ApiKey, ApiKeyValue, HttpAuthScheme, HttpBuilder, SecurityScheme},
};
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;

use crate::{
    auth::{Authenticator, Principal},
    entitlements::{self, Entitlement},
    error::{ApiError, ErrorBody},
    packs::Pack,
    queue::JobQueue,
    rate_limit,
    storage::Storage,
    telemetry::Metrics,
};

/// Rejected credentials allowed per client address and minute.
pub const AUTH_FAILURES_PER_MINUTE: u32 = 30;

/// Largest JSON request body.
pub const JSON_BODY_LIMIT: usize = 1024 * 1024;

/// Refuse a heavy route before it consumes queue/quota capacity. This is an
/// admission floor, not a guarantee that every document fits the budget.
pub fn check_pack_memory(pack: Pack, limits: &ariad_core::limits::Limits) -> Result<(), ApiError> {
    if limits
        .max_memory_mb
        .is_some_and(|limit| limit < pack.minimum_memory_mb())
    {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "limit_exceeded",
            format!(
                "the {pack} pack requires a plan memory limit of at least {} MB",
                pack.minimum_memory_mb()
            ),
        ));
    }
    Ok(())
}

/// Shared state of the API.
#[derive(Clone)]
pub struct AppState(Arc<Inner>);

pub struct Inner {
    pub db: PgPool,
    pub storage: Storage,
    pub queue: Arc<dyn JobQueue>,
    pub auth: Authenticator,
    pub events: events::EventHub,
    /// Packs with workers; the planner only routes through them.
    pub packs: Vec<Pack>,
    pub metrics: Metrics,
}

impl AppState {
    #[must_use]
    pub fn new(inner: Inner) -> Self {
        Self(Arc::new(inner))
    }
}

impl Deref for AppState {
    type Target = Inner;

    fn deref(&self) -> &Inner {
        &self.0
    }
}

/// The authenticated, rate-limited caller with its plan's entitlement.
pub struct Caller {
    pub principal: Principal,
    pub entitlement: Entitlement,
    pub peer: Option<SocketAddr>,
}

impl FromRequestParts<AppState> for Caller {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, ApiError> {
        let peer = peer_addr(parts);
        let client = state.auth.client_hash(&parts.headers, peer);
        let auth_bucket = format!("auth-failure:{client}");
        if (parts
            .headers
            .contains_key(axum::http::header::AUTHORIZATION)
            || parts.headers.contains_key(crate::auth::API_KEY_HEADER))
            && let rate_limit::Decision::Limited { retry_after_s } =
                rate_limit::check(&state.db, &auth_bucket, AUTH_FAILURES_PER_MINUTE).await?
        {
            return Err(ApiError::rate_limited(retry_after_s));
        }
        let principal = match state
            .auth
            .authenticate(&state.db, &parts.headers, peer)
            .await
        {
            Ok(principal) => principal,
            Err(err) if err.status() == StatusCode::UNAUTHORIZED => {
                if let rate_limit::Decision::Limited { retry_after_s } =
                    rate_limit::hit(&state.db, &auth_bucket, AUTH_FAILURES_PER_MINUTE).await?
                {
                    return Err(ApiError::rate_limited(retry_after_s));
                }
                return Err(err);
            }
            Err(err) => return Err(err),
        };
        let entitlement = entitlements::load(&state.db, &principal.plan)
            .await
            .map_err(|err| ApiError::internal(&err))?;
        if let Some(limit) = entitlement.requests_per_minute {
            let bucket = format!("rl:{}", principal.subject);
            if let rate_limit::Decision::Limited { retry_after_s } =
                rate_limit::hit(&state.db, &bucket, limit).await?
            {
                return Err(ApiError::rate_limited(retry_after_s));
            }
        }
        Ok(Self {
            principal,
            entitlement,
            peer,
        })
    }
}

/// The peer address, when the server was started with connect info.
#[must_use]
pub fn peer_addr(parts: &Parts) -> Option<SocketAddr> {
    parts
        .extensions
        .get::<ConnectInfo<SocketAddr>>()
        .map(|info| info.0)
}

/// JSON body extractor whose rejections use the API's error shape.
pub struct ApiJson<T>(pub T);

impl<S, T> FromRequest<S> for ApiJson<T>
where
    S: Send + Sync,
    T: DeserializeOwned,
{
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &S) -> Result<Self, ApiError> {
        match axum::Json::<T>::from_request(req, state).await {
            Ok(axum::Json(value)) => Ok(Self(value)),
            Err(rejection) => Err(ApiError::invalid(rejection.body_text())),
        }
    }
}

/// Parses a path id; anything that is not a UUID cannot exist.
pub fn parse_id(raw: &str, what: &str) -> Result<Uuid, ApiError> {
    Uuid::parse_str(raw).map_err(|_| ApiError::not_found(what))
}

#[derive(OpenApi)]
#[openapi(
    info(
        title = "AriadShift API",
        version = env!("CARGO_PKG_VERSION"),
        description = "Convert documents in the AriadShift cloud. Upload a file with one \
                       presigned PUT, create a job, follow its events, and download the outputs \
                       through short-lived URLs. Inputs and outputs are deleted after the plan's \
                       retention period (one hour by default).",
        license(name = "Apache-2.0", identifier = "Apache-2.0"),
    ),
    servers((url = "/", description = "This deployment")),
    modifiers(&SecuritySchemes),
    components(schemas(ErrorBody)),
    tags(
        (name = "uploads", description = "Send input files"),
        (name = "jobs", description = "Run and follow conversions"),
        (name = "planning", description = "Routes and capabilities"),
        (name = "blobs", description = "Presigned file transfers of the local object store"),
        (name = "health", description = "Liveness and readiness"),
    ),
)]
struct ApiDoc;

struct SecuritySchemes;

impl Modify for SecuritySchemes {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let components = openapi.components.get_or_insert_with(Default::default);
        components.add_security_scheme(
            "bearer",
            SecurityScheme::Http(
                HttpBuilder::new()
                    .scheme(HttpAuthScheme::Bearer)
                    .bearer_format("JWT")
                    .description(Some(
                        "A JWT issued by the AriadShift auth service (Better Auth JWT plugin, \
                         15-minute lifetime). The API verifies its signature against the auth \
                         service's JWKS, selecting the key by `kid` and accepting only the \
                         asymmetric algorithm that key declares, and requires `exp`, `iss`, \
                         `aud` and a non-empty `sub` (the user id).",
                    ))
                    .build(),
            ),
        );
        components.add_security_scheme(
            "api_key",
            SecurityScheme::ApiKey(ApiKey::Header(ApiKeyValue::with_description(
                crate::auth::API_KEY_HEADER,
                "An API key issued by the AriadShift auth service (Better Auth API-key \
                 plugin). Disabled, expired or exhausted keys are refused.",
            ))),
        );
    }
}

/// Routes with their OpenAPI operations.
fn openapi_router(request_timeout: Duration) -> OpenApiRouter<AppState> {
    let bounded = OpenApiRouter::new()
        .routes(routes!(uploads::create_upload))
        .routes(routes!(plan::plan))
        .routes(routes!(plan::capabilities))
        .routes(routes!(jobs::create_job))
        .routes(routes!(jobs::get_job, jobs::delete_job))
        .routes(routes!(jobs::job_outputs))
        .routes(routes!(health::healthz))
        .routes(routes!(health::readyz))
        .layer(RequestBodyLimitLayer::new(JSON_BODY_LIMIT))
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            request_timeout,
        ));
    // Hashing a large upload and streaming events outlive the request timeout.
    let streaming = OpenApiRouter::new()
        .routes(routes!(uploads::complete_upload))
        .routes(routes!(events::job_events))
        .layer(RequestBodyLimitLayer::new(JSON_BODY_LIMIT));
    // Storage enforces signed sizes, transfer admission and body deadlines.
    // Skip compression so `content-length` survives on streaming responses.
    let blobs = OpenApiRouter::new()
        .routes(routes!(blobs::put_blob, blobs::get_blob))
        .layer(DefaultBodyLimit::disable());
    OpenApiRouter::with_openapi(ApiDoc::openapi())
        .merge(bounded)
        .merge(streaming)
        .layer(CompressionLayer::new())
        .merge(blobs)
}

/// The OpenAPI 3.1 document of the `/v1` API.
#[must_use]
pub fn openapi() -> utoipa::openapi::OpenApi {
    openapi_router(Duration::from_secs(30)).into_openapi()
}

/// The committed form of [`openapi`]: pretty JSON with a trailing newline.
pub fn openapi_json() -> Result<String, serde_json::Error> {
    let mut json = openapi().to_pretty_json()?;
    json.push('\n');
    Ok(json)
}

/// Where the committed OpenAPI document lives in the source tree.
#[must_use]
pub fn openapi_document_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/openapi.v1.json")
}

/// Settings for [`router`].
pub struct RouterOptions {
    pub cors_origins: Vec<String>,
    pub request_timeout: Duration,
}

/// The complete HTTP service.
pub fn router(state: AppState, options: &RouterOptions) -> Router {
    let (api, _) = openapi_router(options.request_timeout).split_for_parts();
    let request_id = HeaderName::from_static("x-request-id");
    api.layer(cors(&options.cors_origins))
        .layer(PropagateRequestIdLayer::new(request_id.clone()))
        .layer(TraceLayer::new_for_http().make_span_with(|req: &Request| {
            let id = req
                .headers()
                .get("x-request-id")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");
            tracing::info_span!(
                "http",
                method = %req.method(),
                route = %req.uri().path().split('/').take(3).collect::<Vec<_>>().join("/"),
                request_id = %id,
            )
        }))
        .layer(SetRequestIdLayer::new(request_id, MakeRequestUuid))
        .with_state(state)
}

fn cors(origins: &[String]) -> CorsLayer {
    let origins: Vec<HeaderValue> = origins
        .iter()
        .filter_map(|o| HeaderValue::from_str(o).ok())
        .collect();
    CorsLayer::new()
        .allow_origin(AllowOrigin::list(origins))
        .allow_methods([Method::GET, Method::POST, Method::DELETE, Method::PUT])
        .allow_headers([
            axum::http::header::AUTHORIZATION,
            axum::http::header::CONTENT_TYPE,
            HeaderName::from_static(crate::auth::API_KEY_HEADER),
            HeaderName::from_static(crate::auth::TURNSTILE_HEADER),
            HeaderName::from_static("last-event-id"),
        ])
        .expose_headers([
            HeaderName::from_static("x-request-id"),
            axum::http::header::RETRY_AFTER,
        ])
        .max_age(Duration::from_secs(600))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openapi_lists_every_endpoint() {
        let doc = openapi();
        let paths: Vec<&str> = doc.paths.paths.keys().map(String::as_str).collect();
        for path in [
            "/v1/uploads",
            "/v1/uploads/{id}/complete",
            "/v1/plan",
            "/v1/jobs",
            "/v1/jobs/{id}",
            "/v1/jobs/{id}/events",
            "/v1/jobs/{id}/outputs",
            "/v1/capabilities",
            "/v1/blobs/{token}",
            "/healthz",
            "/readyz",
        ] {
            assert!(paths.contains(&path), "missing {path}; have {paths:?}");
        }
    }

    #[test]
    fn blob_routes_carry_no_api_credentials() {
        let doc = openapi();
        let item = &doc.paths.paths["/v1/blobs/{token}"];
        for operation in [item.get.as_ref(), item.put.as_ref()] {
            let operation = operation.expect("blob GET and PUT are documented");
            assert!(operation.security.is_none(), "the token is the credential");
        }
    }

    #[test]
    fn openapi_json_ends_with_a_newline() {
        let json = openapi_json().expect("serialize");
        assert!(json.ends_with("}\n"));
    }
}
