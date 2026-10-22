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
  | "invalid_capabilities";

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

/// Plans the route from `input` to `to` over the edges that run in the browser.
#[wasm_bindgen(unchecked_return_type = "Plan")]
pub fn plan(
    #[wasm_bindgen(unchecked_param_type = "FormatId")] input: &str,
    #[wasm_bindgen(unchecked_param_type = "FormatId")] to: &str,
    #[wasm_bindgen(unchecked_param_type = "ProfileId | undefined")] profile: Option<String>,
) -> Result<JsValue, JsValue> {
    match api::plan(input, to, profile.as_deref()) {
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
        serde_json::from_str(&text)
            .map_err(|error| to_js_error(&api::invalid_options(&error.to_string())))?
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
