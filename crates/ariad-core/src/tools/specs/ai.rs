//! Offline-first AI tool contracts. Cloud transport is owned by the host.
use crate::tools::{
    InputArity, Tier, ToolGroup, ToolRuntime, ToolSpec, params::ai::*, schema_with_secrets,
};
use schemars::{JsonSchema, Schema};

fn ai_schema<P: JsonSchema>() -> Schema {
    schema_with_secrets::<P>(&["api_key"])
}

/// Returns the five AI tools with explicit secret annotations.
#[must_use]
pub fn specs() -> Vec<ToolSpec> {
    vec![
        ToolSpec {
            id: "ai.summarize",
            version: "1",
            group: ToolGroup::Ai,
            summary: "Summarize a document in the requested language.",
            inputs: InputArity::One,
            input_formats: &["pdf", "docx", "html", "md", "json"],
            output_format: "json",
            output_formats: &["json", "md"],
            tier: Tier::Free,
            runtimes: &[ToolRuntime::Local],
            engine: "ai",
            params_schema: ai_schema::<AiSummarizeParams>,
            secret_params: &["api_key"],
        },
        ToolSpec {
            id: "ai.ask",
            version: "1",
            group: ToolGroup::Ai,
            summary: "Answer a document question with citations.",
            inputs: InputArity::One,
            input_formats: &["pdf", "docx", "html", "md", "json"],
            output_format: "json",
            output_formats: &[],
            tier: Tier::Free,
            runtimes: &[ToolRuntime::Local],
            engine: "ai",
            params_schema: ai_schema::<AiAskParams>,
            secret_params: &["api_key"],
        },
        ToolSpec {
            id: "ai.translate",
            version: "1",
            group: ToolGroup::Ai,
            summary: "Translate a document into another language.",
            inputs: InputArity::One,
            input_formats: &["pdf", "docx", "html", "md", "json"],
            output_format: "pdf",
            output_formats: &["pdf", "docx", "html", "md", "json"],
            tier: Tier::Free,
            runtimes: &[ToolRuntime::Local],
            engine: "ai",
            params_schema: ai_schema::<AiTranslateParams>,
            secret_params: &["api_key"],
        },
        ToolSpec {
            id: "ai.extract_tables",
            version: "1",
            group: ToolGroup::Ai,
            summary: "Extract document tables into structured data.",
            inputs: InputArity::One,
            input_formats: &["pdf", "docx", "html", "md", "json"],
            output_format: "csv",
            output_formats: &["csv", "xlsx", "json"],
            tier: Tier::Free,
            runtimes: &[ToolRuntime::Local],
            engine: "ai",
            params_schema: ai_schema::<AiExtractTablesParams>,
            secret_params: &["api_key"],
        },
        ToolSpec {
            id: "ai.recipe_from_text",
            version: "1",
            group: ToolGroup::Ai,
            summary: "Draft an imported recipe from a natural-language instruction.",
            inputs: InputArity::None,
            input_formats: &[],
            output_format: "json",
            output_formats: &[],
            tier: Tier::Free,
            runtimes: &[ToolRuntime::Local],
            engine: "ai",
            params_schema: ai_schema::<AiRecipeFromTextParams>,
            secret_params: &["api_key"],
        },
    ]
}
