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
    super::pack_spec(id).map_or_else(
        || format!("run 'ashift engines install {id}' to install the {id} engine pack"),
        |spec| spec.install_hint.to_owned(),
    )
}

/// Locates the host sandbox launcher binary (`ashift` or `engine-probe`) when a caller does not
/// supply an explicit `engine_program`.
fn default_launcher() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let stem = exe.file_stem().and_then(|s| s.to_str()).unwrap_or_default();
    if stem == "ashift" || stem == "engine-probe" {
        return Some(exe);
    }
    let dir = exe.parent()?;
    for candidate_dir in [Some(dir), dir.parent()] {
        let Some(candidate_dir) = candidate_dir else {
            continue;
        };
        for bin in ["engine-probe", "ashift"] {
            let candidate = candidate_dir.join(format!("{bin}{}", std::env::consts::EXE_SUFFIX));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    Some(exe)
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
    Ok(command_from_installed_pack(engine_id, pack, None).0)
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
    Ok(command_from_installed_pack(
        engine_id,
        pack,
        launcher_override,
    ))
}

fn command_from_installed_pack(
    engine_id: &str,
    pack: packs::InstalledPack,
    launcher_override: Option<&Path>,
) -> (EngineCommand, BTreeMap<String, Value>) {
    let launcher = launcher_override
        .map(Path::to_path_buf)
        .or_else(default_launcher);
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
    (command, options)
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
