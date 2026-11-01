//! Secrets contract (`x-ariad-secret`, `docs/tools.md` §11).
//!
//! Parameters listed in [`ToolSpec::secret_params`](crate::tools::ToolSpec::secret_params)
//! receive `"x-ariad-secret": true` in their JSON Schema and must never be supplied as literal
//! strings on CLI `argv`, in recipes, or in MCP arguments.

use schemars::{JsonSchema, Schema, generate::SchemaSettings};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::tools::registry::ToolSpec;

/// JSON Schema extension key marking a tool parameter as a sensitive secret.
pub const SECRET_SCHEMA_EXTENSION: &str = "x-ariad-secret";

/// Prefix required for short-lived MCP / HTTP secret handles (`{"$secret": "sec_<id>"}`).
pub const SECRET_HANDLE_PREFIX: &str = "sec_";

/// Known secret parameter field names across the AriadShift tool suite.
///
/// Used as a defence-in-depth fallback when validating recipes that reference tools not yet
/// present in the active registry (`pdf.decrypt`, `pdf.encrypt`, `pdf.permissions`, `pdf.sign`).
pub const KNOWN_SECRET_FIELD_NAMES: &[&str] = &[
    "password",
    "user_password",
    "owner_password",
    "passphrase",
    "key_password",
    "p12_password",
    "pfx_password",
    "pin",
    "token_pin",
    "secret",
    "api_key",
];

/// Wire representation of a `$secret` reference object.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct RawSecretRef {
    #[serde(rename = "$secret")]
    secret: String,
}

/// Out-of-band reference to a secret parameter value (`{"$secret": "prompt"}` or `{"$secret": "sec_<id>"}`).
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(try_from = "RawSecretRef", into = "RawSecretRef")]
pub enum SecretRef {
    /// Prompt the user interactively at runtime (`{"$secret": "prompt"}`).
    Prompt,
    /// Reference a short-lived secret handle (`{"$secret": "<handle_id>"}`, e.g. `"sec_01J..."`).
    Handle(String),
}

impl SecretRef {
    /// Creates a [`SecretRef::Prompt`] reference (`{"$secret": "prompt"}`).
    #[must_use]
    pub const fn prompt() -> Self {
        Self::Prompt
    }

    /// Creates a validated [`SecretRef::Handle`] reference.
    pub fn handle(id: impl Into<String>) -> Result<Self, SecretValidationError> {
        let id = id.into();
        if !is_valid_handle_id(&id) {
            return Err(SecretValidationError::InvalidSecretRef {
                field: "$secret".to_owned(),
                reason: format!(
                    "secret handle must start with '{SECRET_HANDLE_PREFIX}' and contain only ASCII alphanumeric, '_', or '-' characters"
                ),
            });
        }
        Ok(Self::Handle(id))
    }

    /// Returns the wire string inside `{"$secret": "<value>"}`.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Prompt => "prompt",
            Self::Handle(id) => id.as_str(),
        }
    }

    /// Attempts to parse a JSON [`Value`] as a [`SecretRef`].
    pub fn from_json_value(field: &str, value: &Value) -> Result<Self, SecretValidationError> {
        let Some(obj) = value.as_object() else {
            return Err(SecretValidationError::LiteralSecretNotAllowed {
                field: field.to_owned(),
                channel: SecretSource::Recipe,
            });
        };
        let Some((_, secret_val)) = obj.iter().find(|(k, _)| k.eq_ignore_ascii_case("$secret"))
        else {
            return Err(SecretValidationError::InvalidSecretRef {
                field: field.to_owned(),
                reason: "expected an object of the form {\"$secret\": \"prompt\"} or {\"$secret\": \"sec_<id>\"}".to_owned(),
            });
        };
        if obj.len() != 1 {
            return Err(SecretValidationError::InvalidSecretRef {
                field: field.to_owned(),
                reason: "expected an object of the form {\"$secret\": \"prompt\"} or {\"$secret\": \"sec_<id>\"}".to_owned(),
            });
        };
        let Some(secret_str) = secret_val.as_str() else {
            return Err(SecretValidationError::InvalidSecretRef {
                field: field.to_owned(),
                reason: "\"$secret\" value must be a string".to_owned(),
            });
        };
        Self::parse_secret_tag(field, secret_str)
    }

    fn parse_secret_tag(field: &str, secret: &str) -> Result<Self, SecretValidationError> {
        if secret == "prompt" {
            return Ok(Self::Prompt);
        }
        if is_valid_handle_id(secret) {
            return Ok(Self::Handle(secret.to_owned()));
        }
        Err(SecretValidationError::InvalidSecretRef {
            field: field.to_owned(),
            reason: format!(
                "unsupported \"$secret\" target; expected \"prompt\" or a \"{SECRET_HANDLE_PREFIX}<id>\" handle"
            ),
        })
    }
}

