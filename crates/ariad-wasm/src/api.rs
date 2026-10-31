//! Behavior behind the JavaScript exports, expressed in Rust types so it is tested natively.

use std::sync::OnceLock;

use ariad_core::{
    format::{self, Format},
    inspect::{self, MemoryInspection},
    limits::Limits,
    pipeline::{self, Notice, PipelineError, WriteOptions},
    planner::{self, Capabilities, Plan, PlanError, PlanStep, Profile, Runtime},
    tools::{
        self, Plan as AccessPlan, RunSize, SecretSource, Tier, ToolAccess, ToolDescriptor,
        ToolRuntime, ToolSpec,
        access::{allowed as tier_allowed, within_run_quota},
        params::InfoParams,
        secret::validate_secret_params,
    },
};
use ariad_pdf::PdfError;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// The edges of the embedded capabilities that can run in the browser.
pub fn browser_capabilities() -> &'static Capabilities {
    static BROWSER: OnceLock<Capabilities> = OnceLock::new();
    BROWSER.get_or_init(|| planner::embedded().restricted_to(Runtime::Wasm))
}

/// An error thrown to JavaScript as a plain object `{ code, message, ...details }`.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ApiError(Map<String, Value>);

impl ApiError {
    fn new(code: &str, message: impl Into<String>) -> Self {
        let mut fields = Map::new();
        fields.insert("code".to_owned(), Value::from(code));
        fields.insert("message".to_owned(), Value::from(message.into()));
        Self(fields)
    }

    fn with(mut self, key: &str, value: impl Serialize) -> Self {
        let value = serde_json::to_value(value).unwrap_or(Value::Null);
        self.0.insert(key.to_owned(), value);
        self
    }

    /// The stable error code, for example `no_route`.
    #[must_use]
    pub fn code(&self) -> &str {
        self.0.get("code").and_then(Value::as_str).unwrap_or("")
    }

    /// A detail field, if present.
    #[must_use]
    pub fn detail(&self, key: &str) -> Option<&Value> {
        self.0.get(key)
    }
}

impl From<PipelineError> for ApiError {
    fn from(error: PipelineError) -> Self {
        let mut fields = match serde_json::to_value(&error) {
            Ok(Value::Object(fields)) => fields,
            _ => Map::new(),
        };
        fields.insert("message".to_owned(), Value::from(error.to_string()));
        Self(fields)
    }
}

/// Options accepted by [`convert`].
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct ConvertOptions {
    /// Routing profile; defaults to `editable`.
    pub profile: Option<String>,
    /// Title for writers that need one when the document has none, such as HTML.
    pub title: Option<String>,
    /// Limit overrides; omitted fields keep the local defaults.
    pub limits: Option<Limits>,
}

/// Parses `convert` options from JSON, rejecting unknown option and limit names so a typo
/// never silently falls back to a default.
pub fn parse_options(text: &str) -> Result<ConvertOptions, ApiError> {
    let value: Value =
        serde_json::from_str(text).map_err(|error| invalid_options(&error.to_string()))?;
    if let Some(Value::Object(limits)) = value.get("limits")
        && let Some(unknown) = limits
            .keys()
            .find(|key| !Limits::FIELD_NAMES.contains(&key.as_str()))
    {
        return Err(invalid_options(&format!("unknown limit `{unknown}`")).with("option", "limits"));
    }
    serde_json::from_value(value).map_err(|error| invalid_options(&error.to_string()))
}

/// Everything about a conversion except its bytes.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ConversionMeta {
    pub format: Format,
    pub media_type: &'static str,
    pub extension: &'static str,
    pub route: Vec<PlanStep>,
    pub warnings: Vec<Notice>,
}

/// Result of [`convert`].
#[derive(Clone, Debug, PartialEq)]
pub struct Conversion {
    pub bytes: Vec<u8>,
    pub meta: ConversionMeta,
}

/// The file extension, without a dot, for output of `format`.
#[must_use]
pub fn output_extension(format: Format) -> &'static str {
    match format {
        Format::AriadIrJson => "ir.json",
        other => other.extensions().first().copied().unwrap_or("bin"),
    }
}

/// Resolves a registry format id.
pub fn parse_format(id: &str) -> Result<Format, ApiError> {
    Format::from_id(id).ok_or_else(|| {
        ApiError::from(PipelineError::UnknownFormat {
            format: Some(id.chars().filter(|c| !c.is_control()).take(64).collect()),
        })
    })
}

/// An `invalid_options` error for options that could not be decoded.
#[must_use]
pub fn invalid_options(reason: &str) -> ApiError {
    ApiError::new("invalid_options", format!("invalid options: {reason}"))
}

/// Resolves a routing profile name; `None` selects `editable`.
pub fn parse_profile(profile: Option<&str>) -> Result<Profile, ApiError> {
    profile.map_or(Ok(Profile::default()), |name| {
        name.parse().map_err(|error: planner::ProfileParseError| {
            ApiError::new("invalid_options", error.to_string()).with("option", "profile")
        })
    })
}

fn reachable_split(from: Format) -> (Vec<Format>, Vec<Format>) {
    let here = inspect::reachable_targets(browser_capabilities(), from);
    let mut elsewhere = inspect::reachable_targets(planner::embedded(), from);
    elsewhere.retain(|target| !here.contains(target));
    (here, elsewhere)
}

/// Which capability edges a plan may use.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PlanScope {
    /// Only edges the browser worker can execute, including its external WASM engines.
    #[default]
    Browser,
    /// Every known edge, including CLI, desktop and cloud engines; for explanations.
    All,
}

