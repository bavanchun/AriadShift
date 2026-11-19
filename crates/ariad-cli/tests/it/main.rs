//! Every integration test of `ariad-cli` lives in one test target, one module per file.
//!
//! A single target links once instead of once per file. Filter by module path:
//! `cargo test -p ariad-cli --test it docling_pack::` or
//! `cargo nextest run -p ariad-cli -E 'binary(it) & test(/^docling_pack::/)'`.

mod support;

mod automation_ai_cli;
mod builtin_engine_contracts;
mod capabilities_coverage;
mod cli_smoke;
mod clock_cli;
mod commands_cli;
mod compare_cli;
mod compliance_cli;
mod convert_batch_cli;
mod convert_cli;
mod docling_engine;
mod docling_pack;
mod docling_routes;
mod doctor_sandbox_cli;
mod docx_golden;
mod edit_cli;
mod engine_conformance;
mod engine_conformance_v2;
mod fixture_manifest;
mod forms_annotations_cli;
mod fuzz_seed_manifest;
mod languages_cli;
mod mcp_secrets;
mod mcp_stdio;
mod office_routes;
mod package_meta_large_media;
mod packs_cli;
mod pandoc_engine;
mod pdf_routes;
mod pdf_tables_in_process;
mod pdfium_tools_cli;
mod reader_golden;
mod release_guard;
mod route_golden;
mod sandbox_cli;
mod secrets_surfaces;
mod termination;
mod tools_cli;
mod tools_structural;
mod verapdf_pack;
mod web_to_pdf_cli;
