//! Pack-backed out-of-process engine resolution (`pack_name == engine_id`).
//!
//! Replaces engine-specific pack resolution branches with a single fail-closed resolver that
//! verifies the installed pack's minisign-signed manifest and executable SHA-256 digests on
//! every resolve.

use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
    sync::{
        LazyLock, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

use ariad_core::protocol::ErrorCode;
use serde_json::Value;

use crate::{
    packs::{self, PackContext, PackError},
    runner::EngineCommand,
};

static RESOLVE_HASH_COMPUTATIONS: AtomicU64 = AtomicU64::new(0);
static PATH_HASH_COMPUTATIONS: LazyLock<Mutex<HashMap<PathBuf, u64>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Total number of uncached SHA-256 file digest computations performed during pack resolution in
/// this process.
#[must_use]
pub fn resolve_hash_computations() -> u64 {
    RESOLVE_HASH_COMPUTATIONS.load(Ordering::Relaxed)
}

/// Number of uncached SHA-256 file digest computations performed for files under `root` during
/// pack resolution in this process.
#[must_use]
pub fn resolve_hash_computations_for(root: &Path) -> u64 {
    let counts = PATH_HASH_COMPUTATIONS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    counts
        .iter()
        .filter(|(path, _)| path.starts_with(root))
        .map(|(_, count)| *count)
        .sum()
}

pub(crate) fn record_hash_computation(path: &Path) {
    RESOLVE_HASH_COMPUTATIONS.fetch_add(1, Ordering::Relaxed);
    let mut counts = PATH_HASH_COMPUTATIONS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *counts.entry(path.to_path_buf()).or_insert(0) += 1;
}

/// Failure to resolve an out-of-process pack-backed engine.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ResolveError {
    /// The requested engine identifier is not a registered pack engine (fail closed).
    #[error("unknown pack engine '{engine}'")]
    UnknownEngine {
        /// Requested engine identifier.
        engine: String,
    },
    /// The engine pack is not installed (`tool_missing`).
    #[error("tool_missing: engine pack '{engine}' is not installed; {hint}")]
    NotInstalled {
        /// Engine / pack identifier.
        engine: String,
        /// Actionable install hint (`run 'ashift engines install <id>'`).
        hint: String,
    },
    /// The installed engine pack failed signature or SHA-256 verification (`tool_missing`).
    #[error("tool_missing: engine pack '{engine}' failed verification ({reason}); {hint}")]
    VerificationFailed {
        /// Engine / pack identifier.
        engine: String,
        /// Verification failure detail.
        reason: String,
        /// Actionable reinstall hint (`run 'ashift engines install <id>' to reinstall`).
        hint: String,
    },
}

impl ResolveError {
    /// Short machine-readable error code (`"tool_missing"` or `"unknown_engine"`).
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::UnknownEngine { .. } => "unknown_engine",
            Self::NotInstalled { .. } | Self::VerificationFailed { .. } => "tool_missing",
        }
    }

    /// Protocol [`ErrorCode`] corresponding to this resolution failure.
    #[must_use]
    pub const fn error_code(&self) -> ErrorCode {
        ErrorCode::ToolMissing
    }

    /// CLI exit code (`5` for `tool_missing`, `3` for `unknown_engine`).
    #[must_use]
    pub const fn exit_code(&self) -> u8 {
        match self {
            Self::UnknownEngine { .. } => 3,
            Self::NotInstalled { .. } | Self::VerificationFailed { .. } => 5,
        }
    }

    /// Actionable install or reinstall hint, when available.
    #[must_use]
    pub fn hint(&self) -> Option<&str> {
        match self {
            Self::UnknownEngine { .. } => None,
            Self::NotInstalled { hint, .. } | Self::VerificationFailed { hint, .. } => Some(hint),
        }
    }
}

fn reinstall_hint(id: &str) -> String {
    format!("run 'ashift engines install {id}' to reinstall")
}

fn install_hint(id: &str) -> String {
    if id == "tesseract"
        && let Some(hint) =
            tesseract_unsupported_hint(packs::build_target(), &packs::host_targets())
    {
        return hint.to_owned();
    }
    super::pack_spec(id).map_or_else(
        || format!("run 'ashift engines install {id}' to install the {id} engine pack"),
        |spec| spec.install_hint.to_owned(),
    )
}

