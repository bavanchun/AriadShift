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
    collections::{BTreeMap, HashMap},
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, LazyLock, Mutex, RwLock},
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
    confine::{AllowedDirs, ConfineError, ConfinedInput, confine_input},
    convert::{ConvertWarning, classify_file, paths_refer_to_same_file},
    engines,
    runner::EngineCommand,
    tools::{
        engine,
        inprocess::{self, INPROCESS_PDF_ENGINE},
        publish::{
            PublishError, PublishedArtifact, StagedArtifact, default_input_stem,
            default_multi_artifact_name, default_single_artifact_name, ensure_no_parent_traversal,
            extracted_quarantine_warnings, is_safe_extracted_extension, open_confined_output_dir,
            publish_atomic, replace_staged_input_stem, validate_extracted_artifact_extensions,
            validate_output_path_for_spec, validate_spec_artifact_extensions,
            validate_target_filename,
        },
        secrets::{SecretBag, SecretScrubber},
    },
    workspace::{Workspace, WorkspaceError},
};

/// Input file reference for a [`ToolRequest`].
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct InputRef {
    /// Filesystem path to the input document.
    pub path: PathBuf,
    /// Optional pre-opened confined file handle bound under an `allowed_dirs` lock.
    #[serde(skip)]
    #[schemars(skip)]
    confined: Option<Arc<Mutex<ConfinedInput>>>,
}

impl PartialEq for InputRef {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path
    }
}

impl Eq for InputRef {}

impl InputRef {
    /// Creates a new [`InputRef`] from a path.
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            confined: None,
        }
    }

    /// Creates an [`InputRef`] bound to a pre-validated [`ConfinedInput`] file handle.
    ///
    /// Staging copies directly from the confined handle rather than re-opening `path` on disk.
    #[must_use]
    pub fn from_confined(confined: ConfinedInput) -> Self {
        let path = confined.path.clone();
        Self {
            path,
            confined: Some(Arc::new(Mutex::new(confined))),
        }
    }
}

impl From<PathBuf> for InputRef {
    fn from(path: PathBuf) -> Self {
        Self::new(path)
    }
}

impl From<&Path> for InputRef {
    fn from(path: &Path) -> Self {
        Self::new(path.to_path_buf())
    }
}

impl From<&str> for InputRef {
    fn from(path: &str) -> Self {
        Self::new(PathBuf::from(path))
    }
}

impl From<String> for InputRef {
    fn from(path: String) -> Self {
        Self::new(PathBuf::from(path))
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
    /// Validation refused a conformance claim; the structured report explains why.
    #[error("{standard} compliance validation failed: {report}")]
    ComplianceFailed {
        standard: String,
        report: Box<Value>,
    },
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
            Self::EngineFailure(_)
            | Self::ComplianceFailed { .. }
            | Self::InputIo(_)
            | Self::OutputIo(_) => 1,
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
            Self::ComplianceFailed { .. } => "compliance_failed",
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
            Self::ComplianceFailed { standard, report } => Self::ComplianceFailed {
                standard: scrubber.scrub(&standard),
                report: Box::new(scrubber.scrub_value(&report)),
            },
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
    let clock = inprocess::capture_clock()?;
    run_tool_with_clock(request, &mut on_event, clock)
}

pub(crate) fn run_tool_with_clock(
    request: &ToolRequest,
    on_event: &mut dyn FnMut(ToolEvent),
    clock: inprocess::HostClock,
) -> Result<ToolOutcome, ToolError> {
    let base_scrubber = request.secrets.scrubber();
    let result = run_tool_inner(request, on_event, clock);
    result.map_err(|err| err.scrubbed(&base_scrubber))
}

fn run_tool_inner(
    request: &ToolRequest,
    on_event: &mut dyn FnMut(ToolEvent),
    clock: inprocess::HostClock,
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
    validate_spec_params(
        &spec,
        &resolved_secrets.schema_params,
        request.spec_override.is_none(),
    )
    .map_err(|msg| ToolError::InvalidParams(scrubber.scrub(&msg)))?;
    if request.spec_override.is_none() {
        validate_semantic_params(spec.id, &resolved_secrets.schema_params)
            .map_err(|msg| ToolError::InvalidParams(scrubber.scrub(&msg)))?;
    }

    let is_ai = spec.group == ariad_core::tools::ToolGroup::Ai;
    if is_ai {
        super::ai::check_access(&spec, &resolved_secrets.engine_params, request.plan)?;
    }

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

    // 8. Pre-validate output target confinement and engine availability before staging or running.
    let emits_json = tool_emits_json(&spec, &resolved_secrets.schema_params);
    let extracts_attachments =
        tool_action_extracts_attachments(spec.id, &resolved_secrets.schema_params);
    let effective_engine = spec.engine_for_params(&resolved_secrets.schema_params);
    let is_redact_composite = spec.id == "pdf.redact" || spec.id == "pdf.redact_verify";
    let is_ocr_composite = !is_redact_composite
        && effective_engine == ariad_core::tools::specs::ocr_scan::HOST_COMPOSITE_ENGINE
        && spec.group == ariad_core::tools::ToolGroup::Ocr;
    let is_document_edit = matches!(spec.id, "edit.open_document" | "edit.export_document");
    let is_cbz = spec.id == "pdf.to_cbz";
    let is_tables_to_csv = spec.id == "pdf.tables_to_csv";
    let is_compare = spec.id == "pdf.compare";
    let is_web_to_pdf = spec.id == "web.to_pdf";
    let is_compliance = spec.id == "pdf.to_pdfa" || spec.id == "pdf.to_pdfua";

    if spec.id == "pdf.watermark_remove_raster"
        && let OutputTarget::Path(path) = &request.output
        && !path.is_dir()
        && !path
            .as_os_str()
            .to_string_lossy()
            .ends_with(['/', std::path::MAIN_SEPARATOR])
        && !path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("pdf"))
    {
        return Err(ToolError::Usage(
            "raster watermark removal requires a PDF file or an output directory".into(),
        ));
    }

    if spec.id == "pdf.ocr"
        && matches!(
            resolved_secrets
                .schema_params
                .get("output")
                .and_then(Value::as_str),
            Some("pdf+txt" | "pdf+hocr")
        )
        && let OutputTarget::Path(path) = &request.output
        && !path.is_dir()
        && !path
            .as_os_str()
            .to_string_lossy()
            .ends_with(['/', std::path::MAIN_SEPARATOR])
    {
        return Err(ToolError::Usage(
            "OCR with a sidecar requires an output directory".into(),
        ));
    }

    let mut input_paths: Vec<PathBuf> = request.inputs.iter().map(|i| i.path.clone()).collect();
    preflight_output_target(
        &request.output,
        &input_paths,
        request.allowed_dirs.as_ref(),
        request.overwrite,
        &spec,
        &resolved_secrets.schema_params,
        emits_json,
        extracts_attachments,
    )?;

