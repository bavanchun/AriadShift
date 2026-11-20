//! All integration tests of `ariad-core` compile into this one test target.

mod support;

mod deep_list_stack;
mod docling_to_ir;
mod editor_roundtrip;
mod fidelity;
mod html_reader;
mod html_reader_bounds;
mod ir_snapshots;
mod ir_styles;
mod ir_validation;
mod package_meta_entry_names;
mod package_meta_record_count;
mod page_range;
mod pandoc_to_ir;
mod pdf_reader;
mod pdf_reader_bounds;
mod pdf_ruled_tables;
mod pdf_styles;
mod pdf_unruled_tables;
mod planner;
mod reader_styles;
mod recipe_roundtrip;
mod schema_drift;
mod tool_access;
mod tools_registry;
mod typst_writer;
mod writer_bounds;
mod writer_roundtrip;
mod writer_strip_html;
mod writer_styles;
