//! Native engine execution and resource handling.
// On macOS, ld emits an informational warning when the test runner binary's
// `__eh_frame` section exceeds the 16 MB compact-unwind encoding threshold,
// which otherwise trips `CARGO_BUILD_WARNINGS=deny` via `#[warn(linker_messages)]`.
// This attribute is active only under `cfg(test)` because `linker_messages` can
// only be controlled at the root of a crate that needs to be linked.
#![cfg_attr(all(test, target_os = "macos"), allow(linker_messages))]
#![forbid(unsafe_code)]

pub mod archive;
pub mod assets;
pub mod commands;
pub mod confine;
pub mod convert;
pub mod engines;
pub mod ir_io;
pub(crate) mod media;
pub mod net;
pub mod package_meta;
pub mod packs;
pub mod pandoc_bin;
pub mod runner;
pub mod sandbox;
pub mod session;
pub mod tools;
#[cfg(feature = "typst")]
pub mod typst_render;
pub mod watchdog;
pub mod workspace;

pub use convert::{ArchiveToIrOutput, read_archive_to_ir};
pub use pandoc_bin::{PANDOC_GOLDEN_VERSION, PANDOC_SUPPORTED};
