//! Engine packs: signed, downloadable engines installed under the data directory.
//!
//! A pack is a minisign-signed `ariad-pack/1` manifest plus per-target component files. The
//! host verifies the manifest signature against keys compiled into `ashift`, checks every
//! component's size and SHA-256, extracts it under strict path rules, and records a receipt.
//! [`resolve`] turns an installed pack (or a development override) into an engine command.

mod author;
mod extract;
mod fetch;
mod index;
mod install;
pub mod manifest;
mod signature;
mod zstd;

use std::{
    ffi::OsString,
    io,
    path::{Path, PathBuf},
};

use serde_json::{Map, Value};
use thiserror::Error;

pub use author::{ManifestRequest, generate_manifest};
#[doc(hidden)]
pub use extract::extract_for_fuzzing;
pub use extract::{Stats, scan};
pub use fetch::FetchPolicy;
pub use index::{IndexEntry, entries as index_entries, lookup as index_lookup};
pub use install::{
    InstallReport, InstallRequest, InstallSource, InstallStatus, InstalledVersion, PackEvent,
    PackHealth, RemoveReport, VerifyReport, check, install, list_installed, remove, verify,
};
pub use manifest::{
    ALLOWED_ENV, Component, MANIFEST_FILE, MANIFEST_SCHEMA, Manifest, SIGNATURE_FILE, Target,
    Unpack,
};
pub use signature::{SignatureError, TrustedKey, TrustedKeys};

use manifest::{expand_json, expand_os, relative_path};

/// Environment variable that relocates the data directory.
pub const DATA_DIR_ENV: &str = "ASHIFT_DATA_DIR";
/// Prefix of the development override `ASHIFT_PACK_<NAME>=<dir>`.
pub const OVERRIDE_ENV_PREFIX: &str = "ASHIFT_PACK_";

/// A pack operation failure.
#[derive(Debug, Error)]
pub enum PackError {
    #[error("unknown engine pack '{name}'; known packs: {known}")]
    UnknownPack { name: String, known: String },
    #[error("engine pack '{name}'{} is not installed", version.as_ref().map(|v| format!(" {v}")).unwrap_or_default())]
    NotInstalled {
        name: String,
        version: Option<String>,
    },
    #[error(
        "engine pack {name} {version} has no build for this platform ({host}); available targets: {available}"
    )]
    UnsupportedTarget {
        name: String,
        version: String,
        host: String,
        available: String,
    },
    #[error(
        "engine pack {name} {version} requires ashift {requires}, but this is ashift {current}; install a matching ashift or pack version"
    )]
    IncompatibleAshift {
        name: String,
        version: String,
        requires: String,
        current: String,
    },
    #[error("invalid engine pack manifest: {0}")]
    InvalidManifest(String),
    #[error("pack signature verification failed: {0}")]
    Signature(#[from] SignatureError),
    #[error("pack mismatch: expected {expected}, but the manifest describes {found}")]
    Mismatch { expected: String, found: String },
    #[error("download failed for {url}: {detail}")]
    Download { url: String, detail: String },
    #[error(
        "component '{component}' failed the size check: expected {expected} bytes, received {actual}"
    )]
    SizeMismatch {
        component: String,
        expected: u64,
        actual: u64,
    },
    #[error(
        "component '{component}' failed the SHA-256 check: expected {expected}, computed {actual}"
    )]
    HashMismatch {
        component: String,
        expected: String,
        actual: String,
    },
    #[error("unsafe archive entry '{entry}' in component '{component}': {reason}")]
    UnsafeArchive {
        component: String,
        entry: String,
        reason: &'static str,
    },
    #[error("component '{component}' unpacks beyond its declared {what} (limit {limit})")]
    ExtractLimit {
        component: String,
        what: &'static str,
        limit: u64,
    },
    #[error("corrupt archive in component '{component}': {detail}")]
    CorruptArchive { component: String, detail: String },
    #[error("engine pack {name} {version} failed verification: {detail}")]
    VerificationFailed {
        name: String,
        version: String,
        detail: String,
    },
    #[error("invalid install receipt for {name}: {detail}")]
    InvalidReceipt { name: String, detail: String },
    #[error("another ashift process is installing or removing engine pack '{name}'")]
    Busy { name: String },
    #[error("the engine pack data directory is unavailable: {0}")]
    DataDir(String),
    #[error("{context}: {source}")]
    Io {
        context: String,
        #[source]
        source: io::Error,
    },
    #[error("the pack operation was interrupted")]
    Interrupted,
    #[error("{0}")]
    Usage(String),
}

