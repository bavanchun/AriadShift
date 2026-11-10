//! A disposable pack containing the real PDFium engine and the configured native library.

use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
};
use tempfile::TempDir;

pub struct PdfiumPack(TempDir);

pub fn invalid_local_plans() -> Vec<std::ffi::OsString> {
    let values = ["", "  ", "invalid", "PRO"].map(std::ffi::OsString::from);
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        values
            .into_iter()
            .chain([std::ffi::OsString::from_vec(vec![0xff])])
            .collect()
    }
    #[cfg(not(unix))]
    {
        values.into()
    }
}

impl PdfiumPack {
    pub fn new() -> Option<Self> {
        let Some(library_dir) = std::env::var_os("ARIAD_TEST_PDFIUM_DIR") else {
            eprintln!("skipping page edit integration: ARIAD_TEST_PDFIUM_DIR is not set");
            return None;
        };
        let suffix = std::env::consts::EXE_SUFFIX;
        let binary = Path::new(env!("CARGO_BIN_EXE_ashift"))
            .with_file_name(format!("ariad-pdfium-engine{suffix}"));
        assert!(
            binary.is_file(),
            "build ariad-pdfium-engine before page edit integration tests"
        );
        let pack = Self(tempfile::tempdir().unwrap());
        let program = format!("ariad-pdfium-engine{suffix}");
        fs::copy(binary, pack.path().join(&program)).unwrap();
        let library = if cfg!(target_os = "windows") {
            "pdfium.dll"
        } else if cfg!(target_os = "macos") {
            "libpdfium.dylib"
        } else {
            "libpdfium.so"
        };
        fs::copy(
            PathBuf::from(library_dir).join(library),
            pack.path().join(library),
        )
        .unwrap();
        let triple = &ariad_host::packs::host_targets()[0];
        let mut manifest = format!(
            r#"schema = "ariad-pack/1"
name = "pdfium"
version = "0.0.0"
license = "BSD-3-Clause AND Apache-2.0"
requires_ashift = ">=0.1.0"
engine_protocol = "ariad-engine/2"
[[targets]]
triple = "{triple}"
entrypoint = ["{program}"]
"#
        );
        for name in [program.as_str(), library] {
            let bytes = fs::read(pack.path().join(name)).unwrap();
            manifest.push_str(&format!(
                r#"
[[targets.components]]
name = "{name}"
url = "{name}"
sha256 = "{}"
size = {}
unpack = "file"
executable = {}
"#,
                hex::encode(Sha256::digest(&bytes)),
                bytes.len(),
                name == program
            ));
        }
        fs::write(pack.path().join("pack.toml"), manifest).unwrap();
        ariad_host::packs::resolve_override(
            "pdfium",
            pack.path(),
            &ariad_host::packs::host_targets(),
        )
        .unwrap();
        Some(pack)
    }

    pub fn path(&self) -> &Path {
        self.0.path()
    }
}
