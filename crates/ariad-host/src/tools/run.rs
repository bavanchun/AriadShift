//! Host tool execution orchestrator (`ToolRequest`, `ToolOutcome`, `ToolError`, `run_tool`).
//!
//! Enforces the normative execution contract in `docs/tools.md` §§6–13:
//! 1. Registry lookup (`unknown_tool`, exit code `8`).
//! 2. Tier access check via [`ariad_core::tools::access::allowed`] (`tier_not_allowed`, exit code `11`).
//! 3. Secret parameter validation (`x-ariad-secret`) and JSON Schema validation (`invalid_params`, exit code `9`).
//! 4. Input arity check (`wrong_input`, exit code `10`) and Free-plan run quota check (`tier_not_allowed`, exit code `11`).
//! 5. Path confinement (`AllowedDirs`), self-overwrite refusal (`usage`, exit code `2`), and content format sniffing (`wrong_input`, exit code `10`).
//! 6. Isolated workspace staging and dispatch to [`super::inprocess`] (`ariad-pdf`) or [`super::engine`] (`ariad-engine/2`).
//! 7. Atomic multi-artifact publication via [`super::publish::publish_atomic`] (`destination_exists`, exit code `6`).

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    time::Instant,
};

use ariad_core::{
    limits::Limits,
    tools::{
        Plan, RunSize, SecretSource, Tier, ToolId, ToolSpec,
        access::{allowed, within_run_quota},
    },
};
use schemars::{JsonSchema, Schema};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use tokio_util::sync::CancellationToken;

use crate::{
    confine::{AllowedDirs, ConfineError, confine_input},
    convert::{ConvertWarning, classify_file, paths_refer_to_same_file},
    engines,
    runner::EngineCommand,
    tools::{
        engine,
        inprocess::{self, INPROCESS_PDF_ENGINE},
        publish::{
            PublishError, PublishedArtifact, StagedArtifact, default_multi_artifact_name,
            default_single_artifact_name, ensure_no_parent_traversal, open_confined_output_dir,
            publish_atomic, render_naming_template, validate_target_filename,
        },
        secrets::{SecretBag, SecretScrubber},
    },
    workspace::Workspace,
};

/// Input file reference for a [`ToolRequest`].
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct InputRef {
    /// Filesystem path to the input document.
    pub path: PathBuf,
}

impl InputRef {
    /// Creates a new [`InputRef`] from a path.
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }
}

impl From<PathBuf> for InputRef {
    fn from(path: PathBuf) -> Self {
        Self { path }
    }
}

impl From<&Path> for InputRef {
    fn from(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
        }
    }
}

impl From<&str> for InputRef {
    fn from(path: &str) -> Self {
        Self {
            path: PathBuf::from(path),
        }
    }
}

impl From<String> for InputRef {
    fn from(path: String) -> Self {
        Self {
            path: PathBuf::from(path),
        }
    }
}

/// Destination target for artifacts produced by a [`ToolRequest`] (`docs/tools.md` §6).
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", content = "path", rename_all = "snake_case")]
pub enum OutputTarget {
    /// Do not publish any file to disk (used by `pdf.info` without `-o`).
    None,
    /// Write output artifact(s) beside `inputs[0]` using default naming (`<stem>.<suffix>.<ext>`).
    #[default]
    DefaultBesideInput,
    /// Explicit `-o <file_or_dir>` path.
    Path(PathBuf),
    /// Explicit output directory (`-o <dir>/` or MCP `output_dir`).
    Directory(PathBuf),
}

/// Progress or lifecycle event emitted during a tool run.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ToolEvent {
    /// Progress update from an in-process or out-of-process tool.
    Progress {
        /// Human-readable stage identifier.
        stage: String,
        /// Completed units, if known.
        done: Option<u64>,
        /// Total units, if known.
        total: Option<u64>,
    },
}

/// Host request to execute a single registered document tool.
#[derive(Clone, Debug)]
pub struct ToolRequest {
    /// Requested tool identifier (e.g. `"pdf.info"` or `"pdf.rotate"`).
    pub tool: String,
    /// Ordered list of input files.
    pub inputs: Vec<InputRef>,
    /// Output destination target.
    pub output: OutputTarget,
    /// Tool parameter JSON object (defaults to `{}`).
    pub params: Value,
    /// Out-of-band resolved secrets (`--secret`, prompt, or MCP handle).
    pub secrets: SecretBag,
    /// Resource limits enforced on input, engine execution, and output.
    pub limits: Limits,
    /// Originating channel used to validate `x-ariad-secret` rules.
    pub secret_source: SecretSource,
    /// Active subscription plan used for `ToolAccess` tier and quota checks.
    pub plan: Plan,
    /// Whether existing destination files may be replaced.
    pub overwrite: bool,
    /// Whether the hosting surface permits `overwrite: true` (e.g. MCP `--allow-overwrite`).
    pub allow_overwrite: bool,
    /// Optional directory confinement policy (local MCP / server).
    pub allowed_dirs: Option<AllowedDirs>,
    /// Optional recipe naming template (`{stem}`, `{n}`, `{date}`).
    pub naming_template: Option<String>,
    /// Optional path to the `ashift` binary for resolving built-in engines and packs.
    pub engine_program: Option<PathBuf>,
    /// Optional explicit engine command override (used by integration tests).
    pub engine_command: Option<EngineCommand>,
    /// Optional tool specification override (used by integration tests for pack tools).
    pub spec_override: Option<ToolSpec>,
    /// Optional 0-based artifact index at which atomic publication should inject a failure (for tests).
    pub fail_publish_at_index: Option<usize>,
    /// Cooperative cancellation token.
    pub cancel: CancellationToken,
}

