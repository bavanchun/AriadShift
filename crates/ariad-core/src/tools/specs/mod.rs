//! Per-file tool specification lists concatenated into the global tool registry.

pub mod annotate_forms;
pub mod convert_cbz;
pub mod convert_tables;
pub mod ocr_scan;
pub mod optimize;
pub mod organize_document;
pub mod organize_geometry;
pub mod organize_info;
pub mod organize_layout;
pub mod organize_pages;
pub mod organize_sequence;
pub mod overlay_color;
pub mod overlay_furniture;
pub mod overlay_marks;
pub mod overlay_watermark_removal;
pub mod pdfium;
pub mod redact;
pub mod security;
pub mod sign;
pub mod web;

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
    list.extend(overlay_color::specs());
    list.extend(overlay_marks::specs());
    list.extend(overlay_furniture::specs());
    list.extend(overlay_watermark_removal::specs());
    list.extend(pdfium::specs());
    list.extend(optimize::specs());
    list.extend(security::specs());
    list.extend(ocr_scan::specs());
    list.extend(annotate_forms::specs());
    list.extend(redact::specs());
    list.extend(sign::specs());
    list.extend(convert_tables::specs());
    list.extend(convert_cbz::specs());
    list.extend(web::specs());
    list
}