fn is_valid_handle_id(id: &str) -> bool {
    if let Some(rest) = id.strip_prefix(SECRET_HANDLE_PREFIX) {
        !rest.is_empty()
            && rest
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    } else {
        false
    }
}

impl TryFrom<RawSecretRef> for SecretRef {
    type Error = SecretValidationError;

    fn try_from(raw: RawSecretRef) -> Result<Self, Self::Error> {
        Self::parse_secret_tag("$secret", &raw.secret)
    }
}

impl From<SecretRef> for RawSecretRef {
    fn from(secret_ref: SecretRef) -> Self {
        Self {
            secret: secret_ref.as_str().to_owned(),
        }
    }
}

/// Surface or channel from which tool parameters originate when validating secret rules.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SecretSource {
    /// Inline CLI `--param` or `--params-json` arguments.
    /// Literal secrets and handles are rejected; callers must use `--secret <name>=@<file>` or `--secret <name>=-`
    /// (or `{"$secret": "prompt"}` for an interactive terminal prompt).
    Argv,
    /// A recipe step (`ariad-recipe/1`).
    /// Literal strings, `@file` paths, and handles are rejected; only `{"$secret": "prompt"}` is allowed.
    Recipe,
    /// Local or remote MCP tool call arguments (`run_doc_tool`).
    /// Literal secrets and `{"$secret": "prompt"}` are rejected; only a short-lived handle `{"$secret": "sec_<id>"}` is allowed.
    Mcp,
    /// Host-internal resolved `params_file` or in-memory `SecretBag`.
    Resolved,
}

impl SecretSource {
    /// Stable snake_case name of this source.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Argv => "argv",
            Self::Recipe => "recipe",
            Self::Mcp => "mcp",
            Self::Resolved => "resolved",
        }
    }
}

impl std::fmt::Display for SecretSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Error returned when a parameter marked `x-ariad-secret` violates the secrets contract.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum SecretValidationError {
    /// A literal value (string, `@file` path, etc.) was supplied for an `x-ariad-secret` parameter.
    #[error("{}", format_literal_error(.field, *.channel))]
    LiteralSecretNotAllowed {
        /// Secret parameter name.
        field: String,
        /// Originating input channel.
        channel: SecretSource,
    },
    /// A [`SecretRef`] variant is not permitted on the given [`SecretSource`].
    #[error(
        "parameter '{field}' is marked x-ariad-secret and cannot use {} in {channel} input",
        format_secret_ref_kind(.reference)
    )]
    UnsupportedChannel {
        /// Secret parameter name.
        field: String,
        /// The supplied secret reference.
        reference: SecretRef,
        /// Originating input channel.
        channel: SecretSource,
    },
    /// Malformed `{"$secret": ...}` object.
    #[error("invalid secret reference for parameter '{field}': {reason}")]
    InvalidSecretRef {
        /// Secret parameter name.
        field: String,
        /// Validation failure explanation.
        reason: String,
    },
}

const fn format_secret_ref_kind(reference: &SecretRef) -> &'static str {
    match reference {
        SecretRef::Prompt => "{\"$secret\": \"prompt\"}",
        SecretRef::Handle(_) => "a secret handle",
    }
}

fn format_literal_error(field: &str, source: SecretSource) -> String {
    match source {
        SecretSource::Argv => format!(
            "parameter '{field}' is marked x-ariad-secret and cannot be passed via --param; use --secret {field}=@<file> or --secret {field}=-"
        ),
        SecretSource::Recipe => format!(
            "parameter '{field}' is marked x-ariad-secret and cannot contain a literal value in a recipe; use {{\"$secret\": \"prompt\"}}"
        ),
        SecretSource::Mcp => format!(
            "parameter '{field}' is marked x-ariad-secret and cannot be passed as a literal in MCP params; use a secret handle {{\"$secret\": \"sec_<id>\"}}"
        ),
        SecretSource::Resolved => format!("parameter '{field}' failed secret validation"),
    }
}

/// Annotates every property named in `secret_params` with `"x-ariad-secret": true` in `schema`.
pub fn annotate_secret_params(schema: &mut Schema, secret_params: &[&str]) {
    if secret_params.is_empty() {
        return;
    }
    let Some(root_obj) = schema.as_object_mut() else {
        return;
    };
    let Some(props) = root_obj
        .get_mut("properties")
        .and_then(Value::as_object_mut)
    else {
        return;
    };
    for &secret_name in secret_params {
        if let Some(prop_schema) = props.get_mut(secret_name).and_then(Value::as_object_mut) {
            prop_schema.insert(SECRET_SCHEMA_EXTENSION.to_owned(), json!(true));
        }
    }
}