    if is_ai && super::ai::is_cloud(&resolved_secrets.schema_params) {
        super::ai::preflight()?;
    } else if is_document_edit {
        super::edit_document::preflight(spec.id, &resolved_secrets.schema_params)?;
    } else if is_web_to_pdf {
        super::web_to_pdf::preflight()?;
    } else if is_cbz {
        super::cbz::preflight(
            request.engine_program.as_deref(),
            request.engine_command.as_ref(),
        )?;
    } else if is_compare {
        super::compare::preflight(
            &resolved_secrets.schema_params,
            request.engine_program.as_deref(),
            request.engine_command.as_ref(),
        )?;
    } else if is_compliance {
        super::compliance::preflight(
            request.engine_program.as_deref(),
            request.engine_command.as_ref(),
        )?;
    } else if is_ocr_composite {
        super::ocr::preflight(
            spec.id,
            request.engine_program.as_deref(),
            request.engine_command.as_ref(),
        )?;
    } else if is_redact_composite {
        super::redact::preflight(
            spec.id,
            request.engine_program.as_deref(),
            request.engine_command.as_ref(),
        )?;
    } else if is_tables_to_csv {
        super::tables::preflight(request.engine_program.as_deref())?;
    } else if effective_engine != INPROCESS_PDF_ENGINE || request.engine_command.is_some() {
        let _ = engine::resolve_engine_command(
            effective_engine,
            request.engine_program.as_deref(),
            request.engine_command.as_ref(),
        )?;
    }

    // 9. Confine, copy from the confined handle into `<ws>/in/`, size-check, and sniff format on the workspace copy.
    let workspace =
        Workspace::new().map_err(|e| ToolError::OutputIo(scrubber.scrub(&e.to_string())))?;

    let mut total_input_bytes = 0_u64;
    let mut staged_inputs_with_format: Vec<(PathBuf, String)> =
        Vec::with_capacity(request.inputs.len());
    let mut staged_inputs_with_names: Vec<(PathBuf, String)> =
        Vec::with_capacity(request.inputs.len());

