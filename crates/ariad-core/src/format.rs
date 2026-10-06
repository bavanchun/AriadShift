use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A format supported by the core registry.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub enum Format {
    #[serde(rename = "markdown")]
    Markdown,
    #[serde(rename = "html")]
    Html,
    #[serde(rename = "docx")]
    Docx,
    #[serde(rename = "epub")]
    Epub,
    #[serde(rename = "pdf")]
    Pdf,
    #[serde(rename = "png")]
    Png,
    #[serde(rename = "ariad-ir+json")]
    AriadIrJson,
    #[serde(rename = "pandoc+json")]
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
            Self::Epub => "epub",
            Self::Pdf => "pdf",
            Self::Png => "png",
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
            Self::Epub => &["epub"],
            Self::Pdf => &["pdf"],
            Self::Png => &["png"],
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
            Self::Epub => "application/epub+zip",
            Self::Pdf => "application/pdf",
            Self::Png => "image/png",
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
        } else if extension.eq_ignore_ascii_case("epub") {
            Some(Self::Epub)
        } else if extension.eq_ignore_ascii_case("pdf") {
            Some(Self::Pdf)
        } else if extension.eq_ignore_ascii_case("png") {
            Some(Self::Png)
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
        assert_eq!(Format::from_extension("EPUB"), Some(Format::Epub));
        assert_eq!(Format::from_extension(".epub"), Some(Format::Epub));
        assert_eq!(Format::from_extension("PDF"), Some(Format::Pdf));
        assert_eq!(Format::from_extension(".pdf"), Some(Format::Pdf));
        assert_eq!(Format::from_extension("PNG"), Some(Format::Png));
        assert_eq!(Format::from_extension(".png"), Some(Format::Png));
        assert_eq!(Format::from_extension("JSON"), Some(Format::AriadIrJson));
        assert_eq!(Format::from_extension("unknown"), None);
    }

    #[test]
    fn serde_names_equal_id_for_every_variant() {
        let variants = [
            Format::Markdown,
            Format::Html,
            Format::Docx,
            Format::Epub,
            Format::Pdf,
            Format::Png,
            Format::AriadIrJson,
            Format::PandocJson,
        ];
        for variant in variants {
            let json = serde_json::to_string(&variant).expect("serialize format");
            let expected = format!("\"{}\"", variant.id());
            assert_eq!(json, expected, "serialized name for {:?}", variant);
            let round_trip: Format = serde_json::from_str(&json).expect("deserialize format");
            assert_eq!(round_trip, variant, "round trip for {:?}", variant);
        }
    }
}
