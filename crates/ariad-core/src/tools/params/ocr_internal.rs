//! Internal scan and recognition protocol parameters.
use super::ocr::{default_dpi, default_languages, deserialize_languages};
use crate::tools::PageRange;
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};

/// PDFium render operation.
pub const SCAN_RENDER: &str = "scan.render";
/// Tesseract recognition operation.
pub const OCR_RECOGNIZE: &str = "ocr.recognize";
/// Tesseract orientation operation.
pub const OCR_ORIENTATION: &str = "ocr.orientation";
fn scan_dpi<'de, D: Deserializer<'de>>(d: D) -> Result<u32, D::Error> {
    let dpi = u32::deserialize(d)?;
    if !(72..=600).contains(&dpi) {
        return Err(serde::de::Error::custom("dpi outside allowed range"));
    }
    Ok(dpi)
}
/// Outputs `pNNNN.png` gray images and `scan.json` entries with page, width_px,
/// height_px, dpi, counter-clockwise skew_deg, skew_confidence and cleaned.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScanRenderParams {
    /// Pages to render.
    #[serde(default = "PageRange::all")]
    pub pages: PageRange,
    /// Render resolution.
    #[serde(default = "default_dpi", deserialize_with = "scan_dpi")]
    #[schemars(range(min = 72, max = 600))]
    pub dpi: u32,
    /// Clean the render.
    #[serde(default)]
    pub clean: bool,
    /// Estimate and correct skew.
    #[serde(default)]
    pub deskew: bool,
}
/// Outputs one `<n>.ocr.json` containing an `OcrPage` per input.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OcrRecognizeParams {
    /// Recognition languages.
    #[serde(
        default = "default_languages",
        deserialize_with = "deserialize_languages"
    )]
    pub languages: Vec<String>,
    /// Image resolution.
    #[serde(default = "default_dpi")]
    pub dpi: u32,
    /// Detect orientation before recognition.
    #[serde(default)]
    pub auto_orient: bool,
}
/// Outputs one `<n>.osd.json` containing an `OcrOrientation` per input.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OcrOrientationParams {
    /// Image resolution.
    #[serde(default = "default_dpi")]
    pub dpi: u32,
}
