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
pub const ALLOWED_NAMING_TOKENS: &[&str] = &["stem", "n", "date"];

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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct RawToolStep {
    /// Tool identifier to execute.
    tool: ToolId,
    /// Tool parameters object (defaults to `{}`).
    #[serde(default)]
    params: Map<String, Value>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct RawConvertStep {
    /// Standard format conversion step.
    convert: ConvertTarget,
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
    },
    /// Runs the standard conversion pipeline to `to` with optional `profile`.
    Convert {
        /// Target document format.
        to: Format,
        /// Optional conversion profile.
        profile: Option<Profile>,
    },
}

impl TryFrom<RawStep> for Step {
    type Error = String;

    fn try_from(raw: RawStep) -> Result<Self, Self::Error> {
        match raw {
            RawStep::Tool(RawToolStep { tool, params }) => Ok(Self::Tool {
                tool,
                params: Value::Object(params),
            }),
            RawStep::Convert(RawConvertStep {
                convert: ConvertTarget { to, profile },
            }) => Ok(Self::Convert { to, profile }),
        }
    }
}

impl From<Step> for RawStep {
    fn from(step: Step) -> Self {
        match step {
            Step::Tool { tool, params } => {
                let map = match params {
                    Value::Object(map) => map,
                    _ => Map::new(),
                };
                Self::Tool(RawToolStep { tool, params: map })
            }
            Step::Convert { to, profile } => Self::Convert(RawConvertStep {
                convert: ConvertTarget { to, profile },
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
    /// Optional output naming template (`[A-Za-z0-9._ -]` plus `{stem}`, `{n}`, `{date}`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub naming: Option<String>,
    /// Optional watch-folder path (forbidden when `origin` is [`RecipeOrigin::Imported`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub watch: Option<String>,
}

/// Error returned when validating a recipe output naming template (`docs/tools.md` §9).
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum NamingTemplateError {
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
    /// Naming template contains a placeholder token outside `{stem}`, `{n}`, `{date}`.
    #[error(
        "naming template {template:?} contains unknown token {{{token}}}; allowed tokens: {{stem}}, {{n}}, {{date}}"
    )]
    UnknownToken {
        /// Offending template string.
        template: String,
        /// Unrecognized token name.
        token: String,
    },
    /// Naming template contains a character outside `[A-Za-z0-9._ -]`.
    #[error(
        "naming template {template:?} contains disallowed character {character:?}; only [A-Za-z0-9._ -] and {{stem}}, {{n}}, {{date}} are allowed"
    )]
    InvalidCharacter {
        /// Offending template string.
        template: String,
        /// Disallowed character.
        character: char,
    },
}

/// Validates a recipe output naming template against the charset `[A-Za-z0-9._ -]`
/// and token set `{stem}`, `{n}`, `{date}` (`docs/tools.md` §9).
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
            if let Step::Tool { tool, params } = step {
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