    for (idx, input_ref) in request.inputs.iter().enumerate() {
        let allow_arbitrary_payload =
            tool_input_is_attachment_payload(spec.id, &resolved_secrets.schema_params, idx);
        let (canonical_source, staged_path, format_id, bytes) = stage_and_classify_input(
            idx,
            input_ref,
            &spec,
            allow_arbitrary_payload,
            &workspace,
            request.allowed_dirs.as_ref(),
            &request.limits,
            &request.cancel,
        )?;
        let original_name = input_ref
            .path
            .file_name()
            .and_then(|s| s.to_str())
            .filter(|s| !s.trim().is_empty())
            .unwrap_or("document.pdf")
            .to_owned();
        input_paths[idx] = canonical_source;
        total_input_bytes = total_input_bytes.saturating_add(bytes);
        staged_inputs_with_names.push((staged_path.clone(), original_name));
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
    let is_out_of_process =
        effective_engine != INPROCESS_PDF_ENGINE || request.engine_command.is_some();
    let mut raw_exec = if is_ai {
        super::ai::execute(
            &spec,
            &staged_inputs_with_format,
            &resolved_secrets.engine_params,
            resolved_secrets.has_secrets,
            &workspace,
            request,
            scrubber,
            on_event,
        )
        .map_err(|e| e.scrubbed(scrubber))?
    } else if is_document_edit {
        super::edit_document::execute(
            &spec,
            &staged_inputs_with_format,
            &resolved_secrets.schema_params,
            &workspace,
            &request.limits,
            scrubber,
            request.engine_program.as_deref(),
            &request.cancel,
            on_event,
            clock,
        )
        .map_err(|e| e.scrubbed(scrubber))?
    } else if is_web_to_pdf {
        super::web_to_pdf::execute(
            &spec,
            &resolved_secrets.schema_params,
            &workspace,
            &request.limits,
            scrubber,
            &request.cancel,
            on_event,
            clock,
        )
        .map_err(|e| e.scrubbed(scrubber))?
    } else if is_cbz {
        super::cbz::execute(
            &spec,
            &staged_inputs_with_format,
            &resolved_secrets.schema_params,
            &workspace,
            &request.limits,
            scrubber,
            request.engine_program.as_deref(),
            request.engine_command.as_ref(),
            &request.cancel,
            on_event,
        )
        .map_err(|e| e.scrubbed(scrubber))?
    } else if is_compare {
        super::compare::execute(
            &spec,
            &staged_inputs_with_format,
            &resolved_secrets.schema_params,
            &workspace,
            &request.limits,
            scrubber,
            request.engine_program.as_deref(),
            request.engine_command.as_ref(),
            &request.cancel,
            on_event,
            clock.clock,
        )
        .map_err(|e| e.scrubbed(scrubber))?
    } else if is_compliance {
        super::compliance::execute(
            &spec,
            &staged_inputs_with_format,
            &resolved_secrets.schema_params,
            &workspace,
            &request.limits,
            scrubber,
            request.engine_program.as_deref(),
            request.engine_command.as_ref(),
            &request.cancel,
            on_event,
            clock.clock,
        )
        .map_err(|e| e.scrubbed(scrubber))?
    } else if is_ocr_composite {
        super::ocr::execute(
            &spec,
            &staged_inputs_with_format,
            &resolved_secrets.schema_params,
            &workspace,
            &request.limits,
            scrubber,
            request.engine_program.as_deref(),
            &request.cancel,
            on_event,
            clock,
        )
        .map_err(|e| e.scrubbed(scrubber))?
    } else if is_redact_composite {
        super::redact::execute(
            &spec,
            &staged_inputs_with_format,
            &resolved_secrets.schema_params,
            &workspace,
            &request.limits,
            scrubber,
            request.engine_program.as_deref(),
            &request.cancel,
            on_event,
            clock,
        )
        .map_err(|e| e.scrubbed(scrubber))?
    } else if is_tables_to_csv {
        super::tables::execute(
            &spec,
            &staged_inputs_with_format,
            &resolved_secrets.schema_params,
            &workspace,
            &request.limits,
            scrubber,
            request.engine_program.as_deref(),
            &request.cancel,
            on_event,
            clock,
        )
        .map_err(|e| e.scrubbed(scrubber))?
    } else if !is_out_of_process {
        inprocess::execute_with_names_and_clock(
            &spec,
            &staged_inputs_with_names,
            &resolved_secrets.schema_params,
            &workspace,
            &request.limits,
            scrubber,
            &request.cancel,
            on_event,
            clock,
        )
        .map_err(|e| e.scrubbed(scrubber))?
    } else {
        engine::execute(
            &spec,
            effective_engine,
            &staged_inputs_with_format,
            &resolved_secrets.engine_params,
            resolved_secrets.has_secrets,
            &workspace,
            &request.limits,
            scrubber,
            &request.secrets,
            request.engine_program.as_deref(),
            request.engine_command.as_ref(),
            request.cancel.clone(),
            on_event,
        )
        .map_err(|e| e.scrubbed(scrubber))?
    };

    if spec.id == "pdf.compress" && !is_out_of_process {
        let result = staged_inputs_with_names
            .first()
            .zip(raw_exec.artifacts.first())
            .and_then(|((input, _), (output, _))| {
                super::similarity::measure(
                    input,
                    output,
                    &request.limits,
                    request.engine_program.as_deref(),
                    &request.cancel,
                )
                .ok()
            });
        if let Some(score) = result {
            raw_exec.metrics.insert("similarity".into(), json!(score));
            if score < super::similarity::SIMILARITY_FLOOR {
                raw_exec.warnings.push(ConvertWarning {
                    code: "similarity_below_floor".into(),
                    message: format!("rendered similarity {score:.4} is below 0.98"),
                });
            }
        } else {
            raw_exec.warnings.push(ConvertWarning {
                code: "similarity_not_measured".into(),
                message: "PDFium similarity measurement was unavailable".into(),
            });
        }
    }

    if request.cancel.is_cancelled() {
        return Err(ToolError::Interrupted);
    }

    if (is_compare || spec.id == "pdf.watermark_remove_raster")
        && let OutputTarget::Path(ref path) = request.output
    {
        let is_dir = path.is_dir()
            || path
                .as_os_str()
                .to_string_lossy()
                .ends_with(['/', std::path::MAIN_SEPARATOR]);
        if !is_dir {
            let ext = if spec.id == "pdf.watermark_remove_raster" {
                "pdf"
            } else {
                path.extension().and_then(|e| e.to_str()).unwrap_or("")
            };
            raw_exec
                .artifacts
                .retain(|(_, fmt)| fmt.eq_ignore_ascii_case(ext));
        }
    }

    // 11. Plan and execute atomic multi-artifact publication.
    let has_companion_report =
        !emits_json && raw_exec.inline_json.is_none() && raw_exec.companion_report;
    let (published, publish_warnings) = publish_tool_outputs(
        &workspace,
        &spec,
        &resolved_secrets.schema_params,
        is_out_of_process,
        emits_json,
        extracts_attachments,
        raw_exec.inline_json.is_some(),
        has_companion_report,
        &raw_exec.artifacts,
        &input_paths,
        &request.output,
        request.naming_template.as_deref(),
        request.allowed_dirs.as_ref(),
        request.overwrite,
        request.fail_publish_at_index,
        clock.clock,
    )
    .map_err(|e| e.scrubbed(scrubber))?;

    let elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let mut metrics = raw_exec.metrics;
    metrics.insert("elapsed_ms".to_owned(), json!(elapsed_ms));
    let mut warnings = raw_exec.warnings;
    warnings.extend(publish_warnings.into_iter().map(|message| ConvertWarning {
        code: "attachment_quarantined".to_owned(),
        message,
    }));

    let mut outcome = ToolOutcome {
        tool: spec.id.to_owned(),
        version: spec.version.to_owned(),
        engine: effective_engine.to_owned(),
        artifacts: published,
        warnings,
        metrics,
        inline_json: raw_exec.inline_json,
    };
    if spec.id == "edit.export_document" {
        let report = serde_json::from_value(outcome.metrics["fidelity"].clone())
            .map_err(|e| ToolError::EngineFailure(e.to_string()))?;
        super::fidelity::attach_document_fidelity(&mut outcome, &report)
            .map_err(|e| ToolError::EngineFailure(e.to_string()))?;
    }
    Ok(outcome)
}

/// Maximum JSON payload kept inline on [`ToolOutcome::inline_json`] (`32 MiB`, further bounded by
/// `limits.max_decompressed_bytes`).
pub(super) const MAX_INLINE_JSON_BYTES: u64 = 32 * 1024 * 1024;

#[cfg(any(test, feature = "test-probe"))]
thread_local! {
    static TEST_INLINE_JSON_CAP: std::cell::Cell<Option<u64>> = const { std::cell::Cell::new(None) };
}

/// Temporarily lowers `json_inline_cap` on the current thread so tests can verify
/// oversized companion JSON fallback without allocating >32 MiB payloads.
#[cfg(any(test, feature = "test-probe"))]
#[doc(hidden)]
pub fn with_inline_json_cap_for_test<T>(cap: u64, f: impl FnOnce() -> T) -> T {
    struct Reset(Option<u64>);
    impl Drop for Reset {
        fn drop(&mut self) {
            TEST_INLINE_JSON_CAP.with(|cell| cell.set(self.0));
        }
    }
    let previous = TEST_INLINE_JSON_CAP.with(|cell| {
        let old = cell.get();
        cell.set(Some(cap));
        old
    });
    let _reset = Reset(previous);
    f()
}

pub(super) fn json_inline_cap(limits: &Limits) -> u64 {
    let base = limits.max_decompressed_bytes.min(MAX_INLINE_JSON_BYTES);
    #[cfg(any(test, feature = "test-probe"))]
    if let Some(override_cap) = TEST_INLINE_JSON_CAP.with(|cell| cell.get()) {
        return base.min(override_cap);
    }
    base
}

/// Maximum size of a staged engine JSON artifact the host scrubs when secrets are registered
/// (`64 MiB`, further bounded by `limits.max_decompressed_bytes`).
///
/// The cap bounds both the staged input and the scrubbed output: redaction can grow the text
/// (a one-byte secret becomes the ten-byte `[REDACTED]`), and any token or output expansion
/// past the cap fails with `limit_exceeded` before the oversized text is allocated. The scrub
/// holds the staged bytes, the output buffer, one decoded string token and one scrubbed copy of
/// it, each at most this cap, so its peak memory stays under four times the cap (256 MiB)
/// whatever `max_decompressed_bytes` allows and whatever the JSON shape. The cap is twice
/// [`MAX_INLINE_JSON_BYTES`], so every JSON payload small enough to inline, and companion JSON
/// up to twice that size, can still be scrubbed and published.
pub(super) const MAX_SCRUB_JSON_BYTES: u64 = 64 * 1024 * 1024;

#[cfg(any(test, feature = "test-probe"))]
thread_local! {
    static TEST_SCRUB_JSON_CAP: std::cell::Cell<Option<u64>> = const { std::cell::Cell::new(None) };
}

/// Temporarily lowers `scrub_json_cap` on the current thread so tests can verify the
/// over-cap refusal without allocating >64 MiB payloads.
#[cfg(any(test, feature = "test-probe"))]
#[doc(hidden)]
pub fn with_scrub_json_cap_for_test<T>(cap: u64, f: impl FnOnce() -> T) -> T {
    struct Reset(Option<u64>);
    impl Drop for Reset {
        fn drop(&mut self) {
            TEST_SCRUB_JSON_CAP.with(|cell| cell.set(self.0));
        }
    }
    let previous = TEST_SCRUB_JSON_CAP.with(|cell| {
        let old = cell.get();
        cell.set(Some(cap));
        old
    });
    let _reset = Reset(previous);
    f()
}

/// Largest staged engine JSON artifact scrubbed in memory when secrets are registered.
pub(super) fn scrub_json_cap(limits: &Limits) -> u64 {
    let base = limits.max_decompressed_bytes.min(MAX_SCRUB_JSON_BYTES);
    #[cfg(any(test, feature = "test-probe"))]
    if let Some(override_cap) = TEST_SCRUB_JSON_CAP.with(|cell| cell.get()) {
        return base.min(override_cap);
    }
    base
}

pub(super) fn report_not_inlined_warning() -> ConvertWarning {
    ConvertWarning {
        code: "report_not_inlined".into(),
        message: "JSON report exceeded the inline size limit and was written to a companion .json artifact".into(),
    }
}

/// Safely replaces any pre-existing staged file or symlink named `name` inside `<workspace>/out/`
/// through the workspace's capability handle and creates the new artifact with `create_new(true)`.
pub(super) fn write_staged_artifact(
    workspace: &Workspace,
    name: &str,
    bytes: &[u8],
    scrubber: &SecretScrubber,
) -> Result<PathBuf, ToolError> {
    workspace.remove_artifact(name).map_err(|e| {
        ToolError::OutputIo(scrubber.scrub(&format!(
            "could not safely clear staged artifact '{name}': {e}"
        )))
    })?;
    workspace
        .write_artifact(name, bytes)
        .map_err(|e| ToolError::OutputIo(scrubber.scrub(&e.to_string())))
}

/// Streams the regular file at `source` into the staged artifact `name` without buffering it,
/// with the same link-safe clearing and creation as [`write_staged_artifact`].
pub(super) fn write_staged_artifact_from_file(
    workspace: &Workspace,
    name: &str,
    source: &Path,
    scrubber: &SecretScrubber,
    cancel: &CancellationToken,
) -> Result<PathBuf, ToolError> {
    let mut file = open_staged_source(source)
        .map_err(|e| ToolError::OutputIo(scrubber.scrub(&e.to_string())))?;
    if !file
        .metadata()
        .map_err(|e| ToolError::OutputIo(scrubber.scrub(&e.to_string())))?
        .is_file()
    {
        return Err(ToolError::OutputIo(
            "staged source is not a regular file".into(),
        ));
    }
    workspace.remove_artifact(name).map_err(|e| {
        ToolError::OutputIo(scrubber.scrub(&format!(
            "could not safely clear staged artifact '{name}': {e}"
        )))
    })?;
    workspace
        .write_artifact_from_reader(name, &mut file, cancel)
        .map_err(|e| match e {
            WorkspaceError::Io(io) if io.kind() == std::io::ErrorKind::Interrupted => {
                ToolError::Interrupted
            }
            other => ToolError::OutputIo(scrubber.scrub(&other.to_string())),
        })
}

/// Opens a staged source without following a symlink at the final component, so a link planted
/// at the source path is refused instead of exposing the file it points to.
fn open_staged_source(source: &Path) -> std::io::Result<fs::File> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    #[cfg(not(unix))]
    if fs::symlink_metadata(source)?.file_type().is_symlink() {
        return Err(std::io::Error::other("staged source is a symlink"));
    }
    options.open(source)
}