impl ToolRequest {
    /// Creates a new [`ToolRequest`] with default local limits, `Plan::Free`, and `SecretSource::Argv`.
    #[must_use]
    pub fn new(tool: impl Into<String>, inputs: Vec<InputRef>, output: OutputTarget) -> Self {
        Self {
            tool: tool.into(),
            inputs,
            output,
            params: Value::Object(Map::new()),
            secrets: SecretBag::new(),
            limits: Limits::local(),
            secret_source: SecretSource::Argv,
            plan: Plan::Free,
            overwrite: false,
            allow_overwrite: true,
            allowed_dirs: None,
            naming_template: None,
            engine_program: None,
            engine_command: None,
            spec_override: None,
            fail_publish_at_index: None,
            cancel: CancellationToken::new(),
        }
    }

    /// Sets the tool `params` JSON value.
    #[must_use]
    pub fn with_params(mut self, params: Value) -> Self {
        self.params = params;
        self
    }

    /// Sets the [`SecretBag`] for this request.
    #[must_use]
    pub fn with_secrets(mut self, secrets: SecretBag) -> Self {
        self.secrets = secrets;
        self
    }

    /// Sets the [`SecretSource`] channel for secret parameter validation.
    #[must_use]
    pub fn with_secret_source(mut self, source: SecretSource) -> Self {
        self.secret_source = source;
        self
    }

    /// Sets the [`Limits`] for this request.
    #[must_use]
    pub fn with_limits(mut self, limits: Limits) -> Self {
        self.limits = limits;
        self
    }

    /// Sets the [`Plan`] for tier and batch quota evaluation.
    #[must_use]
    pub fn with_plan(mut self, plan: Plan) -> Self {
        self.plan = plan;
        self
    }

    /// Sets `overwrite` on this request.
    #[must_use]
    pub fn with_overwrite(mut self, overwrite: bool) -> Self {
        self.overwrite = overwrite;
        self
    }

    /// Sets `allow_overwrite` on this request.
    #[must_use]
    pub fn with_allow_overwrite(mut self, allow_overwrite: bool) -> Self {
        self.allow_overwrite = allow_overwrite;
        self
    }

    /// Confines all input and output paths to `allowed_dirs`.
    #[must_use]
    pub fn with_allowed_dirs(mut self, allowed_dirs: AllowedDirs) -> Self {
        self.allowed_dirs = Some(allowed_dirs);
        self
    }

    /// Sets an output naming template (`{stem}`, `{n}`, `{date}`).
    #[must_use]
    pub fn with_naming_template(mut self, template: impl Into<String>) -> Self {
        self.naming_template = Some(template.into());
        self
    }

    /// Sets the `ashift` engine launcher binary path.
    #[must_use]
    pub fn with_engine_program(mut self, program: impl Into<PathBuf>) -> Self {
        self.engine_program = Some(program.into());
        self
    }

    /// Overrides the [`EngineCommand`] used for out-of-process execution.
    #[must_use]
    pub fn with_engine_command(mut self, command: EngineCommand) -> Self {
        self.engine_command = Some(command);
        self
    }

    /// Overrides the [`ToolSpec`] used for validation and dispatch.
    #[must_use]
    pub fn with_spec(mut self, spec: ToolSpec) -> Self {
        self.spec_override = Some(spec);
        self
    }

    /// Injects a deterministic publication failure at 0-based artifact `index`.
    #[must_use]
    pub fn with_fail_publish_at(mut self, index: usize) -> Self {
        self.fail_publish_at_index = Some(index);
        self
    }

    /// Sets the [`CancellationToken`] for this request.
    #[must_use]
    pub fn with_cancel(mut self, cancel: CancellationToken) -> Self {
        self.cancel = cancel;
        self
    }
}

/// Structured metrics map returned in [`ToolOutcome`].
pub type ToolMetrics = BTreeMap<String, Value>;

