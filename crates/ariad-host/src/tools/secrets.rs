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

        validate_secret_params(spec, &normalized, source).map_err(|e| e.to_string())?;
        validate_secret_fields(KNOWN_SECRET_FIELD_NAMES, &normalized, source)
            .map_err(|e| e.to_string())?;

        let mut scrubber = self.scrubber();
        let mut has_secrets = !self.entries.is_empty();

        let schema = spec.resolved_params_schema();
        let schema_props = schema
            .as_object()
            .and_then(|o| o.get("properties"))
            .and_then(Value::as_object);

        let mut schema_map = normalized.as_object().cloned().unwrap_or_default();

        // Resolve any `{"$secret": ...}` references inside `schema_map`.
        let keys: Vec<String> = schema_map.keys().cloned().collect();
        for field in keys {
            let Some(val) = schema_map.get(&field) else {
                continue;
            };
            if let Some(inner) = val.as_object()
                && inner.contains_key("$secret")
            {
                let secret_ref =
                    SecretRef::from_json_value(&field, val).map_err(|e| e.to_string())?;
                let resolved = self.lookup_ref(&field, &secret_ref).ok_or_else(|| {
                    format!(
                        "missing secret value for parameter '{field}' ({})",
                        secret_ref.as_str()
                    )
                })?;
                scrubber.register(resolved);
                has_secrets = true;
                schema_map.insert(field, Value::String(resolved.to_owned()));
            } else if source == SecretSource::Resolved
                && (spec.secret_params.contains(&field.as_str())
                    || KNOWN_SECRET_FIELD_NAMES
                        .iter()
                        .any(|s| s.eq_ignore_ascii_case(&field)))
                && let Some(secret_str) = val.as_str()
            {
                scrubber.register(secret_str);
                has_secrets = true;
            }
        }

        // Inject secret parameters supplied via `SecretBag` (e.g. `--secret password=@file`)
        // that belong to `spec.secret_params` or the tool's schema properties.
        for (key, secret_val) in &self.entries {
            if key == "prompt" || key.starts_with(ariad_core::tools::SECRET_HANDLE_PREFIX) {
                continue;
            }
            let in_spec = spec.secret_params.contains(&key.as_str())
                || schema_props.is_some_and(|props| props.contains_key(key));
            if in_spec && !schema_map.contains_key(key) {
                schema_map.insert(key.clone(), Value::String(secret_val.clone()));
            }
        }

        // Build `engine_map`: includes `schema_map` plus any additional non-handle keys in `SecretBag`
        // so out-of-process `ariad-engine/2` engines receive all bag entries in `<work_dir>/params.json`.
        let mut engine_map = schema_map.clone();
        for (key, secret_val) in &self.entries {
            if key == "prompt" || key.starts_with(ariad_core::tools::SECRET_HANDLE_PREFIX) {
                continue;
            }
            engine_map
                .entry(key.clone())
                .or_insert_with(|| Value::String(secret_val.clone()));
        }

        Ok(ResolvedSecrets {
            schema_params: Value::Object(schema_map),
            engine_params: Value::Object(engine_map),
            scrubber,
            has_secrets,
        })
    }

    fn lookup_ref(&self, field: &str, secret_ref: &SecretRef) -> Option<&str> {
        match secret_ref {
            SecretRef::Prompt => self.get(field).or_else(|| self.get("prompt")),
            SecretRef::Handle(handle_id) => self.get(handle_id).or_else(|| self.get(field)),
        }
    }
}

/// Output of [`SecretBag::resolve_params`].
#[derive(Clone, Debug)]
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
}