impl PackError {
    pub(crate) fn io(action: &str, path: &Path, source: io::Error) -> Self {
        Self::Io {
            context: format!("{action} {}", path.display()),
            source,
        }
    }

    /// Process exit code, following the CLI exit-code table.
    #[must_use]
    pub const fn exit_code(&self) -> u8 {
        match self {
            Self::Usage(_) => 2,
            Self::UnknownPack { .. }
            | Self::NotInstalled { .. }
            | Self::UnsupportedTarget { .. } => 3,
            Self::ExtractLimit { .. } => 4,
            Self::IncompatibleAshift { .. } => 5,
            Self::Interrupted => 130,
            Self::InvalidManifest(_)
            | Self::Signature(_)
            | Self::Mismatch { .. }
            | Self::Download { .. }
            | Self::SizeMismatch { .. }
            | Self::HashMismatch { .. }
            | Self::UnsafeArchive { .. }
            | Self::CorruptArchive { .. }
            | Self::VerificationFailed { .. }
            | Self::InvalidReceipt { .. }
            | Self::Busy { .. }
            | Self::DataDir(_)
            | Self::Io { .. } => 1,
        }
    }

    /// Short machine-readable error code.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::UnknownPack { .. } => "unknown_pack",
            Self::NotInstalled { .. } => "not_installed",
            Self::UnsupportedTarget { .. } => "unsupported_target",
            Self::IncompatibleAshift { .. } => "incompatible_ashift",
            Self::InvalidManifest(_) => "invalid_manifest",
            Self::Signature(_) => "signature",
            Self::Mismatch { .. } => "mismatch",
            Self::Download { .. } => "download",
            Self::SizeMismatch { .. } => "size_mismatch",
            Self::HashMismatch { .. } => "hash_mismatch",
            Self::UnsafeArchive { .. } => "unsafe_archive",
            Self::ExtractLimit { .. } => "limit_exceeded",
            Self::CorruptArchive { .. } => "corrupt_archive",
            Self::VerificationFailed { .. } => "verification_failed",
            Self::InvalidReceipt { .. } => "invalid_receipt",
            Self::Busy { .. } => "busy",
            Self::DataDir(_) => "data_dir",
            Self::Io { .. } => "io",
            Self::Interrupted => "interrupted",
            Self::Usage(_) => "usage",
        }
    }
}

/// Everything a pack operation depends on, so tests can run without process-global state.
#[derive(Clone, Debug)]
pub struct PackContext {
    /// Data directory; packs live under `<data_dir>/packs`.
    pub data_dir: PathBuf,
    /// Keys a manifest signature may come from.
    pub keys: TrustedKeys,
    /// Pack target triples this host can run, most preferred first.
    pub targets: Vec<String>,
    /// Network behaviour.
    pub fetch: FetchPolicy,
}

impl PackContext {
    /// The context of this process: [`data_dir`], compiled-in keys and [`host_targets`].
    pub fn from_env() -> Result<Self, PackError> {
        Ok(Self {
            data_dir: data_dir()?,
            keys: env_keys(),
            targets: host_targets(),
            fetch: FetchPolicy::default(),
        })
    }

    /// `<data_dir>/packs`.
    #[must_use]
    pub fn packs_dir(&self) -> PathBuf {
        self.data_dir.join("packs")
    }
}

#[cfg(not(feature = "test-probe"))]
fn env_keys() -> TrustedKeys {
    TrustedKeys::compiled()
}

/// Test builds also trust the public keys listed in `ASHIFT_TEST_PACK_KEYS`.
#[cfg(feature = "test-probe")]
fn env_keys() -> TrustedKeys {
    let mut keys = TrustedKeys::compiled();
    if let Some(paths) = std::env::var_os("ASHIFT_TEST_PACK_KEYS") {
        for path in std::env::split_paths(&paths) {
            if let Ok(text) = std::fs::read_to_string(&path)
                && let Ok(key) = TrustedKey::parse(&path.display().to_string(), &text)
            {
                keys.push(key);
            }
        }
    }
    keys
}