/// Scrubs `value` and returns `(scrubbed_value, serialized_bytes, inline_json)` where `inline_json`
/// is `Some(scrubbed_value)` only if `serialized_bytes.len() <= json_inline_cap(limits)`.
pub(super) fn scrub_and_cap_inline_json(
    value: Value,
    limits: &Limits,
    scrubber: &SecretScrubber,
) -> Result<(Value, Vec<u8>, Option<Value>), ToolError> {
    let scrubbed = scrubber.scrub_value(&value);
    let bytes = serde_json::to_vec_pretty(&scrubbed)
        .map_err(|e| ToolError::EngineFailure(scrubber.scrub(&e.to_string())))?;
    let inline = ((bytes.len() as u64) <= json_inline_cap(limits)).then(|| scrubbed.clone());
    Ok((scrubbed, bytes, inline))
}

pub(crate) fn tool_emits_json(spec: &ToolSpec, params: &Value) -> bool {
    spec.emits_json(params)
}

fn tool_action_extracts_attachments(spec_id: &str, params: &Value) -> bool {
    matches!(spec_id, "pdf.attachments" | "pdf.portfolio")
        && params.get("action").and_then(Value::as_str) == Some("extract")
}

fn tool_input_is_attachment_payload(spec_id: &str, params: &Value, input_index: usize) -> bool {
    let action = params.get("action").and_then(Value::as_str);
    match spec_id {
        "pdf.attachments" => action == Some("add") && input_index >= 1,
        "pdf.portfolio" => action == Some("create"),
        "pdf.annotations_import" | "pdf.forms_import" => input_index >= 1,
        _ => false,
    }
}

#[allow(clippy::too_many_arguments)]
fn stage_and_classify_input(
    idx: usize,
    input_ref: &InputRef,
    spec: &ToolSpec,
    allow_arbitrary_payload: bool,
    workspace: &Workspace,
    allowed_dirs: Option<&AllowedDirs>,
    limits: &Limits,
    cancel: &CancellationToken,
) -> Result<(PathBuf, PathBuf, String, u64), ToolError> {
    if cancel.is_cancelled() {
        return Err(ToolError::Interrupted);
    }

    let raw_ext = input_ref
        .path
        .extension()
        .and_then(|e| e.to_str())
        .filter(|e| !e.is_empty() && e.bytes().all(|b| b.is_ascii_alphanumeric()))
        .unwrap_or("bin");
    let raw_staged_path = workspace
        .input_dir()
        .join(format!("raw-input-{idx}.{raw_ext}"));

    let mut dest_file = open_staged_destination(&raw_staged_path).map_err(|_| {
        ToolError::InputIo(format!(
            "could not stage input file '{}'",
            input_ref.path.display()
        ))
    })?;

    let canonical_source = if let Some(ref pre_confined) = input_ref.confined {
        let mut guard = pre_confined.lock().map_err(|_| {
            ToolError::InputIo(format!(
                "input file '{}' could not be read",
                input_ref.path.display()
            ))
        })?;
        let meta = guard.file.metadata().map_err(|_| {
            ToolError::InputIo(format!(
                "input file '{}' could not be read",
                input_ref.path.display()
            ))
        })?;
        if let Some(max_in) = limits.max_input_bytes
            && meta.len() > max_in
        {
            let _ = fs::remove_file(&raw_staged_path);
            return Err(ToolError::LimitExceeded);
        }
        guard
            .copy_to_file_cancellable(&mut dest_file, limits.max_input_bytes, cancel)
            .map_err(|e| {
                let _ = fs::remove_file(&raw_staged_path);
                map_copy_error(&e, &input_ref.path)
            })?;
        guard.path.clone()
    } else if let Some(allowed) = allowed_dirs {
        let mut confined = confine_input(&input_ref.path, allowed).map_err(|err| {
            let _ = fs::remove_file(&raw_staged_path);
            match err {
                ConfineError::InputNotFound { .. } => ToolError::InputIo(format!(
                    "input file '{}' was not found",
                    input_ref.path.display()
                )),
                _ => ToolError::Confinement(format!(
                    "input path '{}' is not permitted: {err}",
                    input_ref.path.display()
                )),
            }
        })?;
        let meta = confined.file.metadata().map_err(|_| {
            let _ = fs::remove_file(&raw_staged_path);
            ToolError::InputIo(format!(
                "input file '{}' could not be read",
                input_ref.path.display()
            ))
        })?;
        if let Some(max_in) = limits.max_input_bytes
            && meta.len() > max_in
        {
            let _ = fs::remove_file(&raw_staged_path);
            return Err(ToolError::LimitExceeded);
        }
        confined
            .copy_to_file_cancellable(&mut dest_file, limits.max_input_bytes, cancel)
            .map_err(|e| {
                let _ = fs::remove_file(&raw_staged_path);
                map_copy_error(&e, &input_ref.path)
            })?;
        confined.path
    } else {
        copy_unconfined_input_to_file(&input_ref.path, &mut dest_file, limits, cancel)
            .inspect_err(|_| {
                let _ = fs::remove_file(&raw_staged_path);
            })?;
        input_ref.path.clone()
    };

    dest_file.flush().map_err(|_| {
        let _ = fs::remove_file(&raw_staged_path);
        ToolError::InputIo(format!(
            "could not stage input file '{}'",
            input_ref.path.display()
        ))
    })?;
    drop(dest_file);

    // Classify the staged workspace copy rather than re-opening the user's path.
    let classification = classify_file(&raw_staged_path).map_err(|_| {
        let _ = fs::remove_file(&raw_staged_path);
        ToolError::InputIo(format!(
            "the input file '{}' could not be read",
            input_ref.path.display()
        ))
    })?;

    if let Some(max_in) = limits.max_input_bytes
        && classification.bytes > max_in
    {
        let _ = fs::remove_file(&raw_staged_path);
        return Err(ToolError::LimitExceeded);
    }

    let format_id = if allow_arbitrary_payload {
        classification
            .format
            .map(|fmt| fmt.id().to_owned())
            .unwrap_or_else(|| raw_ext.to_ascii_lowercase())
    } else {
        match classification.format {
            Some(fmt) if spec.accepts_core_format(fmt) => fmt.id().to_owned(),
            Some(fmt) => {
                let _ = fs::remove_file(&raw_staged_path);
                return Err(ToolError::WrongInput(format!(
                    "input '{}' has format '{}', which is not accepted by tool '{}' (accepted: {})",
                    input_ref.path.display(),
                    fmt.user_name(),
                    spec.id,
                    spec.input_formats.join(", ")
                )));
            }
            None if spec.accepts_format("bmp") && is_bmp_file(&raw_staged_path) => "bmp".to_owned(),
            None => {
                let _ = fs::remove_file(&raw_staged_path);
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
        }
    };

    let staged_path = workspace
        .input_dir()
        .join(format!("input-{idx}.{format_id}"));
    fs::rename(&raw_staged_path, &staged_path).map_err(|_| {
        let _ = fs::remove_file(&raw_staged_path);
        ToolError::InputIo(format!(
            "could not stage input file '{}'",
            input_ref.path.display()
        ))
    })?;

    Ok((
        canonical_source,
        staged_path,
        format_id,
        classification.bytes,
    ))
}

fn open_staged_destination(path: &Path) -> io::Result<fs::File> {
    let mut opts = fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
        opts.custom_flags(libc::O_NOFOLLOW);
    }
    opts.open(path)
}

