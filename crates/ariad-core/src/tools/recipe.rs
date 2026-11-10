//! Pipeline recipes (`ariad-recipe/1`), naming template validation, and untrusted recipe rules (`docs/tools.md` §9).

use schemars::{JsonSchema, generate::SchemaSettings};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::{
    format::Format,
    planner::Profile,
    tools::{
        registry::{self, ToolId},
        secret::{
            KNOWN_SECRET_FIELD_NAMES, SecretSource, SecretValidationError, validate_secret_fields,
            validate_secret_params,
        },
    },
};

/// Frozen version string for `ariad-recipe/1`.
pub const RECIPE_VERSION: &str = "ariad-recipe/1";

/// Canonical JSON Schema `$id` for `schemas/recipe.v1.json`.
pub const RECIPE_SCHEMA_ID: &str = "https://ariadshift.ariadnev.com/schemas/recipe.v1.json";

/// Allowed placeholder tokens inside a recipe output naming template (`docs/tools.md` §9).
pub const ALLOWED_NAMING_TOKENS: &[&str] = &["stem", "n", "date", "title"];

/// Tools that perform remote network fetches via the host and are forbidden in imported recipes.
pub const NETWORK_FETCH_TOOLS: &[&str] = &["pdf.from_url", "web.to_pdf"];

/// Trust classification of a recipe (`docs/tools.md` §9).
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    Eq,
    PartialEq,
    Ord,
    PartialOrd,
    Hash,
    Serialize,
    Deserialize,
    JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum RecipeOrigin {
    /// Constructed locally by a trusted caller or interactive builder.
    #[default]
    Trusted,
    /// Loaded from an external `.json` file, shared URL, clipboard import, Quick Look, Shortcut,
    /// or natural-language AI generator (`ai.recipe_from_text`).
    Imported,
}

impl RecipeOrigin {
    /// Returns `true` if executing a recipe from this origin requires explicit user confirmation.
    #[must_use]
    pub const fn requires_confirmation(self) -> bool {
        matches!(self, Self::Imported)
    }
}

/// Target format and optional routing profile of a conversion step in a [`Recipe`].
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConvertTarget {
    /// Target format identifier (e.g. `Format::Docx`, `Format::Markdown`).
    pub to: Format,
    /// Optional routing profile (`editable`, `faithful`, `fast`, `private`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<Profile>,
}

/// Behavior when a recipe step fails for an input item.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StepErrorPolicy {
    /// Abort the recipe without publishing any artifacts.
    #[default]
    Stop,
    /// Drop failed items and execute subsequent steps on the remainder.
    Skip,
    /// Pass failed items unchanged to subsequent steps.
    Continue,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct RawToolStep {
    /// Tool identifier to execute.
    tool: ToolId,
    /// Tool parameters object (defaults to `{}`).
    #[serde(default)]
    params: Map<String, Value>,
    #[serde(rename = "if", default, skip_serializing_if = "Option::is_none")]
    r#if: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    on_error: Option<StepErrorPolicy>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct RawConvertStep {
    /// Standard format conversion step.
    convert: ConvertTarget,
    #[serde(rename = "if", default, skip_serializing_if = "Option::is_none")]
    r#if: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    on_error: Option<StepErrorPolicy>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
enum RawStep {
    Tool(RawToolStep),
    Convert(RawConvertStep),
}

/// A single sequential step in a [`Recipe`] (`docs/tools.md` §9).
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(try_from = "RawStep", into = "RawStep")]
pub enum Step {
    /// Invokes a registered tool from [`crate::tools::registry`].
    Tool {
        /// Tool identifier.
        tool: ToolId,
        /// Tool parameters JSON object.
        params: Value,
        /// Optional predicate evaluated against each current input item.
        r#if: Option<String>,
        /// Error policy, defaulting to stopping the entire recipe.
        on_error: Option<StepErrorPolicy>,
    },
    /// Runs the standard conversion pipeline to `to` with optional `profile`.
    Convert {
        /// Target document format.
        to: Format,
        /// Optional conversion profile.
        profile: Option<Profile>,
        /// Optional predicate evaluated against each current input item.
        r#if: Option<String>,
        /// Error policy, defaulting to stopping the entire recipe.
        on_error: Option<StepErrorPolicy>,
    },
}

