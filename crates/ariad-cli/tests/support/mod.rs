#![allow(dead_code)]

pub mod fixtures;
pub mod protocol;

use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::OnceLock,
};

/// Path to the `engine-probe` test binary beside the `ashift` binary under test.
///
/// The probe lives in `ariad-host`, so Cargo does not build it for this package's
/// tests. When it is missing, build it once into the same target directory that
/// produced `ashift`, and fail loudly if that build does not succeed.
pub fn engine_probe_path() -> PathBuf {
    static PROBE: OnceLock<PathBuf> = OnceLock::new();
    PROBE.get_or_init(build_engine_probe).clone()
}

fn build_engine_probe() -> PathBuf {
    let ashift = Path::new(env!("CARGO_BIN_EXE_ashift"));
    let probe = ashift.with_file_name(format!("engine-probe{}", std::env::consts::EXE_SUFFIX));
    if probe.exists() {
        return probe;
    }
    // `<target dir>/<profile dir>/ashift`
    let profile_dir = ashift
        .parent()
        .expect("ashift binary has a profile directory");
    let target_dir = profile_dir
        .parent()
        .expect("profile directory has a target directory");
    let profile = match profile_dir.file_name().and_then(|name| name.to_str()) {
        Some("debug") | None => "dev",
        Some(name) => name,
    };
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let status = Command::new(cargo)
        .args([
            "build",
            "-p",
            "ariad-host",
            "--bin",
            "engine-probe",
            "--features",
            "ariad-host/test-probe",
            "--profile",
            profile,
        ])
        .env("CARGO_TARGET_DIR", target_dir)
        .status()
        .expect("failed to spawn cargo to build engine-probe");
    assert!(status.success(), "building engine-probe failed: {status}");
    assert!(
        probe.exists(),
        "engine-probe was built but not found at {}",
        probe.display()
    );
    probe
}
