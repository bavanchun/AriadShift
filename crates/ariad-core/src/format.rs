use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A format supported by the core registry.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    Markdown,
    Html,
    Docx,
    AriadIrJson,
    PandocJson,
}

impl Format {
    /// The stable registry identifier for this format.
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::Markdown => "markdown",
            Self::Html => "html",
            Self::Docx => "docx",
            Self::AriadIrJson => "ariad-ir+json",
            Self::PandocJson => "pandoc+json",
        }
    }

    /// File extensions associated with this format, without a leading dot.
    #[must_use]
    pub const fn extensions(self) -> &'static [&'static str] {
        match self {
            Self::Markdown => &["md", "markdown", "mdown"],
            Self::Html => &["html", "htm"],
            Self::Docx => &["docx"],
            Self::AriadIrJson | Self::PandocJson => &["json"],
        }
    }

    /// The MIME type associated with this format.
    #[must_use]
    pub const fn media_type(self) -> &'static str {
        match self {
            Self::Markdown => "text/markdown",
            Self::Html => "text/html",
            Self::Docx => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            Self::AriadIrJson | Self::PandocJson => "application/json",
        }
    }

    /// Resolves a supported extension without regard to ASCII case.
    #[must_use]
    pub fn from_extension(extension: &str) -> Option<Self> {
        let extension = extension.trim_start_matches('.');
        if ["md", "markdown", "mdown"]
            .iter()
            .any(|candidate| extension.eq_ignore_ascii_case(candidate))
        {
            Some(Self::Markdown)
        } else if ["html", "htm"]
            .iter()
            .any(|candidate| extension.eq_ignore_ascii_case(candidate))
        {
            Some(Self::Html)
        } else if extension.eq_ignore_ascii_case("docx") {
            Some(Self::Docx)
        } else if extension.eq_ignore_ascii_case("json") {
            Some(Self::AriadIrJson)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Format;

    #[test]
    fn extensions_are_case_insensitive_and_json_defaults_to_ir() {
        assert_eq!(Format::from_extension("MD"), Some(Format::Markdown));
        assert_eq!(Format::from_extension(".Markdown"), Some(Format::Markdown));
        assert_eq!(Format::from_extension("HTM"), Some(Format::Html));
        assert_eq!(Format::from_extension("DocX"), Some(Format::Docx));
        assert_eq!(Format::from_extension("JSON"), Some(Format::AriadIrJson));
        assert_eq!(Format::from_extension("unknown"), None);
    }
}
