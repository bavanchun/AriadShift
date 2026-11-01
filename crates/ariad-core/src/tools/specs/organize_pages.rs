//! Tool specifications for page-level organization tools (`pdf.merge`, `pdf.split`,
//! `pdf.extract_pages`, `pdf.delete_pages`, `pdf.reorder`, `pdf.alternate_mix`,
//! `pdf.insert_blank`).

use crate::tools::ToolSpec;

/// Returns the tool specifications registered by this module.
#[must_use]
pub fn specs() -> Vec<ToolSpec> {
    Vec::new()
}