fn tesseract_unsupported_hint(triple: &str, targets: &[String]) -> Option<&'static str> {
    if triple.contains("-windows-") {
        Some(
            "Tesseract packs are unavailable on Windows; see https://github.com/bavanchun/AriadShift/issues/202",
        )
    } else if triple.contains("-linux-")
        && !targets.iter().any(|target| target.ends_with("-linux-gnu"))
    {
        Some(
            "Tesseract packs require glibc on Linux; see https://github.com/bavanchun/AriadShift/issues/202",
        )
    } else {
        None
    }
}

fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.metadata()
            .map(|m| m.is_file() && (m.permissions().mode() & 0o111) != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}

/// Resolves the engine/helper binary (`ashift` or `engine-probe`).
///
/// Resolution order:
/// 1. Explicit override: if `explicit` is `Some(program)`, returns `Some(program.to_path_buf())`.
/// 2. Self when `ashift` or `engine-probe`: if `current_exe` has stem `"ashift"` or `"engine-probe"`,
///    returns `Some(exe.to_path_buf())`.
/// 3. Relative helper locations next to `current_exe` (checks `engine-probe` then `ashift`,
///    preserving origin/dev candidate order for test probe environments):
///    - Same directory sidecar: `<dir>/engine-probe{EXE_SUFFIX}`, `<dir>/ashift{EXE_SUFFIX}`
///    - macOS app bundle helper directory: `<dir>/../Helpers/engine-probe{EXE_SUFFIX}`, `<dir>/../Helpers/ashift{EXE_SUFFIX}`
///    - Parent directory (Cargo test `target/<profile>/deps/<test>` -> `target/<profile>/`): `<dir>/../engine-probe{EXE_SUFFIX}`, `<dir>/../ashift{EXE_SUFFIX}`
/// 4. `PATH` search: first `<dir>/ashift{EXE_SUFFIX}` in `PATH` that is executable.
/// 5. Fail closed: returns `None` (never falls back to non-ashift/probe `current_exe`).
pub fn resolve_engine_program(explicit: Option<&Path>) -> Option<PathBuf> {
    let current_exe = std::env::current_exe().ok();
    let path_env = std::env::var_os("PATH");
    resolve_engine_program_with(explicit, current_exe.as_deref(), path_env.as_deref())
}

pub(crate) fn resolve_engine_program_with(
    explicit: Option<&Path>,
    current_exe: Option<&Path>,
    path_env: Option<&std::ffi::OsStr>,
) -> Option<PathBuf> {
    // 1. Explicit override
    if let Some(program) = explicit {
        return Some(program.to_path_buf());
    }

    // 2. Self when ashift or engine-probe
    if let Some(exe) = current_exe {
        let stem = exe.file_stem().and_then(|s| s.to_str()).unwrap_or_default();
        if stem == "ashift" || stem == "engine-probe" {
            return Some(exe.to_path_buf());
        }
    }

    let exe_suffix = std::env::consts::EXE_SUFFIX;

    // 3. Relative helper locations next to current_exe
    // Checks engine-probe before ashift in each location, preserving origin/dev candidate order.
    if let Some(dir) = current_exe.and_then(Path::parent) {
        let mut candidates = Vec::new();

        // Same directory sidecar: <dir>/engine-probe{EXE_SUFFIX}, <dir>/ashift{EXE_SUFFIX}
        candidates.push(dir.join(format!("engine-probe{exe_suffix}")));
        candidates.push(dir.join(format!("ashift{exe_suffix}")));

        // macOS app bundle helper directory (<exe>/../../Helpers/ -> <Contents>/Helpers/)
        if let Some(parent) = dir.parent() {
            let helpers = parent.join("Helpers");
            candidates.push(helpers.join(format!("engine-probe{exe_suffix}")));
            candidates.push(helpers.join(format!("ashift{exe_suffix}")));
        }

        // Parent directory (Cargo test target/<profile>/deps/<test> -> target/<profile>/)
        if let Some(parent) = dir.parent() {
            candidates.push(parent.join(format!("engine-probe{exe_suffix}")));
            candidates.push(parent.join(format!("ashift{exe_suffix}")));
        }

        for candidate in candidates {
            if is_executable(&candidate) {
                return Some(candidate);
            }
        }
    }

    // 4. PATH search
    if let Some(path_env) = path_env {
        let ashift_bin = format!("ashift{exe_suffix}");
        for path_dir in std::env::split_paths(path_env) {
            let candidate = path_dir.join(&ashift_bin);
            if is_executable(&candidate) {
                return Some(candidate);
            }
        }
    }

    // 5. Fail closed
    None
}

/// Locates the host sandbox launcher binary (`ashift` or `engine-probe`) when a caller does not
/// supply an explicit `engine_program`.
fn default_launcher() -> Option<PathBuf> {
    resolve_engine_program(None)
}

