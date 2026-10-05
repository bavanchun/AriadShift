use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A source location in one-based line and column coordinates.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SourcePos {
    pub line: u32,
    pub column: u32,
}

/// A non-fatal conversion issue.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Warning {
    pub code: WarningCode,
    pub message: String,
    pub source_pos: Option<SourcePos>,
}

impl Warning {
    #[must_use]
    pub fn new(code: WarningCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            source_pos: None,
        }
    }

    #[must_use]
    pub fn at(mut self, source_pos: SourcePos) -> Self {
        self.source_pos = Some(source_pos);
        self
    }
}

/// Stable warning identifiers shared by readers and writers.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WarningCode {
    FrontMatterInvalid,
    FrontMatterKeyIgnored,
    FrontMatterAliasRejected,
    UnsupportedNode,
    RawDropped,
    LinkDropped,
    ImageNotEmbedded,
    FootnoteMissing,
    FootnoteUnused,
}
