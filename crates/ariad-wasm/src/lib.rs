//! Browser bindings for AriadShift's in-process conversions.
//!
//! Formats are registry ids (`markdown`, `html`, `ariad-ir+json`). Routes use the
//! capability edges whose runtime includes `wasm`; a new in-process reader or writer in
//! `ariad-core` becomes available here without changing this API. Failures are thrown as
//! plain objects `{ code, message, ...details }`.

pub mod api;

use serde::Serialize;
use wasm_bindgen::prelude::*;

#[wasm_bindgen(typescript_custom_section)]
const TYPES: &str = r#"
/** A format registry id such as "markdown", "html" or "ariad-ir+json". */
export type FormatId = string;
export type ProfileId = "editable" | "faithful" | "fast" | "private";
export type RuntimeId = "wasm" | "local" | "cloud";

/** A non-fatal finding of a reader, a writer or the pipeline. */
export interface Notice {
  code: string;
  message: string;
  source_pos?: { line: number; column: number };
}

export interface EdgeMetrics {
  fidelity: number;
  editability: number;
  p50_ms: number;
  peak_mem_mb: number;
  samples: number;
}

export interface CapabilityEdge {
  from: FormatId;
  to: FormatId;
  engine: string;
  runtime: RuntimeId[];
  license: string;
  metrics: EdgeMetrics | null;
}

export interface Capabilities {
  version: string;
  generated_at: string;
  bench: { fixture_count: number; pandoc_version: string | null; ashift_version: string };
  edges: CapabilityEdge[];
}

export interface DocumentMeta {
  title: string | null;
  authors: string[];
  language: string | null;
  date: string | null;
  subject: string | null;
  keywords: string[];
  source_format: FormatId | null;
}

export interface DocumentCounts {
  /** Heading count per level, keyed "1" to "6". */
  headings: Record<string, number>;
  paragraphs: number;
  tables: number;
  images: number;
  links: number;
  footnotes: number;
  words: number;
}

export interface InspectResult {
  format: FormatId;
  bytes: number;
  meta: DocumentMeta | null;
  counts: DocumentCounts | null;
  warnings: Notice[];
  /** Targets this module can produce. */
  reachable: FormatId[];
  /** Targets only the CLI, desktop or cloud engines can produce. */
  reachable_elsewhere: FormatId[];
}

export interface PlanStep {
  from: FormatId;
  to: FormatId;
  engine: string;
}

export interface PlanScore {
  fidelity: number | null;
  editability: number | null;
  estimated_duration_ms: number | null;
}

export interface Plan {
  steps: PlanStep[];
  score: PlanScore;
  measured: boolean;
  alternatives: Plan[];
}

/** Limit overrides; omitted fields keep the local defaults. */
export interface LimitOverrides {
  max_input_bytes?: number | null;
  max_pages?: number | null;
  timeout_s?: number | null;
  max_memory_mb?: number | null;
  max_asset_bytes?: number | null;
  max_nesting_depth?: number;
  max_blocks?: number;
  max_front_matter_bytes?: number;
  max_archive_entries?: number;
  max_decompressed_bytes?: number;
  max_ir_json_bytes?: number;
}

export interface ConvertOptions {
  profile?: ProfileId;
  /** Title for writers that need one when the document has none, such as HTML. */
  title?: string;
  limits?: LimitOverrides;
}

export interface ConvertResult {
  bytes: Uint8Array;
  format: FormatId;
  media_type: string;
  /** Output file extension without a leading dot, for example "ir.json". */
  extension: string;
  route: PlanStep[];
  warnings: Notice[];
}

export type ToolGroup =
  | "organize"
  | "convert"
  | "optimize"
  | "security"
  | "overlay"
  | "ocr"
  | "annotate"
  | "forms"
  | "sign"
  | "redact"
  | "compare"
  | "edit"
  | "ai"
  | "automation";

export type ToolTier = "free" | "pro";

export type ToolInputArity =
  | { kind: "one" }
  | { kind: "many"; min: number; max: number };

export interface ToolDescriptor {
  id: string;
  version: string;
  group: ToolGroup;
  summary: string;
  inputs: ToolInputArity;
  input_formats: string[];
  output_format: string;
  params_schema: Record<string, unknown>;
  tier: ToolTier;
  runtimes: RuntimeId[];
  engine: string;
  secret_params: string[];
}

