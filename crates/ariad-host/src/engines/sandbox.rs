//! Extra filesystem access each out-of-process engine needs inside the Linux sandbox.
//!
//! Every engine already gets the base system allow-list, execute access to its own program and
//! read-write access to its job workspace (see [`crate::sandbox::SandboxPolicy::for_engine`]).
//! An engine that needs more, such as an install root or a helper binary, declares it here.

use std::path::{Path, PathBuf};

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
pub const ENGINE_PATHS: &[(&str, EnginePathsFn)] = &[
    ("pandoc", pandoc_paths),
    ("docling", docling_paths),
    ("probe", EnginePaths::default),
];

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

/// Docling executes system Tesseract and, in development, reads an editable Python environment.
fn docling_paths() -> EnginePaths {
    let mut paths = EnginePaths::default();
    if let Some(path) = std::env::var_os("PATH").and_then(|value| {
        std::env::split_paths(&value)
            .map(|dir| dir.join("tesseract"))
            .find_map(|file| file.canonicalize().ok())
    }) {
        grant_tesseract(&mut paths, &path);
    }
    if std::env::var_os(super::docling::OVERRIDE_ENV).is_some()
        && let Some(command) = super::docling::resolve()
    {
        // Grant the venv, its base interpreter, and the adjacent model/data directories.
        if let Some(prefix) = command.program.parent().and_then(std::path::Path::parent) {
            paths.exec.extend(prefix.canonicalize().ok());
            if let Some(root) = prefix.parent() {
                for name in ["models", "tessdata"] {
                    paths.read_only.extend(root.join(name).canonicalize().ok());
                }
            }
        }
        if let Ok(program) = command.program.canonicalize()
            && let Some(prefix) = program.parent().and_then(std::path::Path::parent)
        {
            paths.exec.push(prefix.to_path_buf());
        }
        grant_editable_source(&mut paths, &command.program);
    }
    paths
}

/// Libraries and data may live beside a non-system Tesseract binary. Keep execution limited
/// to the binary itself and read access to the prefix's library, data and configuration dirs.
fn grant_tesseract(paths: &mut EnginePaths, program: &Path) {
    paths.exec.push(program.to_path_buf());
    if let Some(bin) = program.parent()
        && bin.file_name().is_some_and(|name| name == "bin")
        && let Some(prefix) = bin.parent()
    {
        for name in ["lib", "lib64", "libexec", "share/tessdata", "etc"] {
            paths.read_only.push(prefix.join(name));
        }
    }
}

/// uv writes the editable package's source directory into ariad_docling.pth. Resolve it from
/// the override's environment so release binaries carry no builder checkout path.
fn grant_editable_source(paths: &mut EnginePaths, program: &Path) {
    let Some(prefix) = program.parent().and_then(Path::parent) else {
        return;
    };
    for relative in ["lib/python3.14/site-packages", "Lib/site-packages"] {
        let site = prefix.join(relative);
        if let Ok(contents) = std::fs::read_to_string(site.join("ariad_docling.pth")) {
            for line in contents.lines().map(str::trim) {
                if !line.is_empty()
                    && !line.starts_with('#')
                    && !line.starts_with("import ")
                    && !line.starts_with("import\t")
                {
                    paths.read_only.extend(site.join(line).canonicalize().ok());
                }
            }
        }
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

    #[test]
    fn editable_source_follows_the_override_environment() {
        let root = tempfile::tempdir().expect("relocated checkout");
        let prefix = root.path().join("python-env");
        let site = prefix.join("lib/python3.14/site-packages");
        std::fs::create_dir_all(&site).expect("site packages");
        let source = root.path().join("engine source");
        std::fs::create_dir(&source).expect("editable source");
        std::fs::write(site.join("ariad_docling.pth"), source.to_str().unwrap()).unwrap();
        let mut paths = super::EnginePaths::default();
        super::grant_editable_source(&mut paths, &prefix.join("bin/python"));
        assert_eq!(paths.read_only, vec![source.canonicalize().unwrap()]);
        assert!(paths.exec.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn tesseract_grants_only_installation_data_directories() {
        use std::path::Path;

        let home = tempfile::tempdir().expect("home directory");
        for prefix in [
            Path::new("/"),
            home.path(),
            &home.path().join(".local"),
            Path::new("/opt/tesseract"),
        ] {
            let program = prefix.join("bin/tesseract");
            let mut paths = super::EnginePaths::default();
            super::grant_tesseract(&mut paths, &program);
            assert_eq!(paths.exec, vec![program]);
            assert_eq!(
                paths.read_only,
                ["lib", "lib64", "libexec", "share/tessdata", "etc"].map(|name| prefix.join(name))
            );
            assert!(!paths.read_only.iter().any(|path| {
                path == Path::new("/")
                    || path == home.path()
                    || path == prefix
                    || path == &prefix.join("share")
                    || path == &home.path().join(".local/share")
            }));
        }
    }
}