/// Resolves a registered pack-backed engine by identifier (`pack_name == engine_id`).
///
/// Fails closed with [`ResolveError::UnknownEngine`] for unknown engine identifiers, and returns
/// [`ResolveError::NotInstalled`] or [`ResolveError::VerificationFailed`] (`tool_missing` with a
/// reinstall hint) when the pack is missing or tampered with.
pub fn resolve_pack(engine_id: &str) -> Result<EngineCommand, ResolveError> {
    let (command, _) = resolve_pack_with_options(engine_id, None)?;
    Ok(command)
}

/// Resolves a registered pack-backed engine inside an explicit [`PackContext`].
pub fn resolve_pack_in(
    context: &PackContext,
    engine_id: &str,
) -> Result<EngineCommand, ResolveError> {
    if super::pack_spec(engine_id).is_none() {
        return Err(ResolveError::UnknownEngine {
            engine: engine_id.to_owned(),
        });
    }
    let pack = match packs::resolve_in(context, engine_id) {
        Ok(Some(pack)) => pack,
        Ok(None) => {
            return Err(ResolveError::NotInstalled {
                engine: engine_id.to_owned(),
                hint: install_hint(engine_id),
            });
        }
        Err(error) => {
            return Err(map_pack_error(engine_id, error));
        }
    };
    Ok(command_from_installed_pack(engine_id, pack, None)?.0)
}

/// Resolves a registered pack-backed engine and returns both its [`EngineCommand`] and any
/// pack-level request options (such as Docling's `artifacts_path` and `tessdata_path`).
pub fn resolve_pack_with_options(
    engine_id: &str,
    launcher_override: Option<&Path>,
) -> Result<(EngineCommand, BTreeMap<String, Value>), ResolveError> {
    if super::pack_spec(engine_id).is_none() {
        return Err(ResolveError::UnknownEngine {
            engine: engine_id.to_owned(),
        });
    }
    if engine_id == super::docling::ENGINE_ID
        && std::env::var_os(super::docling::OVERRIDE_ENV).is_some()
    {
        let docling = super::docling::resolve().ok_or_else(|| ResolveError::NotInstalled {
            engine: engine_id.to_owned(),
            hint: install_hint(engine_id),
        })?;
        let launcher = launcher_override
            .map(Path::to_path_buf)
            .or_else(default_launcher);
        let mut command = EngineCommand {
            engine: engine_id.to_owned(),
            program: docling.program,
            args: docling.args,
            env: docling.env,
            install_root: docling.install_root,
            launcher,
            sandbox: None,
        };
        if let Some(launcher) = launcher_override {
            command = command.with_launcher(launcher);
        }
        return Ok((command, docling.options));
    }
    if engine_id == super::sign::ENGINE_ID && std::env::var_os(super::sign::OVERRIDE_ENV).is_some()
    {
        let sign_engine = super::sign::resolve().ok_or_else(|| ResolveError::NotInstalled {
            engine: engine_id.to_owned(),
            hint: install_hint(engine_id),
        })?;
        let launcher = launcher_override
            .map(Path::to_path_buf)
            .or_else(default_launcher);
        let mut command = EngineCommand {
            engine: engine_id.to_owned(),
            program: sign_engine.program,
            args: sign_engine.args,
            env: sign_engine.env,
            install_root: sign_engine.install_root,
            launcher,
            sandbox: None,
        };
        if let Some(launcher) = launcher_override {
            command = command.with_launcher(launcher);
        }
        return Ok((command, sign_engine.options));
    }
    let pack = match packs::try_resolve(engine_id) {
        Ok(Some(pack)) => pack,
        Ok(None) => {
            return Err(ResolveError::NotInstalled {
                engine: engine_id.to_owned(),
                hint: install_hint(engine_id),
            });
        }
        Err(error) => {
            return Err(map_pack_error(engine_id, error));
        }
    };
    command_from_installed_pack(engine_id, pack, launcher_override)
}

