//! Image OCR options.
use super::ocr::{default_dpi, default_languages, deserialize_languages};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};
/// Requested image OCR output.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ImageOcrOutput {
    #[default]
    Pdf,
    Txt,
    Hocr,
}
fn image_dpi<'de, D: Deserializer<'de>>(d: D) -> Result<u32, D::Error> {
    let dpi = u32::deserialize(d)?;
    if !(70..=1200).contains(&dpi) {
        return Err(serde::de::Error::custom("dpi outside allowed range"));
    }
    Ok(dpi)
}
/// Image recognition parameters.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ImageOcrParams {
    /// Recognition languages.
    #[serde(
        default = "default_languages",
        deserialize_with = "deserialize_languages"
    )]
    pub languages: Vec<String>,
    /// Requested output format.
    #[serde(default)]
    pub output: ImageOcrOutput,
    /// Resolution used if the image has none.
    #[serde(default = "default_dpi", deserialize_with = "image_dpi")]
    #[schemars(range(min = 70, max = 1200))]
    pub dpi: u32,
}
