//! Cloud admission for document tools and imported recipes. Execution stays in ariad-host.

use ariad_core::tools::{
    self, KNOWN_SECRET_FIELD_NAMES, Recipe, RecipeOrigin, RunSize, SecretSource, Step, ToolSpec,
};
use axum::http::StatusCode;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{
    api::{AppState, Caller, dto::Profile},
    db::{self, JobRow, NewJob, UploadRow, UploadStatus, UsageEvent},
    error::{ApiError, ApiResult},
    packs::Pack,
    queue::JobMessage,
    telemetry,
};

/// Internal operation marker; this is not a registered document tool.
pub const RECIPE_JOB: &str = "automation.run_recipe";

pub(crate) fn invalid_params() -> ApiError {
    ApiError::new(
        StatusCode::UNPROCESSABLE_ENTITY,
        "invalid_params",
        "parameters do not match the tool contract or contain unsupported secrets",
    )
}

pub(crate) fn denied() -> ApiError {
    ApiError::new(
        StatusCode::FORBIDDEN,
        "tier_not_allowed",
        "the tool or run size is not allowed by the current plan",
    )
}

pub(crate) fn spec(id: &str) -> ApiResult<&'static ToolSpec> {
    tools::find(id).ok_or_else(|| {
        ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "unknown_tool",
            "unknown document tool",
        )
    })
}

/// Never persist literal secrets, including nested and case-variant known fields.
/// Cloud jobs cannot prompt; short-lived handle resolution is not a durable job input.
pub(crate) fn validate_params(spec: &ToolSpec, params: &Value) -> ApiResult<()> {
    tools::validate_secret_params(spec, params, SecretSource::Recipe)
        .map_err(|_| invalid_params())?;
    tools::validate_secret_fields(KNOWN_SECRET_FIELD_NAMES, params, SecretSource::Recipe)
        .map_err(|_| invalid_params())?;
    if contains_secret_ref(params) {
        return Err(invalid_params());
    }
    ariad_host::tools::validate_params_against_schema(&spec.resolved_params_schema(), params)
        .map_err(|_| invalid_params())
}

fn contains_secret_ref(value: &Value) -> bool {
    match value {
        Value::Object(map) => map
            .iter()
            .any(|(k, v)| k.eq_ignore_ascii_case("$secret") || contains_secret_ref(v)),
        Value::Array(values) => values.iter().any(contains_secret_ref),
        _ => false,
    }
}

pub(crate) fn input_ids(single: Option<Uuid>, many: Option<Vec<Uuid>>) -> ApiResult<Vec<Uuid>> {
    match (single, many) {
        (Some(id), None) => Ok(vec![id]),
        (None, Some(ids)) => Ok(ids),
        (None, None) => Ok(vec![]),
        _ => Err(ApiError::invalid(
            "give only one of upload_id and upload_ids",
        )),
    }
}

pub(crate) async fn uploads(
    state: &AppState,
    caller: &Caller,
    ids: &[Uuid],
) -> ApiResult<Vec<UploadRow>> {
    let access = caller.entitlement.tool_access();
    if !access.within_run_quota(RunSize::new(ids.len(), 0)) {
        return Err(denied());
    }
    let mut uploads = Vec::with_capacity(ids.len());
    let mut bytes = 0u64;
    for id in ids {
        let upload = db::get_upload(&state.db, *id)
            .await?
            .filter(|row| row.workspace_id == caller.principal.workspace_id)
            .ok_or_else(|| ApiError::not_found("upload"))?;
        if upload.status() != UploadStatus::Completed || upload.expires_at <= chrono::Utc::now() {
            return Err(ApiError::conflict("a live, completed upload is required"));
        }
        bytes = bytes.saturating_add(u64::try_from(upload.size_bytes).unwrap_or(u64::MAX));
        uploads.push(upload);
    }
    if !access.within_run_quota(RunSize::new(ids.len(), bytes)) {
        return Err(denied());
    }
    if caller
        .entitlement
        .limits
        .max_input_bytes
        .is_some_and(|max| bytes > max)
    {
        return Err(ApiError::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "limit_exceeded",
            "the inputs exceed the plan's byte limit",
        ));
    }
    Ok(uploads)
}

