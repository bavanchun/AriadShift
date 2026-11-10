//! Typed parameters shared by the offline-first AI tools.

use super::ParamsError;
use crate::tools::PageRange;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Execution provider; cloud requires explicit caller consent.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AiProvider {
    /// `local`.
    #[default]
    Local,
    /// `cloud`.
    Cloud,
}

/// Requested summary detail.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SummaryLength {
    /// `brief`.
    Brief,
    /// `standard`.
    #[default]
    Standard,
    /// `detailed`.
    Detailed,
}

/// Summary artifact format.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SummaryOutputFormat {
    /// `json`.
    #[default]
    Json,
    /// `md`.
    Md,
}

/// Translation artifact format.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TranslateOutputFormat {
    /// `pdf`.
    #[default]
    Pdf,
    /// `docx`.
    Docx,
    /// `html`.
    Html,
    /// `md`.
    Md,
    /// `json`.
    Json,
}

/// Extracted table artifact format.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TableOutputFormat {
    /// `csv`.
    #[default]
    Csv,
    /// `xlsx`.
    Xlsx,
    /// `json`.
    Json,
}

/// Parameters for `ai.summarize`.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AiSummarizeParams {
    /// Requested detail (standard by default).
    #[serde(default)]
    pub length: SummaryLength,
    /// Optional output language.
    #[serde(default)]
    pub language: Option<String>,
    /// Summary format (JSON when omitted).
    #[serde(default)]
    pub format: Option<SummaryOutputFormat>,
    /// Execution provider (local by default).
    #[serde(default)]
    pub provider: AiProvider,
    /// Optional provider model identifier.
    #[serde(default)]
    pub model: Option<String>,
    /// Explicit permission to send document content to a cloud provider.
    #[serde(default)]
    pub cloud_consent: bool,
    /// API credential, supplied only through a secret channel.
    #[serde(default)]
    pub api_key: Option<String>,
}

impl AiSummarizeParams {
    /// Checks semantic constraints after deserialization.
    pub fn validate(&self) -> Result<(), ParamsError> {
        Ok(())
    }
}

/// Parameters for `ai.ask`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AiAskParams {
    /// Nonempty question about the document.
    #[schemars(length(min = 1), regex(pattern = r"\S"))]
    pub question: String,
    /// Citation limit (5 when omitted).
    #[serde(default)]
    #[schemars(range(min = 1, max = 20))]
    pub max_citations: Option<u32>,
    /// Execution provider (local by default).
    #[serde(default)]
    pub provider: AiProvider,
    /// Optional provider model identifier.
    #[serde(default)]
    pub model: Option<String>,
    /// Explicit permission to send document content to a cloud provider.
    #[serde(default)]
    pub cloud_consent: bool,
    /// API credential, supplied only through a secret channel.
    #[serde(default)]
    pub api_key: Option<String>,
}

impl AiAskParams {
    /// Checks semantic constraints after deserialization.
    pub fn validate(&self) -> Result<(), ParamsError> {
        if self.question.trim().is_empty() {
            return Err(ParamsError::new("question", "must not be empty"));
        }
        if let Some(count) = self.max_citations {
            super::check_range("max_citations", count, 1, 20)?;
        }
        Ok(())
    }
}

/// Parameters for `ai.translate`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AiTranslateParams {
    /// Nonempty target language identifier.
    #[schemars(length(min = 1), regex(pattern = r"\S"))]
    pub target_lang: String,
    /// Optional source language (detected when omitted).
    #[serde(default)]
    pub source_lang: Option<String>,
    /// Translation format (PDF when omitted).
    #[serde(default)]
    pub format: Option<TranslateOutputFormat>,
    /// Execution provider (local by default).
    #[serde(default)]
    pub provider: AiProvider,
    /// Optional provider model identifier.
    #[serde(default)]
    pub model: Option<String>,
    /// Explicit permission to send document content to a cloud provider.
    #[serde(default)]
    pub cloud_consent: bool,
    /// API credential, supplied only through a secret channel.
    #[serde(default)]
    pub api_key: Option<String>,
}

impl AiTranslateParams {
    /// Checks semantic constraints after deserialization.
    pub fn validate(&self) -> Result<(), ParamsError> {
        if self.target_lang.trim().is_empty() {
            return Err(ParamsError::new("target_lang", "must not be empty"));
        }
        Ok(())
    }
}

/// Parameters for `ai.extract_tables`.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AiExtractTablesParams {
    /// Optional PDF page selector.
    #[serde(default)]
    pub pages: Option<PageRange>,
    /// Table format (CSV when omitted).
    #[serde(default)]
    pub format: Option<TableOutputFormat>,
    /// Execution provider (local by default).
    #[serde(default)]
    pub provider: AiProvider,
    /// Optional provider model identifier.
    #[serde(default)]
    pub model: Option<String>,
    /// Explicit permission to send document content to a cloud provider.
    #[serde(default)]
    pub cloud_consent: bool,
    /// API credential, supplied only through a secret channel.
    #[serde(default)]
    pub api_key: Option<String>,
}

impl AiExtractTablesParams {
    /// Checks semantic constraints after deserialization.
    pub fn validate(&self) -> Result<(), ParamsError> {
        Ok(())
    }
}

/// Parameters for `ai.recipe_from_text`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AiRecipeFromTextParams {
    /// Nonempty instruction to turn into an imported recipe for review.
    #[schemars(length(min = 1), regex(pattern = r"\S"))]
    pub prompt: String,
    /// Execution provider (local by default).
    #[serde(default)]
    pub provider: AiProvider,
    /// Optional provider model identifier.
    #[serde(default)]
    pub model: Option<String>,
    /// Explicit permission to send document content to a cloud provider.
    #[serde(default)]
    pub cloud_consent: bool,
    /// API credential, supplied only through a secret channel.
    #[serde(default)]
    pub api_key: Option<String>,
}

impl AiRecipeFromTextParams {
    /// Checks semantic constraints after deserialization.
    pub fn validate(&self) -> Result<(), ParamsError> {
        if self.prompt.trim().is_empty() {
            return Err(ParamsError::new("prompt", "must not be empty"));
        }
        Ok(())
    }
}
