//! Tool identifier (`ToolId`), specification (`ToolSpec`), and global tool registry (`LazyLock`).

use std::{borrow::Borrow, collections::HashSet, ops::Deref, sync::LazyLock};

use schemars::{JsonSchema, Schema, generate::SchemaSettings};
use serde::{Deserialize, Serialize, Serializer};
use serde_json::json;

use crate::{
    format::Format,
    planner::Runtime,
    tools::{secret::annotate_secret_params, specs},
};

/// Regular expression pattern every [`ToolId`] must satisfy (`docs/tools.md` §2).
pub const TOOL_ID_PATTERN: &str = r"^[a-z]+(\.[a-z0-9_]+)+$";

/// Canonical JSON Schema `$id` for `schemas/tools.v1.json`.
pub const TOOLS_SCHEMA_ID: &str = "https://ariadshift.ariadnev.com/schemas/tools.v1.json";

/// Validated UTF-8 tool identifier matching [`TOOL_ID_PATTERN`] (`^[a-z]+(\.[a-z0-9_]+)+$`).
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ToolId(String);

impl JsonSchema for ToolId {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("ToolId")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> Schema {
        schemars::json_schema!({
            "description": "Validated UTF-8 tool identifier matching `^[a-z]+(\\.[a-z0-9_]+)+$`.",
            "type": "string",
            "pattern": TOOL_ID_PATTERN,
        })
    }
}

/// Error returned when a tool identifier string does not match [`TOOL_ID_PATTERN`].
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
#[error("invalid tool id {0:?}: must match {TOOL_ID_PATTERN}")]
pub struct ToolIdError(pub String);

impl ToolId {
    /// Regular expression pattern enforced by [`ToolId`].
    pub const PATTERN: &'static str = TOOL_ID_PATTERN;

    /// Validates and constructs a [`ToolId`].
    pub fn new(id: impl Into<String>) -> Result<Self, ToolIdError> {
        let id = id.into();
        if Self::is_valid(&id) {
            Ok(Self(id))
        } else {
            Err(ToolIdError(id))
        }
    }

    /// Parses and validates a [`ToolId`] from a string slice.
    pub fn parse(id: &str) -> Result<Self, ToolIdError> {
        Self::new(id.to_owned())
    }

    /// Returns `true` if `id` matches `^[a-z]+(\.[a-z0-9_]+)+$`.
    #[must_use]
    pub fn is_valid(id: &str) -> bool {
        let mut segments = id.split('.');
        let Some(domain) = segments.next() else {
            return false;
        };
        if domain.is_empty() || !domain.bytes().all(|b| b.is_ascii_lowercase()) {
            return false;
        }
        let mut count = 0usize;
        for segment in segments {
            count += 1;
            if segment.is_empty()
                || !segment
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
            {
                return false;
            }
        }
        count >= 1
    }

    /// Returns `true` if this tool ID uses the flat two-segment `<domain>.<action>` namespace.
    #[must_use]
    pub fn is_flat_namespace(&self) -> bool {
        self.0.bytes().filter(|&b| b == b'.').count() == 1
    }

    /// Returns the string slice of this tool identifier.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns the `<domain>` prefix before the first `.`.
    #[must_use]
    pub fn domain(&self) -> &str {
        self.0.split('.').next().unwrap_or(&self.0)
    }

    /// Returns the `<action>` suffix after the first `.`.
    #[must_use]
    pub fn action(&self) -> &str {
        self.0
            .split_once('.')
            .map(|(_, action)| action)
            .unwrap_or("")
    }
}