impl TryFrom<RawStep> for Step {
    type Error = String;

    fn try_from(raw: RawStep) -> Result<Self, Self::Error> {
        match raw {
            RawStep::Tool(RawToolStep {
                tool,
                params,
                r#if,
                on_error,
            }) => Ok(Self::Tool {
                tool,
                params: Value::Object(params),
                r#if,
                on_error,
            }),
            RawStep::Convert(RawConvertStep {
                convert: ConvertTarget { to, profile },
                r#if,
                on_error,
            }) => Ok(Self::Convert {
                to,
                profile,
                r#if,
                on_error,
            }),
        }
    }
}

impl From<Step> for RawStep {
    fn from(step: Step) -> Self {
        match step {
            Step::Tool {
                tool,
                params,
                r#if,
                on_error,
            } => {
                let map = match params {
                    Value::Object(map) => map,
                    _ => Map::new(),
                };
                Self::Tool(RawToolStep {
                    tool,
                    params: map,
                    r#if,
                    on_error,
                })
            }
            Step::Convert {
                to,
                profile,
                r#if,
                on_error,
            } => Self::Convert(RawConvertStep {
                convert: ConvertTarget { to, profile },
                r#if,
                on_error,
            }),
        }
    }
}

/// A reproducible multi-step tool and conversion pipeline (`ariad-recipe/1`, `docs/tools.md` §9).
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Recipe {
    /// Schema version, always `"ariad-recipe/1"`.
    pub version: String,
    /// Ordered pipeline steps executed sequentially.
    #[schemars(length(min = 1))]
    pub steps: Vec<Step>,
    /// Optional output naming template (`[A-Za-z0-9._ -]` plus `{stem}`, `{n}`, `{date}`, `{title}`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub naming: Option<String>,
    /// Optional watch-folder path (forbidden when `origin` is [`RecipeOrigin::Imported`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub watch: Option<String>,
}

/// Error returned when validating a recipe output naming template (`docs/tools.md` §9).
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum NamingTemplateError {
    /// The reproducible timestamp or clock is invalid.
    #[error("invalid date for naming template")]
    InvalidDate,
    /// Naming template is empty or whitespace-only.
    #[error("naming template must not be empty")]
    Empty,
    /// Naming template starts with `.` (hidden file) or contains `..` path traversal.
    #[error("naming template {template:?} must not start with '.' or contain '..'")]
    DisallowedDotSegment {
        /// Offending template string.
        template: String,
    },
    /// Naming template contains an unclosed `{` or stray `}` brace.
    #[error("naming template {template:?} has unbalanced placeholder braces")]
    UnbalancedBraces {
        /// Offending template string.
        template: String,
    },
    /// Naming template contains a placeholder token outside `{stem}`, `{n}`, `{date}`, `{title}`.
    #[error(
        "naming template {template:?} contains unknown token {{{token}}}; allowed tokens: {{stem}}, {{n}}, {{date}}, {{title}}"
    )]
    UnknownToken {
        /// Offending template string.
        template: String,
        /// Unrecognized token name.
        token: String,
    },
    /// Naming template contains a character outside `[A-Za-z0-9._ -]`.
    #[error(
        "naming template {template:?} contains disallowed character {character:?}; only [A-Za-z0-9._ -] and {{stem}}, {{n}}, {{date}}, {{title}} are allowed"
    )]
    InvalidCharacter {
        /// Offending template string.
        template: String,
        /// Disallowed character.
        character: char,
    },
}

