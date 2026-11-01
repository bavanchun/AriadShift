//! Tool specifications for page geometry and stream tools (`pdf.rotate`, `pdf.crop`,
//! `pdf.scale`, `pdf.single_page`, `pdf.rotate_content`, `pdf.decompress`).

use crate::tools::ToolSpec;

/// Returns the tool specifications registered by this module.
#[must_use]
pub fn specs() -> Vec<ToolSpec> {
    Vec::new()
}