impl Deref for ToolId {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<str> for ToolId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl Borrow<str> for ToolId {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ToolId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::str::FromStr for ToolId {
    type Err = ToolIdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

impl TryFrom<String> for ToolId {
    type Error = ToolIdError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl TryFrom<&str> for ToolId {
    type Error = ToolIdError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::parse(value)
    }
}

impl From<ToolId> for String {
    fn from(id: ToolId) -> Self {
        id.0
    }
}

impl PartialEq<str> for ToolId {
    fn eq(&self, other: &str) -> bool {
        self.0 == other
    }
}

impl PartialEq<&str> for ToolId {
    fn eq(&self, other: &&str) -> bool {
        self.0 == *other
    }
}

/// Functional category of a registered document tool (`docs/tools.md` §2).
#[derive(
    Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum ToolGroup {
    /// Page and document structure manipulation (`pdf.info`, `pdf.merge`, `pdf.rotate`, etc.).
    Organize,
    /// Multi-artifact or specialized document conversions (`pdf.render`, `image.to_pdf`, etc.).
    Convert,
    /// File size reduction, stream recompression, linearization, repair.
    Optimize,
    /// Encryption, decryption, permission flags, sanitization.
    Security,
    /// Stamps, page numbers, headers/footers, watermarks, background overlays.
    Overlay,
    /// Optical character recognition, deskewing, blank page removal.
    Ocr,
    /// Highlights, notes, freehand ink, geometric markups, comment flattening.
    Annotate,
    /// AcroForm field inspection, filling, creation, and flattening.
    Forms,
    /// PAdES digital signing, timestamping, and signature verification.
    Sign,
    /// True object-stream and raster redaction with verification report.
    Redact,
    /// Visual overlay comparison and semantic content comparison.
    Compare,
    /// Stateless page-mode in-place edits and document-mode export helpers.
    Edit,
    /// Offline-first summarization, Q&A with citations, translation, table extraction.
    Ai,
    /// Batch recipe execution and folder watch automation.
    Automation,
}

impl ToolGroup {
    /// All 14 tool groups in canonical table order.
    pub const ALL: &'static [Self] = &[
        Self::Organize,
        Self::Convert,
        Self::Optimize,
        Self::Security,
        Self::Overlay,
        Self::Ocr,
        Self::Annotate,
        Self::Forms,
        Self::Sign,
        Self::Redact,
        Self::Compare,
        Self::Edit,
        Self::Ai,
        Self::Automation,
    ];

    /// Stable snake_case identifier for this group.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Organize => "organize",
            Self::Convert => "convert",
            Self::Optimize => "optimize",
            Self::Security => "security",
            Self::Overlay => "overlay",
            Self::Ocr => "ocr",
            Self::Annotate => "annotate",
            Self::Forms => "forms",
            Self::Sign => "sign",
            Self::Redact => "redact",
            Self::Compare => "compare",
            Self::Edit => "edit",
            Self::Ai => "ai",
            Self::Automation => "automation",
        }
    }
}

impl std::fmt::Display for ToolGroup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for ToolGroup {
    type Err = ToolGroupParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .iter()
            .copied()
            .find(|g| g.as_str() == s)
            .ok_or_else(|| ToolGroupParseError(s.to_owned()))
    }
}

/// Error returned when parsing an unknown [`ToolGroup`] identifier.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
#[error("unknown tool group {0:?}")]
pub struct ToolGroupParseError(pub String);

/// Required input file count for a tool (`docs/tools.md` §3).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "arity", rename_all = "snake_case")]
pub enum InputArity {
    /// Exactly 1 input file is required.
    One,
    /// Between `min` and `max` input files (inclusive) are required (`1 <= min <= max`).
    Many {
        /// Minimum number of input files (`>= 1`).
        #[schemars(range(min = 1))]
        min: usize,
        /// Maximum number of input files (`>= min`).
        #[schemars(range(min = 1))]
        max: usize,
    },
}

impl InputArity {
    /// Constructs an [`InputArity::Many`] range (`1 <= min <= max`).
    #[must_use]
    pub const fn many(min: usize, max: usize) -> Self {
        assert!(
            min >= 1 && min <= max,
            "InputArity::many requires 1 <= min <= max"
        );
        Self::Many { min, max }
    }

    /// Returns `true` if `count` input files satisfies this arity requirement.
    #[must_use]
    pub const fn accepts(self, count: usize) -> bool {
        match self {
            Self::One => count == 1,
            Self::Many { min, max } => count >= min && count <= max,
        }
    }

    /// Minimum number of inputs accepted.
    #[must_use]
    pub const fn min(self) -> usize {
        match self {
            Self::One => 1,
            Self::Many { min, .. } => min,
        }
    }

    /// Maximum number of inputs accepted.
    #[must_use]
    pub const fn max(self) -> usize {
        match self {
            Self::One => 1,
            Self::Many { max, .. } => max,
        }
    }