pub(crate) async fn create_tool(
    state: &AppState,
    caller: &Caller,
    tool: &str,
    ids: Vec<Uuid>,
    params: Value,
    profile: Profile,
) -> ApiResult<JobRow> {
    let spec = spec(tool)?;
    if !caller.entitlement.tool_access().allows_spec(spec) {
        return Err(denied());
    }
    validate_params(spec, &params)?;
    if !spec.inputs.accepts(ids.len()) {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "wrong_input",
            "the input count does not match the tool contract",
        ));
    }
    let inputs = uploads(state, caller, &ids).await?;
    check_formats(spec, &inputs)?;
    let pack = Pack::for_engine(spec.engine)
        .map_err(|_| ApiError::invalid("tool engine has no worker pack"))?;
    submit(
        state,
        caller,
        Operation {
            tool,
            params,
            inputs,
            pack,
            profile,
            target_format: None,
        },
    )
    .await
}

fn check_formats(spec: &ToolSpec, inputs: &[UploadRow]) -> ApiResult<()> {
    if inputs
        .iter()
        .any(|u| !u.format.as_deref().is_some_and(|f| spec.accepts_format(f)))
    {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "wrong_input",
            "an upload format is not accepted by the tool",
        ));
    }
    Ok(())
}

pub(crate) fn validate_recipe(recipe: &Recipe, confirmed: bool, caller: &Caller) -> ApiResult<()> {
    recipe
        .validate(RecipeOrigin::Imported)
        .map_err(|_| invalid_params())?;
    if !confirmed {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "confirmation_required",
            "imported recipes require confirmed: true after reviewing every step",
        ));
    }
    let access = caller.entitlement.tool_access();
    for step in &recipe.steps {
        if let Step::Tool { tool, params, .. } = step {
            let spec = spec(tool.as_str())?;
            if !access.allows_spec(spec) {
                return Err(denied());
            }
            validate_params(spec, params)?;
        }
    }
    Ok(())
}

pub(crate) async fn create_recipe(
    state: &AppState,
    caller: &Caller,
    recipe: Recipe,
    confirmed: bool,
    ids: Vec<Uuid>,
) -> ApiResult<JobRow> {
    validate_recipe(&recipe, confirmed, caller)?;
    let inputs = uploads(state, caller, &ids).await?;
    if let Some(Step::Tool { tool, .. }) = recipe.steps.first() {
        let spec = spec(tool.as_str())?;
        // One-input steps fan out across the initial batch.
        if !(spec.inputs.accepts(inputs.len())
            || (spec.inputs == tools::InputArity::One && !inputs.is_empty()))
        {
            return Err(ApiError::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                "wrong_input",
                "recipe input count does not match its first step",
            ));
        }
        check_formats(spec, &inputs)?;
    } else if inputs.is_empty() {
        return Err(ApiError::invalid("a conversion recipe requires inputs"));
    }
    let mut pack = Pack::Core;
    let mut formats: Vec<String> = inputs.iter().filter_map(|u| u.format.clone()).collect();
    let mut output_format = String::new();
    for step in &recipe.steps {
        match step {
            Step::Tool { tool, .. } => {
                let spec = spec(tool.as_str())?;
                pack = pack.max(
                    Pack::for_engine(spec.engine)
                        .map_err(|_| ApiError::invalid("tool engine has no worker pack"))?,
                );
                output_format = spec.output_format.to_owned();
            }
            Step::Convert { to, profile, .. } => {
                for from in &formats {
                    let source = crate::routing::resolve_format(from)
                        .ok_or_else(|| ApiError::invalid("unknown recipe source format"))?;
                    let route = crate::routing::plan(
                        source,
                        *to,
                        profile.unwrap_or_default(),
                        &state.packs,
                    )
                    .map_err(|_| ApiError::invalid("unsupported recipe conversion route"))?;
                    pack = pack.max(crate::api::route_pack(&route.steps)?);
                }
                output_format = to.id().to_owned();
            }
        }
        formats = vec![output_format.clone()];
    }
    let params = serde_json::to_value(recipe).map_err(|err| ApiError::internal(&err))?;
    submit(
        state,
        caller,
        Operation {
            tool: RECIPE_JOB,
            params,
            inputs,
            pack,
            profile: Profile::Editable,
            target_format: Some(output_format),
        },
    )
    .await
}

struct Operation<'a> {
    tool: &'a str,
    params: Value,
    inputs: Vec<UploadRow>,
    pack: Pack,
    profile: Profile,
    target_format: Option<String>,
}

