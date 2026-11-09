//! MCP tool definitions and implementations for AriadShift.

use std::{
    path::PathBuf,
    sync::{Arc, Mutex, RwLock},
};

use ariad_core::{
    limits::Limits,
    planner::Profile,
    tools::{
        InputArity, KNOWN_SECRET_FIELD_NAMES, SecretRef, SecretSource, Tier, ToolGroup,
        ToolRuntime, ToolSpec, validate_secret_fields, validate_secret_params,
    },
};
use ariad_host::{
    assets::AssetBase,
    commands::{active_capabilities, engines, inspect, plan},
    confine::{
        AllowedDirs, ConfineError, ProcessStagingDirectory, confine_input, confine_output,
        parse_file_uri,
    },
    convert::{ConvertError, ConvertOutput, ConvertRequest, convert_confined_assets_in},
    session::SessionPool,
    tools::{InputRef, OutputTarget, ToolError, ToolOutcome, ToolRequest, run_tool_with_events},
};
use rmcp::{
    ErrorData,
    handler::server::wrapper::Parameters,
    model::{CallToolResult, ContentBlock, Resource},
    tool, tool_router,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use super::resources::{SessionResources, mime_type_for_path, path_to_file_uri};

fn parse_profile(profile_str: Option<&str>) -> Result<Profile, CallToolResult> {
    match profile_str {
        None => Ok(Profile::Editable),
        Some(s) => match s.trim().to_ascii_lowercase().as_str() {
            "editable" => Ok(Profile::Editable),
            "faithful" => Ok(Profile::Faithful),
            "fast" => Ok(Profile::Fast),
            "private" => Ok(Profile::Private),
            other => {
                let clean_profile = ariad_host::convert::sanitize_identifier(other, 64);
                let msg = format!(
                    "invalid value '{clean_profile}' for '--profile <PROFILE>'; [possible values: editable, faithful, fast, private]"
                );
                Err(tool_error("invalid_profile", 2, msg))
            }
        },
    }
}

/// Arguments for `list_engines` tool.
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct ListEnginesArgs {}

/// Arguments for `inspect` tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct InspectArgs {
    /// Input document filesystem path or file:// URI.
    pub input: String,
}

/// Arguments for `plan` tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct PlanArgs {
    /// Input document filesystem path or file:// URI.
    pub input: String,
    /// Target format identifier (md, html, docx, epub, pdf, json, xlsx, pptx).
    pub to: String,
    /// Routing optimization profile (editable, faithful, fast, private).
    pub profile: Option<String>,
}

/// Arguments for `convert` tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ConvertArgs {
    /// Input document filesystem path or file:// URI.
    pub input: String,
    /// Target format identifier (md, html, docx, epub, pdf, json, xlsx, pptx).
    pub to: String,
    /// Optional output file path or file:// URI (defaults to input stem with target extension).
    pub output: Option<String>,
    /// Allow overwriting existing destination (only honoured if server has --allow-overwrite).
    pub overwrite: Option<bool>,
    /// Routing optimization profile (editable, faithful, fast, private).
    pub profile: Option<String>,
}

/// Arguments for `list_doc_tools` tool.
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct ListDocToolsArgs {
    /// Optional tool group filter (`organize`, `convert`, `optimize`, `security`, `overlay`, `ocr`, `annotate`, `forms`, `sign`, `redact`, `compare`, `edit`, `ai`, `automation`).
    pub group: Option<String>,
}

/// Descriptor returned by `list_doc_tools`, combining `ToolDescriptor` with live engine status.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct McpToolDescriptor {
    /// Canonical `<domain>.<action>` identifier.
    pub id: ariad_core::tools::ToolId,
    /// Schema/contract version string (`"1"`).
    pub version: String,
    /// Functional group.
    pub group: ToolGroup,
    /// One-line human description.
    pub summary: String,
    /// Input arity contract.
    pub inputs: InputArity,
    /// Accepted input format identifiers.
    pub input_formats: Vec<String>,
    /// Output format identifier.
    pub output_format: String,
    /// Resolved JSON Schema (Draft 2020-12) for `params`.
    pub params_schema: schemars::Schema,
    /// Subscription tier (`free` or `pro`).
    pub tier: Tier,
    /// Supported execution runtimes (`wasm`, `local`, `cloud`).
    pub runtimes: Vec<ToolRuntime>,
    /// Executing engine identifier.
    pub engine: String,
    /// Live availability status of the backing engine (`available`, `missing`, `incompatible`).
    pub engine_status: String,
    /// Parameter field names annotated with `"x-ariad-secret": true`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub secret_params: Vec<String>,
}

