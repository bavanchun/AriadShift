//! Extra filesystem access each out-of-process engine needs inside the Linux sandbox.
//!
//! Every engine already gets the base system allow-list, execute access to its own program and
//! read-write access to its job workspace (see [`crate::sandbox::SandboxPolicy::for_engine`]).
//! An engine that needs more, such as an install root or a helper binary, declares it here.

use std::path::PathBuf;

use crate::pandoc_bin;

/// Paths an engine needs beyond the base allow-list, its program and its workspace.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct EnginePaths {
    /// Paths the engine may read (data files, configuration).
    pub read_only: Vec<PathBuf>,
    /// Paths the engine may read and execute (install roots, helper programs).
    pub exec: Vec<PathBuf>,
}

/// Returns the extra paths of one engine.
pub type EnginePathsFn = fn() -> EnginePaths;

/// The per-engine declaration, keyed by engine id. Add one entry per new engine.
pub const ENGINE_PATHS: &[(&str, EnginePathsFn)] =
    &[("pandoc", pandoc_paths), ("probe", EnginePaths::default)];

/// Extra paths for `engine`; an engine without an entry gets none.
#[must_use]
pub fn paths_for(engine: &str) -> EnginePaths {
    ENGINE_PATHS
        .iter()
        .find(|(id, _)| *id == engine)
        .map(|(_, paths)| paths())
        .unwrap_or_default()
}

/// The Pandoc engine runs the Pandoc binary found through `ASHIFT_PANDOC` or `PATH`. Only that
/// file is granted: release builds are static and distribution builds keep their libraries
/// under `/usr`, which the base allow-list covers.
fn pandoc_paths() -> EnginePaths {
    EnginePaths {
        read_only: Vec::new(),
        exec: pandoc_bin::resolve_path()
            .and_then(|path| std::fs::canonicalize(path).ok())
            .into_iter()
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::{ENGINE_PATHS, paths_for};

    #[test]
    fn engine_ids_are_unique_and_unknown_engines_get_nothing() {
        let mut ids: Vec<&str> = ENGINE_PATHS.iter().map(|(id, _)| *id).collect();
        ids.sort_unstable();
        let count = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), count, "duplicate engine id in ENGINE_PATHS");
        assert_eq!(paths_for("no-such-engine"), super::EnginePaths::default());
        assert_eq!(paths_for("probe"), super::EnginePaths::default());
    }
}
