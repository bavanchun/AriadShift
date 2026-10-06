//! Native engine execution and resource handling.
#![forbid(unsafe_code)]

pub mod archive;
pub mod assets;
pub mod convert;
pub mod docx_meta;
pub mod engines;
pub mod ir_io;
pub mod media;
pub mod pandoc_bin;
pub mod runner;
pub mod workspace;

pub use convert::read_archive_to_ir;
pub use pandoc_bin::{PANDOC_GOLDEN_VERSION, PANDOC_SUPPORTED};