/// Outcome of a successful tool execution (`docs/tools.md` §12, §13).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ToolOutcome {
    /// Canonical tool identifier (e.g. `"pdf.info"`).
    pub tool: String,
    /// Tool contract version string (e.g. `"1"`).
    pub version: String,
    /// Executing engine identifier (e.g. `"ariad-pdf"` or `"qpdf"`).
    pub engine: String,
    /// Atomically published output files.
    pub artifacts: Vec<PublishedArtifact>,
    /// Secret-scrubbed warnings emitted during execution.
    pub warnings: Vec<ConvertWarning>,
    /// Execution metrics (always includes `elapsed_ms`).
    pub metrics: ToolMetrics,
    /// Inline structured JSON payload for JSON-emitting tools (`pdf.info`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inline_json: Option<Value>,
}

/// Normative error type for tool and recipe execution (`docs/tools.md` §12, §13).
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, thiserror::Error)]
#[serde(tag = "code", content = "detail", rename_all = "snake_case")]
pub enum ToolError {
    /// Exit code `8` (`unknown_tool`): requested tool ID is not in the registry or violates grammar.
    #[error("unknown tool '{0}'")]
    UnknownTool(String),
    /// Exit code `11` (`tier_not_allowed`): tool requires `Tier::Pro` or run exceeds Free batch quota.
    #[error("tool '{tool}' requires the {tier} tier (current plan: {plan})")]
    TierNotAllowed {
        /// Tool ID or `"recipe"`.
        tool: String,
        /// Required tier.
        tier: Tier,
        /// Active plan.
        plan: Plan,
    },
    /// Exit code `9` (`invalid_params`): `params` failed JSON Schema validation, secret rules, or recipe rules.
    #[error("invalid tool parameters: {0}")]
    InvalidParams(String),
    /// Exit code `10` (`wrong_input`): input count or sniffed format does not match `ToolSpec`.
    #[error("wrong tool input: {0}")]
    WrongInput(String),
    /// Exit code `12` (`encrypted_input`): input PDF is encrypted and requires a password / `pdf.decrypt`.
    #[error("{}", format_encrypted_error(*.needs_password))]
    EncryptedInput {
        /// Whether a user password is required to open the document.
        needs_password: bool,
    },
    /// Exit code `2` (`usage`): invalid flags, multi-output tool given a file `-o`, or disallowed overwrite.
    #[error("{0}")]
    Usage(String),
    /// Exit code `2` (`usage`): destination path resolves to one of the input files.
    #[error("the output path cannot be the same as an input file")]
    DestinationSameAsInput,
    /// Exit code `2` (`usage`): input or output path violated directory confinement.
    #[error("{0}")]
    Confinement(String),
    /// Exit code `3` (`unsupported_route`): unreachable conversion route inside a recipe `convert` step.
    #[error("unsupported conversion route: {0}")]
    UnsupportedRoute(String),
    /// Exit code `4` (`limit_exceeded`): input bytes, pages, decompressed stream bytes, memory, or timeout exceeded `Limits`.
    #[error("tool resource limit exceeded")]
    LimitExceeded,
    /// Exit code `5` (`tool_missing`): required engine pack is not installed or failed verification.
    #[error("the {engine} engine is not available; {hint}")]
    ToolMissing {
        /// Missing engine identifier.
        engine: String,
        /// Actionable installation hint.
        hint: String,
    },
    /// Exit code `5` (`sandbox_unavailable`): required OS sandbox isolation is unavailable.
    #[error("the engine sandbox is unavailable: {0}")]
    SandboxUnavailable(String),
    /// Exit code `6` (`destination_exists`): one or more target files already exist and `overwrite` is `false`.
    #[error("the destination '{0}' already exists; pass --overwrite to replace it")]
    DestinationExists(String),
    /// Exit code `130` (`interrupted`): cancelled via `SIGINT` / `CancellationToken`.
    #[error("tool execution was interrupted")]
    Interrupted,
    /// Exit code `1` (`io`): input file could not be read.
    #[error("{0}")]
    InputIo(String),
    /// Exit code `1` (`io`): output file could not be written.
    #[error("{0}")]
    OutputIo(String),
    /// Exit code `1` (`engine_failure`): corrupt input, engine crash, or runtime tool failure.
    #[error("{0}")]
    EngineFailure(String),
}

fn format_encrypted_error(needs_password: bool) -> String {
    if needs_password {
        "input PDF is encrypted and requires a password; decrypt it with 'pdf.decrypt' or supply --secret password=@<file>".to_owned()
    } else {
        "input PDF is encrypted; decrypt it with 'pdf.decrypt' before running mutating tools"
            .to_owned()
    }
}