    /// Returns `true` if this arity bounds are structurally valid (`1 <= min <= max`).
    #[must_use]
    pub const fn is_valid(self) -> bool {
        match self {
            Self::One => true,
            Self::Many { min, max } => min >= 1 && min <= max,
        }
    }
}

/// Access tier of a registered tool (`docs/tools.md` §10).
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
pub enum Tier {
    /// Available on all plans (`Free`, `Pro`, `Team`, `Enterprise`).
    #[default]
    Free,
    /// Requires a plan that grants Pro access (`Pro`, `Team`, `Enterprise`).
    Pro,
}

impl Tier {
    /// Stable snake_case identifier (`"free"` or `"pro"`).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Free => "free",
            Self::Pro => "pro",
        }
    }
}

impl std::fmt::Display for Tier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for Tier {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "free" => Ok(Self::Free),
            "pro" => Ok(Self::Pro),
            _ => Err(format!("unknown tier {s:?}; expected 'free' or 'pro'")),
        }
    }
}

/// Execution runtime environment supported by a tool (`docs/tools.md` §2).
#[derive(
    Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum ToolRuntime {
    /// Browser WebAssembly (`ariad-wasm`).
    Wasm,
    /// Local host execution (`ashift` CLI, local MCP, `apps/macos`).
    Local,
    /// Remote cloud worker execution (`ariad-server`).
    Cloud,
}

impl ToolRuntime {
    /// Stable snake_case identifier (`"wasm"`, `"local"`, or `"cloud"`).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Wasm => "wasm",
            Self::Local => "local",
            Self::Cloud => "cloud",
        }
    }
}

impl std::fmt::Display for ToolRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<ToolRuntime> for Runtime {
    fn from(rt: ToolRuntime) -> Self {
        match rt {
            ToolRuntime::Wasm => Self::Wasm,
            ToolRuntime::Local => Self::Local,
            ToolRuntime::Cloud => Self::Cloud,
        }
    }
}

impl From<Runtime> for ToolRuntime {
    fn from(rt: Runtime) -> Self {
        match rt {
            Runtime::Wasm => Self::Wasm,
            Runtime::Local => Self::Local,
            Runtime::Cloud => Self::Cloud,
        }
    }
}

/// Static specification of a registered document tool (`docs/tools.md` §2).
#[derive(Clone, Copy, Debug)]
pub struct ToolSpec {
    /// Canonical tool identifier matching [`TOOL_ID_PATTERN`] (e.g. `"pdf.info"`).
    pub id: &'static str,
    /// Tool contract version string (e.g. `"1"`).
    pub version: &'static str,
    /// Functional group.
    pub group: ToolGroup,
    /// One-line human-readable description.
    pub summary: &'static str,
    /// Required input file count.
    pub inputs: InputArity,
    /// Accepted input format identifiers (e.g. `&["pdf"]`).
    pub input_formats: &'static [&'static str],
    /// Produced output format identifier (e.g. `"pdf"` or `"json"`).
    pub output_format: &'static str,
    /// Access tier (`Free` or `Pro`).
    pub tier: Tier,
    /// Runtimes where this tool can execute.
    pub runtimes: &'static [ToolRuntime],
    /// Engine identifier executing this tool (e.g. `"ariad-pdf"`, `"pdfium"`, `"qpdf"`).
    pub engine: &'static str,
    /// Generator function returning the Draft 2020-12 JSON Schema for this tool's `params`.
    pub params_schema: fn() -> Schema,
    /// Parameter field names marked `"x-ariad-secret": true` (`docs/tools.md` §11).
    pub secret_params: &'static [&'static str],
}

impl ToolSpec {
    /// Returns the parsed [`ToolId`] of this specification.
    ///
    /// # Panics
    /// Panics only if a static spec was registered with an invalid ID (caught by registry unit tests).
    #[must_use]
    pub fn tool_id(&self) -> ToolId {
        ToolId::parse(self.id).expect("registered ToolSpec.id must be a valid ToolId")
    }

    /// Generates the tool's parameter JSON Schema with `"x-ariad-secret": true` annotated on
    /// every property listed in [`Self::secret_params`].
    #[must_use]
    pub fn resolved_params_schema(&self) -> Schema {
        let mut schema = (self.params_schema)();
        annotate_secret_params(&mut schema, self.secret_params);
        schema
    }

