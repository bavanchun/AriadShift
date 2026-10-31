//! MCP adapters to the cloud API's existing admission and job services.

use std::{borrow::Cow, collections::BTreeMap, sync::Arc};

use axum::{
    extract::{Path, State},
    http::{HeaderMap, request::Parts},
    response::IntoResponse,
};
use bytes::Bytes;
use rmcp::{
    ErrorData, RoleServer, ServerHandler,
    model::{
        CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
        ListToolsResult, PaginatedRequestParams, ProtocolVersion, ServerCapabilities, ServerConfig,
        Tool,
    },
    service::RequestContext,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    api::{
        self, ApiJson, AppState, Caller,
        dto::{CreateJobRequest, CreateUploadRequest, PlanRequest, Profile},
    },
    db::{self, JobStatus, UploadStatus},
    error::ApiError,
    routing,
    storage::Direction,
};

#[derive(Clone)]
pub(super) struct RemoteServer {
    state: AppState,
}

impl RemoteServer {
    pub(super) fn new(state: AppState) -> Self {
        Self { state }
    }

    async fn execute(
        &self,
        name: &str,
        arguments: Value,
        caller: &Caller,
    ) -> Result<Value, ApiError> {
        match name {
            "list_engines" => {
                let _: Empty = arguments_as(arguments)?;
                let caps =
                    super::response_json(api::plan::capabilities(State(self.state.clone())).await)
                        .await?;
                let mut engines: BTreeMap<String, Value> = BTreeMap::new();
                if let Some(edges) = caps["edges"].as_array() {
                    for edge in edges {
                        let Some(id) = edge["engine"].as_str() else {
                            continue;
                        };
                        let entry = engines.entry(id.to_owned()).or_insert_with(|| {
                            json!({"id": id, "pack": edge["pack"],
                                "license": edge["license"], "routes": []})
                        });
                        if let Some(routes) = entry["routes"].as_array_mut() {
                            routes.push(json!({"from": edge["from"], "to": edge["to"]}));
                        }
                    }
                }
                Ok(json!({"engines": engines.into_values().collect::<Vec<_>>(),
                    "packs": caps["packs"], "capabilities_version": caps["version"]}))
            }
            "plan" => {
                let request: PlanRequest = arguments_as(arguments)?;
                validate_format(&request.to)?;
                if let Some(from) = &request.from {
                    validate_format(from)?;
                }
                let result = api::plan::plan(
                    State(self.state.clone()),
                    copy_caller(caller),
                    ApiJson(request),
                )
                .await?;
                as_value(result.0)
            }
            "inspect" => {
                let request: UploadArgs = arguments_as(arguments)?;
                let row = db::get_upload(&self.state.db, request.upload_id)
                    .await?
                    .filter(|row| row.workspace_id == caller.principal.workspace_id)
                    .ok_or_else(|| ApiError::not_found("upload"))?;
                if row.status() != UploadStatus::Completed || row.expires_at <= chrono::Utc::now() {
                    return Err(ApiError::conflict(
                        "inspect requires a live, completed upload",
                    ));
                }
                // The upload service has already verified the size/hash and
                // sniffed the format. Do not run binary parsers in the API.
                let source = row.format.as_deref().and_then(routing::resolve_format);
                let mut reachable = Vec::new();
                if let Some(source) = source {
                    for target in ariad_core::planner::embedded()
                        .edges
                        .iter()
                        .map(|edge| edge.to)
                        .filter(|target| target.is_user_facing() && *target != source)
                    {
                        if !reachable.contains(&target.id())
                            && routing::plan(
                                source,
                                target,
                                Profile::Editable.into(),
                                &self.state.packs,
                            )
                            .is_ok()
                        {
                            reachable.push(target.id());
                        }
                    }
                }
                reachable.sort_unstable();
                Ok(json!({
                    "upload_id": row.id, "filename": row.filename,
                    "format": row.format, "bytes": row.size_bytes,
                    "sha256": row.sha256, "expires_at": row.expires_at,
                    "reachable": reachable,
                    "inspection": "verified_upload_metadata"
                }))
            }
            "convert" => {
                let request: ConvertArgs = arguments_as(arguments)?;
                validate_format(&request.to)?;
                // Validate all shape and inline bounds before reserving an upload.
                let input = validate_source(request.upload_id, request.inline)?;
                if routing::resolve_format(&request.to).is_none() {
                    return Err(ApiError::invalid("unknown target format; use list_engines"));
                }
                let automatic = matches!(&input, Source::Inline(_));
                let upload_id = match input {
                    Source::Upload(id) => id,
                    Source::Inline(inline) => self.inline_upload(caller, inline).await?,
                };
                let response = api::jobs::create_job(
                    State(self.state.clone()),
                    copy_caller(caller),
                    ApiJson(CreateJobRequest {
                        upload_id,
                        to: request.to,
                        profile: request.profile,
                    }),
                )
                .await;
                let response = match response {
                    Ok(response) => response,
                    Err(err) if automatic && err.code() == "quota_exceeded" => {
                        // The completed input remains usable. Return its identity
                        // so retries do not consume the daily upload allowance.
                        return Err(err.with_reusable_upload(upload_id));
                    }
                    Err(err) => {
                        if automatic {
                            let _ = api::uploads::delete_upload(
                                State(self.state.clone()),
                                copy_caller(caller),
                                Path(upload_id.to_string()),
                            )
                            .await;
                        }
                        return Err(err);
                    }
                };
                let job = super::response_json(response).await?;
                Ok(json!({"job": job, "outputs": [],
                    "next": "Call job_status with job.id until terminal; succeeded jobs include download URLs."}))
            }
            "job_status" => {
                let request: JobArgs = arguments_as(arguments)?;
                let id = request.job_id.to_string();
                let row = api::jobs::visible_job(&self.state, caller, &id).await?;
                let job = api::jobs::job_dto(&self.state, &row).await?;
                let outputs = if row.status() == JobStatus::Succeeded {
                    api::jobs::job_outputs(State(self.state.clone()), copy_caller(caller), Path(id))
                        .await?
                        .0
                        .outputs
                } else {
                    Vec::new()
                };
                Ok(json!({"job": job, "outputs": outputs}))
            }
            _ => Err(ApiError::invalid("unknown tool; use tools/list")),
        }
    }