fn command_from_installed_pack(
    engine_id: &str,
    pack: packs::InstalledPack,
    launcher_override: Option<&Path>,
) -> Result<(EngineCommand, BTreeMap<String, Value>), ResolveError> {
    if super::pack_spec(engine_id).is_some_and(|spec| spec.sandbox_required) {
        confine_entrypoint(engine_id, &pack)?;
    }
    let launcher = launcher_override
        .map(Path::to_path_buf)
        .or_else(default_launcher);
    if let Some(variable) = super::pack_spec(engine_id).and_then(|spec| spec.pack_program_env) {
        let launcher = launcher.ok_or_else(|| ResolveError::VerificationFailed {
            engine: engine_id.to_owned(),
            reason: "adapter launcher is unavailable".to_owned(),
            hint: reinstall_hint(engine_id),
        })?;
        let root =
            std::fs::canonicalize(&pack.root).map_err(|_| ResolveError::VerificationFailed {
                engine: engine_id.to_owned(),
                reason: "pack root is unreadable".to_owned(),
                hint: reinstall_hint(engine_id),
            })?;
        let program =
            std::fs::canonicalize(&pack.program).map_err(|_| ResolveError::VerificationFailed {
                engine: engine_id.to_owned(),
                reason: "pack entrypoint is unreadable".to_owned(),
                hint: reinstall_hint(engine_id),
            })?;
        program
            .strip_prefix(&root)
            .map_err(|_| ResolveError::VerificationFailed {
                engine: engine_id.to_owned(),
                reason: "pack entrypoint is outside the pack".to_owned(),
                hint: reinstall_hint(engine_id),
            })?;
        let packs::InstalledPack {
            env: mut variables,
            options,
            program: declared_program,
            ..
        } = pack;
        variables.push((variable.into(), declared_program.into_os_string()));
        variables.push((
            "ASHIFT_ADAPTER_PACK_ROOT".into(),
            root.clone().into_os_string(),
        ));
        if engine_id == "verapdf" {
            if let Some(v) = std::env::var_os("JAVA_HOME") {
                variables.push(("JAVA_HOME".into(), v));
            }
            if let Some(v) = std::env::var_os(super::verapdf::JAVA_ENV) {
                variables.push((super::verapdf::JAVA_ENV.into(), v));
            }
            if let Some(v) = std::env::var_os("HOME") {
                variables.push(("HOME".into(), v));
            }
        }
        let command = EngineCommand {
            env: variables,
            install_root: Some(root),
            ..EngineCommand::builtin(&launcher, engine_id)
        };
        return Ok((command, options.into_iter().collect()));
    }
    let options: BTreeMap<String, Value> = pack.options.into_iter().collect();
    let command = EngineCommand {
        engine: engine_id.to_owned(),
        program: pack.program,
        args: pack.args,
        env: pack.env,
        install_root: Some(pack.root),
        launcher,
        sandbox: None,
    };
    Ok((command, options))
}

/// Refuses a sandboxed pack whose entrypoint, after following links, leaves the pack root. The
/// sandbox grants the engine its install root only, so a program elsewhere would run outside
/// the files the signed manifest vouches for.
fn confine_entrypoint(engine_id: &str, pack: &packs::InstalledPack) -> Result<(), ResolveError> {
    let refuse = |reason: &str| ResolveError::VerificationFailed {
        engine: engine_id.to_owned(),
        reason: reason.to_owned(),
        hint: reinstall_hint(engine_id),
    };
    let root = std::fs::canonicalize(&pack.root).map_err(|_| refuse("pack root is unreadable"))?;
    let program = std::fs::canonicalize(&pack.program)
        .map_err(|_| refuse("entrypoint is missing from the pack"))?;
    if program.starts_with(&root) {
        Ok(())
    } else {
        Err(refuse("entrypoint resolves outside the pack root"))
    }
}

fn map_pack_error(engine_id: &str, error: PackError) -> ResolveError {
    match error {
        PackError::NotInstalled { .. } => ResolveError::NotInstalled {
            engine: engine_id.to_owned(),
            hint: install_hint(engine_id),
        },
        other => ResolveError::VerificationFailed {
            engine: engine_id.to_owned(),
            reason: other.to_string(),
            hint: reinstall_hint(engine_id),
        },
    }
}

#[cfg(test)]
mod tesseract_target_tests {
    use super::tesseract_unsupported_hint;

    #[test]
    fn support_depends_on_system_targets_not_binary_linkage() {
        let musl = vec!["x86_64-unknown-linux-musl".to_owned()];
        let gnu = vec!["x86_64-unknown-linux-gnu".to_owned(), musl[0].clone()];
        assert!(
            tesseract_unsupported_hint("x86_64-pc-windows-msvc", &[])
                .unwrap()
                .contains("Windows")
        );
        assert!(
            tesseract_unsupported_hint(&musl[0], &musl)
                .unwrap()
                .contains("glibc")
        );
        assert_eq!(tesseract_unsupported_hint(&musl[0], &gnu), None);
        assert_eq!(tesseract_unsupported_hint(&gnu[0], &gnu), None);
    }
}

#[cfg(test)]
mod resolver_tests {
    use super::resolve_engine_program_with;
    use std::fs;

