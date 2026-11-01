//! Locating and loading libpdfium.
//!
//! The library ships in the pack next to the engine binary, either in the same directory or in
//! `../lib/`. The engine never reads an environment variable to find it; tests pass an explicit
//! directory to [`bind_in`].

use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

use ariad_core::protocol::ErrorCode;
use ariad_engine_io::EngineFailure;
use pdfium_render::prelude::{Pdfium, PdfiumError};

/// File name of the PDFium shared library on this platform (`libpdfium.so`, `libpdfium.dylib`).
#[must_use]
pub fn library_file_name() -> OsString {
    Pdfium::pdfium_platform_library_name()
}

/// Directories searched for libpdfium, in order, for an engine executable in `exe_dir`: the
/// directory itself, then `../lib/` beside it.
#[must_use]
pub fn search_dirs(exe_dir: &Path) -> Vec<PathBuf> {
    let mut dirs = vec![exe_dir.to_path_buf()];
    if let Some(parent) = exe_dir.parent() {
        dirs.push(parent.join("lib"));
    }
    dirs
}

/// Returns the libpdfium file inside `dir`, if it holds a regular file of the expected name.
#[must_use]
pub fn find_in(dir: &Path) -> Option<PathBuf> {
    let candidate = dir.join(library_file_name());
    std::fs::metadata(&candidate)
        .is_ok_and(|meta| meta.is_file())
        .then_some(candidate)
}

/// Locates libpdfium beside the running engine executable.
pub fn locate_default() -> Result<PathBuf, EngineFailure> {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
        .ok_or_else(missing)?;
    search_dirs(&exe_dir)
        .iter()
        .find_map(|dir| find_in(dir))
        .ok_or_else(missing)
}

/// Loads libpdfium from `dir`.
///
/// PDFium binds once per process: a later call returns a handle to the library loaded first,
/// whatever `dir` says, so every call in one process must name the same directory.
pub fn bind_in(dir: &Path) -> Result<Pdfium, EngineFailure> {
    let path = find_in(dir).ok_or_else(missing)?;
    bind_file(&path)
}

/// Loads libpdfium from the engine's own directory or `../lib/` beside it.
pub fn bind_default() -> Result<Pdfium, EngineFailure> {
    bind_file(&locate_default()?)
}

fn bind_file(path: &Path) -> Result<Pdfium, EngineFailure> {
    match Pdfium::bind_to_library(path) {
        Ok(bindings) => Ok(Pdfium::new(bindings)),
        // `Pdfium::default` first tries to bind, which reports "already initialized" and then
        // reuses the existing bindings without searching the working directory or the system.
        // It is only called here, after that exact answer.
        Err(PdfiumError::PdfiumLibraryBindingsAlreadyInitialized) => Ok(Pdfium::default()),
        Err(_) => Err(EngineFailure::new(
            ErrorCode::ToolMissing,
            "libpdfium could not be loaded.",
        )),
    }
}

fn missing() -> EngineFailure {
    EngineFailure::new(
        ErrorCode::ToolMissing,
        "libpdfium was not found next to the engine. Install the pdfium pack.",
    )
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn search_dirs_are_the_executable_directory_then_the_sibling_lib() {
        assert_eq!(
            search_dirs(Path::new("/opt/pack/bin")),
            [
                PathBuf::from("/opt/pack/bin"),
                PathBuf::from("/opt/pack/lib")
            ]
        );
        assert_eq!(search_dirs(Path::new("/")), [PathBuf::from("/")]);
    }

    #[test]
    fn find_in_requires_a_regular_file_with_the_platform_name() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(find_in(dir.path()), None);

        let as_dir = dir.path().join(library_file_name());
        fs::create_dir(&as_dir).expect("create directory with the library name");
        assert_eq!(find_in(dir.path()), None);
        fs::remove_dir(&as_dir).expect("remove directory");

        fs::write(&as_dir, b"not a real library").expect("write file");
        assert_eq!(find_in(dir.path()), Some(as_dir));
    }

    #[test]
    fn bind_in_reports_a_missing_library_as_tool_missing() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(
            bind_in(dir.path()).expect_err("no library").code,
            ErrorCode::ToolMissing
        );
    }

    #[test]
    fn bind_in_reports_an_unloadable_file_as_tool_missing() {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::write(dir.path().join(library_file_name()), b"not a real library").expect("write file");
        assert_eq!(
            bind_in(dir.path()).expect_err("not loadable").code,
            ErrorCode::ToolMissing
        );
    }
}
