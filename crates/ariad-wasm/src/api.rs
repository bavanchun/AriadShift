//! Behavior behind the JavaScript exports, expressed in Rust types so it is tested natively.

use std::sync::OnceLock;

use ariad_core::{
    format::Format,
    inspect::{self, MemoryInspection},
    limits::Limits,
    pipeline::{self, Notice, PipelineError, WriteOptions},
    planner::{self, Capabilities, Plan, PlanError, PlanStep, Profile, Runtime},
};
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
    /// Only edges that run in the browser; the routes [`convert`] executes.
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
        &route.steps,
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
        assert!(ir.contains("\"version\": \"ariad-ir/0\""), "{ir}");
        let back = convert_text(&ir, "ariad-ir+json", "markdown");
        assert_eq!(back.trim_end(), "# Hi");
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
        let error = plan("markdown", "docx", None, None).unwrap_err();
        assert_eq!(error.code(), "no_route");
        assert_eq!(
            error.detail("reachable_elsewhere"),
            Some(&serde_json::json!(["docx", "epub", "pdf"]))
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
        let route = plan("docx", "markdown", None, Some("all")).unwrap();
        assert_eq!(route.steps[0].engine, "pandoc");
        assert_eq!(
            plan("docx", "markdown", None, None).unwrap_err().code(),
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
            vec![Format::AriadIrJson, Format::Markdown]
        );
    }
}