    /// Converts this static specification into an owned, serializable [`ToolDescriptor`].
    #[must_use]
    pub fn to_descriptor(&self) -> ToolDescriptor {
        ToolDescriptor::from(self)
    }

    /// Returns `true` if `format_id` is accepted by this tool's `input_formats`.
    #[must_use]
    pub fn accepts_format(&self, format_id: &str) -> bool {
        self.input_formats
            .iter()
            .any(|candidate| candidate.eq_ignore_ascii_case(format_id))
    }

    /// Returns `true` if `format` is accepted by this tool's `input_formats`.
    #[must_use]
    pub fn accepts_core_format(&self, format: Format) -> bool {
        self.accepts_format(format.id()) || self.accepts_format(format.user_name())
    }

    /// Returns `true` if this tool supports running on `runtime`.
    #[must_use]
    pub fn supports_runtime(&self, runtime: ToolRuntime) -> bool {
        self.runtimes.contains(&runtime)
    }
}

impl Serialize for ToolSpec {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.to_descriptor().serialize(serializer)
    }
}

/// Owned, JSON-serializable tool descriptor matching `schemas/tools.v1.json` and `ashift tools --json`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ToolDescriptor {
    /// Canonical tool identifier.
    pub id: ToolId,
    /// Tool contract version string.
    pub version: String,
    /// Functional group.
    pub group: ToolGroup,
    /// One-line human-readable description.
    pub summary: String,
    /// Required input file count.
    pub inputs: InputArity,
    /// Accepted input format identifiers.
    #[schemars(length(min = 1))]
    pub input_formats: Vec<String>,
    /// Produced output format identifier.
    pub output_format: String,
    /// Access tier (`free` or `pro`).
    pub tier: Tier,
    /// Supported execution runtimes.
    #[schemars(length(min = 1))]
    pub runtimes: Vec<ToolRuntime>,
    /// Executing engine identifier.
    pub engine: String,
    /// Parameter field names marked `"x-ariad-secret": true`.
    pub secret_params: Vec<String>,
    /// JSON Schema for the tool's `params` object.
    pub params_schema: Schema,
}

impl From<&ToolSpec> for ToolDescriptor {
    fn from(spec: &ToolSpec) -> Self {
        Self {
            id: spec.tool_id(),
            version: spec.version.to_owned(),
            group: spec.group,
            summary: spec.summary.to_owned(),
            inputs: spec.inputs,
            input_formats: spec.input_formats.iter().map(|s| (*s).to_owned()).collect(),
            output_format: spec.output_format.to_owned(),
            tier: spec.tier,
            runtimes: spec.runtimes.to_vec(),
            engine: spec.engine.to_owned(),
            secret_params: spec.secret_params.iter().map(|s| (*s).to_owned()).collect(),
            params_schema: spec.resolved_params_schema(),
        }
    }
}

/// Error returned when validating a slice of [`ToolSpec`] definitions.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum RegistryError {
    /// A tool ID did not match [`TOOL_ID_PATTERN`] or violated the flat `<domain>.<action>` convention.
    #[error("invalid tool id {id:?}: {reason}")]
    InvalidId {
        /// Offending tool ID.
        id: String,
        /// Explanation.
        reason: &'static str,
    },
    /// Two tool specifications shared the same tool ID.
    #[error("duplicate tool id {0:?} in registry")]
    DuplicateId(String),
    /// A required metadata field on [`ToolSpec`] was empty or invalid.
    #[error("tool {id:?} has invalid specification: {reason}")]
    InvalidSpec {
        /// Tool ID.
        id: String,
        /// Explanation.
        reason: &'static str,
    },
}