    async fn inline_upload(&self, caller: &Caller, input: Inline) -> Result<Uuid, ApiError> {
        let bytes = Bytes::from(input.content);
        let size = bytes.len() as u64;
        let sha256 = hex::encode(Sha256::digest(&bytes));
        // Reuse upload admission, including abandoned-upload quotas, so inline
        // conversion cannot bypass limits by avoiding /v1/uploads.
        let (_, upload) = api::uploads::create_upload(
            State(self.state.clone()),
            copy_caller(caller),
            HeaderMap::new(),
            ApiJson(CreateUploadRequest {
                filename: input.filename,
                size,
                sha256: sha256.clone(),
            }),
        )
        .await?;
        let id = upload.0.id;
        let key = self
            .state
            .storage
            .key(caller.principal.workspace_id, id, Direction::In, &sha256);
        let stored = async {
            self.state
                .storage
                .put_bytes(&key, bytes)
                .await
                .map_err(|err| ApiError::internal(&err))?;
            api::uploads::complete_upload(
                State(self.state.clone()),
                copy_caller(caller),
                Path(id.to_string()),
            )
            .await
            .map(|_| ())
        }
        .await;
        if let Err(err) = stored {
            // Retire admission even if object cleanup also fails. The sweeper
            // retries deletion; preserve the original storage/completion error.
            let _ = api::uploads::delete_upload(
                State(self.state.clone()),
                copy_caller(caller),
                Path(id.to_string()),
            )
            .await;
            return Err(err);
        }
        Ok(id)
    }
}

fn copy_caller(caller: &Caller) -> Caller {
    Caller {
        principal: caller.principal.clone(),
        entitlement: caller.entitlement.clone(),
        peer: caller.peer,
    }
}

fn arguments_as<T: DeserializeOwned>(value: Value) -> Result<T, ApiError> {
    // Do not reflect arbitrary argument values into errors (document content
    // and paths can appear in serde's invalid-value messages).
    serde_json::from_value(value)
        .map_err(|_| ApiError::invalid("arguments do not match the tool's input schema"))
}