/// Arguments for `run_doc_tool` tool.
#[derive(Deserialize, JsonSchema)]
pub struct RunDocToolArgs {
    /// Canonical tool identifier (for example `pdf.info`, `pdf.merge`, `pdf.split`).
    pub tool: String,
    /// Input document filesystem paths or `file://` URIs inside `--allow-dir`.
    #[serde(default)]
    pub inputs: Vec<String>,
    /// Optional directory inside `--allow-dir` for published outputs (defaults to the first input's parent directory).
    pub output_dir: Option<String>,
    /// Optional explicit output file or directory path inside `--allow-dir`.
    pub output: Option<String>,
    /// Optional JSON object matching the tool schema; secret fields accept handles from `secret_put`.
    pub params: Option<serde_json::Value>,
    /// Allow overwriting existing destination files (only honoured if server has `--allow-overwrite`).
    pub overwrite: Option<bool>,
}

/// Arguments for creating a short-lived local secret handle.
#[derive(Deserialize, JsonSchema)]
pub struct SecretPutArgs {
    /// Secret value, between 1 and 4096 UTF-8 bytes (or up to 14 MiB for base64-encoded binary certificates).
    pub value: String,
    /// Set to true when storing a base64-encoded binary certificate (allows up to 14 MiB).
    #[serde(default)]
    pub certificate: Option<bool>,
}

/// Handle returned for a stored secret.
#[derive(Serialize, JsonSchema)]
pub struct SecretPutResult {
    /// Opaque single-use handle.
    pub handle: String,
    /// Lifetime in seconds.
    pub expires_in_s: u64,
}

/// The core MCP server state and tool router.
#[derive(Clone)]
pub struct McpServer {
    pub(super) secrets: Arc<super::secrets::SecretStore>,
    pub allow_overwrite: bool,
    pub allowed_dirs: Arc<RwLock<AllowedDirs>>,
    pub(super) explicit_allowed_dirs: AllowedDirs,
    pub(super) roots_refresh: Arc<Mutex<super::RootsRefresh>>,
    pub staging: Arc<ProcessStagingDirectory>,
    pub resources: SessionResources,
    pub engine_program: PathBuf,
    /// Warm engine sessions shared by every `convert` call of this server.
    pub pool: SessionPool,
    pub cancel_token: tokio_util::sync::CancellationToken,
    pub in_flight: Arc<std::sync::atomic::AtomicUsize>,
}

fn parse_path_or_uri(s: &str) -> Result<PathBuf, ConfineError> {
    if s.starts_with("file://") {
        parse_file_uri(s)
    } else {
        Ok(PathBuf::from(s))
    }
}

fn tool_error(code: &str, exit_code: u8, message: impl Into<String>) -> CallToolResult {
    let msg = message.into();
    let val = serde_json::json!({
        "code": code,
        "exit_code": exit_code,
        "message": &msg,
    });
    let mut res = CallToolResult::structured_error(val);
    res.content = vec![ContentBlock::text(format!("ashift: {msg}"))];
    res
}

fn tool_confine_error(err: &ConfineError) -> CallToolResult {
    tool_error(err.code(), err.exit_code(), err.to_string())
}

fn tool_convert_error(err: &ConvertError) -> CallToolResult {
    tool_error(err.code(), err.exit_code(), err.to_string())
}

fn tool_run_error(err: &ToolError) -> CallToolResult {
    let code = err.code();
    let exit_code = err.exit_code();
    let msg = err.to_string();
    let mut val = serde_json::json!({
        "code": code,
        "exit_code": exit_code,
        "message": &msg,
        "error": {
            "code": code,
            "exit_code": exit_code,
            "message": &msg,
        },
    });
    if let ToolError::ComplianceFailed { standard, report } = err {
        val["error"]["standard"] = serde_json::json!(standard);
        val["error"]["report"] = report.as_ref().clone();
    }
    let mut res = CallToolResult::structured_error(val);
    res.content = vec![ContentBlock::text(format!("ashift: error[{code}]: {msg}"))];
    res
}