async fn submit(state: &AppState, caller: &Caller, operation: Operation<'_>) -> ApiResult<JobRow> {
    let Operation {
        tool,
        params,
        inputs,
        pack,
        profile,
        target_format,
    } = operation;
    if !state.packs.contains(&pack) {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "tool_missing",
            "the required worker pack is not enabled",
        ));
    }
    crate::api::check_pack_memory(pack, &caller.entitlement.limits)?;
    let id = Uuid::new_v4();
    let bytes = inputs.iter().fold(0u64, |n, u| {
        n.saturating_add(u64::try_from(u.size_bytes).unwrap_or(u64::MAX))
    });
    let subject = &caller.principal.subject;
    let mut tx = state.db.begin().await?;
    if !db::admit_global_job(&mut tx, state.queue.active_limit()).await? {
        return Err(ApiError::quota_exceeded(
            "the fleet's active job capacity is full",
        ));
    }
    db::lock_subject(&mut tx, subject).await?;
    // Lock in UUID order so overlapping multi-input submissions cannot deadlock.
    let ids: Vec<_> = inputs.iter().map(|u| u.id).collect();
    let live: i64 = sqlx::query_scalar("SELECT count(*) FROM (SELECT id FROM uploads WHERE id = ANY($1) AND status = 'completed' AND expires_at > now() ORDER BY id FOR SHARE) live")
        .bind(&ids).fetch_one(&mut *tx).await?;
    let unique: std::collections::HashSet<_> = ids.iter().collect();
    if live as usize != unique.len() {
        return Err(ApiError::conflict("an upload is no longer available"));
    }
    caller
        .entitlement
        .admit(crate::entitlements::usage(&mut tx, subject).await?, bytes)
        .map_err(|v| ApiError::quota_exceeded(v.to_string()))?;
    let row = db::insert_job(
        &mut tx,
        &NewJob {
            id,
            workspace_id: caller.principal.workspace_id,
            subject,
            upload_id: inputs.first().map(|u| u.id),
            source_format: inputs
                .first()
                .and_then(|u| u.format.as_deref())
                .unwrap_or("none"),
            target_format: target_format.as_deref(),
            tool_id: Some(tool),
            params: Some(params),
            tool_plan: caller.entitlement.tool_plan(),
            profile: profile.as_str(),
            pack,
            limits: serde_json::to_value(&caller.entitlement.limits)
                .map_err(|err| ApiError::internal(&err))?,
            retention_s: i64::try_from(caller.entitlement.retention.as_secs()).unwrap_or(i64::MAX),
        },
    )
    .await?;
    db::set_job_inputs(&mut tx, id, &ids).await?;
    db::record_usage(
        &mut tx,
        &UsageEvent {
            event_key: format!("job:{id}:submitted"),
            workspace_id: caller.principal.workspace_id,
            subject,
            job_id: Some(id),
            kind: "job_submitted",
            input_bytes: i64::try_from(bytes).unwrap_or(i64::MAX),
            output_bytes: 0,
            pages: 0,
            engine_ms: 0,
        },
    )
    .await?;
    db::add_event(&mut tx, id, "queued", json!({"pack":pack})).await?;
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
    Ok(row)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_validation_covers_nested_fields_and_never_echoes_values() {
        let spec = spec("pdf.rotate").unwrap();
        for params in [
            json!({"angle":90, "password":"private-value"}),
            json!({"angle":90, "nested":[{"API_KEY":"private-value"}]}),
            json!({"angle":90, "password":{"$secret":"prompt"}}),
            json!({"angle":90, "password":{"$secret":"sec_handle"}}),
        ] {
            let error = validate_params(spec, &params).unwrap_err();
            assert_eq!(error.code(), "invalid_params");
            assert!(!format!("{error:?}").contains("private-value"));
        }
        assert!(validate_params(spec, &json!({"angle":90})).is_ok());
    }

    #[test]
    fn input_aliases_preserve_order_and_refuse_ambiguity() {
        let first = Uuid::new_v4();
        let second = Uuid::new_v4();
        assert_eq!(
            input_ids(None, Some(vec![second, first])).unwrap(),
            [second, first]
        );
        assert_eq!(input_ids(Some(first), None).unwrap(), [first]);
        assert!(input_ids(Some(first), Some(vec![])).is_err());
        assert!(input_ids(None, None).unwrap().is_empty());
    }
}
