//! Native engine execution and resource handling.
#![forbid(unsafe_code)]

pub mod archive;
pub mod assets;
pub mod commands;
pub mod confine;
pub mod convert;
pub mod engines;
pub mod ir_io;
pub(crate) mod media;
pub mod package_meta;
pub mod pandoc_bin;
pub mod runner;
pub mod workspace;

pub use convert::{ArchiveToIrOutput, read_archive_to_ir};
pub use pandoc_bin::{PANDOC_GOLDEN_VERSION, PANDOC_SUPPORTED};
