//! Per-job secret bag (`SecretBag`), parameter secret resolution, and output scrubbing (`SecretScrubber`).
//!
//! Enforces the `x-ariad-secret` contract (`docs/tools.md` §11):
//! 1. Literal secret strings are rejected when `SecretSource` is `Argv`, `Recipe`, or `Mcp`.
//! 2. `{"$secret": "prompt"}` and `{"$secret": "sec_<id>"}` references are resolved against [`SecretBag`].
//! 3. Every resolved secret value (along with its JSON-escaped and URL-encoded forms) is registered in
//!    [`SecretScrubber`] and replaced with `[REDACTED]` across all warnings, errors, stderr tails, and
//!    structured reports.

use std::{collections::BTreeMap, fmt};

use ariad_core::tools::{
    KNOWN_SECRET_FIELD_NAMES, SecretRef, SecretSource, SecretValidationError, ToolSpec,
    validate_secret_fields, validate_secret_params,
};
use serde_json::{Map, Value};

use crate::convert::ConvertWarning;

/// Redaction marker substituted for any leaked secret string.
pub const REDACTED: &str = "[REDACTED]";

/// Per-job container of resolved secret values keyed by parameter name or secret handle ID.
///
/// [`fmt::Debug`] intentionally redacts all secret values so [`SecretBag`] can never leak
/// sensitive strings in logs or panic messages.
#[derive(Clone, Default, Eq, PartialEq)]
pub struct SecretBag {
    entries: BTreeMap<String, String>,
}

impl fmt::Debug for SecretBag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut map = f.debug_map();
        for key in self.entries.keys() {
            map.entry(key, &REDACTED);
        }
        map.finish()
    }
}

impl SecretBag {
    /// Creates an empty [`SecretBag`].
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserts a secret `value` under `key` (a parameter name such as `"password"` or a handle such as `"sec_01J..."`).
    pub fn insert(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.entries.insert(key.into(), value.into());
    }

    /// Builder-style helper that inserts a secret `value` under `key` and returns `self`.
    #[must_use]
    pub fn with(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.insert(key, value);
        self
    }

