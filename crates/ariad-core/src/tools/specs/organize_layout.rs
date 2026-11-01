//! Tool specifications for multi-page layout and imposition tools (`pdf.nup`, `pdf.booklet`,
//! `pdf.overlay`, `pdf.divide_pages`, `pdf.posterize`).

use crate::tools::ToolSpec;

/// Returns the tool specifications registered by this module.
#[must_use]
pub fn specs() -> Vec<ToolSpec> {
    Vec::new()
}