/// Resolves a plan scope name; `None` selects `browser`.
pub fn parse_scope(scope: Option<&str>) -> Result<PlanScope, ApiError> {
    match scope {
        None | Some("browser") => Ok(PlanScope::Browser),
        Some("all") => Ok(PlanScope::All),
        Some(other) => Err(ApiError::new(
            "invalid_options",
            format!(
                "unknown plan scope '{}'; valid scopes: browser, all",
                other
                    .chars()
                    .filter(|c| !c.is_control())
                    .take(32)
                    .collect::<String>()
            ),
        )
        .with("option", "scope")),
    }
}

/// Plans a route over the edges that run in the browser.
pub fn plan_route(from: Format, to: Format, profile: Profile) -> Result<Plan, ApiError> {
    plan_in(PlanScope::Browser, from, to, profile)
}

fn plan_in(scope: PlanScope, from: Format, to: Format, profile: Profile) -> Result<Plan, ApiError> {
    let caps = match scope {
        PlanScope::Browser => browser_capabilities(),
        PlanScope::All => planner::embedded(),
    };
    let engines = caps.registered_engines();
    planner::plan(caps, from, to, profile, &engines).map_err(|error| match error {
        PlanError::SameFormat { .. } => ApiError::new("same_format", error.to_string())
            .with("from", from)
            .with("to", to),
        PlanError::NoRoute { .. } => {
            let (reachable, reachable_elsewhere) = reachable_split(from);
            let message = if reachable_elsewhere.contains(&to) {
                format!(
                    "{} to {} does not run in the browser yet; the AriadShift CLI or desktop app can convert it",
                    from.id(),
                    to.id()
                )
            } else {
                error.to_string()
            };
            ApiError::new("no_route", message)
                .with("from", from)
                .with("to", to)
                .with("reachable", reachable)
                .with("reachable_elsewhere", reachable_elsewhere)
        }
        PlanError::InvalidCapabilities(_) => {
            ApiError::new("invalid_capabilities", error.to_string())
        }
    })
}

/// Plans `input` to `to` by format id within `scope` (`browser` or `all`).
pub fn plan(
    input: &str,
    to: &str,
    profile: Option<&str>,
    scope: Option<&str>,
) -> Result<Plan, ApiError> {
    let scope = parse_scope(scope)?;
    plan_in(
        scope,
        parse_format(input)?,
        parse_format(to)?,
        parse_profile(profile)?,
    )
}

/// Inspects a document. `total_bytes` is the full size when `bytes` is only a prefix.
pub fn inspect(
    bytes: &[u8],
    name: &str,
    total_bytes: Option<u64>,
) -> Result<MemoryInspection, ApiError> {
    inspect::inspect_bytes(
        bytes,
        name,
        total_bytes,
        browser_capabilities(),
        planner::embedded(),
        &Limits::local(),
    )
    .map_err(ApiError::from)
}

/// Converts `bytes` of format `from` into format `to` with the in-process engine.
pub fn convert(
    bytes: &[u8],
    from: &str,
    to: &str,
    options: ConvertOptions,
) -> Result<Conversion, ApiError> {
    let from = parse_format(from)?;
    let to = parse_format(to)?;
    let profile = parse_profile(options.profile.as_deref())?;
    let limits = options.limits.unwrap_or_else(Limits::local);
    limits.validate().map_err(PipelineError::from)?;

    let route = plan_route(from, to, profile)?;
    let converted = pipeline::execute(
        bytes,
        &route
            .steps
            .iter()
            .cloned()
            .map(|mut step| {
                if step.engine == "pdf" {
                    step.engine = "ariad-core".to_owned();
                }
                step
            })
            .collect::<Vec<_>>(),
        &limits,
        WriteOptions {
            title_fallback: options.title.as_deref(),
        },
    )?;
    Ok(Conversion {
        bytes: converted.bytes,
        meta: ConversionMeta {
            format: to,
            media_type: to.media_type(),
            extension: output_extension(to),
            route: route.steps,
            warnings: converted.warnings,
        },
    })
}

/// An in-memory artifact produced by [`run_tool`].
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ToolArtifact {
    /// Default output file name (for example `"doc.info.json"`).
    pub name: String,
    /// Output format identifier (`"json"`, `"pdf"`, `"zip"`, `"md"`).
    pub format: &'static str,
    /// MIME media type (`"application/json"`, `"application/pdf"`, etc.).
    pub media_type: &'static str,
    /// File extension without a leading dot (`"json"`, `"pdf"`, etc.).
    pub extension: &'static str,
    /// Raw artifact bytes.
    #[serde(skip_serializing)]
    pub bytes: Vec<u8>,
}

/// Output of [`run_tool`].
///
/// When a tool emits structured JSON (`format == "json"`, such as `pdf.info`), [`Self::data`]
/// holds the parsed JSON value and `Serialize` serializes that value directly so JSON output
/// across CLI, local MCP `run_doc_tool`, and WASM `runTool` is identical.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolRun {
    /// Canonical tool identifier (for example `"pdf.info"`).
    pub tool: &'static str,
    /// Declared output format (`"json"`, `"pdf"`, `"zip"`, `"md"`).
    pub format: &'static str,
    /// MIME media type for the primary output.
    pub media_type: &'static str,
    /// File extension without a leading dot.
    pub extension: &'static str,
    /// Primary output bytes (for `"json"` tools, canonical UTF-8 JSON bytes of `data`).
    pub bytes: Vec<u8>,
    /// Produced artifacts in deterministic order.
    pub artifacts: Vec<ToolArtifact>,
    /// Structured JSON payload when `format == "json"`.
    pub data: Option<Value>,
    /// Non-fatal warnings emitted during execution.
    pub warnings: Vec<Notice>,
}

