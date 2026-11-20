//! The integration tests of `ariad-host`, linked as one test binary.
//!
//! Each former test target is a module here; run one with, for example,
//! `cargo nextest run -p ariad-host --features ariad-host/test-probe --test it -E 'test(/^packs::/)'`
//! or `cargo test -p ariad-host --features test-probe --test it packs::`. Several tests re-run this
//! binary with `--exact <module>::<test>`, so a child name always carries its module path.
//!
//! Invariant: no test may apply a sandbox (seccomp, Landlock, Seatbelt), change the working
//! directory, or set environment variables in its own process, because every module shares one
//! process under `cargo test` (nextest runs one process per test, `cargo test` runs the tests of
//! this binary as threads). A test that needs process-wide state re-runs this binary as a child
//! (`--exact <module>::<test>`, with the parent asserting through `support::assert_ran_one_test`
//! that the child ran exactly one test) or lives in its own target.

mod support;

mod assets;
mod compress_similarity;
mod confine;
mod deep_list_stack;
mod docx_styles;
mod edit_document_tools;
mod engine_event_rules;
mod engine_json_scrub;
mod engine_program_resolver;
mod fidelity;
mod font_subset_similarity;
mod ir_io;
mod ir_validation;
mod ir_version_normalization;
mod net_property;
mod ocr_pipeline;
mod pack_extract_confinement;
mod pack_resolve_verify;
mod pack_schema;
mod package_meta_record_count;
mod packs;
mod pdfium_pack;
mod pdfium_sandbox;
mod qpdf_adapter;
mod qpdf_secrets;
mod redact_pipeline;
mod runner;
mod sandbox;
mod sandbox_macos;
mod sandbox_refusal_macos;
mod session;
mod sign_adapter;
mod tesseract_adapter;
mod tool_publish_atomic;
mod tool_runner_limits;
mod tool_secrets;
mod typst_engine;
mod watch_folder;
mod workspace;