/// Validates a recipe output naming template against the charset `[A-Za-z0-9._ -]`
/// and token set `{stem}`, `{n}`, `{date}`, `{title}` (`docs/tools.md` §9).
pub fn validate_naming_template(template: &str) -> Result<(), NamingTemplateError> {
    if template.trim().is_empty() {
        return Err(NamingTemplateError::Empty);
    }
    if template.starts_with('.') || template.contains("..") {
        return Err(NamingTemplateError::DisallowedDotSegment {
            template: template.to_owned(),
        });
    }

    let mut chars = template.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '{' => {
                let mut token = String::new();
                let mut closed = false;
                for inner in chars.by_ref() {
                    if inner == '}' {
                        closed = true;
                        break;
                    }
                    if inner == '{' {
                        return Err(NamingTemplateError::UnbalancedBraces {
                            template: template.to_owned(),
                        });
                    }
                    token.push(inner);
                }
                if !closed {
                    return Err(NamingTemplateError::UnbalancedBraces {
                        template: template.to_owned(),
                    });
                }
                if !ALLOWED_NAMING_TOKENS.contains(&token.as_str()) {
                    return Err(NamingTemplateError::UnknownToken {
                        template: template.to_owned(),
                        token,
                    });
                }
            }
            '}' => {
                return Err(NamingTemplateError::UnbalancedBraces {
                    template: template.to_owned(),
                });
            }
            c if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | ' ' | '-') => {}
            other => {
                return Err(NamingTemplateError::InvalidCharacter {
                    template: template.to_owned(),
                    character: other,
                });
            }
        }
    }

    Ok(())
}