impl ToolRun {
    /// Returns the structured JSON payload when the tool output format is `"json"`.
    #[must_use]
    pub fn json(&self) -> Option<&Value> {
        self.data.as_ref()
    }
}

impl Serialize for ToolRun {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        if let Some(ref data) = self.data {
            data.serialize(serializer)
        } else {
            use serde::ser::SerializeMap;
            let mut map = serializer.serialize_map(Some(5))?;
            map.serialize_entry("tool", &self.tool)?;
            map.serialize_entry("format", &self.format)?;
            map.serialize_entry("media_type", &self.media_type)?;
            map.serialize_entry("extension", &self.extension)?;
            map.serialize_entry("warnings", &self.warnings)?;
            map.end()
        }
    }
}

/// Lists the descriptors of every registered tool whose supported runtimes include [`ToolRuntime::Wasm`].
#[must_use]
pub fn list_tools() -> Vec<ToolDescriptor> {
    tools::all()
        .iter()
        .filter(|spec| spec.supports_runtime(ToolRuntime::Wasm))
        .map(ToolSpec::to_descriptor)
        .collect()
}

/// Validates and converts a JavaScript `budgetBytes` number into a `u64` byte cap.
pub fn parse_budget_bytes(budget_bytes: f64) -> Result<u64, ApiError> {
    if !budget_bytes.is_finite() || budget_bytes < 0.0 {
        return Err(ApiError::new(
            "invalid_limits",
            "budgetBytes must be a non-negative finite number",
        )
        .with("limit", "max_input_bytes"));
    }
    Ok(budget_bytes as u64)
}

/// Rejects with `limit_exceeded` when the summed input byte size exceeds `budget_bytes`.
pub fn check_budget(total_bytes: u64, budget_bytes: u64) -> Result<(), ApiError> {
    if total_bytes > budget_bytes {
        return Err(ApiError::new(
            "limit_exceeded",
            format!("input size ({total_bytes} B) exceeds the limit of {budget_bytes} B"),
        )
        .with("limit", "max_input_bytes")
        .with("budget_bytes", budget_bytes)
        .with("actual_bytes", total_bytes)
        .with("exit_code", 4u8));
    }
    Ok(())
}

/// Evaluates browser-tier access and batch run quotas for `ariad-wasm`.
///
/// # Advisory Enforcement
///
/// Because WebAssembly executes on user-controlled hardware in the browser, tier gating in
/// `ariad-wasm` is **advisory** (`docs/tools.md` §2). By default (`entitled == false`), the
/// browser surface runs under [`AccessPlan::Free`]. When the TypeScript caller passes an
/// entitlement token flag (`entitled == true`), [`AccessPlan::Pro`] is selected.
pub fn check_browser_access(
    tool_id: &str,
    tier: Tier,
    files: usize,
    total_bytes: u64,
    entitled: bool,
) -> Result<ToolAccess, ApiError> {
    let plan = if entitled {
        AccessPlan::Pro
    } else {
        AccessPlan::Free
    };
    let access = ToolAccess::for_plan(plan);
    if !tier_allowed(tier, plan) || !access.allows(tier) {
        return Err(ApiError::new(
            "not_allowed",
            format!("tool '{tool_id}' requires the {tier} tier (current plan: {plan})"),
        )
        .with("tool", tool_id)
        .with("tier", tier)
        .with("plan", plan)
        .with("advisory", true)
        .with("exit_code", 11u8));
    }
    if !within_run_quota(plan, RunSize::new(files, total_bytes)) {
        return Err(ApiError::new(
            "not_allowed",
            format!(
                "run size ({files} files, {total_bytes} B) exceeds the {plan} plan batch quota"
            ),
        )
        .with("tool", tool_id)
        .with("tier", tier)
        .with("plan", plan)
        .with("advisory", true)
        .with("exit_code", 11u8));
    }
    Ok(access)
}

/// Executes a registered browser tool in memory.
///
/// Rejects immediately with `limit_exceeded` when the summed byte size of `inputs` exceeds
/// `budget_bytes` (supplied by the TypeScript worker layer). Evaluates advisory browser-plan
/// access via [`ariad_core::tools::access`] (`Plan::Free` when `entitled == false`, `Plan::Pro`
/// when `entitled == true`), returning `not_allowed` for `Tier::Pro` tools unless the caller
/// passes the advisory entitlement flag.
pub fn run_tool<I, B, N, S>(
    inputs: I,
    names: N,
    tool_id: &str,
    params_json: &str,
    budget_bytes: u64,
    entitled: bool,
) -> Result<ToolRun, ApiError>
where
    I: AsRef<[B]>,
    B: AsRef<[u8]>,
    N: AsRef<[S]>,
    S: AsRef<str>,
{
    let inputs_slice = inputs.as_ref();
    let names_slice = names.as_ref();
    let total_bytes = inputs_slice
        .iter()
        .map(|b| b.as_ref().len() as u64)
        .fold(0u64, u64::saturating_add);
    check_budget(total_bytes, budget_bytes)?;

    let Some(spec) = tools::find(tool_id) else {
        let sanitized: String = tool_id
            .chars()
            .filter(|c| !c.is_control())
            .take(64)
            .collect();
        return Err(
            ApiError::new("unknown_tool", format!("unknown tool '{sanitized}'"))
                .with("tool", sanitized)
                .with("exit_code", 8u8),
        );
    };

    run_tool_with_spec(
        spec,
        inputs_slice,
        names_slice,
        params_json,
        budget_bytes,
        entitled,
    )
}