    /// Looks up a secret value by `key`.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries.get(key).map(String::as_str)
    }

    /// Returns `true` if `key` exists in this bag.
    #[must_use]
    pub fn contains_key(&self, key: &str) -> bool {
        self.entries.contains_key(key)
    }

    /// Returns `true` if no secrets are stored in this bag.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Returns the number of secret entries stored in this bag.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Iterates over `(key, value)` pairs in deterministic key order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.entries.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    /// Builds a [`SecretScrubber`] pre-populated with every non-empty secret value in this bag.
    #[must_use]
    pub fn scrubber(&self) -> SecretScrubber {
        SecretScrubber::from_bag(self)
    }

    /// Returns a new [`SecretBag`] containing only the entries declared by `spec` (or referenced
    /// by a `{"$secret": ...}` tag on one of `spec`'s secret parameters inside `step_params`).
    #[must_use]
    pub fn filter_for_spec(&self, spec: &ToolSpec, step_params: &Value) -> Self {
        let spec_secrets = collect_spec_secret_fields(spec);
        if spec_secrets.is_empty() {
            return Self::new();
        }
        let referenced = collect_referenced_secret_tags(step_params, &spec_secrets);
        let mut filtered = Self::new();
        for (key, value) in &self.entries {
            if key == "prompt" {
                if referenced.iter().any(|r| r == "prompt") {
                    filtered.insert(key.clone(), value.clone());
                }
            } else if key.starts_with(ariad_core::tools::SECRET_HANDLE_PREFIX) {
                if referenced.iter().any(|r| r == key) {
                    filtered.insert(key.clone(), value.clone());
                }
            } else if spec_secrets.iter().any(|sp| sp.eq_ignore_ascii_case(key)) {
                filtered.insert(key.clone(), value.clone());
            }
        }
        filtered
    }

    /// Validates secret rules on `params` for `source`, resolves any `{"$secret": ...}` references
    /// and declared secret parameters from this bag, and returns the resolved parameter objects
    /// plus a [`SecretScrubber`] covering all secret values.
    pub fn resolve_params(
        &self,
        spec: &ToolSpec,
        params: &Value,
        source: SecretSource,
    ) -> Result<ResolvedSecrets, String> {
        let normalized = match params {
            Value::Null => Value::Object(Map::new()),
            Value::Object(_) => params.clone(),
            _ => return Err("tool params must be a JSON object".to_owned()),
        };

        let spec_secrets = collect_spec_secret_fields(spec);
        let spec_secret_refs: Vec<&str> = spec_secrets.iter().map(String::as_str).collect();

        validate_secret_params(spec, &normalized, source).map_err(|e| e.to_string())?;
        validate_secret_fields(&spec_secret_refs, &normalized, source)
            .map_err(|e| e.to_string())?;
        validate_secret_fields(KNOWN_SECRET_FIELD_NAMES, &normalized, source)
            .map_err(|e| e.to_string())?;

        let mut scrubber = self.scrubber();
        let mut has_secrets = false;

        let mut resolved_value = normalized;
        self.resolve_value_recursive(
            spec,
            &spec_secret_refs,
            None,
            &mut resolved_value,
            source,
            &mut scrubber,
            &mut has_secrets,
        )?;

        let mut schema_map = resolved_value.as_object().cloned().unwrap_or_default();

        // Inject secret parameters supplied via `SecretBag` (e.g. `--secret password=@file`)
        // only when they are declared by this tool's `spec`.
        for (key, secret_val) in &self.entries {
            if key == "prompt" || key.starts_with(ariad_core::tools::SECRET_HANDLE_PREFIX) {
                continue;
            }
            if let Some(&canonical_key) = spec_secret_refs
                .iter()
                .find(|sp| sp.eq_ignore_ascii_case(key))
            {
                scrubber.register(secret_val);
                has_secrets = true;
                if !schema_map.contains_key(canonical_key) {
                    schema_map.insert(canonical_key.to_owned(), Value::String(secret_val.clone()));
                }
            }
        }

        let engine_map = schema_map.clone();

        Ok(ResolvedSecrets {
            schema_params: Value::Object(schema_map),
            engine_params: Value::Object(engine_map),
            scrubber,
            has_secrets,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn resolve_value_recursive(
        &self,
        spec: &ToolSpec,
        spec_secret_fields: &[&str],
        current_field: Option<&str>,
        value: &mut Value,
        source: SecretSource,
        scrubber: &mut SecretScrubber,
        has_secrets: &mut bool,
    ) -> Result<(), String> {
        match value {
            Value::Null => Ok(()),
            Value::Object(map) => {
                if map.keys().any(|k| k.eq_ignore_ascii_case("$secret")) {
                    let field = current_field.unwrap_or("$secret");
                    let Some(&canonical_field) = spec_secret_fields
                        .iter()
                        .find(|sp| sp.eq_ignore_ascii_case(field))
                    else {
                        return Err(format!(
                            "parameter '{field}' is not declared in secret_params for tool '{}'",
                            spec.id
                        ));
                    };
                    let secret_ref =
                        SecretRef::from_json_value(canonical_field, &Value::Object(map.clone()))
                            .map_err(|e| e.to_string())?;
                    let resolved =
                        self.lookup_ref(canonical_field, &secret_ref)
                            .ok_or_else(|| {
                                format!("missing secret value for parameter '{canonical_field}'")
                            })?;
                    scrubber.register(resolved);
                    *has_secrets = true;
                    *value = Value::String(resolved.to_owned());
                    return Ok(());
                }

                let keys: Vec<String> = map.keys().cloned().collect();
                for key in keys {
                    let Some(mut child) = map.remove(&key) else {
                        continue;
                    };
                    let matched_canonical = spec_secret_fields
                        .iter()
                        .copied()
                        .find(|sp| sp.eq_ignore_ascii_case(&key));
                    let field_ctx = matched_canonical.unwrap_or(key.as_str());
                    self.resolve_value_recursive(
                        spec,
                        spec_secret_fields,
                        Some(field_ctx),
                        &mut child,
                        source,
                        scrubber,
                        has_secrets,
                    )?;
                    let insert_key = matched_canonical.map_or(key, str::to_owned);
                    map.insert(insert_key, child);
                }
                Ok(())
            }
            Value::Array(items) => {
                for item in items {
                    self.resolve_value_recursive(
                        spec,
                        spec_secret_fields,
                        current_field,
                        item,
                        source,
                        scrubber,
                        has_secrets,
                    )?;
                }
                Ok(())
            }
            Value::String(s) => {
                if source == SecretSource::Resolved
                    && let Some(field) = current_field
                    && (spec_secret_fields
                        .iter()
                        .any(|sp| sp.eq_ignore_ascii_case(field))
                        || KNOWN_SECRET_FIELD_NAMES
                            .iter()
                            .any(|sp| sp.eq_ignore_ascii_case(field)))
                {
                    scrubber.register(s);
                    *has_secrets = true;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn get_case_insensitive(&self, key: &str) -> Option<&str> {
        self.entries.get(key).map(String::as_str).or_else(|| {
            self.entries
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(key))
                .map(|(_, v)| v.as_str())
        })
    }

    fn lookup_ref(&self, field: &str, secret_ref: &SecretRef) -> Option<&str> {
        match secret_ref {
            SecretRef::Prompt => self
                .get_case_insensitive(field)
                .or_else(|| self.get("prompt")),
            SecretRef::Handle(handle_id) => self
                .get(handle_id)
                .or_else(|| self.get_case_insensitive(field)),
        }
    }
}

fn collect_spec_secret_fields(spec: &ToolSpec) -> Vec<String> {
    let mut fields: Vec<String> = spec.secret_params.iter().map(|s| (*s).to_owned()).collect();
    let schema = spec.resolved_params_schema();
    if let Some(props) = schema
        .as_object()
        .and_then(|o| o.get("properties"))
        .and_then(Value::as_object)
    {
        for (prop_name, prop_schema) in props {
            if prop_schema
                .as_object()
                .and_then(|o| o.get(ariad_core::tools::SECRET_SCHEMA_EXTENSION))
                .and_then(Value::as_bool)
                == Some(true)
                && !fields.iter().any(|f| f.eq_ignore_ascii_case(prop_name))
            {
                fields.push(prop_name.clone());
            }
        }
    }
    fields
}

fn collect_referenced_secret_tags(value: &Value, spec_secrets: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    collect_referenced_secret_tags_inner(value, None, spec_secrets, &mut out);
    out
}

fn collect_referenced_secret_tags_inner(
    value: &Value,
    current_field: Option<&str>,
    spec_secrets: &[String],
    out: &mut Vec<String>,
) {
    match value {
        Value::Object(map) => {
            if let Some((_, tag_val)) = map.iter().find(|(k, _)| k.eq_ignore_ascii_case("$secret"))
            {
                if map.len() == 1
                    && let Some(field) = current_field
                    && spec_secrets.iter().any(|sp| sp.eq_ignore_ascii_case(field))
                    && let Some(tag_str) = tag_val.as_str()
                {
                    out.push(tag_str.to_owned());
                }
                return;
            }
            for (k, v) in map {
                collect_referenced_secret_tags_inner(v, Some(k.as_str()), spec_secrets, out);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_referenced_secret_tags_inner(item, current_field, spec_secrets, out);
            }
        }
        _ => {}
    }
}

/// Output of [`SecretBag::resolve_params`].
///
/// [`fmt::Debug`] intentionally redacts `schema_params` and `engine_params` so resolved secret
/// values can never leak through debug formatting.
#[derive(Clone)]
pub struct ResolvedSecrets {
    /// Parameters validated against the tool's JSON Schema.
    pub schema_params: Value,
    /// Parameters delivered to out-of-process engines via `<work_dir>/params.json`.
    pub engine_params: Value,
    /// Scrubber registered with every resolved secret string.
    pub scrubber: SecretScrubber,
    /// Whether any secret was present in the request or [`SecretBag`].
    pub has_secrets: bool,
}

impl fmt::Debug for ResolvedSecrets {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ResolvedSecrets")
            .field("schema_params", &REDACTED)
            .field("engine_params", &REDACTED)
            .field("scrubber", &self.scrubber)
            .field("has_secrets", &self.has_secrets)
            .finish()
    }
}

/// Scrubs secret values (and their JSON-escaped and URL-encoded forms) from strings, warnings,
/// and JSON payloads (`docs/tools.md` §11).
#[derive(Clone, Default, Eq, PartialEq)]
pub struct SecretScrubber {
    patterns: Vec<String>,
}

impl fmt::Debug for SecretScrubber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SecretScrubber")
            .field("pattern_count", &self.patterns.len())
            .finish()
    }
}

impl SecretScrubber {
    /// Creates an empty [`SecretScrubber`].
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a [`SecretScrubber`] populated from all non-empty values in `bag`.
    #[must_use]
    pub fn from_bag(bag: &SecretBag) -> Self {
        let mut scrubber = Self::new();
        for (_, value) in bag.iter() {
            scrubber.register(value);
        }
        scrubber
    }

    /// Returns `true` if no secret patterns are registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.patterns.is_empty()
    }

    /// Registers a secret string along with its JSON-escaped and URL-encoded forms.
    ///
    /// Empty or whitespace-only strings are ignored to avoid corrupting output formatting.
    pub fn register(&mut self, secret: &str) {
        if secret.is_empty() {
            return;
        }
        self.push_pattern(secret.to_owned());

        // JSON-escaped interior (without surrounding quotes).
        if let Ok(json_quoted) = serde_json::to_string(secret)
            && json_quoted.len() >= 2
        {
            let escaped = &json_quoted[1..json_quoted.len() - 1];
            if !escaped.is_empty() {
                self.push_pattern(escaped.to_owned());
            }
        }

        // URL-encoded forms (uppercase and lowercase hex).
        let url_upper = url_encode(secret, true);
        if !url_upper.is_empty() {
            self.push_pattern(url_upper);
        }
        let url_lower = url_encode(secret, false);
        if !url_lower.is_empty() {
            self.push_pattern(url_lower);
        }

        // Longest patterns first so escaped or compound forms match before substrings.
        self.patterns
            .sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
        self.patterns.dedup();
    }

    fn push_pattern(&mut self, pattern: String) {
        if !pattern.is_empty() {
            self.patterns.push(pattern);
        }
    }

    /// Replaces every occurrence of any registered secret pattern in `input` with `[REDACTED]`.
    #[must_use]
    pub fn scrub(&self, input: &str) -> String {
        if self.patterns.is_empty() || input.is_empty() {
            return input.to_owned();
        }
        let mut out = input.to_owned();
        for pattern in &self.patterns {
            if out.contains(pattern.as_str()) {
                out = out.replace(pattern.as_str(), REDACTED);
            }
        }
        out
    }

    /// Returns a copy of `warning` with its `code` and `message` scrubbed.
    #[must_use]
    pub fn scrub_warning(&self, warning: &ConvertWarning) -> ConvertWarning {
        ConvertWarning {
            code: self.scrub(&warning.code),
            message: self.scrub(&warning.message),
        }
    }

    /// Recursively scrubs all string values in a JSON [`Value`].
    #[must_use]
    pub fn scrub_value(&self, value: &Value) -> Value {
        if self.patterns.is_empty() {
            return value.clone();
        }
        match value {
            Value::String(s) => Value::String(self.scrub(s)),
            Value::Array(items) => {
                Value::Array(items.iter().map(|v| self.scrub_value(v)).collect())
            }
            Value::Object(map) => {
                let mut out = Map::with_capacity(map.len());
                for (k, v) in map {
                    out.insert(self.scrub(k), self.scrub_value(v));
                }
                Value::Object(out)
            }
            other => other.clone(),
        }
    }
}

fn url_encode(input: &str, uppercase_hex: bool) -> String {
    let mut out = String::with_capacity(input.len());
    for &byte in input.as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(char::from(byte));
        } else if uppercase_hex {
            out.push_str(&format!("%{byte:02X}"));
        } else {
            out.push_str(&format!("%{byte:02x}"));
        }
    }
    out
}

