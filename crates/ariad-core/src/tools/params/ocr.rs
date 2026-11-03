//! Parameters shared by OCR operations.
use crate::tools::PageRange;
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};

/// Policy for pages with existing text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum OcrMode {
    #[default]
    SkipText,
    Force,
    Redo,
}
/// Output artifacts for PDF OCR.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum OcrOutput {
    #[default]
    #[serde(rename = "pdf")]
    Pdf,
    #[serde(rename = "pdf+txt")]
    PdfText,
    #[serde(rename = "pdf+hocr")]
    PdfHocr,
}

pub(crate) fn default_languages() -> Vec<String> {
    vec!["vie".into(), "eng".into()]
}
pub(crate) fn deserialize_languages<'de, D: Deserializer<'de>>(
    d: D,
) -> Result<Vec<String>, D::Error> {
    let languages = Vec::<String>::deserialize(d)?;
    if languages.is_empty() || languages.len() > 8 {
        return Err(serde::de::Error::custom("expected one to eight languages"));
    }
    for (index, language) in languages.iter().enumerate() {
        let mut parts = language.split('_');
        let base = parts.next().unwrap_or_default();
        let suffix = parts.next();
        if base.len() != 3
            || !base.bytes().all(|b| b.is_ascii_lowercase())
            || base == "osd"
            || suffix.is_some_and(|s| {
                !(2..=8).contains(&s.len())
                    || !s
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
            })
            || parts.next().is_some()
            || languages[..index].contains(language)
        {
            return Err(serde::de::Error::custom("invalid or duplicate language"));
        }
    }
    Ok(languages)
}
pub(crate) fn default_dpi() -> u32 {
    300
}
pub(crate) fn dpi_150_600<'de, D: Deserializer<'de>>(d: D) -> Result<u32, D::Error> {
    let dpi = u32::deserialize(d)?;
    if !(150..=600).contains(&dpi) {
        return Err(serde::de::Error::custom("dpi outside allowed range"));
    }
    Ok(dpi)
}
/// Searchable PDF OCR options.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OcrParams {
    /// Pages to recognize.
    #[serde(default = "PageRange::all")]
    pub pages: PageRange,
    /// Recognition languages.
    #[serde(
        default = "default_languages",
        deserialize_with = "deserialize_languages"
    )]
    pub languages: Vec<String>,
    /// Existing text policy.
    #[serde(default)]
    pub mode: OcrMode,
    /// Correct skew.
    #[serde(default)]
    pub deskew: bool,
    /// Correct orientation.
    #[serde(default)]
    pub auto_rotate: bool,
    /// Clean eligible image-only pages.
    #[serde(default)]
    pub clean: bool,
    /// Render resolution.
    #[serde(default = "default_dpi", deserialize_with = "dpi_150_600")]
    #[schemars(range(min = 150, max = 600))]
    pub dpi: u32,
    /// Requested artifacts.
    #[serde(default)]
    pub output: OcrOutput,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::params::{
        auto_rotate::AutoRotateParams, deskew::DeskewParams, image_ocr::ImageOcrParams,
        scan_split::ScanSplitParams, scanner_effect::ScannerEffectParams,
    };
    use serde_json::json;
    #[test]
    fn defaults_and_boundaries() {
        let p: OcrParams = serde_json::from_value(json!({})).unwrap();
        assert_eq!(p.languages, ["vie", "eng"]);
        assert_eq!(p.pages.as_str(), "all");
        assert_eq!(p.dpi, 300);
        assert_eq!(p.mode, OcrMode::SkipText);
        for bad in [
            json!([]),
            json!(["osd"]),
            json!(["eng", "eng"]),
            json!(["../eng"]),
            json!(["eng_XY"]),
        ] {
            assert!(serde_json::from_value::<OcrParams>(json!({"languages": bad})).is_err());
        }
        assert!(
            serde_json::from_value::<OcrParams>(json!({"languages":["eng", "chi_sim"], "dpi":600}))
                .is_ok()
        );
        assert!(serde_json::from_value::<OcrParams>(json!({"dpi":149})).is_err());
        assert!(serde_json::from_value::<ImageOcrParams>(json!({"dpi":1200})).is_ok());
        assert!(
            serde_json::from_value::<crate::tools::params::ocr_internal::OcrRecognizeParams>(
                json!({"dpi":1200})
            )
            .is_ok()
        );
        assert!(serde_json::from_value::<ImageOcrParams>(json!({"dpi":1201})).is_err());
        assert!(serde_json::from_value::<AutoRotateParams>(json!({"min_confidence":101})).is_err());
        assert!(serde_json::from_value::<DeskewParams>(json!({"max_angle_deg":0})).is_err());
        assert!(serde_json::from_value::<ScanSplitParams>(json!({"min_area_ratio":1})).is_err());
        assert!(serde_json::from_value::<ScannerEffectParams>(json!({"quality":9})).is_err());
        assert!(serde_json::from_value::<AutoRotateParams>(json!({})).is_ok());
        assert!(serde_json::from_value::<DeskewParams>(json!({})).is_ok());
        assert!(serde_json::from_value::<ScanSplitParams>(json!({})).is_ok());
        assert!(serde_json::from_value::<ScannerEffectParams>(json!({})).is_ok());
    }
}