/// Executes a tool against an explicit [`ToolSpec`].
pub fn run_tool_with_spec<B, S>(
    spec: &ToolSpec,
    inputs: &[B],
    names: &[S],
    params_json: &str,
    budget_bytes: u64,
    entitled: bool,
) -> Result<ToolRun, ApiError>
where
    B: AsRef<[u8]>,
    S: AsRef<str>,
{
    let total_bytes = inputs
        .iter()
        .map(|b| b.as_ref().len() as u64)
        .fold(0u64, u64::saturating_add);
    check_budget(total_bytes, budget_bytes)?;
    check_browser_access(spec.id, spec.tier, inputs.len(), total_bytes, entitled)?;

    if !spec.supports_runtime(ToolRuntime::Wasm) {
        return Err(ApiError::new(
            "engine_unavailable",
            format!(
                "tool '{}' does not run in the browser; run it with the AriadShift CLI or desktop app",
                spec.id
            ),
        )
        .with("tool", spec.id)
        .with("engine", spec.engine)
        .with("exit_code", 5u8));
    }

    if names.len() != inputs.len() {
        return Err(ApiError::new(
            "wrong_input",
            format!(
                "inputs count ({}) does not match names count ({})",
                inputs.len(),
                names.len()
            ),
        )
        .with("tool", spec.id)
        .with("exit_code", 10u8));
    }

    if !spec.inputs.accepts(inputs.len()) {
        return Err(ApiError::new(
            "wrong_input",
            format!(
                "tool '{}' does not accept {} input file(s)",
                spec.id,
                inputs.len()
            ),
        )
        .with("tool", spec.id)
        .with("exit_code", 10u8));
    }

    let raw_params = if params_json.trim().is_empty() {
        "{}"
    } else {
        params_json
    };
    let params_value: Value = serde_json::from_str(raw_params).map_err(|err| {
        ApiError::new(
            "invalid_params",
            format!("invalid tool parameters JSON: {err}"),
        )
        .with("tool", spec.id)
        .with("exit_code", 9u8)
    })?;
    if !params_value.is_object() {
        return Err(
            ApiError::new("invalid_params", "tool parameters must be a JSON object")
                .with("tool", spec.id)
                .with("exit_code", 9u8),
        );
    }

    validate_secret_params(spec, &params_value, SecretSource::Argv).map_err(|err| {
        ApiError::new("invalid_params", err.to_string())
            .with("tool", spec.id)
            .with("exit_code", 9u8)
    })?;

    for (idx, (input_bytes, input_name)) in inputs.iter().zip(names.iter()).enumerate() {
        let bytes = input_bytes.as_ref();
        let name = input_name.as_ref();
        let detected = sniff_tool_input_format(bytes, name);
        let accepted = detected.is_some_and(|fmt| spec.accepts_core_format(fmt));
        if !accepted {
            let found = detected.map_or("unknown", Format::id);
            return Err(ApiError::new(
                "wrong_input",
                format!(
                    "input #{idx} ('{name}') has format '{found}', which is not accepted by '{}'",
                    spec.id
                ),
            )
            .with("tool", spec.id)
            .with("input_index", idx)
            .with("detected_format", found)
            .with("accepted_formats", spec.input_formats)
            .with("exit_code", 10u8));
        }
    }

    let limits = Limits {
        max_input_bytes: Some(budget_bytes.max(1)),
        ..Limits::local()
    };

    match spec.id {
        "pdf.info" => execute_pdf_info(
            spec,
            inputs[0].as_ref(),
            names[0].as_ref(),
            &params_value,
            &limits,
        ),
        other => Err(ApiError::new(
            "engine_unavailable",
            format!("tool '{other}' is not implemented in ariad-wasm"),
        )
        .with("tool", spec.id)
        .with("engine", spec.engine)
        .with("exit_code", 5u8)),
    }
}

fn execute_pdf_info(
    spec: &ToolSpec,
    bytes: &[u8],
    name: &str,
    params_value: &Value,
    limits: &Limits,
) -> Result<ToolRun, ApiError> {
    let _: InfoParams = serde_json::from_value(params_value.clone()).map_err(|err| {
        ApiError::new("invalid_params", format!("invalid tool parameters: {err}"))
            .with("tool", spec.id)
            .with("exit_code", 9u8)
    })?;

    let info = ariad_pdf::info::run(bytes, limits).map_err(|err| map_pdf_error(spec.id, err))?;
    let data = serde_json::to_value(&info)
        .map_err(|err| ApiError::new("engine_failure", err.to_string()).with("exit_code", 1u8))?;
    let json_bytes = serde_json::to_vec(&data)
        .map_err(|err| ApiError::new("engine_failure", err.to_string()).with("exit_code", 1u8))?;
    let artifact_name = default_artifact_name(name, "info", "json");

    Ok(ToolRun {
        tool: spec.id,
        format: "json",
        media_type: "application/json",
        extension: "json",
        bytes: json_bytes.clone(),
        artifacts: vec![ToolArtifact {
            name: artifact_name,
            format: "json",
            media_type: "application/json",
            extension: "json",
            bytes: json_bytes,
        }],
        data: Some(data),
        warnings: Vec::new(),
    })
}

fn sniff_tool_input_format(bytes: &[u8], name: &str) -> Option<Format> {
    if let Some(content_format) = format::classify(bytes, None, None) {
        return Some(content_format);
    }
    let extension = name.rsplit_once('.').map(|(_, ext)| ext);
    let candidate = format::classify(bytes, None, extension)?;
    match candidate {
        Format::Pdf | Format::Png | Format::Jpeg | Format::Tiff | Format::Webp => None,
        other => Some(other),
    }
}