impl ToolError {
    /// Normative CLI and MCP exit code (`docs/tools.md` §12).
    #[must_use]
    pub const fn exit_code(&self) -> u8 {
        match self {
            Self::EngineFailure(_) | Self::InputIo(_) | Self::OutputIo(_) => 1,
            Self::Usage(_) | Self::DestinationSameAsInput | Self::Confinement(_) => 2,
            Self::UnsupportedRoute(_) => 3,
            Self::LimitExceeded => 4,
            Self::ToolMissing { .. } | Self::SandboxUnavailable(_) => 5,
            Self::DestinationExists(_) => 6,
            Self::UnknownTool(_) => 8,
            Self::InvalidParams(_) => 9,
            Self::WrongInput(_) => 10,
            Self::TierNotAllowed { .. } => 11,
            Self::EncryptedInput { .. } => 12,
            Self::Interrupted => 130,
        }
    }

    /// Normative machine-readable error code string (`docs/tools.md` §12, §13).
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::EngineFailure(_) => "engine_failure",
            Self::InputIo(_) | Self::OutputIo(_) => "io",
            Self::Usage(_) | Self::DestinationSameAsInput | Self::Confinement(_) => "usage",
            Self::UnsupportedRoute(_) => "unsupported_route",
            Self::LimitExceeded => "limit_exceeded",
            Self::ToolMissing { .. } => "tool_missing",
            Self::SandboxUnavailable(_) => "sandbox_unavailable",
            Self::DestinationExists(_) => "destination_exists",
            Self::UnknownTool(_) => "unknown_tool",
            Self::InvalidParams(_) => "invalid_params",
            Self::WrongInput(_) => "wrong_input",
            Self::TierNotAllowed { .. } => "tier_not_allowed",
            Self::EncryptedInput { .. } => "encrypted_input",
            Self::Interrupted => "interrupted",
        }
    }

    /// Constructs a [`ToolError::ToolMissing`] with the install hint from [`crate::engines`].
    #[must_use]
    pub fn from_missing_engine(engine: &str) -> Self {
        let hint = engines::pack_spec(engine)
            .or_else(|| engines::spec(engine))
            .map_or_else(
                || format!("run 'ashift engines install {engine}'"),
                |spec| spec.install_hint.to_owned(),
            );
        Self::ToolMissing {
            engine: engine.to_owned(),
            hint,
        }
    }

    /// Scrubs any secret strings from this error's message payload.
    #[must_use]
    pub fn scrubbed(self, scrubber: &SecretScrubber) -> Self {
        if scrubber.is_empty() {
            return self;
        }
        match self {
            Self::UnknownTool(s) => Self::UnknownTool(scrubber.scrub(&s)),
            Self::InvalidParams(s) => Self::InvalidParams(scrubber.scrub(&s)),
            Self::WrongInput(s) => Self::WrongInput(scrubber.scrub(&s)),
            Self::Usage(s) => Self::Usage(scrubber.scrub(&s)),
            Self::Confinement(s) => Self::Confinement(scrubber.scrub(&s)),
            Self::UnsupportedRoute(s) => Self::UnsupportedRoute(scrubber.scrub(&s)),
            Self::ToolMissing { engine, hint } => Self::ToolMissing {
                engine: scrubber.scrub(&engine),
                hint: scrubber.scrub(&hint),
            },
            Self::SandboxUnavailable(s) => Self::SandboxUnavailable(scrubber.scrub(&s)),
            Self::DestinationExists(s) => Self::DestinationExists(scrubber.scrub(&s)),
            Self::InputIo(s) => Self::InputIo(scrubber.scrub(&s)),
            Self::OutputIo(s) => Self::OutputIo(scrubber.scrub(&s)),
            Self::EngineFailure(s) => Self::EngineFailure(scrubber.scrub(&s)),
            other => other,
        }
    }
}

impl From<PublishError> for ToolError {
    fn from(err: PublishError) -> Self {
        match err {
            PublishError::DestinationSameAsInput => Self::DestinationSameAsInput,
            PublishError::Confinement(msg) => Self::Confinement(msg),
            PublishError::DestinationExists(name) => Self::DestinationExists(name),
            PublishError::NamingTemplate(e) => Self::InvalidParams(e.to_string()),
            PublishError::InvalidStagedArtifact(msg) => Self::EngineFailure(msg),
            PublishError::InjectedFailure { index } => Self::EngineFailure(format!(
                "injected publication failure at artifact index {index}"
            )),
            PublishError::Io(msg) => Self::OutputIo(msg),
        }
    }
}

/// Executes a single registered document tool synchronously with no event callback.
pub fn run_tool(request: &ToolRequest) -> Result<ToolOutcome, ToolError> {
    run_tool_with_events(request, |_| {})
}

/// Executes a single registered document tool synchronously, streaming [`ToolEvent`]s to `on_event`.
pub fn run_tool_with_events<F>(
    request: &ToolRequest,
    mut on_event: F,
) -> Result<ToolOutcome, ToolError>
where
    F: FnMut(ToolEvent),
{
    let base_scrubber = request.secrets.scrubber();
    let result = run_tool_inner(request, &mut on_event);
    result.map_err(|err| err.scrubbed(&base_scrubber))
}