    #[test]
    fn explicit_override_wins_over_non_ashift_current_exe() {
        let tmp = tempfile::tempdir().unwrap();
        let host_exe = tmp.path().join("SampleHost");
        let explicit = tmp.path().join("custom_bin");
        let resolved = resolve_engine_program_with(Some(&explicit), Some(&host_exe), None);
        assert_eq!(resolved, Some(explicit));
    }

    #[test]
    fn self_ashift_and_engine_probe_are_accepted() {
        let tmp = tempfile::tempdir().unwrap();
        let ashift = tmp.path().join("ashift");
        let probe = tmp.path().join("engine-probe");
        assert_eq!(
            resolve_engine_program_with(None, Some(&ashift), None),
            Some(ashift)
        );
        assert_eq!(
            resolve_engine_program_with(None, Some(&probe), None),
            Some(probe)
        );
    }

    #[test]
    fn macos_bundle_helpers_ashift_is_discovered() {
        let tmp = tempfile::tempdir().unwrap();
        let app_dir = tmp.path().join("SampleHost.app").join("Contents");
        let macos_dir = app_dir.join("MacOS");
        let helpers_dir = app_dir.join("Helpers");
        fs::create_dir_all(&macos_dir).unwrap();
        fs::create_dir_all(&helpers_dir).unwrap();
        let host_exe = macos_dir.join("SampleHost");
        let helper_ashift = helpers_dir.join(format!("ashift{}", std::env::consts::EXE_SUFFIX));
        fs::write(&helper_ashift, b"").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&helper_ashift, fs::Permissions::from_mode(0o755)).unwrap();
        }
        let resolved = resolve_engine_program_with(None, Some(&host_exe), None);
        assert_eq!(resolved, Some(helper_ashift));
    }

    #[test]
    fn sidecar_and_parent_dir_helpers_are_discovered() {
        let tmp = tempfile::tempdir().unwrap();
        let sidecar_dir = tmp.path().join("sidecar");
        fs::create_dir_all(&sidecar_dir).unwrap();
        let host_exe = sidecar_dir.join("SampleHost");
        let sidecar_ashift = sidecar_dir.join(format!("ashift{}", std::env::consts::EXE_SUFFIX));
        fs::write(&sidecar_ashift, b"").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&sidecar_ashift, fs::Permissions::from_mode(0o755)).unwrap();
        }
        assert_eq!(
            resolve_engine_program_with(None, Some(&host_exe), None),
            Some(sidecar_ashift)
        );

        let deps_dir = tmp.path().join("target").join("deps");
        fs::create_dir_all(&deps_dir).unwrap();
        let target_dir = deps_dir.parent().unwrap();
        let test_exe = deps_dir.join("test_runner");
        let target_ashift = target_dir.join(format!("ashift{}", std::env::consts::EXE_SUFFIX));
        fs::write(&target_ashift, b"").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&target_ashift, fs::Permissions::from_mode(0o755)).unwrap();
        }
        assert_eq!(
            resolve_engine_program_with(None, Some(&test_exe), None),
            Some(target_ashift)
        );
    }

    #[test]
    fn path_env_ashift_is_discovered_when_bundle_has_no_helper() {
        let tmp = tempfile::tempdir().unwrap();
        let bundle_macos = tmp
            .path()
            .join("SampleHost.app")
            .join("Contents")
            .join("MacOS");
        fs::create_dir_all(&bundle_macos).unwrap();
        let host_exe = bundle_macos.join("SampleHost");

        let bin_dir = tmp.path().join("bin");
        fs::create_dir_all(&bin_dir).unwrap();
        let path_ashift = bin_dir.join(format!("ashift{}", std::env::consts::EXE_SUFFIX));
        fs::write(&path_ashift, b"").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path_ashift, fs::Permissions::from_mode(0o755)).unwrap();
        }

        let path_env = std::ffi::OsString::from(bin_dir.as_os_str());
        let resolved = resolve_engine_program_with(None, Some(&host_exe), Some(&path_env));
        assert_eq!(resolved, Some(path_ashift));
    }

    #[test]
    fn non_ashift_current_exe_without_helpers_returns_none() {
        let tmp = tempfile::tempdir().unwrap();
        let host_exe = tmp.path().join("SampleHost");
        fs::write(&host_exe, b"").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&host_exe, fs::Permissions::from_mode(0o755)).unwrap();
        }
        let empty_path = std::ffi::OsString::new();
        let resolved = resolve_engine_program_with(None, Some(&host_exe), Some(&empty_path));
        assert_eq!(resolved, None);
    }
}
