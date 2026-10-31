//! Host tool execution, atomic multi-artifact publication, secret scrubbing, and recipe runner.
//!
//! Dispatches registered tools from [`ariad_core::tools`] either in-process (`ariad-pdf`) or
//! out of process over `ariad-engine/2` (`op = "run_tool"`), enforces `ToolAccess` tiers and
//! batch quotas, validates `x-ariad-secret` rules, scrubs secret values from all outputs, and
//! atomically publishes output artifacts into the target directory.

pub mod engine;
pub mod inprocess;
pub mod publish;
pub mod recipe;
pub mod run;
pub mod secrets;

pub use publish::{
    PublishError, PublishedArtifact, StagedArtifact, default_multi_artifact_name,
    default_single_artifact_name, default_tool_suffix, publish_atomic, render_naming_template,
};
pub use recipe::{RecipeOutcome, RecipeRequest, StepOutcome, run_recipe, run_recipe_with_events};
pub use run::{
    InputRef, OutputTarget, ToolError, ToolEvent, ToolMetrics, ToolOutcome, ToolRequest, run_tool,
    run_tool_with_events, validate_params_against_schema,
};
pub use secrets::{REDACTED, ResolvedSecrets, SecretBag, SecretError, SecretScrubber};