fn run_tool_inner(
    request: &ToolRequest,
    on_event: &mut dyn FnMut(ToolEvent),
) -> Result<ToolOutcome, ToolError> {
    let started = Instant::now();

    if request.cancel.is_cancelled() {
        return Err(ToolError::Interrupted);
    }

    // 1. Resolve tool specification from registry (exit code 8 if unknown or invalid ID).
    let spec: ToolSpec = if let Some(override_spec) = request.spec_override {
        override_spec
    } else {
        if !ToolId::is_valid(&request.tool) {
            return Err(ToolError::UnknownTool(request.tool.clone()));
        }
        *ariad_core::tools::find(&request.tool)
            .ok_or_else(|| ToolError::UnknownTool(request.tool.clone()))?
    };

    // 2. Check tier access (exit code 11 if Pro tool on Free plan).
    if !allowed(spec.tier, request.plan) {
        return Err(ToolError::TierNotAllowed {
            tool: spec.id.to_owned(),
            tier: spec.tier,
            plan: request.plan,
        });
    }

    // 3. Validate resource limits (exit code 4).
    if request.limits.validate().is_err() {
        return Err(ToolError::LimitExceeded);
    }

    // 4. Validate secret rules and resolve secrets from `SecretBag` (exit code 9).
    let resolved_secrets = request
        .secrets
        .resolve_params(&spec, &request.params, request.secret_source)
        .map_err(ToolError::InvalidParams)?;
    let scrubber = &resolved_secrets.scrubber;

    // 5. Validate resolved parameters against the tool's JSON Schema (exit code 9).
    let param_schema = spec.resolved_params_schema();
    validate_params_against_schema(&param_schema, &resolved_secrets.schema_params)
        .map_err(|msg| ToolError::InvalidParams(scrubber.scrub(&msg)))?;

    // Validate optional naming template early if provided (exit code 9).
    if let Some(ref template) = request.naming_template {
        ariad_core::tools::validate_naming_template(template)
            .map_err(|e| ToolError::InvalidParams(e.to_string()))?;
    }

    // 6. Check overwrite permission (exit code 2).
    if request.overwrite && !request.allow_overwrite {
        return Err(ToolError::Usage(
            "overwriting existing files is not enabled".to_owned(),
        ));
    }

    // 7. Check input arity (exit code 10).
    if !spec.inputs.accepts(request.inputs.len()) {
        return Err(ToolError::WrongInput(format!(
            "tool '{}' requires {:?} input(s), got {}",
            spec.id,
            spec.inputs,
            request.inputs.len()
        )));
    }

    // 8. Pre-validate output target confinement before staging or running the tool (exit code 2 / 6).
    let input_paths: Vec<PathBuf> = request.inputs.iter().map(|i| i.path.clone()).collect();
    preflight_output_target(
        &request.output,
        &input_paths,
        request.allowed_dirs.as_ref(),
        request.overwrite,
    )?;

    // 9. Confine, size-check, sniff format, and stage inputs into an isolated workspace.
    let workspace =
        Workspace::new().map_err(|e| ToolError::OutputIo(scrubber.scrub(&e.to_string())))?;

    let mut total_input_bytes = 0_u64;
    let mut staged_inputs_with_format: Vec<(PathBuf, String)> =
        Vec::with_capacity(request.inputs.len());
    let mut staged_input_paths: Vec<PathBuf> = Vec::with_capacity(request.inputs.len());

    for (idx, input_ref) in request.inputs.iter().enumerate() {
        let resolved_input = resolve_and_confine_input(
            &input_ref.path,
            request.allowed_dirs.as_ref(),
            &request.limits,
        )?;

        let classification = classify_file(&resolved_input).map_err(|_| {
            ToolError::InputIo(format!(
                "the input file '{}' could not be read",
                input_ref.path.display()
            ))
        })?;

        if let Some(max_in) = request.limits.max_input_bytes
            && classification.bytes > max_in
        {
            return Err(ToolError::LimitExceeded);
        }
        total_input_bytes = total_input_bytes.saturating_add(classification.bytes);

        let format_id = match classification.format {
            Some(fmt) if spec.accepts_core_format(fmt) => fmt.id().to_owned(),
            Some(fmt) => {
                return Err(ToolError::WrongInput(format!(
                    "input '{}' has format '{}', which is not accepted by tool '{}' (accepted: {})",
                    input_ref.path.display(),
                    fmt.user_name(),
                    spec.id,
                    spec.input_formats.join(", ")
                )));
            }
            None => {
                let ext = input_ref
                    .path
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("unknown");
                return Err(ToolError::WrongInput(format!(
                    "input '{}' format ({ext}) is not accepted by tool '{}' (accepted: {})",
                    input_ref.path.display(),
                    spec.id,
                    spec.input_formats.join(", ")
                )));
            }
        };

        let staged_name = format!("input-{idx}.{format_id}");
        let staged_path = workspace.input_dir().join(staged_name);
        fs::copy(&resolved_input, &staged_path).map_err(|_| {
            ToolError::InputIo(format!(
                "could not stage input file '{}'",
                input_ref.path.display()
            ))
        })?;

        staged_input_paths.push(staged_path.clone());
        staged_inputs_with_format.push((staged_path, format_id));
    }

    // Enforce Free-plan per-run batch quota (25 files / 250 MB; exit code 11).
    if !within_run_quota(
        request.plan,
        RunSize {
            files: request.inputs.len(),
            bytes: total_input_bytes,
        },
    ) {
        return Err(ToolError::TierNotAllowed {
            tool: spec.id.to_owned(),
            tier: Tier::Pro,
            plan: request.plan,
        });
    }

    // 10. Dispatch to in-process `ariad-pdf` or out-of-process `ariad-engine/2`.
    let raw_exec = if spec.engine == INPROCESS_PDF_ENGINE && request.engine_command.is_none() {
        inprocess::execute(
            &spec,
            &staged_input_paths,
            &resolved_secrets.schema_params,
            &workspace,
            &request.limits,
            scrubber,
            &request.cancel,
            on_event,
        )
        .map_err(|e| e.scrubbed(scrubber))?
    } else {
        engine::execute(
            &spec,
            &staged_inputs_with_format,
            &resolved_secrets.engine_params,
            resolved_secrets.has_secrets,
            &workspace,
            &request.limits,
            scrubber,
            request.engine_program.as_deref(),
            request.engine_command.as_ref(),
            request.cancel.clone(),
            on_event,
        )
        .map_err(|e| e.scrubbed(scrubber))?
    };

    // 11. Plan and execute atomic multi-artifact publication.
    let published = publish_tool_outputs(
        &workspace,
        &spec,
        &raw_exec.artifacts,
        &input_paths,
        &request.output,
        request.naming_template.as_deref(),
        request.allowed_dirs.as_ref(),
        request.overwrite,
        request.fail_publish_at_index,
    )
    .map_err(|e| ToolError::from(e).scrubbed(scrubber))?;

    let elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let mut metrics = raw_exec.metrics;
    metrics.insert("elapsed_ms".to_owned(), json!(elapsed_ms));

    Ok(ToolOutcome {
        tool: spec.id.to_owned(),
        version: spec.version.to_owned(),
        engine: spec.engine.to_owned(),
        artifacts: published,
        warnings: raw_exec.warnings,
        metrics,
        inline_json: raw_exec.inline_json,
    })
}