export interface ToolArtifactResult {
  name: string;
  format: string;
  media_type: string;
  extension: string;
  bytes: Uint8Array;
}

export interface ToolRunResult {
  tool: string;
  format: string;
  media_type: string;
  extension: string;
  bytes: Uint8Array;
  artifacts: ToolArtifactResult[];
  data: Record<string, unknown> | null;
  warnings: Notice[];
  [field: string]: unknown;
}

export type AriadErrorCode =
  | "unknown_format"
  | "same_format"
  | "no_route"
  | "engine_unavailable"
  | "reader_unavailable"
  | "writer_unavailable"
  | "invalid_options"
  | "invalid_limits"
  | "invalid_input"
  | "invalid_ir"
  | "unsupported_ir_version"
  | "limit_exceeded"
  | "invalid_capabilities"
  | "unknown_tool"
  | "invalid_params"
  | "wrong_input"
  | "not_allowed"
  | "encrypted_input"
  | "engine_failure";

/** The object every export throws on failure. Extra fields depend on the code. */
export interface AriadError {
  code: AriadErrorCode;
  message: string;
  [detail: string]: unknown;
}
"#;

/// Crosses into JavaScript through JSON, so values have exactly the shape of the CLI's
/// `--json` output (integer map keys become strings, `None` becomes `null`).
fn to_js(value: &impl Serialize) -> Result<JsValue, JsValue> {
    let text = serde_json::to_string(value).map_err(|error| JsValue::from(error.to_string()))?;
    js_sys::JSON::parse(&text)
}

fn to_js_error(error: &api::ApiError) -> JsValue {
    to_js(error).unwrap_or_else(|error| error)
}

/// The AriadShift version this module was built from.
#[wasm_bindgen]
#[must_use]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_owned()
}

/// Every known capability edge with its benchmark metrics.
#[wasm_bindgen(unchecked_return_type = "Capabilities")]
pub fn capabilities() -> Result<JsValue, JsValue> {
    to_js(ariad_core::planner::embedded())
}

/// Detects the format of `bytes`, counts its structure and lists reachable targets.
///
/// `name` supplies the extension. Pass `total_bytes` when `bytes` is only a prefix of a
/// large file; above 32 MiB the structure is not analysed.
#[wasm_bindgen(unchecked_return_type = "InspectResult")]
pub fn inspect(bytes: &[u8], name: &str, total_bytes: Option<f64>) -> Result<JsValue, JsValue> {
    let total_bytes = total_bytes.map(|size| size.max(0.0) as u64);
    match api::inspect(bytes, name, total_bytes) {
        Ok(inspection) => to_js(&inspection),
        Err(error) => Err(to_js_error(&error)),
    }
}

/// Plans the route from `input` to `to`.
///
/// `profile` is `editable` (default), `faithful`, `fast` or `private`. `scope` is
/// `browser` (default; the routes `convert` runs) or `all`, which also uses CLI, desktop
/// and cloud engines so a page can explain where a conversion runs.
#[wasm_bindgen(unchecked_return_type = "Plan")]
pub fn plan(
    #[wasm_bindgen(unchecked_param_type = "FormatId")] input: &str,
    #[wasm_bindgen(unchecked_param_type = "FormatId")] to: &str,
    profile: Option<String>,
    scope: Option<String>,
) -> Result<JsValue, JsValue> {
    match api::plan(input, to, profile.as_deref(), scope.as_deref()) {
        Ok(plan) => to_js(&plan),
        Err(error) => Err(to_js_error(&error)),
    }
}