/// The data directory: `ASHIFT_DATA_DIR`, else the platform's local data directory for
/// `com.ariadnev.ariadshift`.
pub fn data_dir() -> Result<PathBuf, PackError> {
    if let Some(dir) = std::env::var_os(DATA_DIR_ENV).filter(|dir| !dir.is_empty()) {
        let dir = PathBuf::from(dir);
        return if dir.is_absolute() {
            Ok(dir)
        } else {
            std::env::current_dir()
                .map(|cwd| cwd.join(dir))
                .map_err(|error| PackError::DataDir(error.to_string()))
        };
    }
    directories::ProjectDirs::from("com", "ariadnev", "ariadshift")
        .map(|dirs| dirs.data_local_dir().to_path_buf())
        .ok_or_else(|| {
            PackError::DataDir(format!(
                "no home directory was found; set {DATA_DIR_ENV} to choose one"
            ))
        })
}

/// The target triple `ashift` was built for.
#[must_use]
pub const fn build_target() -> &'static str {
    env!("ARIAD_HOST_TARGET")
}

/// Pack target triples this host can run, most preferred first.
///
/// A pack's triple describes its own binaries. On Linux the static musl `ashift` also runs
/// glibc packs when the system has a glibc loader.
#[must_use]
pub fn host_targets() -> Vec<String> {
    let build = build_target();
    if !build.contains("-linux-") {
        return vec![build.to_owned()];
    }
    let arch = build.split('-').next().unwrap_or("x86_64");
    let gnu = format!("{arch}-unknown-linux-gnu");
    let musl = format!("{arch}-unknown-linux-musl");
    if build.ends_with("-gnu") || has_glibc_loader(arch) {
        vec![gnu, musl]
    } else {
        vec![musl]
    }
}

fn has_glibc_loader(arch: &str) -> bool {
    let candidates: &[&str] = match arch {
        "x86_64" => &[
            "/lib64/ld-linux-x86-64.so.2",
            "/lib/x86_64-linux-gnu/ld-linux-x86-64.so.2",
        ],
        "aarch64" => &[
            "/lib/ld-linux-aarch64.so.1",
            "/lib/aarch64-linux-gnu/ld-linux-aarch64.so.1",
        ],
        _ => &[],
    };
    candidates.iter().any(|path| Path::new(path).exists())
}

/// Where a resolved pack comes from.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PackSource {
    /// Installed and verified under the data directory.
    Installed,
    /// Development override `ASHIFT_PACK_<NAME>`, not signature-checked.
    Override,
}

impl PackSource {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Installed => "installed",
            Self::Override => "override",
        }
    }
}

/// A pack ready to launch: the engine command for the current target.
#[derive(Clone, Debug, PartialEq)]
pub struct InstalledPack {
    pub name: String,
    pub version: String,
    /// Target triple of the selected build.
    pub target: String,
    /// Absolute install root (not canonicalized, so Windows paths keep their usual form).
    /// Engines need read access to it and nothing else of the pack.
    pub root: PathBuf,
    /// Engine program: the entrypoint's first element joined to `root`.
    pub program: PathBuf,
    /// Engine arguments with `{root}` substituted.
    pub args: Vec<OsString>,
    /// Allow-listed engine environment with `{root}` substituted. The runner adds these to the
    /// cleared engine environment.
    pub env: Vec<(OsString, OsString)>,
    /// Default request options with `{root}` substituted.
    pub options: Map<String, Value>,
    pub source: PackSource,
}

impl InstalledPack {
    pub(crate) fn from_manifest(
        manifest: &Manifest,
        target: &Target,
        root: PathBuf,
        source: PackSource,
    ) -> Result<Self, PackError> {
        let (program, args) = target
            .entrypoint
            .split_first()
            .ok_or_else(|| PackError::InvalidManifest("entrypoint is empty".to_owned()))?;
        let parts = relative_path(program).map_err(|reason| {
            PackError::InvalidManifest(format!("entrypoint '{program}': {reason}"))
        })?;
        let program = manifest::join_relative(&root, &parts);
        let options = match expand_json(&Value::Object(target.options.clone()), &root)? {
            Value::Object(map) => map,
            _ => Map::new(),
        };
        Ok(Self {
            name: manifest.name.clone(),
            version: manifest.version.clone(),
            target: target.triple.clone(),
            program,
            args: args.iter().map(|arg| expand_os(arg, &root)).collect(),
            env: target
                .env
                .iter()
                .map(|(key, value)| (OsString::from(key), expand_os(value, &root)))
                .collect(),
            options,
            root,
            source,
        })
    }
}

/// Resolves a pack for the current target: the `ASHIFT_PACK_<NAME>` override first, then the
/// active installed version. Returns `None` when the pack is absent or unusable; use
/// [`try_resolve`] for the reason.
#[must_use]
pub fn resolve(name: &str) -> Option<InstalledPack> {
    try_resolve(name).ok().flatten()
}

