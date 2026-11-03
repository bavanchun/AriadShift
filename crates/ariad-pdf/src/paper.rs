//! Standard paper size table (`A0`..`A6`, `Letter`, `Legal`, `Tabloid`, `Ledger`) in PDF points
//! (`1 pt = 1/72 inch`) and parsing of the `paper` parameter type (`name` or `{w, h}`).

use ariad_core::tools::params::common::{CustomPaperSize, PaperSize, StandardPaper};
use serde_json::Value;

use crate::error::PdfError;

/// Largest page side dimension accepted in PDF points.
pub const MAX_PAPER_SIDE_PT: f64 = 1_000_000.0;

/// Resolved page width and height in PDF points (`1 pt = 1/72 inch`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PaperDimensions {
    /// Page width in PDF points (`> 0`).
    pub width_pt: f64,
    /// Page height in PDF points (`> 0`).
    pub height_pt: f64,
}

impl PaperDimensions {
    /// Constructs validated paper dimensions in PDF points.
    pub fn new(width_pt: f64, height_pt: f64) -> Result<Self, PdfError> {
        if !width_pt.is_finite()
            || !height_pt.is_finite()
            || width_pt <= 0.0
            || height_pt <= 0.0
            || width_pt > MAX_PAPER_SIDE_PT
            || height_pt > MAX_PAPER_SIDE_PT
        {
            return Err(PdfError::InvalidParams(format!(
                "paper dimensions ({width_pt} x {height_pt} pt) must be finite numbers in (0, {MAX_PAPER_SIDE_PT}]"
            )));
        }
        Ok(Self {
            width_pt,
            height_pt,
        })
    }

    /// Returns a `[0.0, 0.0, width_pt, height_pt]` PDF `/MediaBox` array.
    #[must_use]
    pub const fn media_box(self) -> [f64; 4] {
        [0.0, 0.0, self.width_pt, self.height_pt]
    }

    /// Returns a landscape-oriented copy (`width_pt >= height_pt`).
    #[must_use]
    pub const fn landscape(self) -> Self {
        if self.width_pt >= self.height_pt {
            self
        } else {
            Self {
                width_pt: self.height_pt,
                height_pt: self.width_pt,
            }
        }
    }

    /// Returns a portrait-oriented copy (`width_pt <= height_pt`).
    #[must_use]
    pub const fn portrait(self) -> Self {
        if self.width_pt <= self.height_pt {
            self
        } else {
            Self {
                width_pt: self.height_pt,
                height_pt: self.width_pt,
            }
        }
    }
}

/// Canonical table of standard paper sizes (`(name, width_pt, height_pt)`).
pub const PAPER_TABLE: &[(&str, f64, f64)] = &[
    ("A0", 2383.94, 3370.39),
    ("A1", 1683.78, 2383.94),
    ("A2", 1190.55, 1683.78),
    ("A3", 841.89, 1190.55),
    ("A4", 595.28, 841.89),
    ("A5", 419.53, 595.28),
    ("A6", 297.64, 419.53),
    ("Letter", 612.0, 792.0),
    ("Legal", 612.0, 1008.0),
    ("Tabloid", 792.0, 1224.0),
    ("Ledger", 1224.0, 792.0),
];

/// Looks up a standard paper size by name (case-insensitive).
#[must_use]
pub fn lookup_paper(name: &str) -> Option<PaperDimensions> {
    let trimmed = name.trim();
    PAPER_TABLE
        .iter()
        .find(|(canonical, _, _)| canonical.eq_ignore_ascii_case(trimmed))
        .map(|&(_, width_pt, height_pt)| PaperDimensions {
            width_pt,
            height_pt,
        })
}

/// Returns the dimensions in PDF points for a [`StandardPaper`] variant.
#[must_use]
pub const fn dimensions_for_standard(paper: StandardPaper) -> PaperDimensions {
    let (width_pt, height_pt) = paper.dimensions_pt();
    PaperDimensions {
        width_pt,
        height_pt,
    }
}

/// Resolves a [`PaperSize`] parameter (named standard paper or `{w, h}`) into [`PaperDimensions`].
pub fn resolve_paper_size(spec: &PaperSize) -> Result<PaperDimensions, PdfError> {
    match *spec {
        PaperSize::Named(standard) => Ok(dimensions_for_standard(standard)),
        PaperSize::Custom(CustomPaperSize { w, h }) => PaperDimensions::new(w, h),
    }
}

/// Parses a raw JSON value (`"A4"`, `"Letter"`, or `{"w": 612, "h": 792}`) into [`PaperDimensions`].
pub fn parse_paper(value: &Value) -> Result<PaperDimensions, PdfError> {
    if let Some(s) = value.as_str() {
        return lookup_paper(s)
            .ok_or_else(|| PdfError::InvalidParams(format!("unknown paper size {s:?}")));
    }
    let parsed: PaperSize = serde_json::from_value(value.clone())
        .map_err(|e| PdfError::InvalidParams(format!("invalid paper specification: {e}")))?;
    resolve_paper_size(&parsed)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn paper_table_lookup_and_parsing_cover_all_standard_and_custom_sizes() {
        let expected = [
            ("A0", 2383.94, 3370.39),
            ("A1", 1683.78, 2383.94),
            ("A2", 1190.55, 1683.78),
            ("A3", 841.89, 1190.55),
            ("A4", 595.28, 841.89),
            ("A5", 419.53, 595.28),
            ("A6", 297.64, 419.53),
            ("Letter", 612.0, 792.0),
            ("Legal", 612.0, 1008.0),
            ("Tabloid", 792.0, 1224.0),
            ("Ledger", 1224.0, 792.0),
        ];

        for (name, w, h) in expected {
            let dims = lookup_paper(name).unwrap_or_else(|| panic!("missing {name}"));
            assert_eq!(dims.width_pt, w);
            assert_eq!(dims.height_pt, h);
            assert_eq!(lookup_paper(&name.to_ascii_lowercase()), Some(dims));

            let from_json = parse_paper(&json!(name)).unwrap();
            assert_eq!(from_json, dims);
        }

        let custom = parse_paper(&json!({"w": 500.0, "h": 700.0})).unwrap();
        assert_eq!(custom.width_pt, 500.0);
        assert_eq!(custom.height_pt, 700.0);
        assert_eq!(custom.media_box(), [0.0, 0.0, 500.0, 700.0]);
        assert_eq!(
            custom.landscape(),
            PaperDimensions {
                width_pt: 700.0,
                height_pt: 500.0
            }
        );
        assert_eq!(
            custom.landscape().portrait(),
            PaperDimensions {
                width_pt: 500.0,
                height_pt: 700.0
            }
        );

        assert!(parse_paper(&json!("B99")).is_err());
        assert!(parse_paper(&json!({"w": -10.0, "h": 100.0})).is_err());
        assert!(parse_paper(&json!({"w": 100.0, "h": 0.0})).is_err());
    }
}
