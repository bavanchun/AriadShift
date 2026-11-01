//! PDFium engine for AriadShift, served over `ariad-engine/2` (`op = "run_tool"`).
//!
//! The engine is a stateless process: it reads one request, runs one tool against the job's
//! workspace and exits. PDFium itself is loaded at run time from the pack directory (see
//! [`library`]); this crate contains no `unsafe` code.
//!
//! Each tool lives in its own module and receives a [`output::ToolContext`] with the validated
//! request, the resource limits and the helpers that emit progress, warnings and artifacts.

#![forbid(unsafe_code)]

pub mod blank;
pub mod crop_whitespace;
pub mod detect_scanned;
pub mod dispatch;
pub mod extract_images;
pub mod library;
pub mod output;
pub mod render;
pub mod text_layout;
pub mod thumbnails;

/// Engine identifier, equal to the pack name and to `ToolSpec::engine` of the tools it runs.
pub const ENGINE_ID: &str = "pdfium";