/// Generates a Draft 2020-12 JSON Schema for `P` and marks every field in `secret_params`
/// with `"x-ariad-secret": true`.
#[must_use]
pub fn schema_with_secrets<P: JsonSchema>(secret_params: &[&str]) -> Schema {
    let mut schema = SchemaSettings::draft2020_12()
        .into_generator()
        .into_root_schema_for::<P>();
    annotate_secret_params(&mut schema, secret_params);
    schema
}

/// Validates secret fields on `params` for a given [`ToolSpec`] and [`SecretSource`].
pub fn validate_secret_params(
    spec: &ToolSpec,
    params: &Value,
    source: SecretSource,
) -> Result<(), SecretValidationError> {
    validate_secret_fields(spec.secret_params, params, source)
}

/// Validates that every field listed in `secret_params` (if present and non-null in `params`,
/// including inside nested objects and arrays, matched case-insensitively) obeys the channel
/// rules for `source`.
pub fn validate_secret_fields(
    secret_params: &[&str],
    params: &Value,
    source: SecretSource,
) -> Result<(), SecretValidationError> {
    if source == SecretSource::Resolved {
        return Ok(());
    }
    validate_secret_value_recursive(secret_params, None, params, source)
}

fn is_secret_tag_object(obj: &serde_json::Map<String, Value>) -> bool {
    obj.keys().any(|k| k.eq_ignore_ascii_case("$secret"))
}

fn validate_secret_value_recursive(
    secret_params: &[&str],
    current_field: Option<&str>,
    value: &Value,
    source: SecretSource,
) -> Result<(), SecretValidationError> {
    match value {
        Value::Null => Ok(()),
        Value::Object(obj) => {
            if is_secret_tag_object(obj) {
                let field_name = current_field.unwrap_or("$secret");
                validate_single_secret_value(field_name, value, source)?;
                return Ok(());
            }
            if let Some(secret_field) = current_field
                && let Some(&canonical) = secret_params
                    .iter()
                    .find(|sp| sp.eq_ignore_ascii_case(secret_field))
            {
                for (k, v) in obj {
                    validate_secret_value_recursive(secret_params, Some(k.as_str()), v, source)?;
                }
                return Err(SecretValidationError::LiteralSecretNotAllowed {
                    field: canonical.to_owned(),
                    channel: source,
                });
            }
            for (field, val) in obj {
                if val.is_null() {
                    continue;
                }
                if let Some(&canonical) = secret_params
                    .iter()
                    .find(|sp| sp.eq_ignore_ascii_case(field))
                {
                    if val.as_object().is_some_and(is_secret_tag_object) {
                        validate_single_secret_value(canonical, val, source)?;
                    } else {
                        if val.is_object() || val.is_array() {
                            validate_secret_value_recursive(
                                secret_params,
                                Some(canonical),
                                val,
                                source,
                            )?;
                        }
                        return Err(SecretValidationError::LiteralSecretNotAllowed {
                            field: canonical.to_owned(),
                            channel: source,
                        });
                    }
                } else if val.as_object().is_some_and(is_secret_tag_object) {
                    validate_single_secret_value(field, val, source)?;
                } else {
                    validate_secret_value_recursive(
                        secret_params,
                        Some(field.as_str()),
                        val,
                        source,
                    )?;
                }
            }
            Ok(())
        }
        Value::Array(items) => {
            for item in items {
                if item.is_null() {
                    continue;
                }
                if let Some(secret_field) = current_field
                    && let Some(&canonical) = secret_params
                        .iter()
                        .find(|sp| sp.eq_ignore_ascii_case(secret_field))
                {
                    if item.as_object().is_some_and(is_secret_tag_object) {
                        validate_single_secret_value(canonical, item, source)?;
                    } else {
                        validate_secret_value_recursive(
                            secret_params,
                            Some(canonical),
                            item,
                            source,
                        )?;
                        return Err(SecretValidationError::LiteralSecretNotAllowed {
                            field: canonical.to_owned(),
                            channel: source,
                        });
                    }
                } else {
                    validate_secret_value_recursive(secret_params, current_field, item, source)?;
                }
            }
            Ok(())
        }
        _ => {
            if let Some(secret_field) = current_field
                && let Some(&canonical) = secret_params
                    .iter()
                    .find(|sp| sp.eq_ignore_ascii_case(secret_field))
            {
                return Err(SecretValidationError::LiteralSecretNotAllowed {
                    field: canonical.to_owned(),
                    channel: source,
                });
            }
            Ok(())
        }
    }
}