fn map_copy_error(err: &io::Error, input_path: &Path) -> ToolError {
    match err.kind() {
        io::ErrorKind::Interrupted => ToolError::Interrupted,
        io::ErrorKind::FileTooLarge | io::ErrorKind::InvalidData => ToolError::LimitExceeded,
        _ => ToolError::InputIo(format!(
            "could not stage input file '{}'",
            input_path.display()
        )),
    }
}

fn copy_unconfined_input_to_file(
    path: &Path,
    dest: &mut fs::File,
    limits: &Limits,
    cancel: &CancellationToken,
) -> Result<(), ToolError> {
    let mut src = crate::convert::open_regular_input(path).map_err(|_| {
        ToolError::InputIo(format!("input file '{}' could not be read", path.display()))
    })?;
    let meta = src.metadata().map_err(|_| {
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

    let mut buf = [0_u8; 64 * 1024];
    let mut copied = 0_u64;
    loop {
        if cancel.is_cancelled() {
            return Err(ToolError::Interrupted);
        }
        let n = src.read(&mut buf).map_err(|_| {
            ToolError::InputIo(format!("input file '{}' could not be read", path.display()))
        })?;
        if n == 0 {
            break;
        }
        copied = copied.saturating_add(n as u64);
        if let Some(max_in) = limits.max_input_bytes
            && copied > max_in
        {
            return Err(ToolError::LimitExceeded);
        }
        dest.write_all(&buf[..n]).map_err(|_| {
            ToolError::InputIo(format!("could not stage input file '{}'", path.display()))
        })?;
    }
    Ok(())
}

fn is_bmp_file(path: &Path) -> bool {
    let Ok(mut f) = fs::File::open(path) else {
        return false;
    };
    let mut header = [0_u8; 14];
    f.read_exact(&mut header).is_ok() && header.starts_with(b"BM")
}

#[allow(clippy::too_many_arguments)]
fn preflight_output_target(
    output: &OutputTarget,
    inputs: &[PathBuf],
    allowed_dirs: Option<&AllowedDirs>,
    _overwrite: bool,
    spec: &ToolSpec,
    params: &Value,
    emits_json: bool,
    extracts_attachments: bool,
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
                if extracts_attachments {
                    let ext = if spec.id == "pdf.watermark_remove_raster" {
                        "pdf"
                    } else {
                        path.extension().and_then(|e| e.to_str()).unwrap_or("")
                    };
                    if !is_safe_extracted_extension(ext) {
                        return Err(ToolError::Confinement(format!(
                            "output path '{}' does not have a valid extracted attachment extension",
                            path.display()
                        )));
                    }
                } else {
                    validate_output_path_for_spec(path, spec, params, emits_json)?;
                }
            }
            Ok(())
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn publish_tool_outputs(
    workspace: &Workspace,
    spec: &ToolSpec,
    params: &Value,
    is_out_of_process: bool,
    emits_json: bool,
    extracts_attachments: bool,
    has_inline_json: bool,
    has_companion_report: bool,
    raw_artifacts: &[(PathBuf, String)],
    inputs: &[PathBuf],
    output: &OutputTarget,
    naming_template: Option<&str>,
    allowed_dirs: Option<&AllowedDirs>,
    overwrite: bool,
    fail_at_index: Option<usize>,
    clock: ariad_pdf::ToolClock,
) -> Result<(Vec<PublishedArtifact>, Vec<String>), ToolError> {
    if raw_artifacts.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }

    let first_input = inputs
        .first()
        .map(PathBuf::as_path)
        .unwrap_or_else(|| Path::new("document.pdf"));
    let first_stem = first_input
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("document");

    let is_default_beside_input = matches!(output, OutputTarget::DefaultBesideInput);
    let is_default_compare =
        is_default_beside_input && naming_template.is_none() && spec.id == "pdf.compare";
    let single_primary_with_companion = has_companion_report
        && raw_artifacts.len() == 2
        && !raw_artifacts[0].1.eq_ignore_ascii_case("json")
        && raw_artifacts[1].1.eq_ignore_ascii_case("json");

    let (dest_dir, explicit_single_filename): (PathBuf, Option<String>) = match output {
        OutputTarget::None => return Ok((Vec::new(), Vec::new())),
        OutputTarget::DefaultBesideInput => {
            // Inline-JSON actions (`pdf.info`, `outline export`, `attachments list`, `portfolio list`,
            // `layers list`, `pdf.text_layout`, `pdf.detect_scanned`, etc.) without `-o` emit structured
            // JSON inline without writing a file next to input unless the JSON exceeded the inline cap.
            if emits_json && naming_template.is_none() && has_inline_json {
                return Ok((Vec::new(), Vec::new()));
            }
            if emits_json
                && (inputs.is_empty() || spec.inputs == ariad_core::tools::InputArity::None)
            {
                return Err(ToolError::Usage(format!(
                    "tool '{}' result exceeds the inline JSON limit (or uses a naming template) and has no input file to publish beside; pass -o <file> or -o <dir>/",
                    spec.id
                )));
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
                return Err(ToolError::Confinement(format!(
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
                if raw_artifacts.len() > 1 && !single_primary_with_companion {
                    return Err(ToolError::Confinement(format!(
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
                        ToolError::Confinement(format!(
                            "invalid output filename '{}'",
                            path.display()
                        ))
                    })?
                    .to_owned();
                (parent.to_path_buf(), Some(file_name))
            }
        }
    };

    let filtered_compare_artifacts: Vec<(PathBuf, String)>;
    let artifacts_to_publish: &[(PathBuf, String)] = if is_default_compare {
        let mut ordered = Vec::with_capacity(raw_artifacts.len());
        for item in raw_artifacts {
            if item.1.eq_ignore_ascii_case("pdf") {
                ordered.push(item.clone());
            }
        }
        if !has_inline_json {
            for item in raw_artifacts {
                if !item.1.eq_ignore_ascii_case("pdf") {
                    ordered.push(item.clone());
                }
            }
        }
        filtered_compare_artifacts = ordered;
        &filtered_compare_artifacts
    } else {
        raw_artifacts
    };

    if artifacts_to_publish.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }

    let multi = artifacts_to_publish.len() > 1;
    let mut staged = Vec::with_capacity(artifacts_to_publish.len());
    for (idx, (staged_path, format)) in artifacts_to_publish.iter().enumerate() {
        let index_1_based = idx + 1;
        let is_companion =
            single_primary_with_companion && idx == 1 && format.eq_ignore_ascii_case("json");
        let base_target_name = if let Some(ref single_name) = explicit_single_filename {
            if is_companion {
                default_single_artifact_name(Path::new(single_name), spec.id, format)
            } else {
                single_name.clone()
            }
        } else if let Some(template) = naming_template {
            let template_index = if is_companion { 1 } else { index_1_based };
            let rendered = super::publish::render_naming_template_with_clock(
                template,
                first_stem,
                template_index,
                clock,
                params.get("date").and_then(Value::as_str),
            )
            .map_err(PublishError::from)?;
            if is_companion {
                companion_name_from_template(&rendered, &artifacts_to_publish[0].1)
            } else {
                rendered
            }
        } else if is_companion || single_primary_with_companion {
            default_single_artifact_name(first_input, spec.id, format)
        } else if extracts_attachments || (spec.id == "pdf.forms_import" && multi) {
            staged_path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or(if extracts_attachments {
                    "attachment.bin"
                } else {
                    "output.pdf"
                })
                .to_owned()
        } else if spec.id == "pdf.watermark_remove_raster" && multi {
            if format.eq_ignore_ascii_case("pdf") {
                format!("{first_stem}.watermark_remove_raster.pdf")
            } else {
                format!(
                    "{first_stem}.{}",
                    staged_path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("preview.png")
                )
            }
        } else if is_default_compare {
            let stem = default_input_stem(first_input);
            if format.eq_ignore_ascii_case("pdf") {
                format!("{stem}.diff.pdf")
            } else if format.eq_ignore_ascii_case("json") {
                format!("{stem}.compare.json")
            } else {
                let clean_ext = format.trim_start_matches('.');
                format!("{stem}.compare.{clean_ext}")
            }
        } else if is_out_of_process
            && let Some(engine_named) = replace_staged_input_stem(staged_path, first_input)
        {
            engine_named
        } else if spec.id == "pdf.compare" {
            let clean_ext = format.trim_start_matches('.');
            format!("{first_stem}.compared.{clean_ext}")
        } else if multi {
            default_multi_artifact_name(first_input, spec.id, index_1_based, format)
        } else {
            default_single_artifact_name(first_input, spec.id, format)
        };
        staged.push(StagedArtifact::new(
            staged_path.clone(),
            format.clone(),
            base_target_name,
        ));
    }

    if extracts_attachments {
        validate_extracted_artifact_extensions(&staged)?;
    } else {
        validate_spec_artifact_extensions(spec, params, emits_json, has_companion_report, &staged)?;
    }

    let quarantine_warnings = if extracts_attachments {
        extracted_quarantine_warnings(&staged)
    } else {
        Vec::new()
    };
    let published = publish_atomic(
        workspace,
        &staged,
        &dest_dir,
        inputs,
        allowed_dirs,
        overwrite,
        fail_at_index,
    )?;
    Ok((published, quarantine_warnings))
}

fn companion_name_from_template(rendered: &str, primary_format: &str) -> String {
    let path = Path::new(rendered);
    let clean_primary = primary_format.trim_start_matches('.');
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        if ext.eq_ignore_ascii_case("json") {
            return rendered.to_owned();
        }
        if ext.eq_ignore_ascii_case(clean_primary)
            && let Some(json_name) = path
                .with_extension("json")
                .file_name()
                .and_then(|n| n.to_str())
        {
            return json_name.to_owned();
        }
    }
    let stem = rendered.trim_end_matches('.');
    format!("{stem}.json")
}

static COMPILED_PARAM_SCHEMAS: LazyLock<RwLock<HashMap<String, Arc<jsonschema::Validator>>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

fn validate_spec_params(spec: &ToolSpec, params: &Value, use_cache: bool) -> Result<(), String> {
    if !params.is_object() {
        return Err("tool parameters must be a JSON object".to_owned());
    }
    if use_cache {
        if let Ok(cache) = COMPILED_PARAM_SCHEMAS.read()
            && let Some(validator) = cache.get(spec.id)
        {
            return validate_with_compiled(validator, params);
        }
        let schema = spec.resolved_params_schema();
        let validator = Arc::new(compile_json_schema(&schema)?);
        if let Ok(mut cache) = COMPILED_PARAM_SCHEMAS.write() {
            cache.insert(spec.id.to_owned(), Arc::clone(&validator));
        }
        return validate_with_compiled(&validator, params);
    }
    let schema = spec.resolved_params_schema();
    validate_params_against_schema(&schema, params)
}

/// Validates a JSON [`Value`] against a Draft 2020-12 [`Schema`] using `jsonschema`.
pub fn validate_params_against_schema(schema: &Schema, params: &Value) -> Result<(), String> {
    if !params.is_object() {
        return Err("tool parameters must be a JSON object".to_owned());
    }
    let validator = compile_json_schema(schema)?;
    validate_with_compiled(&validator, params)
}

fn compile_json_schema(schema: &Schema) -> Result<jsonschema::Validator, String> {
    jsonschema::draft202012::new(schema.as_value())
        .map_err(|e| format!("invalid tool parameter schema: {e}"))
}

fn validate_with_compiled(validator: &jsonschema::Validator, params: &Value) -> Result<(), String> {
    if let Some(error) = validator.iter_errors(params).next() {
        let path = error.instance_path().to_string();
        if path.is_empty() {
            return Err(error.to_string());
        }
        return Err(format!("{path}: {error}"));
    }
    Ok(())
}

pub(super) fn validate_semantic_params(tool_id: &str, params: &Value) -> Result<(), String> {
    use ariad_core::tools::params::{
        CropWhitespaceParams, ExtractImagesParams, ImageToPdfParams, RemoveBlankParams,
        RenderParams, SplitParams, ThumbnailsParams, ToImagesParams,
    };

    match tool_id {
        "pdf.watermark_remove_raster" => {
            let p: ariad_core::tools::params::WatermarkRemoveRasterParams =
                serde_json::from_value(params.clone()).map_err(|e| e.to_string())?;
            p.validate().map_err(|e| e.to_string())
        }
        "ai.recipe_from_text" => {
            let p: ariad_core::tools::params::AiRecipeFromTextParams =
                serde_json::from_value(params.clone()).map_err(|e| e.to_string())?;
            p.validate().map_err(|e| e.to_string())
        }
        "ai.extract_tables" => {
            let p: ariad_core::tools::params::AiExtractTablesParams =
                serde_json::from_value(params.clone()).map_err(|e| e.to_string())?;
            p.validate().map_err(|e| e.to_string())
        }
        "ai.translate" => {
            let p: ariad_core::tools::params::AiTranslateParams =
                serde_json::from_value(params.clone()).map_err(|e| e.to_string())?;
            p.validate().map_err(|e| e.to_string())
        }
        "ai.ask" => {
            let p: ariad_core::tools::params::AiAskParams =
                serde_json::from_value(params.clone()).map_err(|e| e.to_string())?;
            p.validate().map_err(|e| e.to_string())
        }
        "ai.summarize" => {
            let p: ariad_core::tools::params::AiSummarizeParams =
                serde_json::from_value(params.clone()).map_err(|e| e.to_string())?;
            p.validate().map_err(|e| e.to_string())
        }
        "pdf.annotations" => {
            let p: ariad_core::tools::params::annotations::AnnotationsParams =
                serde_json::from_value(params.clone()).map_err(|e| e.to_string())?;
            p.validate()
        }
        "pdf.forms_fill" => {
            let p: ariad_core::tools::params::forms::FormsFillParams =
                serde_json::from_value(params.clone()).map_err(|e| e.to_string())?;
            p.validate()
        }
        "pdf.forms_create" => {
            let p: ariad_core::tools::params::forms::FormsCreateParams =
                serde_json::from_value(params.clone()).map_err(|e| e.to_string())?;
            p.validate()
        }
        "pdf.forms_detect" => {
            let p: ariad_core::tools::params::forms::FormsDetectParams =
                serde_json::from_value(params.clone()).map_err(|e| e.to_string())?;
            p.validate()
        }
        "pdf.forms_import" => {
            let p: ariad_core::tools::params::forms::FormsImportParams =
                serde_json::from_value(params.clone()).map_err(|e| e.to_string())?;
            p.validate()
        }
        "web.to_pdf" => {
            let p: ariad_core::tools::params::WebToPdfParams =
                serde_json::from_value(params.clone()).map_err(|e| e.to_string())?;
            p.validate()
        }
        "pdf.split" => {
            let p: SplitParams =
                serde_json::from_value(params.clone()).map_err(|e| e.to_string())?;
            p.validate().map_err(|e| e.to_string())
        }
        "pdf.render" => {
            let p: RenderParams =
                serde_json::from_value(params.clone()).map_err(|e| e.to_string())?;
            p.validate().map_err(|e| e.to_string())
        }
        "pdf.to_images" => {
            let p: ToImagesParams =
                serde_json::from_value(params.clone()).map_err(|e| e.to_string())?;
            p.validate().map_err(|e| e.to_string())
        }
        "pdf.thumbnails" => {
            let p: ThumbnailsParams =
                serde_json::from_value(params.clone()).map_err(|e| e.to_string())?;
            p.validate().map_err(|e| e.to_string())
        }
        "pdf.extract_images" => {
            let p: ExtractImagesParams =
                serde_json::from_value(params.clone()).map_err(|e| e.to_string())?;
            p.validate().map_err(|e| e.to_string())
        }
        "pdf.remove_blank" => {
            let p: RemoveBlankParams =
                serde_json::from_value(params.clone()).map_err(|e| e.to_string())?;
            p.validate().map_err(|e| e.to_string())
        }
        "pdf.crop_whitespace" => {
            let p: CropWhitespaceParams =
                serde_json::from_value(params.clone()).map_err(|e| e.to_string())?;
            p.validate().map_err(|e| e.to_string())
        }
        "image.to_pdf" => {
            let p: ImageToPdfParams =
                serde_json::from_value(params.clone()).map_err(|e| e.to_string())?;
            p.validate().map_err(|e| e.to_string())
        }
        "pdf.to_cbz" => {
            let p: ariad_core::tools::params::ToCbzParams =
                serde_json::from_value(params.clone()).map_err(|e| e.to_string())?;
            p.validate().map_err(|e| e.to_string())
        }
        "pdf.ocr" => serde_json::from_value::<ariad_core::tools::params::OcrParams>(params.clone())
            .map(|_| ())
            .map_err(|e| e.to_string()),
        "image.ocr" => {
            serde_json::from_value::<ariad_core::tools::params::ImageOcrParams>(params.clone())
                .map(|_| ())
                .map_err(|e| e.to_string())
        }
        "pdf.auto_rotate" => {
            serde_json::from_value::<ariad_core::tools::params::AutoRotateParams>(params.clone())
                .map(|_| ())
                .map_err(|e| e.to_string())
        }
        "pdf.deskew" => {
            serde_json::from_value::<ariad_core::tools::params::DeskewParams>(params.clone())
                .map(|_| ())
                .map_err(|e| e.to_string())
        }
        "pdf.scan_split" => {
            serde_json::from_value::<ariad_core::tools::params::ScanSplitParams>(params.clone())
                .map(|_| ())
                .map_err(|e| e.to_string())
        }
        "pdf.scanner_effect" => {
            serde_json::from_value::<ariad_core::tools::params::ScannerEffectParams>(params.clone())
                .map(|_| ())
                .map_err(|e| e.to_string())
        }
        "pdf.sign" => {
            let p: ariad_core::tools::params::SignParams =
                serde_json::from_value(params.clone()).map_err(|e| e.to_string())?;
            p.validate().map_err(|e| e.to_string())
        }
        "pdf.sign_verify" => {
            let p: ariad_core::tools::params::SignVerifyParams =
                serde_json::from_value(params.clone()).map_err(|e| e.to_string())?;
            p.validate().map_err(|e| e.to_string())
        }
        "pdf.sign_remove" => {
            let p: ariad_core::tools::params::SignRemoveParams =
                serde_json::from_value(params.clone()).map_err(|e| e.to_string())?;
            p.validate().map_err(|e| e.to_string())
        }
        "pdf.sign_visual" => {
            let p: ariad_core::tools::params::SignVisualParams =
                serde_json::from_value(params.clone()).map_err(|e| e.to_string())?;
            p.validate().map_err(|e| e.to_string())
        }
        "pdf.sign_token" => {
            let p: ariad_core::tools::params::SignTokenParams =
                serde_json::from_value(params.clone()).map_err(|e| e.to_string())?;
            p.validate().map_err(|e| e.to_string())
        }
        "pdf.timestamp" => {
            let p: ariad_core::tools::params::TimestampParams =
                serde_json::from_value(params.clone()).map_err(|e| e.to_string())?;
            p.validate().map_err(|e| e.to_string())
        }
        "pdf.compare" => {
            serde_json::from_value::<ariad_core::tools::params::CompareParams>(params.clone())
                .map(|_| ())
                .map_err(|e| e.to_string())
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod annotation_form_routing_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn import_payload_and_annotation_list_are_routed_by_action() {
        let empty = json!({});
        for id in ["pdf.forms_import", "pdf.annotations_import"] {
            assert!(!tool_input_is_attachment_payload(id, &empty, 0));
            assert!(tool_input_is_attachment_payload(id, &empty, 1));
        }
        let annotations = ariad_core::tools::find("pdf.annotations").unwrap();
        let forms_export = ariad_core::tools::find("pdf.forms_export").unwrap();
        let annotations_export = ariad_core::tools::find("pdf.annotations_export").unwrap();
        assert!(tool_emits_json(annotations, &json!({"action":"list"})));
        assert!(!tool_emits_json(annotations, &json!({"action":"add"})));
        assert!(tool_emits_json(forms_export, &empty));
        assert!(!tool_emits_json(forms_export, &json!({"format":"csv"})));
        assert!(tool_emits_json(
            annotations_export,
            &json!({"format":"json"})
        ));
        assert!(!tool_emits_json(annotations_export, &empty));
        assert!(validate_semantic_params("pdf.forms_import", &json!({"max_records":0})).is_err());
    }

    #[test]
    fn tool_emits_json_covers_entire_tool_registry_exhaustively() {
        let empty = json!({});
        for spec in ariad_core::tools::all() {
            let is_pure_json = spec.output_format.eq_ignore_ascii_case("json")
                && (spec.output_formats.is_empty()
                    || (spec.output_formats.len() == 1
                        && spec.output_formats[0].eq_ignore_ascii_case("json")));
            if is_pure_json {
                assert!(
                    tool_emits_json(spec, &empty),
                    "pure JSON tool '{}' must return true from tool_emits_json",
                    spec.id
                );
                continue;
            }
            let mentions_json = spec.output_format.eq_ignore_ascii_case("json")
                || spec
                    .output_formats
                    .iter()
                    .any(|f| f.eq_ignore_ascii_case("json"))
                || matches!(
                    spec.id,
                    "pdf.outline" | "pdf.attachments" | "pdf.portfolio" | "pdf.layers"
                );
            if !mentions_json {
                assert!(
                    !tool_emits_json(spec, &empty),
                    "non-JSON tool '{}' must return false from tool_emits_json",
                    spec.id
                );
                continue;
            }
            match spec.id {
                "pdf.compare" | "pdf.thumbnails" | "pdf.to_pdfa" | "pdf.to_pdfua" => {
                    assert!(
                        !tool_emits_json(spec, &empty),
                        "{} must not use inline-only JSON routing",
                        spec.id
                    );
                }
                "ai.summarize" => {
                    assert!(tool_emits_json(spec, &empty));
                    assert!(tool_emits_json(spec, &json!({"format": "json"})));
                    assert!(!tool_emits_json(spec, &json!({"format": "md"})));
                }
                "ai.translate" | "ai.extract_tables" | "pdf.annotations_export" => {
                    assert!(!tool_emits_json(spec, &empty));
                    assert!(tool_emits_json(spec, &json!({"format": "json"})));
                }
                "pdf.forms_export" => {
                    assert!(tool_emits_json(spec, &empty));
                    assert!(tool_emits_json(spec, &json!({"format": "json"})));
                    assert!(!tool_emits_json(spec, &json!({"format": "xfdf"})));
                }
                "pdf.outline" => {
                    assert!(tool_emits_json(spec, &json!({"action": "export"})));
                    assert!(!tool_emits_json(spec, &json!({"action": "clear"})));
                }
                "pdf.attachments" | "pdf.portfolio" | "pdf.layers" | "pdf.annotations" => {
                    assert!(tool_emits_json(spec, &json!({"action": "list"})));
                    assert!(!tool_emits_json(spec, &json!({"action": "remove"})));
                }
                other => panic!(
                    "multi-format JSON tool '{other}' must be explicitly handled by tool_emits_json and this test"
                ),
            }
        }
    }

    #[test]
    fn scrub_and_cap_inline_json_enforces_cap_and_scrubs_secrets() {
        let mut secrets = SecretBag::new();
        secrets.insert("api_key", "top-secret-token").unwrap();
        let scrubber = SecretScrubber::from_bag(&secrets);
        let value = json!({"token": "top-secret-token", "status": "ok"});
        let expected_scrubbed = scrubber.scrub_value(&value);
        assert!(
            !expected_scrubbed["token"]
                .as_str()
                .unwrap()
                .contains("top-secret-token")
        );
        let expected_bytes =
            serde_json::to_vec_pretty(&expected_scrubbed).expect("serialize scrubbed");
        let exact_cap = Limits {
            max_decompressed_bytes: expected_bytes.len() as u64,
            ..Limits::local()
        };
        let (scrubbed, bytes, inline) =
            scrub_and_cap_inline_json(value.clone(), &exact_cap, &scrubber).unwrap();
        assert_eq!(bytes, expected_bytes);
        assert_eq!(scrubbed, expected_scrubbed);
        assert_eq!(inline, Some(scrubbed));

        let too_small = Limits {
            max_decompressed_bytes: (expected_bytes.len() - 1) as u64,
            ..Limits::local()
        };
        let (_, bytes_over, inline_over) =
            scrub_and_cap_inline_json(value, &too_small, &scrubber).unwrap();
        assert_eq!(bytes_over, expected_bytes);
        assert!(inline_over.is_none());
    }
}

#[cfg(all(test, unix))]
mod staged_artifact_error_tests {
    use super::*;

    #[test]
    fn clearing_an_unremovable_artifact_reports_output_io() {
        let workspace = Workspace::new().unwrap();
        std::fs::create_dir(workspace.output_dir().join("result.json")).unwrap();

        let error = write_staged_artifact(&workspace, "result.json", b"{}", &SecretScrubber::new())
            .expect_err("a directory cannot be cleared as an artifact");
        assert!(matches!(error, ToolError::OutputIo(_)), "{error:?}");

        let source = workspace.work_dir().join("source.json");
        std::fs::write(&source, b"{}").unwrap();
        let error = write_staged_artifact_from_file(
            &workspace,
            "result.json",
            &source,
            &SecretScrubber::new(),
            &CancellationToken::new(),
        )
        .expect_err("a directory cannot be cleared as an artifact");
        assert!(matches!(error, ToolError::OutputIo(_)), "{error:?}");
    }

    #[test]
    fn symlink_source_is_refused_with_output_io() {
        let workspace = Workspace::new().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let target = outside.path().join("target.bin");
        std::fs::write(&target, b"secret-bytes").unwrap();
        let link = workspace.work_dir().join("link.bin");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let error = write_staged_artifact_from_file(
            &workspace,
            "final.bin",
            &link,
            &SecretScrubber::new(),
            &CancellationToken::new(),
        )
        .expect_err("a symlink source is refused");
        assert!(matches!(error, ToolError::OutputIo(_)), "{error:?}");
        assert!(!workspace.output_dir().join("final.bin").exists());
    }

    #[test]
    fn cancelled_stage_reports_interrupted() {
        let workspace = Workspace::new().unwrap();
        let source = workspace.work_dir().join("source.bin");
        std::fs::write(&source, vec![7u8; 200 * 1024]).unwrap();
        let cancel = CancellationToken::new();
        cancel.cancel();
        let error = write_staged_artifact_from_file(
            &workspace,
            "final.bin",
            &source,
            &SecretScrubber::new(),
            &cancel,
        )
        .expect_err("cancelled");
        assert!(matches!(error, ToolError::Interrupted), "{error:?}");
        assert!(!workspace.output_dir().join("final.bin").exists());
    }
}