fn mcp_spec(tool_id: &str) -> Option<&'static ToolSpec> {
    #[cfg(feature = "test-probe")]
    if std::env::var_os("ARIAD_MCP_TEST_PROBE").is_some() {
        static SPECS: std::sync::OnceLock<Vec<ToolSpec>> = std::sync::OnceLock::new();
        if let Some(spec) = SPECS
            .get_or_init(ariad_core::tools::specs::security::specs)
            .iter()
            .find(|spec| spec.id == tool_id)
        {
            return Some(spec);
        }
    }
    ariad_core::tools::find(tool_id)
}

fn contains_secret_ref(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Object(map) => {
            map.keys().any(|key| key.eq_ignore_ascii_case("$secret"))
                || map.values().any(contains_secret_ref)
        }
        serde_json::Value::Array(items) => items.iter().any(contains_secret_ref),
        _ => false,
    }
}

fn validate_mcp_tool_params(
    tool_id: &str,
    params: Option<&serde_json::Value>,
) -> Result<serde_json::Value, ToolError> {
    let Some(raw) = params else {
        return Ok(serde_json::Value::Object(serde_json::Map::new()));
    };
    if raw.is_null() {
        return Ok(serde_json::Value::Object(serde_json::Map::new()));
    }
    let Some(map) = raw.as_object() else {
        return Err(ToolError::InvalidParams(
            "params must be a JSON object".to_owned(),
        ));
    };

    let spec = mcp_spec(tool_id);
    if let Some(spec) = spec {
        validate_secret_params(spec, raw, SecretSource::Mcp)
            .map_err(|err| ToolError::InvalidParams(err.to_string()))?;
    }
    validate_secret_fields(KNOWN_SECRET_FIELD_NAMES, raw, SecretSource::Mcp)
        .map_err(|err| ToolError::InvalidParams(err.to_string()))?;
    for (key, value) in map {
        if value.is_null() {
            continue;
        }
        let secret = spec.is_some_and(|s| s.secret_params.contains(&key.as_str()));
        let has_ref = contains_secret_ref(value);
        if secret {
            if !matches!(
                SecretRef::from_json_value(key, value),
                Ok(SecretRef::Handle(_))
            ) {
                return Err(ToolError::InvalidParams(
                    "a valid secret handle is required".to_owned(),
                ));
            }
        } else if has_ref {
            return Err(ToolError::InvalidParams(
                "secret handles are allowed only in secret fields".to_owned(),
            ));
        }
    }

    Ok(raw.clone())
}

fn input_error(err: &std::io::Error) -> ConvertError {
    match err.kind() {
        std::io::ErrorKind::FileTooLarge => ConvertError::LimitExceeded,
        std::io::ErrorKind::Interrupted => ConvertError::Interrupted,
        _ => ConvertError::InputIo,
    }
}

struct InFlightGuard(Arc<std::sync::atomic::AtomicUsize>);
impl Drop for InFlightGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

impl McpServer {
    async fn blocking_job<T, F>(
        &self,
        request_cancel: CancellationToken,
        job: F,
    ) -> Result<T, ErrorData>
    where
        T: Send + 'static,
        F: FnOnce(CancellationToken) -> T + Send + 'static,
    {
        let cancel = self.cancel_token.child_token();
        let _cancel_on_drop = cancel.clone().drop_guard();
        let forwarded = cancel.clone();
        tokio::spawn(async move {
            tokio::select! {
                _ = request_cancel.cancelled() => forwarded.cancel(),
                _ = forwarded.cancelled() => {},
            }
        });
        self.in_flight
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let guard = InFlightGuard(self.in_flight.clone());
        tokio::task::spawn_blocking(move || {
            let _guard = guard;
            job(cancel)
        })
        .await
        .map_err(|_| ErrorData::internal_error("blocking task failed", None))
    }
}

fn tool_success<T: serde::Serialize>(
    val: &T,
    extra_blocks: Vec<ContentBlock>,
) -> Result<CallToolResult, ErrorData> {
    let json = serde_json::to_string_pretty(val)
        .map_err(|e| ErrorData::internal_error(format!("JSON error: {e}"), None))?;
    let structured = serde_json::to_value(val)
        .map_err(|e| ErrorData::internal_error(format!("JSON error: {e}"), None))?;
    let mut content = vec![ContentBlock::text(json)];
    content.extend(extra_blocks);
    let mut res = CallToolResult::structured(structured);
    res.content = content;
    Ok(res)
}