fn resolve_and_confine_input(
    path: &Path,
    allowed_dirs: Option<&AllowedDirs>,
    limits: &Limits,
) -> Result<PathBuf, ToolError> {
    if let Some(allowed) = allowed_dirs {
        let confined = confine_input(path, allowed).map_err(|err| match err {
            ConfineError::InputNotFound { .. } => {
                ToolError::InputIo(format!("input file '{}' was not found", path.display()))
            }
            _ => ToolError::Confinement(format!(
                "input path '{}' is not permitted: {err}",
                path.display()
            )),
        })?;
        let meta = confined.file.metadata().map_err(|_| {
            ToolError::InputIo(format!("input file '{}' could not be read", path.display()))
        })?;
        if let Some(max_in) = limits.max_input_bytes
            && meta.len() > max_in
        {
            return Err(ToolError::LimitExceeded);
        }
        Ok(confined.path)
    } else {
        let meta = fs::metadata(path).map_err(|_| {
            ToolError::InputIo(format!("input file '{}' could not be read", path.display()))
        })?;
        if !meta.file_type().is_file() {
            return Err(ToolError::InputIo(format!(
                "input '{}' is not a regular file",
                path.display()
            )));
        }
        if let Some(max_in) = limits.max_input_bytes
            && meta.len() > max_in
        {
            return Err(ToolError::LimitExceeded);
        }
        Ok(path.to_path_buf())
    }
}

