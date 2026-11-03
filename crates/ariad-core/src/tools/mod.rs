//! Typed document manipulation tools: registry, parameter schemas, access control,
//! 1-based page-range grammar, secrets contract, and pipeline recipes (`docs/tools.md`).
//!
//! Conversions use the capability graph in [`crate::planner`]; tools live in this module and
//! never enter the route planner.

pub mod access;
pub mod page_range;
pub mod params;
pub mod recipe;
pub mod registry;
pub mod secret;
pub mod specs;

pub use access::{
    FREE_MAX_RUN_BYTES, FREE_MAX_RUN_FILES, Plan, PlanParseError, RunSize, ToolAccess, allowed,
    within_run_quota,
};
pub use page_range::{
    DuplicatePolicy, PageRange, PageRangeError, PageRef, PageToken, PageTokenKind,
};
pub use recipe::{
    ALLOWED_NAMING_TOKENS, ConvertTarget, NETWORK_FETCH_TOOLS, NamingTemplateError,
    RECIPE_SCHEMA_ID, RECIPE_VERSION, Recipe, RecipeError, RecipeOrigin, Step,
    generated_recipe_schema, validate_naming_template,
};
pub use registry::{
    InputArity, RegistryError, TOOL_ID_PATTERN, TOOLS_SCHEMA_ID, Tier, ToolDescriptor, ToolGroup,
    ToolGroupParseError, ToolId, ToolIdError, ToolRuntime, ToolSpec, all, by_group, find,
    generated_tools_schema, normalize_tool_format, validate_specs,
};
pub use secret::{
    KNOWN_SECRET_FIELD_NAMES, SECRET_HANDLE_PREFIX, SECRET_SCHEMA_EXTENSION, SecretRef,
    SecretSource, SecretValidationError, annotate_secret_params, schema_with_secrets,
    validate_secret_fields, validate_secret_params, validate_single_secret_value,
};