/// Like [`resolve`], but reports why an existing pack cannot be used.
pub fn try_resolve(name: &str) -> Result<Option<InstalledPack>, PackError> {
    manifest::validate_pack_name(name).map_err(PackError::Usage)?;
    let targets = host_targets();
    if let Some(dir) = std::env::var_os(override_var(name)).filter(|dir| !dir.is_empty()) {
        return resolve_override(name, Path::new(&dir), &targets).map(Some);
    }
    let context = PackContext::from_env()?;
    install::resolve_installed(&context, name)
}

/// Resolves an installed pack in an explicit context (ignores overrides).
pub fn resolve_in(context: &PackContext, name: &str) -> Result<Option<InstalledPack>, PackError> {
    manifest::validate_pack_name(name).map_err(PackError::Usage)?;
    install::resolve_installed(context, name)
}

/// Name of the override variable for `name`: `ASHIFT_PACK_` plus the upper-cased name with
/// `-` replaced by `_`.
#[must_use]
pub fn override_var(name: &str) -> String {
    format!(
        "{OVERRIDE_ENV_PREFIX}{}",
        name.to_ascii_uppercase().replace('-', "_")
    )
}

/// Pack names that have an `ASHIFT_PACK_<NAME>` override set in this process.
#[must_use]
pub fn override_names() -> Vec<String> {
    let mut names: Vec<String> = std::env::vars_os()
        .filter_map(|(key, value)| {
            let key = key.to_str()?;
            let name = key.strip_prefix(OVERRIDE_ENV_PREFIX)?;
            if value.is_empty() {
                return None;
            }
            let name = name.to_ascii_lowercase().replace('_', "-");
            manifest::validate_pack_name(&name).ok().map(|()| name)
        })
        .collect();
    names.sort();
    names.dedup();
    names
}

/// Resolves an unpacked development pack: `<dir>/pack.toml` (unsigned) with `<dir>` as root.
pub fn resolve_override(
    name: &str,
    dir: &Path,
    targets: &[String],
) -> Result<InstalledPack, PackError> {
    let manifest_path = dir.join(MANIFEST_FILE);
    let text = std::fs::read_to_string(&manifest_path)
        .map_err(|error| PackError::io("could not read", &manifest_path, error))?;
    let manifest = Manifest::parse(&text)?;
    if manifest.name != name {
        return Err(PackError::Mismatch {
            expected: format!("pack '{name}'"),
            found: format!("pack '{}'", manifest.name),
        });
    }
    let target = manifest
        .select_target(targets)
        .ok_or_else(|| PackError::UnsupportedTarget {
            name: manifest.name.clone(),
            version: manifest.version.clone(),
            host: targets.join(", "),
            available: manifest.triples(),
        })?;
    let root =
        std::path::absolute(dir).map_err(|error| PackError::io("could not resolve", dir, error))?;
    let pack = InstalledPack::from_manifest(&manifest, target, root, PackSource::Override)?;
    if !pack.program.is_file() {
        return Err(PackError::InvalidManifest(format!(
            "entrypoint {} does not exist",
            pack.program.display()
        )));
    }
    Ok(pack)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn override_variable_names() {
        assert_eq!(override_var("docling"), "ASHIFT_PACK_DOCLING");
        assert_eq!(override_var("ocr-extra"), "ASHIFT_PACK_OCR_EXTRA");
    }

    #[test]
    fn host_targets_include_the_build_family() {
        let targets = host_targets();
        assert!(!targets.is_empty());
        let build = build_target();
        if build.contains("-linux-") {
            assert!(targets.iter().all(|t| t.contains("-unknown-linux-")));
        } else {
            assert_eq!(targets, vec![build.to_owned()]);
        }
    }

    #[test]
    fn exit_codes_follow_the_table() {
        assert_eq!(
            PackError::UnknownPack {
                name: "x".into(),
                known: String::new()
            }
            .exit_code(),
            3
        );
        assert_eq!(
            PackError::Signature(SignatureError::Malformed).exit_code(),
            1
        );
        assert_eq!(
            PackError::IncompatibleAshift {
                name: "x".into(),
                version: "1.0.0".into(),
                requires: ">=9".into(),
                current: "0.1.0".into()
            }
            .exit_code(),
            5
        );
        assert_eq!(PackError::Interrupted.exit_code(), 130);
    }
}