/// Converts `bytes` from format `from` into format `to`.
#[wasm_bindgen(unchecked_return_type = "ConvertResult")]
pub fn convert(
    bytes: &[u8],
    #[wasm_bindgen(unchecked_param_type = "FormatId")] from: &str,
    #[wasm_bindgen(unchecked_param_type = "FormatId")] to: &str,
    #[wasm_bindgen(unchecked_param_type = "ConvertOptions | undefined")] options: JsValue,
) -> Result<JsValue, JsValue> {
    let options = if options.is_undefined() || options.is_null() {
        api::ConvertOptions::default()
    } else {
        let text = js_sys::JSON::stringify(&options)
            .ok()
            .and_then(|text| text.as_string())
            .ok_or_else(|| to_js_error(&api::invalid_options("options must be a plain object")))?;
        api::parse_options(&text).map_err(|error| to_js_error(&error))?
    };
    let conversion = api::convert(bytes, from, to, options).map_err(|error| to_js_error(&error))?;
    let result = to_js(&conversion.meta)?;
    js_sys::Reflect::set(
        &result,
        &JsValue::from_str("bytes"),
        &js_sys::Uint8Array::from(conversion.bytes.as_slice()),
    )?;
    Ok(result)
}

/// Maps Pandoc JSON and extracted media using the shared core mapper.
#[wasm_bindgen(js_name = readPandoc, unchecked_return_type = "ConvertResult")]
pub fn read_pandoc(bytes: &[u8], media: &[u8], source: &str) -> Result<JsValue, JsValue> {
    let format = api::parse_format(source).map_err(|e| to_js_error(&e))?;
    let read = ariad_core::pandoc::package::read(
        bytes,
        media,
        format,
        &ariad_core::limits::Limits::local(),
    )
    .map_err(|e| to_js_error(&api::ApiError::from(e)))?;
    let bytes =
        serde_json::to_vec(&read.document).map_err(|e| JsValue::from_str(&e.to_string()))?;
    bridge_result(
        &bytes,
        "ariad-ir+json",
        "application/json",
        "ir.json",
        &read.warnings,
    )
}

/// Maps IR to Pandoc JSON with the same writer as native surfaces.
#[wasm_bindgen(js_name = writePandoc, unchecked_return_type = "ConvertResult")]
pub fn write_pandoc(bytes: &[u8], title: Option<String>) -> Result<JsValue, JsValue> {
    let written = ariad_core::pandoc::package::write(
        bytes,
        title.as_deref(),
        &ariad_core::limits::Limits::local(),
    )
    .map_err(|e| to_js_error(&api::ApiError::from(e)))?;
    bridge_result(
        &written.bytes,
        "pandoc+json",
        "application/json",
        "json",
        &written.warnings,
    )
}

/// Stamps only package dates; Pandoc stays a separate aggregated module.
#[wasm_bindgen(js_name = stampPackage)]
pub fn stamp_package(bytes: &[u8], epub: bool, timestamp: &str) -> Result<Vec<u8>, JsValue> {
    ariad_core::package_meta::stamp_bytes(bytes, epub, timestamp).map_err(|e| {
        to_js_error(&api::ApiError::from(
            ariad_core::pipeline::PipelineError::InvalidInput {
                reason: e.to_string(),
            },
        ))
    })
}

/// Lists every registered document tool whose supported runtimes include `wasm`.
#[wasm_bindgen(js_name = listTools, unchecked_return_type = "ToolDescriptor[]")]
pub fn list_tools() -> Result<JsValue, JsValue> {
    to_js(&api::list_tools())
}

/// Executes a browser-capable document tool in memory.
///
/// Rejects with `limit_exceeded` when the summed byte size of `inputs` exceeds `budgetBytes`
/// (supplied by the TypeScript worker layer). Calls `ariad-core::tools::access` with the
/// browser plan (`free` by default, or `pro` when the optional `entitled` flag is `true`);
/// browser tier enforcement is advisory because WebAssembly runs on user-controlled hardware.
///
/// Returns a `ToolRunResult` object with:
/// - `tool`, `format`, `media_type`, `extension`,
/// - `bytes` (`Uint8Array` of the primary artifact),
/// - `artifacts` (`ToolArtifactResult[]` in deterministic order, each with `{ name, format, media_type, extension, bytes }`),
/// - `data` (structured JSON object for JSON tools such as `pdf.info`, or `null` for PDF/ZIP/Markdown artifact tools), and
/// - `warnings` (`Notice[]`).
#[wasm_bindgen(js_name = runTool, unchecked_return_type = "ToolRunResult")]
pub fn run_tool(
    #[wasm_bindgen(unchecked_param_type = "Uint8Array[]")] inputs: Vec<js_sys::Uint8Array>,
    names: Vec<String>,
    #[wasm_bindgen(js_name = toolId)] tool_id: &str,
    #[wasm_bindgen(js_name = paramsJson)] params_json: &str,
    #[wasm_bindgen(js_name = budgetBytes)] budget_bytes: f64,
    entitled: Option<bool>,
) -> Result<JsValue, JsValue> {
    let budget = api::parse_budget_bytes(budget_bytes).map_err(|e| to_js_error(&e))?;
    let total_bytes = inputs
        .iter()
        .map(|arr| u64::from(arr.byte_length()))
        .fold(0u64, u64::saturating_add);
    api::check_budget(total_bytes, budget).map_err(|e| to_js_error(&e))?;

    let input_vecs: Vec<Vec<u8>> = inputs.iter().map(js_sys::Uint8Array::to_vec).collect();
    let run = api::run_tool(
        &input_vecs,
        &names,
        tool_id,
        params_json,
        budget,
        entitled.unwrap_or(false),
    )
    .map_err(|e| to_js_error(&e))?;

    tool_run_to_js(&run)
}

