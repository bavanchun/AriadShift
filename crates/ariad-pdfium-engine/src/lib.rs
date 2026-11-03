//! PDFium engine for AriadShift, served over `ariad-engine/2` (`op = "run_tool"`).
//!
//! The engine is a stateless process: it reads one request, runs one tool against the job's
//! workspace and exits. PDFium itself is loaded at run time from the pack directory (see
//! [`library`]); this crate contains no `unsafe` code.
//!
//! Each tool lives in its own module and receives a [`output::ToolContext`] with the validated
//! request, the resource limits and the helpers that emit progress, warnings and artifacts.

// On macOS, ld emits an informational warning when the unstripped binary's
// `__eh_frame` section exceeds the 16 MB compact-unwind encoding threshold,
// which otherwise trips `CARGO_BUILD_WARNINGS=deny` via `#[warn(linker_messages)]`.
#![cfg_attr(target_os = "macos", allow(linker_messages))]
#![forbid(unsafe_code)]

pub mod blank;
mod canonical;
mod content_writer;
pub mod crop_whitespace;
pub mod detect_scanned;
pub mod dispatch;
mod execution_budget;
pub mod extract_images;
pub mod forms_detect;
pub mod library;
pub mod output;
pub mod render;
pub mod scan;
pub mod text_layout;
pub mod thumbnails;

/// Engine identifier, equal to the pack name and to `ToolSpec::engine` of the tools it runs.
pub const ENGINE_ID: &str = "pdfium";