#[tool_router(vis = "pub")]
impl McpServer {
    /// Store a secret temporarily for one document tool call.
    #[tool(
        description = "Store a secret for one local document tool call (expires after 300 seconds)",
        output_schema = rmcp::handler::server::tool::schema_for_output::<SecretPutResult>()
    )]
    pub async fn secret_put(
        &self,
        Parameters(args): Parameters<SecretPutArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let is_cert = args.certificate.unwrap_or(false);
        match self.secrets.put_with_options(args.value, is_cert) {
            Ok(handle) => tool_success(
                &SecretPutResult {
                    handle,
                    expires_in_s: 300,
                },
                vec![],
            ),
            Err(()) => Ok(tool_error(
                "invalid_params",
                9,
                "secret is empty, too large, or the handle store is full",
            )),
        }
    }

    /// List available transformation engines, licenses, statuses, and supported routes.
    #[tool(
        description = "List available transformation engines, licenses, statuses, and supported routes",
        output_schema = rmcp::handler::server::tool::schema_for_output::<Vec<ariad_host::commands::engines::EngineRow>>()
    )]
    pub async fn list_engines(
        &self,
        Parameters(_): Parameters<ListEnginesArgs>,
        ct: CancellationToken,
    ) -> Result<CallToolResult, ErrorData> {
        let caps = match active_capabilities() {
            Ok(c) => c,
            Err(e) => {
                return Ok(tool_convert_error(&e));
            }
        };

        let engine_prog = self.engine_program.clone();
        let rows = tokio::task::spawn_blocking(move || engines::engines(&engine_prog, &caps, ct))
            .await
            .map_err(|_| ErrorData::internal_error("blocking task failed", None))?;

        match rows {
            Ok(engine_rows) => tool_success(&engine_rows, vec![]),
            Err(e) => Ok(tool_convert_error(&e)),
        }
    }

    /// Inspect document format, metrics, structure counts, and reachable routes.
    #[tool(
        description = "Inspect document format, metrics, structure counts, and reachable routes",
        output_schema = rmcp::handler::server::tool::schema_for_output::<ariad_host::commands::inspect::InspectOutput>()
    )]
    pub async fn inspect(
        &self,
        Parameters(args): Parameters<InspectArgs>,
        ct: CancellationToken,
    ) -> Result<CallToolResult, ErrorData> {
        let input_path = match parse_path_or_uri(&args.input) {
            Ok(p) => p,
            Err(e) => return Ok(tool_confine_error(&e)),
        };

        let mut validated_input = {
            let allowed = self
                .allowed_dirs
                .read()
                .map_err(|_| ErrorData::internal_error("internal lock error", None))?;
            match confine_input(&input_path, &allowed) {
                Ok(v) => v,
                Err(e) => return Ok(tool_confine_error(&e)),
            }
        };

        let engine_prog = self.engine_program.clone();
        let staging = self.staging.path().to_path_buf();
        let res = self
            .blocking_job(ct, move |cancel| {
                let (_temp, input) = validated_input
                    .create_isolated_copy_cancellable_in(&staging, None, None, &cancel)
                    .map_err(|e| input_error(&e))?;
                let caps = active_capabilities()?;
                inspect::inspect(&input, Some(&engine_prog), &caps, cancel)
            })
            .await?;

        match res {
            Ok(report) => tool_success(&report, vec![]),
            Err(e) => Ok(tool_convert_error(&e)),
        }
    }

    /// Plan and explain conversion route, metrics score, and alternatives without converting.
    #[tool(
        description = "Plan and explain conversion route, metrics score, and alternatives without converting",
        output_schema = rmcp::handler::server::tool::schema_for_output::<ariad_host::commands::plan::PlanOutput>()
    )]
    pub async fn plan(
        &self,
        Parameters(args): Parameters<PlanArgs>,
        ct: CancellationToken,
    ) -> Result<CallToolResult, ErrorData> {
        let input_path = match parse_path_or_uri(&args.input) {
            Ok(p) => p,
            Err(e) => return Ok(tool_confine_error(&e)),
        };

        let mut validated_input = {
            let allowed = self
                .allowed_dirs
                .read()
                .map_err(|_| ErrorData::internal_error("internal lock error", None))?;
            match confine_input(&input_path, &allowed) {
                Ok(v) => v,
                Err(e) => return Ok(tool_confine_error(&e)),
            }
        };

        let profile = match parse_profile(args.profile.as_deref()) {
            Ok(p) => p,
            Err(e) => return Ok(e),
        };
        let to = args.to;
        let engine_prog = self.engine_program.clone();
        let staging = self.staging.path().to_path_buf();
        let res = self
            .blocking_job(ct, move |cancel| {
                let (_temp, input) = validated_input
                    .create_isolated_copy_cancellable_in(&staging, None, None, &cancel)
                    .map_err(|e| input_error(&e))?;
                let caps = active_capabilities()?;
                plan::plan(&input, &to, profile, &caps, Some(&engine_prog), cancel)
            })
            .await?;

        match res {
            Ok(report) => tool_success(&report, vec![]),
            Err(e) => Ok(tool_convert_error(&e)),
        }
    }

    /// Convert a document between supported formats (Markdown, HTML, DOCX, EPUB, PDF, Office, IR JSON).
    #[tool(
        description = "Convert a document between supported formats (Markdown, HTML, DOCX, EPUB, PDF, Office, IR JSON)",
        output_schema = rmcp::handler::server::tool::schema_for_output::<ariad_host::convert::ConvertOutput>()
    )]
    pub async fn convert(
        &self,
        Parameters(args): Parameters<ConvertArgs>,
        ct: CancellationToken,
    ) -> Result<CallToolResult, ErrorData> {
        let profile = match parse_profile(args.profile.as_deref()) {
            Ok(p) => p,
            Err(e) => return Ok(e),
        };

        let input_path = match parse_path_or_uri(&args.input) {
            Ok(p) => p,
            Err(e) => return Ok(tool_confine_error(&e)),
        };

        if ariad_core::format::Format::from_user_name(&args.to).is_none() {
            let clean_target = ariad_host::convert::sanitize_identifier(&args.to, 64);
            let detail = match ariad_host::convert::detect_format_from_path(&input_path) {
                Some(in_fmt) => {
                    let caps = ariad_host::commands::active_capabilities().ok();
                    let reachable = caps.as_ref().map(|c| {
                        let engines = c.registered_engines();
                        ariad_core::planner::reachable_formats(c, in_fmt, profile, &engines)
                    });
                    if let Some(r) = reachable {
                        let targets_detail =
                            ariad_host::convert::reachable_targets_detail(in_fmt.user_name(), &r);
                        format!("unknown target format '{clean_target}'; {targets_detail}")
                    } else {
                        format!("unknown target format '{clean_target}'")
                    }
                }
                None => format!("unknown target format '{clean_target}'"),
            };
            let err = ConvertError::UnsupportedRoute {
                detail: Some(detail),
            };
            return Ok(tool_convert_error(&err));
        }

        let raw_output = match args.output.as_deref().map(parse_path_or_uri).transpose() {
            Ok(opt) => opt,
            Err(e) => return Ok(tool_confine_error(&e)),
        };

        let (mut validated_input, validated_output) = {
            let allowed = self
                .allowed_dirs
                .read()
                .map_err(|_| ErrorData::internal_error("internal lock error", None))?;

            let valid_in = match confine_input(&input_path, &allowed) {
                Ok(v) => v,
                Err(e) => return Ok(tool_confine_error(&e)),
            };

            let valid_out = match confine_output(
                &valid_in,
                raw_output.as_deref(),
                &args.to,
                args.overwrite.unwrap_or(false),
                self.allow_overwrite,
                &allowed,
            ) {
                Ok(v) => v,
                Err(e) => return Ok(tool_confine_error(&e)),
            };

            (valid_in, valid_out)
        };

        let target_format = args.to.clone();
        let final_dest = validated_output.destination.clone();
        let pool = self.pool.clone();
        let engine_program = self.engine_program.clone();
        let staging = self.staging.path().to_path_buf();
        let res = self
            .blocking_job(ct, move |cancel| {
                let (_temp, input) = validated_input
                    .create_isolated_copy_cancellable_in(
                        &staging,
                        None,
                        Limits::local().max_input_bytes,
                        &cancel,
                    )
                    .map_err(|e| input_error(&e))?;
                let asset_base = AssetBase::confined(
                    validated_input
                        .parent_dir
                        .try_clone()
                        .map_err(|e| input_error(&e))?,
                );
                let request = ConvertRequest {
                    input,
                    output: validated_output.destination,
                    target_format: args.to,
                    profile,
                    overwrite: validated_output.overwrite,
                    engine_program,
                    title_fallback: None,
                    asset_base_dir: None,
                    reader_engine: None,
                    pdf_standard: None,
                };
                convert_confined_assets_in(
                    &pool,
                    &request,
                    &validated_output.parent_dir,
                    std::path::Path::new(&validated_output.file_name),
                    &asset_base,
                    cancel,
                    |_| {},
                )
            })
            .await?;

        let report = match res {
            Ok(r) => r,
            Err(e) => return Ok(tool_convert_error(&e)),
        };

        // On successful promotion, build output blocks without reading destination by path (C1)
        let file_uri = path_to_file_uri(&final_dest);
        let file_size = report.output_bytes;
        let mime = mime_type_for_path(&final_dest);

        // Record in session resource map for resources/read
        self.resources.record(file_uri.clone(), final_dest.clone());

        let mut resource = Resource::new(
            file_uri,
            final_dest
                .file_name()
                .map(|f| f.to_string_lossy().into_owned())
                .unwrap_or_else(|| "output".to_owned()),
        )
        .with_mime_type(mime);
        resource.size = Some(file_size);

        let mut extra_blocks = vec![ContentBlock::resource_link(resource)];

        // When `to` is markdown, provide inline text if <= 256 KiB read from trusted workspace artifact
        if let Some(ref md_text) = report.inline_markdown {
            extra_blocks.push(ContentBlock::text(md_text.clone()));
        } else if (target_format == "md" || target_format == "markdown") && file_size > 256 * 1024 {
            extra_blocks.push(ContentBlock::text(
                "Markdown content exceeds 256 KiB; read the file directly or via resources/read.",
            ));
        }

        let convert_output = ConvertOutput::from(&report);
        tool_success(&convert_output, extra_blocks)
    }

    /// List registered document manipulation tools, parameter schemas, and engine availability.
    #[tool(
        description = "List document tools, schemas, and availability. For secret fields use a handle from secret_put.",
        output_schema = rmcp::handler::server::tool::schema_for_output::<Vec<McpToolDescriptor>>()
    )]
    pub async fn list_doc_tools(
        &self,
        Parameters(args): Parameters<ListDocToolsArgs>,
        ct: CancellationToken,
    ) -> Result<CallToolResult, ErrorData> {
        let specs: Vec<&'static ToolSpec> = match args.group.as_deref() {
            Some(group_str) => match group_str.parse::<ToolGroup>() {
                Ok(group) => ariad_core::tools::by_group(group)
                    .filter(|spec| spec.supports_runtime(ToolRuntime::Local))
                    .collect(),
                Err(err) => return Ok(tool_run_error(&ToolError::Usage(err.to_string()))),
            },
            None => ariad_core::tools::all()
                .iter()
                .filter(|spec| spec.supports_runtime(ToolRuntime::Local))
                .collect(),
        };

        let needs_external_probe = specs
            .iter()
            .any(|spec| spec.engine != "ariad-pdf" && spec.engine != "ariad-core");
        let external_statuses = if needs_external_probe {
            let caps = match active_capabilities() {
                Ok(c) => c,
                Err(e) => return Ok(tool_convert_error(&e)),
            };
            let engine_prog = self.engine_program.clone();
            let rows =
                tokio::task::spawn_blocking(move || engines::engines(&engine_prog, &caps, ct))
                    .await
                    .map_err(|e| {
                        ErrorData::internal_error(format!("blocking task failed: {e}"), None)
                    })?;
            match rows {
                Ok(rows) => rows,
                Err(e) => return Ok(tool_convert_error(&e)),
            }
        } else {
            Vec::new()
        };

        let descriptors: Vec<McpToolDescriptor> = specs
            .into_iter()
            .map(|spec| {
                let d = spec.to_descriptor();
                let engine_status = if d.engine == "ariad-pdf" || d.engine == "ariad-core" {
                    "available".to_owned()
                } else {
                    external_statuses
                        .iter()
                        .find(|row| row.id == d.engine)
                        .map_or_else(
                            || "missing".to_owned(),
                            |row| match row.status.as_str() {
                                "ready" => "available".to_owned(),
                                "wrong version" | "cannot start" => "incompatible".to_owned(),
                                _ => "missing".to_owned(),
                            },
                        )
                };
                McpToolDescriptor {
                    id: d.id,
                    version: d.version,
                    group: d.group,
                    summary: d.summary,
                    inputs: d.inputs,
                    input_formats: d.input_formats,
                    output_format: d.output_format,
                    params_schema: d.params_schema,
                    tier: d.tier,
                    runtimes: d.runtimes,
                    engine: d.engine,
                    engine_status,
                    secret_params: d.secret_params,
                }
            })
            .collect();

        tool_success(&descriptors, vec![])
    }

    /// Execute a registered document manipulation tool inside configured allowed directories.
    #[tool(
        description = "Execute a document tool inside allowed directories; use secret_put handles for secret fields.",
        output_schema = rmcp::handler::server::tool::schema_for_output::<ToolOutcome>()
    )]
    pub async fn run_doc_tool(
        &self,
        Parameters(args): Parameters<RunDocToolArgs>,
        ct: CancellationToken,
    ) -> Result<CallToolResult, ErrorData> {
        let params = match validate_mcp_tool_params(&args.tool, args.params.as_ref()) {
            Ok(p) => p,
            Err(err) => return Ok(tool_run_error(&err)),
        };

        let handles: Vec<String> = mcp_spec(&args.tool)
            .into_iter()
            .flat_map(|spec| spec.secret_params.iter())
            .filter_map(|key| {
                params
                    .get(key)
                    .and_then(|value| match SecretRef::from_json_value(key, value) {
                        Ok(SecretRef::Handle(id)) => Some(id),
                        _ => None,
                    })
            })
            .collect();
        let secrets = match self.secrets.take(&handles) {
            Ok(bag) => bag,
            Err(()) => {
                return Ok(tool_run_error(&ToolError::InvalidParams(
                    "secret handle is unavailable".to_owned(),
                )));
            }
        };

        if args.output.is_some() && args.output_dir.is_some() {
            return Ok(tool_run_error(&ToolError::Usage(
                "specify at most one of 'output' or 'output_dir'".to_owned(),
            )));
        }

        if !ariad_core::tools::ToolId::is_valid(&args.tool) || mcp_spec(&args.tool).is_none() {
            return Ok(tool_run_error(&ToolError::UnknownTool(args.tool)));
        }

        let mut raw_inputs = Vec::with_capacity(args.inputs.len());
        for raw_in in &args.inputs {
            match parse_path_or_uri(raw_in) {
                Ok(path) => raw_inputs.push(path),
                Err(err) => return Ok(tool_confine_error(&err)),
            }
        }

        let output_target = if let Some(ref out_str) = args.output {
            match parse_path_or_uri(out_str) {
                Ok(path) => OutputTarget::Path(path),
                Err(err) => return Ok(tool_confine_error(&err)),
            }
        } else if let Some(ref dir_str) = args.output_dir {
            match parse_path_or_uri(dir_str) {
                Ok(dir) => OutputTarget::Directory(dir),
                Err(err) => return Ok(tool_confine_error(&err)),
            }
        } else {
            OutputTarget::DefaultBesideInput
        };

        let (input_refs, allowed) = {
            let allowed = self
                .allowed_dirs
                .read()
                .map_err(|_| ErrorData::internal_error("internal lock error", None))?;
            let mut confined_refs = Vec::with_capacity(raw_inputs.len());
            for path in &raw_inputs {
                match confine_input(path, &allowed) {
                    Ok(valid_in) => confined_refs.push(InputRef::from_confined(valid_in)),
                    Err(err) => return Ok(tool_confine_error(&err)),
                }
            }
            (confined_refs, allowed.clone())
        };

        #[cfg(feature = "test-probe")]
        let spec = mcp_spec(&args.tool).expect("tool checked above");
        let scrubber = secrets.scrubber();
        let request = ToolRequest::new(args.tool, input_refs, output_target)
            .with_params(params)
            .with_secrets(secrets)
            .with_secret_source(SecretSource::Mcp)
            .with_overwrite(args.overwrite.unwrap_or(false))
            .with_allow_overwrite(self.allow_overwrite)
            .with_allowed_dirs(allowed)
            .with_engine_program(self.engine_program.clone());

        #[cfg(feature = "test-probe")]
        let request = if std::env::var_os("ARIAD_MCP_TEST_PROBE").is_some() {
            let mut command =
                ariad_host::runner::EngineCommand::builtin(&self.engine_program, spec.engine);
            if let Some(program) = std::env::var_os("ARIAD_MCP_TEST_PROBE_PROGRAM") {
                command.program = program.into();
                command.args.clear();
                command.launcher = None;
            }
            command.sandbox = Some(ariad_host::sandbox::SandboxMode::Off);
            request.with_spec(*spec).with_engine_command(command)
        } else {
            request
        };

        let res = self
            .blocking_job(ct, move |cancel| {
                let req = request.with_cancel(cancel);
                run_tool_with_events(&req, |_| {})
            })
            .await?;

        let outcome = match res {
            Ok(outcome) => outcome,
            Err(err) => {
                let result = tool_run_error(&err);
                let value =
                    scrubber.scrub_value(&serde_json::to_value(result).map_err(|_| {
                        ErrorData::internal_error("result serialization failed", None)
                    })?);
                return serde_json::from_value(value)
                    .map_err(|_| ErrorData::internal_error("result serialization failed", None));
            }
        };

        let mut extra_blocks = Vec::new();
        for artifact in &outcome.artifacts {
            let canonical_path =
                std::fs::canonicalize(&artifact.path).unwrap_or_else(|_| artifact.path.clone());
            let file_uri = path_to_file_uri(&canonical_path);
            let mime = mime_type_for_path(&canonical_path);
            self.resources
                .record(file_uri.clone(), canonical_path.clone());

            let mut resource = Resource::new(
                file_uri,
                canonical_path
                    .file_name()
                    .map(|f| f.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "output".to_owned()),
            )
            .with_mime_type(mime);
            resource.size = Some(artifact.bytes);
            extra_blocks.push(ContentBlock::resource_link(resource));
        }

        if let Some(ref inline_json) = outcome.inline_json
            && let Ok(inline_str) = serde_json::to_string_pretty(inline_json)
            && inline_str.len() <= 256 * 1024
        {
            extra_blocks.push(ContentBlock::text(inline_str));
        }

        let result = tool_success(&outcome, extra_blocks)?;
        let value = scrubber.scrub_value(
            &serde_json::to_value(result)
                .map_err(|_| ErrorData::internal_error("result serialization failed", None))?,
        );
        serde_json::from_value(value)
            .map_err(|_| ErrorData::internal_error("result serialization failed", None))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Error, ErrorKind};

    #[test]
    fn compliance_failure_exposes_structured_report() {
        let report =
            serde_json::json!({"report": {"jobs": [{"validationResult": [{"compliant": false}]}]}});
        let error = ToolError::ComplianceFailed {
            standard: "PDF/A-2b".into(),
            report: Box::new(report.clone()),
        };
        let result = tool_run_error(&error);
        assert_eq!(result.is_error, Some(true));
        let content = result.structured_content.unwrap();
        assert_eq!(content["error"]["code"], "compliance_failed");
        assert_eq!(content["error"]["exit_code"], 1);
        assert_eq!(content["error"]["standard"], "PDF/A-2b");
        assert_eq!(content["error"]["report"], report);
    }

    #[test]
    fn staging_errors_match_host_codes_and_exit_statuses() {
        for (kind, expected) in [
            (ErrorKind::FileTooLarge, ConvertError::LimitExceeded),
            (ErrorKind::Interrupted, ConvertError::Interrupted),
            (ErrorKind::PermissionDenied, ConvertError::InputIo),
            (ErrorKind::NotFound, ConvertError::InputIo),
            (ErrorKind::Other, ConvertError::InputIo),
        ] {
            let result = tool_convert_error(&input_error(&Error::from(kind)));
            assert_eq!(result.is_error, Some(true));
            let value = result.structured_content.unwrap();
            assert_eq!(value["code"], expected.code());
            assert_eq!(value["exit_code"], expected.exit_code());
            assert_eq!(value["message"], expected.to_string());
        }
    }
}