fn tool_run_to_js(run: &api::ToolRun) -> Result<JsValue, JsValue> {
    let artifacts = js_sys::Array::new();
    for artifact in &run.artifacts {
        let entry = to_js(artifact)?;
        js_sys::Reflect::set(
            &entry,
            &JsValue::from_str("bytes"),
            &js_sys::Uint8Array::from(artifact.bytes.as_slice()),
        )?;
        artifacts.push(&entry);
    }

    let bytes = js_sys::Uint8Array::from(run.bytes.as_slice());
    let warnings = to_js(&run.warnings)?;

    if let Some(ref data) = run.data {
        let result = to_js(data)?;
        let data_clone = to_js(data)?;
        define_hidden_prop(&result, "tool", &JsValue::from_str(run.tool))?;
        define_hidden_prop(&result, "format", &JsValue::from_str(run.format))?;
        define_hidden_prop(&result, "media_type", &JsValue::from_str(run.media_type))?;
        define_hidden_prop(&result, "extension", &JsValue::from_str(run.extension))?;
        define_hidden_prop(&result, "bytes", &bytes.into())?;
        define_hidden_prop(&result, "artifacts", &artifacts.into())?;
        define_hidden_prop(&result, "data", &data_clone)?;
        define_hidden_prop(&result, "warnings", &warnings)?;
        Ok(result)
    } else {
        let result = to_js(run)?;
        js_sys::Reflect::set(&result, &JsValue::from_str("bytes"), &bytes)?;
        js_sys::Reflect::set(&result, &JsValue::from_str("artifacts"), &artifacts)?;
        js_sys::Reflect::set(&result, &JsValue::from_str("data"), &JsValue::NULL)?;
        Ok(result)
    }
}

fn define_hidden_prop(target: &JsValue, key: &str, value: &JsValue) -> Result<(), JsValue> {
    let Some(obj) = target.dyn_ref::<js_sys::Object>() else {
        return Ok(());
    };
    let desc = js_sys::Object::new();
    js_sys::Reflect::set(&desc, &JsValue::from_str("value"), value)?;
    js_sys::Reflect::set(&desc, &JsValue::from_str("enumerable"), &JsValue::FALSE)?;
    js_sys::Reflect::set(&desc, &JsValue::from_str("configurable"), &JsValue::TRUE)?;
    js_sys::Reflect::set(&desc, &JsValue::from_str("writable"), &JsValue::TRUE)?;
    js_sys::Object::define_property(obj, &JsValue::from_str(key), &desc);
    Ok(())
}

fn bridge_result(
    bytes: &[u8],
    format: &str,
    media_type: &str,
    extension: &str,
    warnings: &[ariad_core::pipeline::Notice],
) -> Result<JsValue, JsValue> {
    let result = to_js(
        &serde_json::json!({ "format": format, "media_type": media_type, "extension": extension, "route": [], "warnings": warnings }),
    )?;
    js_sys::Reflect::set(
        &result,
        &JsValue::from_str("bytes"),
        &js_sys::Uint8Array::from(bytes),
    )?;
    Ok(result)
}
