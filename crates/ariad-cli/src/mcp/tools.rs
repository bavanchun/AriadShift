//! MCP tool definitions and implementations for AriadShift.

use std::{
    path::PathBuf,
    sync::{Arc, RwLock},
};

use ariad_core::planner::Profile;
use ariad_host::{
    commands::{active_capabilities, engines, inspect, plan},
    confine::{AllowedDirs, ConfineError, confine_input, confine_output, parse_file_uri},
    convert::{ANALYSIS_MAX_BYTES, ConvertError, ConvertOutput, ConvertRequest, convert_confined},
};
use rmcp::{
    ErrorData,
    handler::server::wrapper::Parameters,
    model::{CallToolResult, ContentBlock, Resource},
    tool, tool_router,
};
use schemars::JsonSchema;
use serde::Deserialize;
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
    /// Target format identifier (md, html, docx, epub).
    pub to: String,
    /// Routing optimization profile (editable, faithful, fast, private).
    pub profile: Option<String>,
}

/// Arguments for `convert` tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ConvertArgs {
    /// Input document filesystem path or file:// URI.
    pub input: String,
    /// Target format identifier (md, html, docx, epub).
    pub to: String,
    /// Optional output file path or file:// URI (defaults to input stem with target extension).
    pub output: Option<String>,
    /// Allow overwriting existing destination (only honoured if server has --allow-overwrite).
    pub overwrite: Option<bool>,
    /// Routing optimization profile (editable, faithful, fast, private).
    pub profile: Option<String>,
}