/// Validates a single secret parameter value against `source`.
pub fn validate_single_secret_value(
    field: &str,
    value: &Value,
    source: SecretSource,
) -> Result<SecretRef, SecretValidationError> {
    let Some(obj) = value.as_object() else {
        return Err(SecretValidationError::LiteralSecretNotAllowed {
            field: field.to_owned(),
            channel: source,
        });
    };
    if !is_secret_tag_object(obj) {
        return Err(SecretValidationError::LiteralSecretNotAllowed {
            field: field.to_owned(),
            channel: source,
        });
    };
    let secret_ref = SecretRef::from_json_value(field, value)?;
    match (source, &secret_ref) {
        (SecretSource::Recipe, SecretRef::Prompt) => Ok(secret_ref),
        (SecretSource::Recipe, SecretRef::Handle(_)) => {
            Err(SecretValidationError::UnsupportedChannel {
                field: field.to_owned(),
                reference: secret_ref,
                channel: source,
            })
        }
        (SecretSource::Argv, SecretRef::Prompt) => Ok(secret_ref),
        (SecretSource::Argv, SecretRef::Handle(_)) => {
            Err(SecretValidationError::UnsupportedChannel {
                field: field.to_owned(),
                reference: secret_ref,
                channel: source,
            })
        }
        (SecretSource::Mcp, SecretRef::Handle(_)) => Ok(secret_ref),
        (SecretSource::Mcp, SecretRef::Prompt) => Err(SecretValidationError::UnsupportedChannel {
            field: field.to_owned(),
            reference: secret_ref,
            channel: source,
        }),
        (SecretSource::Resolved, _) => Ok(secret_ref),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Serialize, Deserialize, JsonSchema)]
    #[serde(deny_unknown_fields)]
    struct DummyDecryptParams {
        password: String,
    }

    #[test]
    fn annotates_secret_fields_with_x_ariad_secret() {
        let schema = schema_with_secrets::<DummyDecryptParams>(&["password"]);
        let val = serde_json::to_value(&schema).unwrap();
        assert_eq!(
            val["properties"]["password"][SECRET_SCHEMA_EXTENSION],
            json!(true)
        );
    }

    #[test]
    fn rejects_literal_values_across_recipe_argv_and_mcp() {
        let literal = json!({ "password": "hunter2" });
        for source in [SecretSource::Recipe, SecretSource::Argv, SecretSource::Mcp] {
            let err = validate_secret_fields(&["password"], &literal, source).unwrap_err();
            assert!(matches!(
                err,
                SecretValidationError::LiteralSecretNotAllowed { .. }
            ));
        }

        let prompt = json!({ "password": { "$secret": "prompt" } });
        assert!(validate_secret_fields(&["password"], &prompt, SecretSource::Recipe).is_ok());
        assert!(validate_secret_fields(&["password"], &prompt, SecretSource::Argv).is_ok());
        assert!(validate_secret_fields(&["password"], &prompt, SecretSource::Mcp).is_err());

        let handle = json!({ "password": { "$secret": "sec_01JABCDEF" } });
        assert!(validate_secret_fields(&["password"], &handle, SecretSource::Mcp).is_ok());
        assert!(validate_secret_fields(&["password"], &handle, SecretSource::Recipe).is_err());
        assert!(validate_secret_fields(&["password"], &handle, SecretSource::Argv).is_err());
    }

    #[test]
    fn nested_and_case_insensitive_secrets_and_bad_secret_tags_never_echo_value() {
        let secret_val = "s3cr3t-never-echo-987654";
        let nested_cases = [
            json!({ "nested": { "password": secret_val } }),
            json!({ "nested": { "PASSWORD": secret_val } }),
            json!({ "items": [{ "password": secret_val }] }),
            json!({ "password": [secret_val] }),
            json!({ "password": { "$secret": secret_val } }),
            json!({ "nested": { "password": { "$secret": secret_val } } }),
            json!({ "items": [{ "$secret": secret_val }] }),
        ];

        for source in [SecretSource::Recipe, SecretSource::Argv, SecretSource::Mcp] {
            for payload in &nested_cases {
                let err = validate_secret_fields(&["password"], payload, source).unwrap_err();
                let msg = err.to_string();
                assert!(
                    !msg.contains(secret_val),
                    "secret value leaked in error message for {payload} on {source}: {msg}"
                );
            }
        }

        let handle_err = SecretRef::handle(secret_val).unwrap_err().to_string();
        assert!(
            !handle_err.contains(secret_val),
            "handle validation error must not echo caller input: {handle_err}"
        );
    }
}