fn default_artifact_name(input_name: &str, suffix: &str, ext: &str) -> String {
    let file_name = input_name.rsplit(['/', '\\']).next().unwrap_or(input_name);
    let stem = file_name
        .rsplit_once('.')
        .map_or(file_name, |(stem, _)| stem);
    let clean_stem = if stem.is_empty() { "document" } else { stem };
    format!("{clean_stem}.{suffix}.{ext}")
}

fn map_pdf_error(tool_id: &str, error: PdfError) -> ApiError {
    match error {
        PdfError::Malformed => ApiError::new("invalid_input", error.to_string())
            .with("tool", tool_id)
            .with("reason", error.to_string())
            .with("pdf_error", error.code())
            .with("exit_code", 1u8),
        PdfError::Encrypted { needs_password } => {
            ApiError::new("encrypted_input", error.to_string())
                .with("tool", tool_id)
                .with("needs_password", needs_password)
                .with("exit_code", 12u8)
        }
        PdfError::LimitExceeded(read_error) => map_read_limit_error(tool_id, read_error),
        PdfError::InvalidParams(reason) => ApiError::new(
            "invalid_params",
            format!("invalid tool parameters: {reason}"),
        )
        .with("tool", tool_id)
        .with("exit_code", 9u8),
        PdfError::NotImplemented(tool) => ApiError::new(
            "engine_unavailable",
            format!("tool `{tool}` is not implemented"),
        )
        .with("tool", tool)
        .with("exit_code", 5u8),
    }
}