/// Error returned when parsing or validating a [`Recipe`].
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum RecipeError {
    /// Recipe JSON syntax or schema error.
    #[error("invalid recipe JSON: {0}")]
    InvalidJson(String),
    /// Recipe `version` is not `"ariad-recipe/1"`.
    #[error("unsupported recipe version {actual:?}; expected {RECIPE_VERSION:?}")]
    InvalidVersion {
        /// Version string found in JSON.
        actual: String,
    },
    /// Recipe `steps` array is empty.
    #[error("recipe must contain at least one step")]
    EmptySteps,
    /// Step `params` is not a JSON object.
    #[error("step {step_index} ({tool}) params must be a JSON object")]
    ParamsNotObject {
        /// 0-based step index.
        step_index: usize,
        /// Tool ID.
        tool: ToolId,
    },
    /// A step predicate uses unsupported syntax or values.
    #[error("step {step_index} has invalid condition {condition:?}: {reason}")]
    InvalidCondition {
        /// 0-based step index.
        step_index: usize,
        /// Original predicate.
        condition: String,
        /// Explanation of the invalid expression.
        reason: String,
    },
    /// Naming template failed validation.
    #[error("{0}")]
    InvalidNamingTemplate(#[from] NamingTemplateError),
    /// Secret parameter rule violated in a recipe step.
    #[error("step {step_index} ({tool}): {source}")]
    InvalidSecret {
        /// 0-based step index.
        step_index: usize,
        /// Tool ID.
        tool: ToolId,
        /// Underlying secret validation error.
        #[source]
        source: SecretValidationError,
    },
    /// Imported recipe contained a forbidden feature such as `watch` or a network-fetch step.
    #[error("imported recipe is not allowed to {reason}")]
    ForbiddenInImportedRecipe {
        /// Explanation of the forbidden feature.
        reason: String,
    },
}

impl Recipe {
    /// Creates a new [`Recipe`] with `version = "ariad-recipe/1"` and the given `steps`.
    #[must_use]
    pub fn new(steps: Vec<Step>) -> Self {
        Self {
            version: RECIPE_VERSION.to_owned(),
            steps,
            naming: None,
            watch: None,
        }
    }

    /// Parses a [`Recipe`] from a JSON string and validates it under `origin`.
    pub fn from_json_str(json: &str, origin: RecipeOrigin) -> Result<Self, RecipeError> {
        let recipe: Self =
            serde_json::from_str(json).map_err(|e| RecipeError::InvalidJson(e.to_string()))?;
        recipe.validate(origin)?;
        Ok(recipe)
    }

    /// Validates version, non-empty steps, naming template, secret rules,
    /// and untrusted/imported recipe restrictions.
    pub fn validate(&self, origin: RecipeOrigin) -> Result<(), RecipeError> {
        if self.version != RECIPE_VERSION {
            return Err(RecipeError::InvalidVersion {
                actual: self.version.clone(),
            });
        }
        if self.steps.is_empty() {
            return Err(RecipeError::EmptySteps);
        }
        if let Some(ref naming) = self.naming {
            validate_naming_template(naming)?;
        }
        if origin == RecipeOrigin::Imported && self.watch.is_some() {
            return Err(RecipeError::ForbiddenInImportedRecipe {
                reason: "configure watch-folder automation ('watch')".to_owned(),
            });
        }

        for (step_index, step) in self.steps.iter().enumerate() {
            if let Some(condition) = step.condition() {
                validate_step_condition(condition).map_err(|reason| {
                    RecipeError::InvalidCondition {
                        step_index,
                        condition: condition.to_owned(),
                        reason,
                    }
                })?;
            }
            if let Step::Tool { tool, params, .. } = step {
                if !params.is_object() {
                    return Err(RecipeError::ParamsNotObject {
                        step_index,
                        tool: tool.clone(),
                    });
                }

                if origin == RecipeOrigin::Imported {
                    if NETWORK_FETCH_TOOLS.contains(&tool.as_str()) {
                        return Err(RecipeError::ForbiddenInImportedRecipe {
                            reason: format!("run network-fetch tool '{}'", tool.as_str()),
                        });
                    }
                    if tool.domain() == "watch" || tool.as_str() == "automation.watch" {
                        return Err(RecipeError::ForbiddenInImportedRecipe {
                            reason: format!("run watch-folder tool '{}'", tool.as_str()),
                        });
                    }
                }

                if let Some(spec) = registry::find(tool.as_str()) {
                    validate_secret_params(spec, params, SecretSource::Recipe).map_err(
                        |source| RecipeError::InvalidSecret {
                            step_index,
                            tool: tool.clone(),
                            source,
                        },
                    )?;
                }
                // Always enforce secret rules on known secret field names and any `$secret` object.
                validate_secret_fields(KNOWN_SECRET_FIELD_NAMES, params, SecretSource::Recipe)
                    .map_err(|source| RecipeError::InvalidSecret {
                        step_index,
                        tool: tool.clone(),
                        source,
                    })?;
            }
        }

        Ok(())
    }
}

/// Generates the pretty-printed `schemas/recipe.v1.json` document for [`Recipe`].
#[must_use]
pub fn generated_recipe_schema() -> String {
    let mut schema = SchemaSettings::draft2020_12()
        .into_generator()
        .into_root_schema_for::<Recipe>();
    let object = schema.as_object_mut().expect("Recipe schema is an object");
    object.insert("$id".to_owned(), json!(RECIPE_SCHEMA_ID));
    object.insert("x-status".to_owned(), json!("draft"));
    if let Some(props) = object.get_mut("properties").and_then(Value::as_object_mut)
        && let Some(version_prop) = props.get_mut("version").and_then(Value::as_object_mut)
    {
        version_prop.insert("const".to_owned(), json!(RECIPE_VERSION));
    }
    let mut contents = serde_json::to_string_pretty(&schema).expect("schema serializes to JSON");
    contents.push('\n');
    contents
}

impl Step {
    /// The predicate for this step, if any.
    #[must_use]
    pub fn condition(&self) -> Option<&str> {
        match self {
            Self::Tool { r#if, .. } | Self::Convert { r#if, .. } => r#if.as_deref(),
        }
    }

    /// Effective error policy (omission means stop).
    #[must_use]
    pub fn error_policy(&self) -> StepErrorPolicy {
        match self {
            Self::Tool { on_error, .. } | Self::Convert { on_error, .. } => {
                on_error.unwrap_or_default()
            }
        }
    }
}

/// Metadata available to a step predicate for its current input.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConditionContext {
    /// Page count, if available for the format and encryption state.
    pub page_count: Option<u32>,
    /// Serialized input length.
    pub size_bytes: u64,
    /// Whether a PDF contains raster pages without a text layer.
    pub needs_ocr: bool,
    /// Whether the input is encrypted.
    pub is_encrypted: bool,
    /// Whether the input contains digital signatures.
    pub has_signatures: bool,
    /// Canonical input format identifier.
    pub format: String,
}

#[derive(Clone, Copy)]
enum Comparison {
    Gt,
    Ge,
    Lt,
    Le,
    Eq,
    Ne,
}

impl Comparison {
    fn compare<T: PartialOrd + PartialEq>(self, left: T, right: T) -> bool {
        match self {
            Self::Gt => left > right,
            Self::Ge => left >= right,
            Self::Lt => left < right,
            Self::Le => left <= right,
            Self::Eq => left == right,
            Self::Ne => left != right,
        }
    }
}

#[derive(Clone, Copy)]
enum IntegerField {
    PageCount,
    SizeBytes,
}

#[derive(Clone, Copy)]
enum BooleanField {
    NeedsOcr,
    IsEncrypted,
    HasSignatures,
}

impl BooleanField {
    fn parse(field: &str) -> Option<Self> {
        match field {
            "needs_ocr" => Some(Self::NeedsOcr),
            "is_encrypted" => Some(Self::IsEncrypted),
            "has_signatures" => Some(Self::HasSignatures),
            _ => None,
        }
    }

    fn value(self, context: &ConditionContext) -> bool {
        match self {
            Self::NeedsOcr => context.needs_ocr,
            Self::IsEncrypted => context.is_encrypted,
            Self::HasSignatures => context.has_signatures,
        }
    }
}

enum Predicate<'a> {
    Integer(IntegerField, Comparison, u64),
    Boolean(BooleanField, Comparison, bool),
    Format(Comparison, &'a str),
}

fn parse_condition(expr: &str) -> Result<Predicate<'_>, String> {
    let expr = expr.trim();
    let bare = expr.strip_prefix('!').unwrap_or(expr).trim();
    if let Some(field) = BooleanField::parse(bare) {
        return Ok(Predicate::Boolean(
            field,
            Comparison::Eq,
            !expr.starts_with('!'),
        ));
    }
    let (field, rest) = expr
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .map(|idx| (&expr[..idx], expr[idx..].trim()))
        .ok_or_else(|| "expected a known field and comparison".to_owned())?;
    let (op, value) = [
        (">=", Comparison::Ge),
        ("<=", Comparison::Le),
        ("==", Comparison::Eq),
        ("!=", Comparison::Ne),
        (">", Comparison::Gt),
        ("<", Comparison::Lt),
    ]
    .into_iter()
    .find_map(|(prefix, op)| rest.strip_prefix(prefix).map(|v| (op, v.trim())))
    .ok_or_else(|| "unsupported comparison operator".to_owned())?;
    let integer_field = match field {
        "page_count" => Some(IntegerField::PageCount),
        "size_bytes" => Some(IntegerField::SizeBytes),
        _ => None,
    };
    if let Some(field) = integer_field {
        if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
            return Err("expected an unsigned integer".to_owned());
        }
        return value
            .parse()
            .map(|v| Predicate::Integer(field, op, v))
            .map_err(|_| "integer exceeds u64".to_owned());
    }
    if !matches!(op, Comparison::Eq | Comparison::Ne) {
        return Err("this field supports only == and !=".to_owned());
    }
    if let Some(field) = BooleanField::parse(field) {
        return match value {
            "true" => Ok(Predicate::Boolean(field, op, true)),
            "false" => Ok(Predicate::Boolean(field, op, false)),
            _ => Err("expected true or false".to_owned()),
        };
    }
    if field == "format" {
        let quote = value.chars().next().unwrap_or(' ');
        if matches!(quote, '\'' | '"') && value.len() >= 2 && value.ends_with(quote) {
            let inner = &value[1..value.len() - 1];
            if !inner.is_empty()
                && inner
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_')
            {
                return Ok(Predicate::Format(op, inner));
            }
        }
        return Err("expected a quoted format identifier".to_owned());
    }
    Err(format!("unknown condition field {field:?}"))
}

/// Checks the supported single-predicate grammar without inspecting an input.
pub fn validate_step_condition(expr: &str) -> Result<(), String> {
    parse_condition(expr).map(|_| ())
}

/// Evaluates a validated predicate. Unknown page counts never match numeric comparisons.
/// Errors use step index 0; recipe validation supplies the actual index before execution.
pub fn eval_step_condition(expr: &str, ctx: &ConditionContext) -> Result<bool, RecipeError> {
    let predicate = parse_condition(expr).map_err(|reason| RecipeError::InvalidCondition {
        step_index: 0,
        condition: expr.to_owned(),
        reason,
    })?;
    Ok(match predicate {
        Predicate::Integer(IntegerField::PageCount, op, value) => ctx
            .page_count
            .is_some_and(|n| op.compare(u64::from(n), value)),
        Predicate::Integer(IntegerField::SizeBytes, op, value) => op.compare(ctx.size_bytes, value),
        Predicate::Boolean(field, op, value) => op.compare(field.value(ctx), value),
        Predicate::Format(op, value) => op.compare(ctx.format.as_str(), value),
    })
}
