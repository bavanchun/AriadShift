//! Per-file tool specification lists concatenated into the global tool registry.

pub mod organize_document;
pub mod organize_geometry;
pub mod organize_info;
pub mod organize_layout;
pub mod organize_pages;
pub mod organize_sequence;
pub mod pdfium;

use crate::tools::ToolSpec;

/// Returns all tool specifications across all per-module registration lists in deterministic order.
#[must_use]
pub fn all_specs() -> Vec<ToolSpec> {
    let mut list = Vec::new();
    list.extend(organize_info::specs());
    list.extend(organize_pages::specs());
    list.extend(organize_sequence::specs());
    list.extend(organize_geometry::specs());
    list.extend(organize_layout::specs());
    list.extend(organize_document::specs());
    list.extend(pdfium::specs());
    list
}