/// The core MCP server state and tool router.
#[derive(Clone)]
pub struct McpServer {
    pub allow_overwrite: bool,
    pub allowed_dirs: Arc<RwLock<AllowedDirs>>,
    pub cli_allow_dirs: Vec<PathBuf>,
    pub resources: SessionResources,
    pub engine_program: PathBuf,
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
                return Ok(tool_error("invalid_capabilities", 2, e.to_string()));
            }
        };

        let engine_prog = self.engine_program.clone();
        let rows = tokio::task::spawn_blocking(move || engines::engines(&engine_prog, &caps, ct))
            .await
            .map_err(|e| ErrorData::internal_error(format!("blocking task failed: {e}"), None))?;

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

        // Create isolated copy to eliminate C2 input swap race
        let (_isolated_temp, isolated_input) =
            match validated_input.create_isolated_copy(None, Some(ANALYSIS_MAX_BYTES)) {
                Ok(pair) => pair,
                Err(e) => {
                    return Ok(tool_error(
                        "input_io",
                        2,
                        format!("failed to read input file: {e}"),
                    ));
                }
            };

        let caps = match active_capabilities() {
            Ok(c) => c,
            Err(e) => {
                return Ok(tool_error("invalid_capabilities", 2, e.to_string()));
            }
        };

        let engine_prog = self.engine_program.clone();
        let res = tokio::task::spawn_blocking(move || {
            inspect::inspect(&isolated_input, Some(&engine_prog), &caps, ct)
        })
        .await
        .map_err(|e| ErrorData::internal_error(format!("blocking task failed: {e}"), None))?;

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

        // Create isolated copy to eliminate C2 input swap race
        let (_isolated_temp, isolated_input) =
            match validated_input.create_isolated_copy(None, Some(ANALYSIS_MAX_BYTES)) {
                Ok(pair) => pair,
                Err(e) => {
                    return Ok(tool_error(
                        "input_io",
                        2,
                        format!("failed to read input file: {e}"),
                    ));
                }
            };

        let caps = match active_capabilities() {
            Ok(c) => c,
            Err(e) => {
                return Ok(tool_error("invalid_capabilities", 2, e.to_string()));
            }
        };

        let profile = match parse_profile(args.profile.as_deref()) {
            Ok(p) => p,
            Err(e) => return Ok(e),
        };
        let to = args.to.clone();
        let engine_prog = self.engine_program.clone();
        let res = tokio::task::spawn_blocking(move || {
            plan::plan(&isolated_input, &to, profile, &caps, Some(&engine_prog), ct)
        })
        .await
        .map_err(|e| ErrorData::internal_error(format!("blocking task failed: {e}"), None))?;

        match res {
            Ok(report) => tool_success(&report, vec![]),
            Err(e) => Ok(tool_convert_error(&e)),
        }
    }

    /// Convert a document between supported formats (Markdown, HTML, DOCX, EPUB).
    #[tool(
        description = "Convert a document between supported formats (Markdown, HTML, DOCX, EPUB)",
        output_schema = rmcp::handler::server::tool::schema_for_output::<ariad_host::convert::ConvertOutput>()
    )]
    pub async fn convert(
        &self,
        Parameters(args): Parameters<ConvertArgs>,
        ct: CancellationToken,
    ) -> Result<CallToolResult, ErrorData> {
        self.in_flight
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let in_flight = self.in_flight.clone();
        struct InFlightGuard(Arc<std::sync::atomic::AtomicUsize>);
        impl Drop for InFlightGuard {
            fn drop(&mut self) {
                self.0.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
            }
        }
        let _guard = InFlightGuard(in_flight);

        let profile = match parse_profile(args.profile.as_deref()) {
            Ok(p) => p,
            Err(e) => return Ok(e),
        };

        let input_path = match parse_path_or_uri(&args.input) {
            Ok(p) => p,
            Err(e) => return Ok(tool_confine_error(&e)),
        };

        if ariad_host::convert::DocumentFormat::parse(&args.to).is_none() {
            let clean_target = ariad_host::convert::sanitize_identifier(&args.to, 64);
            let detail = match ariad_host::convert::detect_format_from_path(&input_path) {
                Some(in_doc_fmt) => {
                    let in_fmt = ariad_core::format::Format::from(in_doc_fmt);
                    let caps = ariad_host::commands::active_capabilities().ok();
                    let reachable = caps.as_ref().map(|c| {
                        let engines = c.registered_engines();
                        ariad_core::planner::reachable_formats(c, in_fmt, profile, &engines)
                    });
                    if let Some(r) = reachable {
                        let targets_detail =
                            ariad_host::convert::reachable_targets_detail(in_doc_fmt.as_str(), &r);
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

        // Create isolated copy to eliminate C2 input swap race
        let (_isolated_temp, isolated_input) =
            match validated_input.create_isolated_copy(None, Some(ANALYSIS_MAX_BYTES)) {
                Ok(pair) => pair,
                Err(e) => {
                    return Ok(tool_error(
                        "input_io",
                        2,
                        format!("failed to read input file: {e}"),
                    ));
                }
            };

        let original_parent = validated_input.path.parent().map(|p| p.to_path_buf());
        let request = ConvertRequest {
            input: isolated_input,
            output: validated_output.destination.clone(),
            target_format: args.to.clone(),
            profile,
            overwrite: validated_output.overwrite,
            engine_program: self.engine_program.clone(),
            title_fallback: None,
            asset_base_dir: original_parent,
        };

        let parent_dir = validated_output.parent_dir;
        let file_name = validated_output.file_name;
        let cancel = tokio_util::sync::CancellationToken::new();
        let cancel_child = cancel.clone();
        let s_cancel = self.cancel_token.clone();
        let req_cancel = ct.clone();
        tokio::spawn(async move {
            tokio::select! {
                _ = s_cancel.cancelled() => cancel_child.cancel(),
                _ = req_cancel.cancelled() => cancel_child.cancel(),
            }
        });

        let res = tokio::task::spawn_blocking(move || {
            convert_confined(
                &request,
                &parent_dir,
                std::path::Path::new(&file_name),
                cancel,
                |_| {},
            )
        })
        .await
        .map_err(|e| ErrorData::internal_error(format!("blocking task failed: {e}"), None))?;

        let report = match res {
            Ok(r) => r,
            Err(e) => return Ok(tool_convert_error(&e)),
        };

        // On successful promotion, build output blocks without reading destination by path (C1)
        let final_dest = validated_output.destination;
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
        } else if (args.to == "md" || args.to == "markdown") && file_size > 256 * 1024 {
            extra_blocks.push(ContentBlock::text(
                "Markdown content exceeds 256 KiB; read the file directly or via resources/read.",
            ));
        }

        let convert_output = ConvertOutput::from(&report);
        tool_success(&convert_output, extra_blocks)
    }
}
