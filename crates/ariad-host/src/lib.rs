//! Native engine execution and resource handling.
#![forbid(unsafe_code)]

pub mod assets;
pub mod ir_io;
pub mod pandoc_bin;
pub mod runner;
pub mod workspace;

pub use pandoc_bin::{PANDOC_GOLDEN_VERSION, PANDOC_SUPPORTED};