fn map_read_limit_error(tool_id: &str, error: ariad_core::reader::pdf::ReadError) -> ApiError {
    use ariad_core::reader::pdf::ReadError;

    match error {
        ReadError::InvalidLimits(limits_error) => {
            ApiError::from(PipelineError::from(limits_error)).with("tool", tool_id)
        }
        ReadError::InputTooLarge { .. } => ApiError::new("limit_exceeded", error.to_string())
            .with("limit", "max_input_bytes")
            .with("tool", tool_id)
            .with("exit_code", 4u8),
        ReadError::TooManyPages { .. } => ApiError::new("limit_exceeded", error.to_string())
            .with("limit", "max_pages")
            .with("tool", tool_id)
            .with("exit_code", 4u8),
        ReadError::TooManyObjects { .. } | ReadError::DecompressedTooLarge { .. } => {
            ApiError::new("limit_exceeded", error.to_string())
                .with("limit", "max_decompressed_bytes")
                .with("tool", tool_id)
                .with("exit_code", 4u8)
        }
        ReadError::NestingTooDeep { .. } => ApiError::new("limit_exceeded", error.to_string())
            .with("limit", "max_nesting_depth")
            .with("tool", tool_id)
            .with("exit_code", 4u8),
        ReadError::TooManyBlocks { .. } => ApiError::new("limit_exceeded", error.to_string())
            .with("limit", "max_blocks")
            .with("tool", tool_id)
            .with("exit_code", 4u8),
        ReadError::Encrypted | ReadError::Malformed => {
            ApiError::new("limit_exceeded", error.to_string())
                .with("tool", tool_id)
                .with("exit_code", 4u8)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn convert_text(text: &str, from: &str, to: &str) -> String {
        let conversion = convert(text.as_bytes(), from, to, ConvertOptions::default()).unwrap();
        String::from_utf8(conversion.bytes).unwrap()
    }

    #[test]
    fn converts_between_markdown_html_and_ir_json() {
        let html = convert_text("# Hi\n\nText\n", "markdown", "html");
        assert!(html.contains("<h1>Hi</h1>"), "{html}");

        let markdown = convert_text("<h2>Title</h2><p>Body</p>", "html", "markdown");
        assert!(markdown.contains("## Title"), "{markdown}");

        let ir = convert_text("# Hi\n", "markdown", "ariad-ir+json");
        assert!(ir.contains("\"version\": \"ariad-ir/1\""), "{ir}");
        let back = convert_text(&ir, "ariad-ir+json", "markdown");
        assert_eq!(back.trim_end(), "# Hi");
    }

    #[test]
    fn plans_every_document_pair_in_the_browser() {
        let formats = ["markdown", "html", "docx", "epub"];
        for from in formats {
            for to in formats {
                if from != to {
                    let route = plan(from, to, None, None).unwrap();
                    assert_eq!(route.steps.first().unwrap().from.id(), from);
                    assert_eq!(route.steps.last().unwrap().to.id(), to);
                    assert!(
                        route
                            .steps
                            .iter()
                            .all(|step| matches!(step.engine.as_str(), "ariad-core" | "pandoc"))
                    );
                }
            }
        }
    }

    #[test]
    fn reports_route_media_type_and_extension() {
        let conversion = convert(
            b"# Hi\n",
            "markdown",
            "ariad-ir+json",
            ConvertOptions::default(),
        )
        .unwrap();
        assert_eq!(conversion.meta.format, Format::AriadIrJson);
        assert_eq!(conversion.meta.media_type, "application/json");
        assert_eq!(conversion.meta.extension, "ir.json");
        assert_eq!(conversion.meta.route.len(), 1);
        assert_eq!(conversion.meta.route[0].engine, "ariad-core");
    }

    #[test]
    fn typed_errors_carry_codes_and_details() {
        let error = plan("odt", "markdown", None, None).unwrap_err();
        assert_eq!(error.code(), "no_route");
        assert_eq!(
            error.detail("reachable_elsewhere"),
            Some(&serde_json::json!([
                "ariad-ir+json",
                "docx",
                "epub",
                "html",
                "markdown",
                "pdf"
            ]))
        );
        assert!(
            error
                .detail("message")
                .unwrap()
                .as_str()
                .unwrap()
                .contains("CLI")
        );

        assert_eq!(
            plan("markdown", "markdown", None, None).unwrap_err().code(),
            "same_format"
        );
        assert_eq!(
            plan("md", "html", None, None).unwrap_err().code(),
            "unknown_format"
        );
        assert_eq!(
            plan("markdown", "html", Some("best"), None)
                .unwrap_err()
                .code(),
            "invalid_options"
        );

        let error = convert(b"\xff", "markdown", "html", ConvertOptions::default()).unwrap_err();
        assert_eq!(error.code(), "invalid_input");
    }

    #[test]
    fn plans_cli_routes_only_in_the_all_scope() {
        let route = plan("odt", "docx", None, Some("all")).unwrap();
        assert_eq!(route.steps[0].engine, "libreoffice");
        assert_eq!(
            plan("odt", "docx", None, None).unwrap_err().code(),
            "no_route"
        );
        assert_eq!(
            plan("docx", "markdown", None, Some("cloud"))
                .unwrap_err()
                .code(),
            "invalid_options"
        );
    }

    #[test]
    fn limits_override_local_defaults() {
        let options = ConvertOptions {
            limits: Some(Limits {
                max_blocks: 1,
                ..Limits::local()
            }),
            ..ConvertOptions::default()
        };
        let error = convert(b"a\n\nb\n", "markdown", "html", options).unwrap_err();
        assert_eq!(error.code(), "limit_exceeded");
        assert_eq!(
            error.detail("limit"),
            Some(&serde_json::json!("max_blocks"))
        );

        let options = ConvertOptions {
            limits: Some(Limits {
                max_nesting_depth: 200,
                ..Limits::local()
            }),
            ..ConvertOptions::default()
        };
        let error = convert(b"a", "markdown", "html", options).unwrap_err();
        assert_eq!(error.code(), "invalid_limits");
    }

    #[test]
    fn options_reject_unknown_fields() {
        let error = parse_options(r#"{"speed":"fast"}"#).unwrap_err();
        assert_eq!(error.code(), "invalid_options");
        let error = parse_options(r#"{"limits":{"max_block":5}}"#).unwrap_err();
        assert_eq!(error.detail("option"), Some(&serde_json::json!("limits")));
        let options = parse_options(r#"{"limits":{"max_blocks":5}}"#).unwrap();
        let limits = options.limits.unwrap();
        assert_eq!(limits.max_blocks, 5);
        assert_eq!(limits.max_nesting_depth, Limits::local().max_nesting_depth);
    }

    #[test]
    fn inspects_with_browser_reachability() {
        let inspection = inspect(b"<!doctype html><p>x</p>", "page.html", None).unwrap();
        assert_eq!(inspection.format, Format::Html);
        assert_eq!(
            inspection.reachable,
            vec![
                Format::AriadIrJson,
                Format::Docx,
                Format::Epub,
                Format::Markdown,
                Format::Pdf
            ]
        );
    }

    fn pdf_fixtures_dir() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/pdf-tools")
    }

    fn read_pdf_fixture(name: &str) -> Vec<u8> {
        std::fs::read(pdf_fixtures_dir().join(name)).expect("read pdf fixture")
    }

    fn read_pdf_truth(stem: &str) -> Value {
        let raw = std::fs::read_to_string(pdf_fixtures_dir().join(format!("{stem}.truth.json")))
            .expect("read truth file");
        serde_json::from_str(&raw).expect("parse truth JSON")
    }

    fn encrypt_aes256_for_test(
        plaintext_bytes: &[u8],
        user_password: &str,
        owner_password: &str,
        allow_print_extract: bool,
    ) -> Vec<u8> {
        use lopdf::{
            Document, EncryptionState, EncryptionVersion, Permissions,
            encryption::crypt_filters::{Aes256CryptFilter, CryptFilter},
        };
        use std::{collections::BTreeMap, sync::Arc};

        let mut doc = Document::load_mem(plaintext_bytes).expect("load plaintext PDF");
        let permissions = if allow_print_extract {
            Permissions::PRINTABLE
                | Permissions::PRINTABLE_IN_HIGH_QUALITY
                | Permissions::COPYABLE
                | Permissions::COPYABLE_FOR_ACCESSIBILITY
                | Permissions::ANNOTABLE
                | Permissions::FILLABLE
                | Permissions::MODIFIABLE
                | Permissions::ASSEMBLABLE
        } else {
            Permissions::empty()
        };
        let filter: Arc<dyn CryptFilter> = Arc::new(Aes256CryptFilter);
        let crypt_filters = BTreeMap::from([(b"StdCF".to_vec(), filter)]);
        let key = [0x42_u8; 32];
        let version = EncryptionVersion::V5 {
            encrypt_metadata: true,
            crypt_filters,
            file_encryption_key: &key,
            stream_filter: b"StdCF".to_vec(),
            string_filter: b"StdCF".to_vec(),
            owner_password,
            user_password,
            permissions,
        };
        let state = EncryptionState::try_from(version).expect("build AES-256 encryption state");
        doc.encrypt(&state).expect("encrypt PDF with AES-256");
        let mut out = Vec::new();
        doc.save_to(&mut out).expect("save encrypted PDF");
        out
    }

    #[test]
    fn list_tools_returns_only_wasm_runtime_specs() {
        let tools = list_tools();
        assert!(!tools.is_empty());
        assert!(
            tools
                .iter()
                .all(|descriptor| descriptor.runtimes.contains(&ToolRuntime::Wasm))
        );
        let pdf_info = tools
            .iter()
            .find(|descriptor| descriptor.id.as_str() == "pdf.info")
            .expect("pdf.info must be in list_tools()");
        assert_eq!(pdf_info.tier, Tier::Free);
        assert_eq!(pdf_info.output_format, "json");
        assert_eq!(pdf_info.engine, "ariad-pdf");
    }

    #[test]
    fn run_tool_pdf_info_equals_truth_for_every_corpus_file() {
        let dir = pdf_fixtures_dir();
        let mut truth_paths: Vec<std::path::PathBuf> = std::fs::read_dir(&dir)
            .expect("read fixtures/pdf-tools")
            .filter_map(|entry| {
                let path = entry.ok()?.path();
                path.file_name()?
                    .to_str()?
                    .ends_with(".truth.json")
                    .then_some(path)
            })
            .collect();
        truth_paths.sort();
        assert_eq!(truth_paths.len(), 22);

        let budget = 64 * 1024 * 1024;
        for truth_path in truth_paths {
            let file_name = truth_path.file_name().unwrap().to_str().unwrap();
            let stem = file_name.strip_suffix(".truth.json").unwrap();
            let truth = read_pdf_truth(stem);
            let expected_value = truth
                .get("pdf_info")
                .cloned()
                .unwrap_or_else(|| panic!("missing pdf_info in {file_name}"));

            match stem {
                "encrypted-owner-only" => {
                    let plaintext = read_pdf_fixture("encrypted-owner-only.plaintext.pdf");
                    let encrypted =
                        encrypt_aes256_for_test(&plaintext, "", "ariad-test-owner-only", false);
                    let run = run_tool(
                        [&encrypted],
                        ["encrypted-owner-only.pdf"],
                        "pdf.info",
                        "{}",
                        budget,
                        false,
                    )
                    .expect("pdf.info succeeds on owner-password-only PDF");
                    let actual_value = serde_json::to_value(&run).unwrap();
                    assert_eq!(actual_value, expected_value, "JSON mismatch on {stem}");
                    assert_eq!(
                        actual_value.get("encrypted"),
                        Some(&Value::Bool(true)),
                        "owner-password-only PDF must report encrypted: true"
                    );
                    let from_bytes: Value = serde_json::from_slice(&run.bytes).unwrap();
                    assert_eq!(from_bytes, expected_value);
                }
                "encrypted-aes256" => {
                    let plaintext = read_pdf_fixture("encrypted-aes256.plaintext.pdf");
                    let encrypted = encrypt_aes256_for_test(
                        &plaintext,
                        "ariad-test-user-aes256",
                        "ariad-test-owner-aes256",
                        true,
                    );
                    let err = run_tool(
                        [&encrypted],
                        ["encrypted-aes256.pdf"],
                        "pdf.info",
                        "{}",
                        budget,
                        false,
                    )
                    .unwrap_err();
                    assert_eq!(err.code(), "encrypted_input");
                    assert_eq!(err.detail("needs_password"), Some(&Value::Bool(true)));
                    assert_eq!(err.detail("exit_code"), Some(&serde_json::json!(12)));
                }
                "encrypted-rc4-40" => {
                    let plaintext = read_pdf_fixture("encrypted-rc4-40.plaintext.pdf");
                    let encrypted = encrypt_aes256_for_test(
                        &plaintext,
                        "ariad-user-rc4",
                        "ariad-owner-rc4",
                        true,
                    );
                    let err = run_tool(
                        [&encrypted],
                        ["encrypted-rc4-40.pdf"],
                        "pdf.info",
                        "{}",
                        budget,
                        false,
                    )
                    .unwrap_err();
                    assert_eq!(err.code(), "encrypted_input");
                    assert_eq!(err.detail("needs_password"), Some(&Value::Bool(true)));
                }
                _ => {
                    let pdf_bytes = read_pdf_fixture(&format!("{stem}.pdf"));
                    let run = run_tool(
                        [&pdf_bytes],
                        [format!("{stem}.pdf")],
                        "pdf.info",
                        "{}",
                        budget,
                        false,
                    )
                    .unwrap_or_else(|err| panic!("run_tool failed on {stem}: {err:?}"));
                    let actual_value = serde_json::to_value(&run).unwrap();
                    assert_eq!(actual_value, expected_value, "JSON mismatch on {stem}");
                    let from_bytes: Value = serde_json::from_slice(&run.bytes).unwrap();
                    assert_eq!(from_bytes, expected_value);
                }
            }
        }
    }

    #[test]
    fn run_tool_rejects_when_summed_input_size_exceeds_budget_bytes() {
        let multipage = read_pdf_fixture("multipage-20.pdf");
        let single_budget = (multipage.len() as u64) - 1;
        let err = run_tool(
            [&multipage],
            ["multipage-20.pdf"],
            "pdf.info",
            "{}",
            single_budget,
            false,
        )
        .unwrap_err();
        assert_eq!(err.code(), "limit_exceeded");
        assert_eq!(
            err.detail("limit"),
            Some(&serde_json::json!("max_input_bytes"))
        );
        assert_eq!(
            err.detail("budget_bytes"),
            Some(&serde_json::json!(single_budget))
        );
        assert_eq!(
            err.detail("actual_bytes"),
            Some(&serde_json::json!(multipage.len() as u64))
        );

        // Two inputs whose individual sizes are below the budget but whose sum exceeds it.
        let sum_budget = (multipage.len() as u64) + 10;
        let err = run_tool(
            [&multipage, &multipage],
            ["a.pdf", "b.pdf"],
            "pdf.info",
            "{}",
            sum_budget,
            false,
        )
        .unwrap_err();
        assert_eq!(err.code(), "limit_exceeded");
        assert_eq!(
            err.detail("actual_bytes"),
            Some(&serde_json::json!((multipage.len() * 2) as u64))
        );
    }

    #[test]
    fn run_tool_enforces_advisory_browser_tier_and_entitlement_flag() {
        use ariad_core::tools::{InputArity, ToolGroup};

        let multipage = read_pdf_fixture("multipage-20.pdf");
        let budget = 64 * 1024 * 1024;
        let pro_spec = ToolSpec {
            id: "pdf.info",
            version: "1",
            group: ToolGroup::Organize,
            summary: "Inspect PDF metadata.",
            inputs: InputArity::One,
            input_formats: &["pdf"],
            output_format: "json",
            params_schema: ariad_core::tools::params::schema_for_params::<InfoParams>,
            tier: Tier::Pro,
            runtimes: &[ToolRuntime::Wasm, ToolRuntime::Local],
            engine: "ariad-pdf",
            secret_params: &[],
        };

        let denied = run_tool_with_spec(
            &pro_spec,
            &[&multipage],
            &["multipage-20.pdf"],
            "{}",
            budget,
            false,
        )
        .unwrap_err();
        assert_eq!(denied.code(), "not_allowed");
        assert_eq!(denied.detail("tier"), Some(&serde_json::json!("pro")));
        assert_eq!(denied.detail("plan"), Some(&serde_json::json!("free")));
        assert_eq!(denied.detail("advisory"), Some(&Value::Bool(true)));
        assert_eq!(denied.detail("exit_code"), Some(&serde_json::json!(11)));

        let allowed = run_tool_with_spec(
            &pro_spec,
            &[&multipage],
            &["multipage-20.pdf"],
            "{}",
            budget,
            true,
        )
        .expect("entitled browser caller is allowed to run Tier::Pro tool");
        assert_eq!(allowed.tool, "pdf.info");
        assert!(allowed.json().is_some());
    }

    #[test]
    fn run_tool_validates_tool_id_inputs_formats_params_and_secrets() {
        use ariad_core::tools::{InputArity, ToolGroup};

        let multipage = read_pdf_fixture("multipage-20.pdf");
        let budget = 64 * 1024 * 1024;

        let unknown = run_tool(
            [&multipage],
            ["multipage-20.pdf"],
            "pdf.no_such_tool",
            "{}",
            budget,
            false,
        )
        .unwrap_err();
        assert_eq!(unknown.code(), "unknown_tool");
        assert_eq!(unknown.detail("exit_code"), Some(&serde_json::json!(8)));

        let wrong_arity = run_tool(
            [&multipage, &multipage],
            ["a.pdf", "b.pdf"],
            "pdf.info",
            "{}",
            budget,
            false,
        )
        .unwrap_err();
        assert_eq!(wrong_arity.code(), "wrong_input");
        assert_eq!(
            wrong_arity.detail("exit_code"),
            Some(&serde_json::json!(10))
        );

        let wrong_format = run_tool(
            [b"# Markdown\n".as_slice()],
            ["notes.md"],
            "pdf.info",
            "{}",
            budget,
            false,
        )
        .unwrap_err();
        assert_eq!(wrong_format.code(), "wrong_input");
        assert_eq!(
            wrong_format.detail("detected_format"),
            Some(&serde_json::json!("markdown"))
        );

        let fake_pdf = run_tool(
            [b"not a pdf at all".as_slice()],
            ["fake.pdf"],
            "pdf.info",
            "{}",
            budget,
            false,
        )
        .unwrap_err();
        assert_eq!(fake_pdf.code(), "wrong_input");

        let bad_params = run_tool(
            [&multipage],
            ["multipage-20.pdf"],
            "pdf.info",
            r#"{"unknown": 1}"#,
            budget,
            false,
        )
        .unwrap_err();
        assert_eq!(bad_params.code(), "invalid_params");
        assert_eq!(bad_params.detail("exit_code"), Some(&serde_json::json!(9)));

        let secret_spec = ToolSpec {
            id: "pdf.info",
            version: "1",
            group: ToolGroup::Security,
            summary: "Inspect PDF metadata.",
            inputs: InputArity::One,
            input_formats: &["pdf"],
            output_format: "json",
            params_schema: ariad_core::tools::params::schema_for_params::<InfoParams>,
            tier: Tier::Free,
            runtimes: &[ToolRuntime::Wasm],
            engine: "ariad-pdf",
            secret_params: &["password"],
        };
        let inline_secret = run_tool_with_spec(
            &secret_spec,
            &[&multipage],
            &["multipage-20.pdf"],
            r#"{"password": "inline-secret"}"#,
            budget,
            false,
        )
        .unwrap_err();
        assert_eq!(inline_secret.code(), "invalid_params");

        let local_only_spec = ToolSpec {
            id: "pdf.info",
            version: "1",
            group: ToolGroup::Organize,
            summary: "Inspect PDF metadata.",
            inputs: InputArity::One,
            input_formats: &["pdf"],
            output_format: "json",
            params_schema: ariad_core::tools::params::schema_for_params::<InfoParams>,
            tier: Tier::Free,
            runtimes: &[ToolRuntime::Local],
            engine: "qpdf",
            secret_params: &[],
        };
        let unavailable = run_tool_with_spec(
            &local_only_spec,
            &[&multipage],
            &["multipage-20.pdf"],
            "{}",
            budget,
            false,
        )
        .unwrap_err();
        assert_eq!(unavailable.code(), "engine_unavailable");
        assert_eq!(
            unavailable.detail("engine"),
            Some(&serde_json::json!("qpdf"))
        );
    }
}