/// Convenience re-export of [`SecretValidationError`].
pub type SecretError = SecretValidationError;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn scrubber_redacts_raw_json_escaped_and_url_encoded_secrets() {
        let secret = "s3cr3t\"p@ss\\w0rd!";
        let bag = SecretBag::new().with("password", secret);
        let scrubber = bag.scrubber();

        assert!(format!("{bag:?}").contains(REDACTED));
        assert!(!format!("{bag:?}").contains("s3cr3t"));

        let raw_msg = format!("failed with password={secret} in engine");
        assert_eq!(
            scrubber.scrub(&raw_msg),
            "failed with password=[REDACTED] in engine"
        );

        let json_msg = serde_json::to_string(&json!({ "err": secret })).unwrap();
        let scrubbed_json = scrubber.scrub(&json_msg);
        assert!(!scrubbed_json.contains("s3cr3t"));
        assert!(scrubbed_json.contains(REDACTED));

        let url_msg = format!("https://example.test/?pw={}", url_encode(secret, true));
        let scrubbed_url = scrubber.scrub(&url_msg);
        assert!(!scrubbed_url.contains("s3cr3t"));
        assert!(scrubbed_url.contains(REDACTED));
    }

    #[test]
    fn resolved_secrets_debug_redacts_and_filters_to_declared_spec_secrets() {
        use ariad_core::tools::{InputArity, Tier, ToolGroup, ToolRuntime, schema_with_secrets};
        use schemars::JsonSchema;
        use serde::{Deserialize, Serialize};

        #[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
        #[serde(deny_unknown_fields)]
        struct TestSecretParams {
            #[serde(default)]
            password: Option<String>,
        }

        let spec = ToolSpec {
            id: "pdf.decrypt",
            version: "1",
            group: ToolGroup::Security,
            summary: "Decrypt a PDF.",
            inputs: InputArity::One,
            input_formats: &["pdf"],
            output_format: "pdf",
            tier: Tier::Free,
            runtimes: &[ToolRuntime::Local],
            engine: "qpdf",
            params_schema: || schema_with_secrets::<TestSecretParams>(&["password"]),
            secret_params: &["password"],
        };
        let bag = SecretBag::new()
            .with("password", "declared-secret-value-111")
            .with("unrelated_step_secret", "other-step-secret-222");

        let resolved = bag
            .resolve_params(&spec, &json!({}), SecretSource::Recipe)
            .expect("resolve params");
        let debug_str = format!("{resolved:?}");
        assert!(debug_str.contains(REDACTED));
        assert!(!debug_str.contains("declared-secret-value-111"));
        assert!(!debug_str.contains("other-step-secret-222"));

        assert_eq!(
            resolved.engine_params["password"],
            json!("declared-secret-value-111")
        );
        assert!(
            resolved
                .engine_params
                .get("unrelated_step_secret")
                .is_none(),
            "unrelated secret must not be injected into engine_params"
        );
    }
}
