//! `POST /v1/plan` and `GET /v1/capabilities`.

use std::collections::BTreeMap;

use ariad_core::planner;
use axum::{
    Json,
    extract::State,
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};

use super::{ApiJson, AppState, Caller};
use crate::{
    api::dto::{
        Capabilities, CapabilityEdge, EdgeMetrics, FormatInfo, PlanAlternative, PlanRequest,
        PlanResponse, RouteScore, RouteStep,
    },
    db::{self, UploadStatus},
    error::{ApiError, ApiResult, ErrorBody},
    packs::Pack,
    routing,
};

/// Plan a conversion without running it.
///
/// Returns the route the planner picks for the deployment's worker packs, its
/// benchmark scores, and up to two alternatives.
#[utoipa::path(
    post,
    path = "/v1/plan",
    tag = "planning",
    request_body = PlanRequest,
    responses(
        (status = 200, description = "The planned route", body = PlanResponse),
        (status = 401, description = "Credentials rejected", body = ErrorBody),
        (status = 404, description = "No such upload", body = ErrorBody),
        (status = 409, description = "The upload is not complete", body = ErrorBody),
        (status = 422, description = "Invalid request or unsupported route (`unsupported_route`)", body = ErrorBody),
        (status = 429, description = "Rate limited", body = ErrorBody,
         headers(("retry-after" = u64, description = "Seconds to wait before retrying; sent with the `rate_limited` code"))),
        (status = 500, description = "Internal service error", body = ErrorBody),
        (status = 503, description = "Identity verification is unavailable", body = ErrorBody),
    ),
    security((), ("bearer" = []), ("api_key" = [])),
)]
pub async fn plan(
    State(state): State<AppState>,
    caller: Caller,
    ApiJson(request): ApiJson<PlanRequest>,
) -> ApiResult<Json<PlanResponse>> {
    let source = match (&request.from, request.upload_id) {
        (Some(name), None) => routing::resolve_format(name)
            .ok_or_else(|| ApiError::invalid(format!("unknown source format '{}'", name.trim())))?,
        (None, Some(upload_id)) => {
            let upload = db::get_upload(&state.db, upload_id)
                .await?
                .filter(|row| caller.principal.can_access(row.workspace_id))
                .ok_or_else(|| ApiError::not_found("upload"))?;
            if upload.status() != UploadStatus::Completed {
                return Err(ApiError::conflict("the upload is not complete"));
            }
            upload
                .format
                .as_deref()
                .and_then(routing::resolve_format)
                .ok_or_else(|| {
                    ApiError::new(
                        StatusCode::UNPROCESSABLE_ENTITY,
                        "unsupported_route",
                        "the upload's format is not recognized",
                    )
                })?
        }
        _ => return Err(ApiError::invalid("give exactly one of from and upload_id")),
    };
    let target = routing::resolve_format(&request.to).ok_or_else(|| {
        ApiError::invalid(format!("unknown target format '{}'", request.to.trim()))
    })?;
    let plan =
        routing::plan(source, target, request.profile.into(), &state.packs).map_err(|detail| {
            ApiError::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                "unsupported_route",
                detail,
            )
        })?;
    let pack = super::route_pack(&plan.steps)?;
    super::check_pack_memory(pack, &caller.entitlement.limits)?;
    let alternatives = plan
        .alternatives
        .iter()
        .map(|alt| {
            Ok(PlanAlternative {
                route: alt.steps.iter().map(RouteStep::from).collect(),
                score: RouteScore::from(&alt.score),
                measured: alt.measured,
                pack: super::route_pack(&alt.steps)?,
            })
        })
        .collect::<Result<Vec<_>, ApiError>>()?;
    Ok(Json(PlanResponse {
        from: source.id().to_owned(),
        to: target.id().to_owned(),
        profile: request.profile,
        pack,
        route: plan.steps.iter().map(RouteStep::from).collect(),
        score: RouteScore::from(&plan.score),
        measured: plan.measured,
        alternatives,
    }))
}

/// List formats and conversion edges.
///
/// Only edges whose engines run on this deployment's worker packs are listed.
#[utoipa::path(
    get,
    path = "/v1/capabilities",
    tag = "planning",
    responses((status = 200, description = "Formats and edges", body = Capabilities)),
)]
pub async fn capabilities(State(state): State<AppState>) -> Response {
    let caps = planner::embedded();
    let supported = ariad_host::convert::executor_edges();
    let engines = routing::deployable_engines(caps, &state.packs);
    let mut formats = BTreeMap::new();
    let edges: Vec<CapabilityEdge> = caps
        .edges
        .iter()
        .filter(|edge| {
            engines.contains(&edge.engine)
                && supported.contains(&(edge.from, edge.to, edge.engine.as_str()))
        })
        .filter_map(|edge| {
            // `deployable_engines` only lists engines with a pack; skip anything unmapped.
            let pack = Pack::for_engine(&edge.engine).ok()?;
            for format in [edge.from, edge.to] {
                if routing::resolve_format(format.id()).is_none() {
                    continue;
                }
                formats
                    .entry(format.id())
                    .or_insert_with(|| FormatInfo::from(format));
            }
            Some(CapabilityEdge {
                from: edge.from.id().to_owned(),
                to: edge.to.id().to_owned(),
                engine: edge.engine.clone(),
                pack,
                license: edge.license.clone(),
                metrics: edge.metrics.map(|m| EdgeMetrics {
                    fidelity: m.fidelity,
                    editability: m.editability,
                    p50_ms: m.p50_ms,
                    peak_mem_mb: m.peak_mem_mb,
                    samples: m.samples,
                }),
            })
        })
        .collect();
    let body = Capabilities {
        version: caps.version.clone(),
        generated_at: caps.generated_at.clone(),
        packs: state.packs.clone(),
        formats: formats.into_values().collect(),
        edges,
    };
    let mut response = Json(body).into_response();
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=300"),
    );
    response
}