fn as_value(value: impl Serialize) -> Result<Value, ApiError> {
    serde_json::to_value(value).map_err(|err| ApiError::internal(&err))
}

fn validate_format(raw: &str) -> Result<(), ApiError> {
    if raw.is_empty() || raw.chars().count() > 64 {
        return Err(ApiError::invalid("format must contain 1 to 64 characters"));
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Empty {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UploadArgs {
    upload_id: Uuid,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct JobArgs {
    job_id: Uuid,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Inline {
    filename: String,
    content: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConvertArgs {
    upload_id: Option<Uuid>,
    inline: Option<Inline>,
    to: String,
    #[serde(default)]
    profile: Profile,
}

enum Source {
    Upload(Uuid),
    Inline(Inline),
}

fn validate_source(upload_id: Option<Uuid>, inline: Option<Inline>) -> Result<Source, ApiError> {
    match (upload_id, inline) {
        (Some(id), None) => Ok(Source::Upload(id)),
        (None, Some(input)) => {
            if input.content.is_empty() || input.content.len() > super::INLINE_CONTENT_LIMIT {
                return Err(ApiError::invalid(
                    "inline content must contain 1 to 262144 UTF-8 bytes; use /v1/uploads for larger files",
                ));
            }
            if input.filename.chars().count() > 255
                || input.filename.chars().any(char::is_control)
                || input.filename.contains(['/', '\\'])
            {
                return Err(ApiError::invalid(
                    "inline filename must be a plain filename",
                ));
            }
            let format = input
                .filename
                .rsplit_once('.')
                .and_then(|(_, ext)| routing::resolve_format(ext));
            if !matches!(
                format,
                Some(ariad_core::format::Format::Markdown | ariad_core::format::Format::Html)
            ) {
                return Err(ApiError::invalid(
                    "inline supports Markdown and HTML filenames; use /v1/uploads for binary files",
                ));
            }
            Ok(Source::Inline(input))
        }
        _ => Err(ApiError::invalid(
            "give exactly one of upload_id and inline",
        )),
    }
}

impl ServerHandler for RemoteServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("ariadshift-remote", env!("CARGO_PKG_VERSION")))
            .with_instructions("Inspect verified uploads, plan routes, submit conversion jobs, and poll job_status for short-lived output URLs. File paths and URL inputs are not accepted.")
    }

    fn supported_protocol_versions(&self) -> Cow<'static, [ProtocolVersion]> {
        Cow::Owned(vec![ProtocolVersion::V_2026_07_28])
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        definitions().into_iter().find(|tool| tool.name == name)
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult::with_all_items(definitions()))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        if self.get_tool(&request.name).is_none() {
            return Err(ErrorData::invalid_params(
                "unknown tool; use tools/list",
                None,
            ));
        }
        let caller = context
            .extensions
            .get::<Parts>()
            .and_then(|parts| parts.extensions.get::<Arc<Caller>>())
            .ok_or_else(|| {
                ErrorData::internal_error("authenticated request context is missing", None)
            })?;
        let arguments = Value::Object(request.arguments.unwrap_or_default());
        let result = match self.execute(&request.name, arguments, caller).await {
            Ok(value) => {
                let mut result =
                    CallToolResult::success(vec![ContentBlock::text(value.to_string())]);
                result.structured_content = Some(value);
                result
            }
            Err(err) => {
                let status = err.status().as_u16();
                let upload_id = err.reusable_upload_id();
                let response = err.into_response();
                let retry = response
                    .headers()
                    .get("retry-after")
                    .and_then(|value| value.to_str().ok())
                    .and_then(|value| value.parse::<u64>().ok());
                let mut value = super::response_json(response).await.unwrap_or_else(
                    |_| json!({"error": {"code": "internal", "message": "retry later"}}),
                );
                value["status"] = json!(status);
                if let Some(upload_id) = upload_id {
                    value["upload_id"] = json!(upload_id);
                }
                if let Some(retry) = retry {
                    value["retry_after_s"] = json!(retry);
                }
                let mut result = CallToolResult::error(vec![ContentBlock::text(value.to_string())]);
                result.structured_content = Some(value);
                result
            }
        };
        Ok(result.into())
    }
}

fn definitions() -> Vec<Tool> {
    let uuid = json!({"type": "string", "format": "uuid"});
    let format = json!({"type": "string", "minLength": 1, "maxLength": 64});
    let profile =
        json!({"type": "string", "enum": ["editable", "faithful", "fast"], "default": "editable"});
    let schemas = [
        (
            "list_engines",
            "List engines and routes configured for this deployment's worker packs. Use plan to choose a route.",
            json!({"type": "object", "properties": {}, "additionalProperties": false}),
            true,
        ),
        (
            "plan",
            "Plan a conversion using a source format or completed upload_id, without submitting a job. Returns the REST plan with route scores and alternatives.",
            json!({"type": "object", "properties": {"from": format, "upload_id": uuid, "to": format, "profile": profile}, "required": ["to"], "oneOf": [{"required": ["from"]}, {"required": ["upload_id"]}], "additionalProperties": false}),
            true,
        ),
        (
            "inspect",
            "Inspect a live completed upload owned by your workspace: verified format, size, checksum, expiry and reachable targets. Does not perform deep structural parsing. Upload with /v1/uploads first.",
            json!({"type": "object", "properties": {"upload_id": uuid}, "required": ["upload_id"], "additionalProperties": false}),
            true,
        ),
        (
            "convert",
            "Submit a conversion job. Give upload_id from /v1/uploads, or inline UTF-8 Markdown/HTML (1–262144 bytes). Returns job.id; poll job_status for output URLs. Consumes upload/job quotas. Inline quota errors include upload_id: retry with that reference and no inline content after capacity returns. Repeating a successful call creates another job.",
            json!({"type": "object", "properties": {"upload_id": uuid, "inline": {"type": "object", "properties": {"filename": {"type": "string", "minLength": 1, "maxLength": 255}, "content": {"type": "string", "minLength": 1, "maxLength": super::INLINE_CONTENT_LIMIT}}, "required": ["filename", "content"], "additionalProperties": false}, "to": format, "profile": profile}, "required": ["to"], "oneOf": [{"required": ["upload_id"]}, {"required": ["inline"]}], "additionalProperties": false}),
            false,
        ),
        (
            "job_status",
            "Get an authorized job's state and warnings. Succeeded jobs include output filenames, checksums, sizes and presigned URLs valid for five minutes. Poll again to refresh URLs; expired files cannot be recovered.",
            json!({"type": "object", "properties": {"job_id": uuid}, "required": ["job_id"], "additionalProperties": false}),
            true,
        ),
    ];
    schemas
        .into_iter()
        .map(|(name, description, input_schema, read_only)| {
            serde_json::from_value(json!({
                "name": name, "description": description, "inputSchema": input_schema,
                "outputSchema": {"type": "object"},
                "annotations": {"readOnlyHint": read_only, "destructiveHint": false,
                    "idempotentHint": read_only, "openWorldHint": false}
            }))
            .expect("static tool definitions are valid")
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_bounds_use_utf8_bytes_and_reject_paths() {
        let input = |filename: &str, content: String| Inline {
            filename: filename.to_owned(),
            content,
        };
        assert!(
            validate_source(
                None,
                Some(input(
                    "notes.md",
                    "x".repeat(super::super::INLINE_CONTENT_LIMIT)
                ))
            )
            .is_ok()
        );
        assert!(
            validate_source(
                None,
                Some(input(
                    "notes.md",
                    "é".repeat(super::super::INLINE_CONTENT_LIMIT / 2 + 1)
                ))
            )
            .is_err()
        );
        for filename in ["../notes.md", "dir\\notes.md", "file.pdf", "notes", ""] {
            assert!(validate_source(None, Some(input(filename, "text".to_owned()))).is_err());
        }
        assert!(
            validate_source(
                Some(Uuid::nil()),
                Some(input("notes.md", "text".to_owned()))
            )
            .is_err()
        );
        assert!(validate_source(None, None).is_err());
    }
}
