//! Tool specifications for document-level structure and metadata tools (`pdf.metadata`,
//! `pdf.outline`, `pdf.attachments`, `pdf.page_labels`, `pdf.viewer_prefs`, `pdf.portfolio`,
//! `pdf.layers`).

use crate::tools::ToolSpec;

/// Returns the tool specifications registered by this module.
#[must_use]
pub fn specs() -> Vec<ToolSpec> {
    Vec::new()
}