/// Validates a slice of [`ToolSpec`] entries for ID grammar, flat namespace, uniqueness, and metadata completeness.
pub fn validate_specs(specs: &[ToolSpec]) -> Result<(), RegistryError> {
    let mut seen_ids = HashSet::new();
    for spec in specs {
        let Ok(tool_id) = ToolId::parse(spec.id) else {
            return Err(RegistryError::InvalidId {
                id: spec.id.to_owned(),
                reason: "does not match ^[a-z]+(\\.[a-z0-9_]+)+$",
            });
        };
        if !tool_id.is_flat_namespace() {
            return Err(RegistryError::InvalidId {
                id: spec.id.to_owned(),
                reason: "must use flat two-segment <domain>.<action> namespace",
            });
        }
        if !seen_ids.insert(spec.id) {
            return Err(RegistryError::DuplicateId(spec.id.to_owned()));
        }
        if spec.version.trim().is_empty() {
            return Err(RegistryError::InvalidSpec {
                id: spec.id.to_owned(),
                reason: "version must not be empty",
            });
        }
        if spec.summary.trim().is_empty() {
            return Err(RegistryError::InvalidSpec {
                id: spec.id.to_owned(),
                reason: "summary must not be empty",
            });
        }
        if !spec.inputs.is_valid() {
            return Err(RegistryError::InvalidSpec {
                id: spec.id.to_owned(),
                reason: "inputs arity bounds must satisfy 1 <= min <= max",
            });
        }
        if spec.input_formats.is_empty() || spec.input_formats.iter().any(|f| f.trim().is_empty()) {
            return Err(RegistryError::InvalidSpec {
                id: spec.id.to_owned(),
                reason: "input_formats must be non-empty",
            });
        }
        if spec.output_format.trim().is_empty() {
            return Err(RegistryError::InvalidSpec {
                id: spec.id.to_owned(),
                reason: "output_format must not be empty",
            });
        }
        if spec.runtimes.is_empty() {
            return Err(RegistryError::InvalidSpec {
                id: spec.id.to_owned(),
                reason: "runtimes must not be empty",
            });
        }
        if spec.engine.trim().is_empty() {
            return Err(RegistryError::InvalidSpec {
                id: spec.id.to_owned(),
                reason: "engine must not be empty",
            });
        }
        let schema = spec.resolved_params_schema();
        let props = schema
            .as_object()
            .and_then(|o| o.get("properties"))
            .and_then(|p| p.as_object());
        for &secret_field in spec.secret_params {
            let Some(prop) = props.and_then(|p| p.get(secret_field)) else {
                return Err(RegistryError::InvalidSpec {
                    id: spec.id.to_owned(),
                    reason: "secret_params entry is not a property in params_schema",
                });
            };
            if prop.get("x-ariad-secret") != Some(&json!(true)) {
                return Err(RegistryError::InvalidSpec {
                    id: spec.id.to_owned(),
                    reason: "secret_params property is missing x-ariad-secret: true in resolved schema",
                });
            }
        }
    }
    Ok(())
}

static REGISTRY: LazyLock<Vec<ToolSpec>> = LazyLock::new(|| {
    let mut list = Vec::new();
    list.extend(specs::organize_info::specs());
    validate_specs(&list).expect("built-in tool registry must be valid");
    list
});

/// Returns all registered [`ToolSpec`]s in deterministic registration order.
#[must_use]
pub fn all() -> &'static [ToolSpec] {
    &REGISTRY
}

/// Looks up a registered [`ToolSpec`] by its identifier (`"pdf.info"`, etc.).
#[must_use]
pub fn find(id: &str) -> Option<&'static ToolSpec> {
    all().iter().find(|spec| spec.id == id)
}

/// Iterates over registered [`ToolSpec`]s belonging to `group`.
pub fn by_group(group: ToolGroup) -> impl Iterator<Item = &'static ToolSpec> {
    all().iter().filter(move |spec| spec.group == group)
}

/// Generates the pretty-printed `schemas/tools.v1.json` document from [`ToolDescriptor`] and [`all()`].
#[must_use]
pub fn generated_tools_schema() -> String {
    let mut schema = SchemaSettings::draft2020_12()
        .into_generator()
        .into_root_schema_for::<ToolDescriptor>();
    let descriptors: Vec<ToolDescriptor> = all().iter().map(ToolSpec::to_descriptor).collect();
    let object = schema
        .as_object_mut()
        .expect("ToolDescriptor schema is an object");
    object.insert("$id".to_owned(), json!(TOOLS_SCHEMA_ID));
    object.insert("x-status".to_owned(), json!("draft"));
    object.insert(
        "tools".to_owned(),
        serde_json::to_value(&descriptors).expect("tool descriptors serialize"),
    );
    let mut contents = serde_json::to_string_pretty(&schema).expect("schema serializes to JSON");
    contents.push('\n');
    contents
}