fn preflight_output_target(
    output: &OutputTarget,
    inputs: &[PathBuf],
    allowed_dirs: Option<&AllowedDirs>,
    _overwrite: bool,
) -> Result<(), ToolError> {
    match output {
        OutputTarget::None | OutputTarget::DefaultBesideInput => Ok(()),
        OutputTarget::Directory(dir) => {
            ensure_no_parent_traversal(dir)?;
            let _ = open_confined_output_dir(dir, allowed_dirs)?;
            Ok(())
        }
        OutputTarget::Path(path) => {
            ensure_no_parent_traversal(path)?;
            if let Ok(meta) = fs::symlink_metadata(path)
                && meta.file_type().is_symlink()
            {
                return Err(ToolError::Confinement(format!(
                    "output path '{}' must not be a symlink",
                    path.display()
                )));
            }
            for input in inputs {
                if paths_refer_to_same_file(input, path) {
                    return Err(ToolError::DestinationSameAsInput);
                }
            }
            let is_dir_target = path.is_dir()
                || path
                    .as_os_str()
                    .to_string_lossy()
                    .ends_with(['/', std::path::MAIN_SEPARATOR]);
            if is_dir_target {
                let _ = open_confined_output_dir(path, allowed_dirs)?;
            } else {
                let parent = path
                    .parent()
                    .filter(|p| !p.as_os_str().is_empty())
                    .unwrap_or_else(|| Path::new("."));
                let _ = open_confined_output_dir(parent, allowed_dirs)?;
                let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else {
                    return Err(ToolError::Usage(format!(
                        "invalid output file path '{}'",
                        path.display()
                    )));
                };
                validate_target_filename(file_name, allowed_dirs.is_some())?;
            }
            Ok(())
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn publish_tool_outputs(
    workspace: &Workspace,
    spec: &ToolSpec,
    raw_artifacts: &[(PathBuf, String)],
    inputs: &[PathBuf],
    output: &OutputTarget,
    naming_template: Option<&str>,
    allowed_dirs: Option<&AllowedDirs>,
    overwrite: bool,
    fail_at_index: Option<usize>,
) -> Result<Vec<PublishedArtifact>, PublishError> {
    if raw_artifacts.is_empty() {
        return Ok(Vec::new());
    }

    let first_input = inputs
        .first()
        .map(PathBuf::as_path)
        .unwrap_or_else(|| Path::new("document.pdf"));
    let first_stem = first_input
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("document");

    let (dest_dir, explicit_single_filename): (PathBuf, Option<String>) = match output {
        OutputTarget::None => return Ok(Vec::new()),
        OutputTarget::DefaultBesideInput => {
            // `pdf.info` without `-o` emits structured JSON inline without writing a file next to input.
            if spec.id == "pdf.info" && naming_template.is_none() {
                return Ok(Vec::new());
            }
            let parent = first_input
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or_else(|| Path::new("."));
            (parent.to_path_buf(), None)
        }
        OutputTarget::Directory(dir) => (dir.clone(), None),
        OutputTarget::Path(path) => {
            ensure_no_parent_traversal(path)?;
            if let Ok(meta) = fs::symlink_metadata(path)
                && meta.file_type().is_symlink()
            {
                return Err(PublishError::Confinement(format!(
                    "output path '{}' must not be a symlink",
                    path.display()
                )));
            }
            let is_dir = path.is_dir()
                || path
                    .as_os_str()
                    .to_string_lossy()
                    .ends_with(['/', std::path::MAIN_SEPARATOR]);
            if is_dir {
                (path.clone(), None)
            } else {
                if raw_artifacts.len() > 1 {
                    return Err(PublishError::Confinement(format!(
                        "tool '{}' produced {} artifacts and requires an output directory (-o <dir>/)",
                        spec.id,
                        raw_artifacts.len()
                    )));
                }
                let parent = path
                    .parent()
                    .filter(|p| !p.as_os_str().is_empty())
                    .unwrap_or_else(|| Path::new("."));
                let file_name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .ok_or_else(|| {
                        PublishError::Confinement(format!(
                            "invalid output filename '{}'",
                            path.display()
                        ))
                    })?
                    .to_owned();
                (parent.to_path_buf(), Some(file_name))
            }
        }
    };

    let multi = raw_artifacts.len() > 1;
    let mut staged = Vec::with_capacity(raw_artifacts.len());
    for (idx, (staged_path, format)) in raw_artifacts.iter().enumerate() {
        let index_1_based = idx + 1;
        let target_name = if let Some(ref single_name) = explicit_single_filename {
            single_name.clone()
        } else if let Some(template) = naming_template {
            render_naming_template(template, first_stem, index_1_based)?
        } else if multi {
            default_multi_artifact_name(first_input, spec.id, index_1_based, format)
        } else {
            default_single_artifact_name(first_input, spec.id, format)
        };
        staged.push(StagedArtifact::new(
            staged_path.clone(),
            format.clone(),
            target_name,
        ));
    }

    publish_atomic(
        workspace,
        &staged,
        &dest_dir,
        inputs,
        allowed_dirs,
        overwrite,
        fail_at_index,
    )
}

/// Validates a JSON [`Value`] against a Draft 2020-12 [`Schema`] generated by `schemars`.
pub fn validate_params_against_schema(schema: &Schema, params: &Value) -> Result<(), String> {
    let Some(root_obj) = schema.as_object() else {
        return Ok(());
    };
    let defs = root_obj
        .get("$defs")
        .or_else(|| root_obj.get("definitions"))
        .and_then(Value::as_object);
    validate_schema_node(root_obj, defs, params, "params")
}

fn validate_schema_node(
    node: &Map<String, Value>,
    defs: Option<&Map<String, Value>>,
    value: &Value,
    path: &str,
) -> Result<(), String> {
    if let Some(ref_str) = node.get("$ref").and_then(Value::as_str)
        && let Some(def_name) = ref_str
            .strip_prefix("#/$defs/")
            .or_else(|| ref_str.strip_prefix("#/definitions/"))
        && let Some(def_node) = defs
            .and_then(|d| d.get(def_name))
            .and_then(Value::as_object)
    {
        return validate_schema_node(def_node, defs, value, path);
    }

    if let Some(any_of) = node.get("anyOf").and_then(Value::as_array) {
        let matched = any_of.iter().any(|sub| {
            sub.as_object()
                .is_none_or(|sub_obj| validate_schema_node(sub_obj, defs, value, path).is_ok())
        });
        if !matched {
            return Err(format!("'{path}' did not match any allowed schema variant"));
        }
    }

    if let Some(one_of) = node.get("oneOf").and_then(Value::as_array) {
        let matched = one_of.iter().any(|sub| {
            sub.as_object()
                .is_none_or(|sub_obj| validate_schema_node(sub_obj, defs, value, path).is_ok())
        });
        if !matched {
            return Err(format!("'{path}' did not match any allowed schema variant"));
        }
    }

    if let Some(enum_vals) = node.get("enum").and_then(Value::as_array)
        && !enum_vals.contains(value)
    {
        return Err(format!(
            "'{path}' value {value} is not one of the allowed enum values"
        ));
    }

    if let Some(const_val) = node.get("const")
        && value != const_val
    {
        return Err(format!("'{path}' must equal {const_val}"));
    }

    if let Some(type_val) = node.get("type") {
        let matches = match type_val {
            Value::String(t) => value_matches_type(value, t),
            Value::Array(types) => types
                .iter()
                .filter_map(Value::as_str)
                .any(|t| value_matches_type(value, t)),
            _ => true,
        };
        if !matches {
            return Err(format!(
                "'{path}' has invalid type (expected {type_val}, got {value})"
            ));
        }
    }

    if let Some(obj) = value.as_object() {
        let props = node.get("properties").and_then(Value::as_object);
        let additional = node.get("additionalProperties");

        if let Some(req_list) = node.get("required").and_then(Value::as_array) {
            for req in req_list.iter().filter_map(Value::as_str) {
                if !obj.contains_key(req) {
                    return Err(format!("missing required parameter '{req}'"));
                }
            }
        }

        for (key, val) in obj {
            let child_path = format!("{path}.{key}");
            if let Some(prop_schema) = props.and_then(|p| p.get(key)) {
                if let Some(prop_obj) = prop_schema.as_object() {
                    validate_schema_node(prop_obj, defs, val, &child_path)?;
                } else if prop_schema == &Value::Bool(false) {
                    return Err(format!("parameter '{key}' is not allowed"));
                }
            } else if additional == Some(&Value::Bool(false)) {
                return Err(format!("unknown parameter '{key}'"));
            }
        }
    }

    if let Some(arr) = value.as_array() {
        if let Some(min_items) = node.get("minItems").and_then(Value::as_u64)
            && (arr.len() as u64) < min_items
        {
            return Err(format!(
                "'{path}' must have at least {min_items} item(s), got {}",
                arr.len()
            ));
        }
        if let Some(max_items) = node.get("maxItems").and_then(Value::as_u64)
            && (arr.len() as u64) > max_items
        {
            return Err(format!(
                "'{path}' must have at most {max_items} item(s), got {}",
                arr.len()
            ));
        }
        if let Some(item_schema) = node.get("items").and_then(Value::as_object) {
            for (idx, item) in arr.iter().enumerate() {
                validate_schema_node(item_schema, defs, item, &format!("{path}[{idx}]"))?;
            }
        }
    }

    if let Some(s) = value.as_str() {
        if let Some(min_len) = node.get("minLength").and_then(Value::as_u64)
            && (s.chars().count() as u64) < min_len
        {
            return Err(format!(
                "'{path}' length must be at least {min_len} character(s)"
            ));
        }
        if let Some(max_len) = node.get("maxLength").and_then(Value::as_u64)
            && (s.chars().count() as u64) > max_len
        {
            return Err(format!(
                "'{path}' length must be at most {max_len} character(s)"
            ));
        }
    }

    if let Some(num) = value.as_f64() {
        if let Some(min) = node.get("minimum").and_then(Value::as_f64)
            && num < min
        {
            return Err(format!("'{path}' ({num}) must be >= {min}"));
        }
        if let Some(max) = node.get("maximum").and_then(Value::as_f64)
            && num > max
        {
            return Err(format!("'{path}' ({num}) must be <= {max}"));
        }
    }

    Ok(())
}

fn value_matches_type(value: &Value, expected: &str) -> bool {
    match expected {
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "boolean" => value.is_boolean(),
        "integer" => value.is_i64() || value.is_u64(),
        "number" => value.is_number(),
        "null" => value.is_null(),
        _ => true,
    }
}
