//! Private PDFium evidence protocol for the host redaction pipeline.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::tools::PageRange;

/// Extracts original page text in PDFium character order and raw page coordinates.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RedactInspectParams {
    /// One-based pages to inspect.
    #[serde(default = "PageRange::all")]
    pub pages: PageRange,
}

/// Private evidence, containing original text. Never use in logs or user reports.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RedactGeometry {
    /// Evidence format identifier.
    pub schema: String,
    /// SHA-256 of the exact original input bytes.
    pub source_sha256: [u8; 32],
    /// Total number of pages in the source.
    pub page_count: usize,
    /// Selected page evidence.
    pub pages: Vec<RedactGeometryPage>,
}

/// Page dimensions and extracted characters.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RedactGeometryPage {
    /// One-based source page index.
    pub page: u32,
    /// Page width in PDF points as reported by PDFium.
    pub width: f64,
    /// Page height in PDF points as reported by PDFium.
    pub height: f64,
    /// Clockwise page rotation.
    pub rotation: u16,
    /// Characters in PDFium's extraction order.
    pub chars: Vec<RedactGeometryChar>,
}

/// One extracted character and its PDFium geometry.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RedactGeometryChar {
    /// Unicode value, including whitespace and generated separators.
    pub value: char,
    /// Character index in the PDFium text page.
    pub index: usize,
    /// Whether PDFium synthesized this character rather than reading a glyph.
    pub generated: bool,
    /// Baseline origin in PDF points, when available.
    pub origin: Option<[f64; 2]>,
    /// Bounding rectangle in PDF points, when available.
    pub bbox: Option<[f64; 4]>,
}

/// Outcome of one mandatory verification check.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RedactionCheckStatus {
    /// The check completed and found no violation.
    Pass,
    /// The check found a violation.
    Fail,
    /// The check could not establish its claim.
    Unavailable,
}

/// Summary of the redaction evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RedactionVerificationStatus {
    /// Every mandatory check passed for every carrier.
    Verified,
    /// The request excluded one or more carriers.
    Partial,
    /// Evidence or a mandatory check is missing.
    Unverifiable,
    /// At least one completed check found a violation.
    Failed,
}

/// Named mandatory checks; none contains a target value or extracted text.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RedactionChecks {
    /// PDFium text extraction.
    pub pdfium_text: RedactionCheckStatus,
    /// Independent core text extraction.
    pub core_text: RedactionCheckStatus,
    /// Opaque rendering of every affected region.
    pub region_render: RedactionCheckStatus,
    /// Removal of original image pixels and masks.
    pub image_pixels: RedactionCheckStatus,
    /// Bounded decoded graph scan.
    pub object_graph: RedactionCheckStatus,
    /// Canonical rewrite and absence of historical revisions.
    pub rewrite_history: RedactionCheckStatus,
}

impl RedactionChecks {
    /// Derives the only status justified by the checks and original evidence.
    #[must_use]
    pub fn status(
        &self,
        complete_scope: bool,
        original_evidence_required: bool,
        has_original_evidence: bool,
    ) -> RedactionVerificationStatus {
        use RedactionCheckStatus::{Fail, Unavailable};
        let checks = [
            self.pdfium_text,
            self.core_text,
            self.region_render,
            self.image_pixels,
            self.object_graph,
            self.rewrite_history,
        ];
        if checks.contains(&Fail) {
            RedactionVerificationStatus::Failed
        } else if original_evidence_required && !has_original_evidence {
            RedactionVerificationStatus::Unverifiable
        } else if !complete_scope {
            RedactionVerificationStatus::Partial
        } else if checks.contains(&Unavailable) {
            RedactionVerificationStatus::Unverifiable
        } else {
            RedactionVerificationStatus::Verified
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn passed() -> RedactionChecks {
        RedactionChecks {
            pdfium_text: RedactionCheckStatus::Pass,
            core_text: RedactionCheckStatus::Pass,
            region_render: RedactionCheckStatus::Pass,
            image_pixels: RedactionCheckStatus::Pass,
            object_graph: RedactionCheckStatus::Pass,
            rewrite_history: RedactionCheckStatus::Pass,
        }
    }

    #[test]
    fn verified_requires_every_check_full_scope_and_evidence() {
        use RedactionVerificationStatus::{Failed, Partial, Unverifiable, Verified};
        let mut checks = passed();
        assert_eq!(checks.status(true, true, true), Verified);
        assert_eq!(checks.status(false, true, true), Partial);
        assert_eq!(checks.status(false, true, false), Unverifiable);
        checks.image_pixels = RedactionCheckStatus::Unavailable;
        assert_eq!(checks.status(true, false, false), Unverifiable);
        checks.object_graph = RedactionCheckStatus::Fail;
        assert_eq!(checks.status(false, true, false), Failed);
    }
}
