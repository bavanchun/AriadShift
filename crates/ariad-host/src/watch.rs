//! Cross-platform settled-file polling and content-addressed recipe execution.

use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use ariad_core::tools::{Plan, RecipeOrigin, RunSize, Tier, access::within_run_quota};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::tools::{
    publish::PublishedArtifact,
    recipe::{RecipeRequest, run_recipe_with_sources},
    run::{InputRef, ToolError},
};

/// A watch directory and the recipe execution settings to apply to each settled input.
#[derive(Clone, Debug)]
pub struct WatchRequest {
    /// Incoming regular files; dot-prefixed entries and symlinks are ignored.
    pub watch_dir: PathBuf,
    /// Shared execution settings. Inputs are replaced by each claimed file.
    pub recipe: RecipeRequest,
    /// Settle and process one snapshot, then return; otherwise poll until cancellation.
    pub once: bool,
    /// Stability window in milliseconds (200 by default).
    pub settle_ms: u64,
}

impl WatchRequest {
    /// Constructs a continuous watcher, cancelled through `recipe.cancel`.
    #[must_use]
    pub fn new(watch_dir: impl Into<PathBuf>, recipe: RecipeRequest) -> Self {
        Self {
            watch_dir: watch_dir.into(),
            recipe,
            once: false,
            settle_ms: 200,
        }
    }
}

/// Aggregate counters across all trigger batches in a watch run.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WatchOutcome {
    /// Inputs successfully executed and moved to `.done`.
    pub processed: usize,
    /// Inputs already completed for the same recipe and content.
    pub skipped_idempotent: usize,
    /// Recipe executions attempted (including failed executions).
    pub runs_executed: usize,
    /// Inputs moved to `.failed` with a structured error report.
    pub failed: usize,
    /// Artifacts published by successful executions during this run.
    pub artifacts: Vec<PublishedArtifact>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct LedgerEntry {
    filename: String,
    completed_at_epoch_s: u64,
    artifacts: Vec<PathBuf>,
}
type Ledger = BTreeMap<String, LedgerEntry>;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Fingerprint {
    size: u64,
    modified: SystemTime,
}

/// Polls settled inputs until cancellation, or executes one settled snapshot in `once` mode.
/// Quota rejection happens before any input is claimed or executed.
pub async fn run_watch_folder(request: &WatchRequest) -> Result<WatchOutcome, ToolError> {
    let scrubber = request.recipe.secrets.scrubber();
    run_watch_inner(request)
        .await
        .map_err(|e| e.scrubbed(&scrubber))
}

async fn run_watch_inner(request: &WatchRequest) -> Result<WatchOutcome, ToolError> {
    let recipe = &request.recipe;
    recipe
        .recipe
        .validate(recipe.origin)
        .map_err(|e| ToolError::InvalidParams(e.to_string()))?;
    if recipe.origin == RecipeOrigin::Imported && !recipe.confirmed {
        return Err(ToolError::InvalidParams(
            "imported recipe requires explicit confirmation (--yes)".into(),
        ));
    }
    if recipe.limits.validate().is_err() {
        return Err(ToolError::LimitExceeded);
    }
    super::tools::publish::ensure_no_parent_traversal(&request.watch_dir)?;
    super::tools::publish::ensure_no_parent_traversal(&recipe.output_dir)?;
    let watch_dir = fs::canonicalize(&request.watch_dir).map_err(input_io)?;
    if !watch_dir.is_dir() {
        return Err(ToolError::InputIo("watch path must be a directory".into()));
    }
    let output_dir = super::tools::publish::open_confined_output_dir(
        &recipe.output_dir,
        recipe.allowed_dirs.as_ref(),
    )?;
    drop(output_dir);
    let output_root = fs::canonicalize(&recipe.output_dir).map_err(output_io)?;
    if output_root == watch_dir {
        return Err(ToolError::InvalidParams(
            "watch and output directories must differ".into(),
        ));
    }
    if let Some(allowed) = &recipe.allowed_dirs {
        // Check directory confinement before moving any incoming document.
        let permitted = fs::read_dir(&watch_dir).map_err(input_io)?;
        for entry in permitted {
            let entry = entry.map_err(input_io)?;
            if !entry.file_name().to_string_lossy().starts_with('.')
                && entry.file_type().map_err(input_io)?.is_file()
            {
                crate::confine::confine_input(&entry.path(), allowed)
                    .map_err(|e| ToolError::Confinement(e.to_string()))?;
            }
        }
    }
    if let Some(allowed) = &recipe.allowed_dirs
        && !allowed
            .roots()
            .iter()
            .any(|root| watch_dir.starts_with(&root.canonical_path))
    {
        return Err(ToolError::Confinement(
            "watch directory is outside allowed directories".into(),
        ));
    }
    let _guard = lock_watch_dir(&watch_dir)?;
    let processing = watch_dir.join(".processing");
    let done = watch_dir.join(".done");
    let failed = watch_dir.join(".failed");
    for path in [&processing, &done, &failed] {
        ensure_private_subdir(path)?;
    }
    let ledger_path = watch_dir.join(".ariad-watch-ledger.json");
    let mut ledger: Ledger = match fs::read(&ledger_path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|e| ToolError::InputIo(format!("invalid watch ledger: {e}")))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
        Err(e) => return Err(input_io(e)),
    };
    let canonical = canonical_json(
        serde_json::to_value(&recipe.recipe)
            .map_err(|e| ToolError::InvalidParams(e.to_string()))?,
    );
    let recipe_hash = hex::encode(Sha256::digest(
        serde_json::to_vec(&canonical).map_err(|e| ToolError::InvalidParams(e.to_string()))?,
    ));
    let mut outcome = WatchOutcome::default();
    let delay = Duration::from_millis(request.settle_ms);
    loop {
        if recipe.cancel.is_cancelled() {
            return Ok(outcome);
        }
        let first = snapshot(&watch_dir)?;
        let pending_first = snapshot(&processing)?;
        tokio::select! {
            () = recipe.cancel.cancelled() => return Ok(outcome),
            () = tokio::time::sleep(delay) => {},
        }
        let second = snapshot(&watch_dir)?;
        let pending_second = snapshot(&processing)?;
        // Recover claims left by interruption or an archive/ledger write failure first.
        let settled: Vec<_> = pending_first
            .into_iter()
            .filter(|(name, stamp)| pending_second.get(name) == Some(stamp))
            .map(|(name, stamp)| (name, stamp, true))
            .chain(
                first
                    .into_iter()
                    .filter(|(name, stamp)| second.get(name) == Some(stamp))
                    .map(|(name, stamp)| (name, stamp, false)),
            )
            .collect();
        let bytes = settled
            .iter()
            .fold(0_u64, |sum, (_, stamp, _)| sum.saturating_add(stamp.size));
        check_quota(recipe.plan, settled.len(), bytes)?;
        let mut batch = Vec::new();
        for (name, stamp, already_claimed) in settled {
            if recipe.cancel.is_cancelled() {
                return Ok(outcome);
            }
            let input = watch_dir.join(&name);
            let claimed = processing.join(&name);
            if !already_claimed {
                if claimed.try_exists().map_err(input_io)? {
                    continue;
                }
                // Recheck stability immediately before the atomic claim.
                if fingerprint(&input)?.as_ref() != Some(&stamp) {
                    continue;
                }
                if let Some(allowed) = &recipe.allowed_dirs {
                    crate::confine::confine_input(&input, allowed)
                        .map_err(|e| ToolError::Confinement(e.to_string()))?;
                }
                match fs::rename(&input, &claimed) {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(e) => return Err(input_io(e)),
                }
            }
            match content_hash(request, &claimed) {
                Ok(hash)
                    if ledger
                        .get(&format!("{hash}:{recipe_hash}"))
                        .is_some_and(|entry| {
                            entry.artifacts.iter().all(|path| {
                                path.is_file()
                                    && fs::canonicalize(path).is_ok_and(|artifact| {
                                        artifact.parent() == Some(output_root.as_path())
                                    })
                            })
                        }) =>
                {
                    archive_input(&claimed, &done, &name)?;
                    outcome.skipped_idempotent += 1;
                }
                Ok(hash) => batch.push((claimed, name, hash)),
                Err(ToolError::Interrupted) => return Ok(outcome),
                Err(error) => {
                    archive_failure(
                        &claimed,
                        &failed,
                        &name,
                        &error.scrubbed(&recipe.secrets.scrubber()),
                    )?;
                    outcome.failed += 1;
                }
            }
        }
        if !batch.is_empty() {
            let mut batch_recipe = recipe.clone();
            batch_recipe.inputs = batch
                .iter()
                .map(|(path, _, _)| InputRef::new(path))
                .collect();
            if let Some(allowed) = &recipe.allowed_dirs {
                let mut dirs = vec![processing.clone()];
                dirs.extend(allowed.dirs());
                batch_recipe.allowed_dirs = Some(
                    crate::confine::AllowedDirs::new(&dirs, &[])
                        .map_err(|e| ToolError::Confinement(e.to_string()))?,
                );
            }
            outcome.runs_executed += 1;
            let result =
                tokio::task::spawn_blocking(move || run_recipe_with_sources(&batch_recipe))
                    .await
                    .map_err(|_| ToolError::EngineFailure("watch recipe worker failed".into()))?;
            match result {
                Ok((run, sources)) => {
                    let mut entries = Ledger::new();
                    for (index, (_, name, hash)) in batch.iter().enumerate() {
                        entries
                            .entry(format!("{hash}:{recipe_hash}"))
                            .or_insert_with(|| LedgerEntry {
                                filename: name.to_string_lossy().into_owned(),
                                completed_at_epoch_s: SystemTime::now()
                                    .duration_since(UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_secs(),
                                artifacts: Vec::new(),
                            })
                            .artifacts
                            .extend(
                                run.artifacts
                                    .iter()
                                    .zip(&sources)
                                    .filter(|(_, origins)| origins.contains(&index))
                                    .map(|(artifact, _)| artifact.path.clone()),
                            );
                    }
                    ledger.extend(entries);
                    persist_ledger(&ledger_path, &ledger)?;
                    for (path, name, _) in batch {
                        archive_input(&path, &done, &name)?;
                        outcome.processed += 1;
                    }
                    outcome.artifacts.extend(run.artifacts);
                }
                Err(ToolError::Interrupted) => return Ok(outcome),
                Err(error) => {
                    let error = error.scrubbed(&recipe.secrets.scrubber());
                    for (path, name, _) in batch {
                        archive_failure(&path, &failed, &name, &error)?;
                        outcome.failed += 1;
                    }
                }
            }
        }
        if request.once {
            return Ok(outcome);
        }
        // Avoid busy polling when settle_ms is zero or the directory is empty.
        tokio::select! {
            () = recipe.cancel.cancelled() => return Ok(outcome),
            () = tokio::time::sleep(delay.max(Duration::from_millis(10))) => {},
        }
    }
}

fn archive_failure(
    path: &Path,
    failed: &Path,
    name: &std::ffi::OsStr,
    error: &ToolError,
) -> Result<(), ToolError> {
    let archived = archive_input(path, failed, name)?;
    let mut report_path = archived.into_os_string();
    report_path.push(".error.json");
    let report = serde_json::json!({"code":error.code(), "message":error.to_string()});
    fs::write(
        PathBuf::from(report_path),
        serde_json::to_vec_pretty(&report).map_err(|e| ToolError::OutputIo(e.to_string()))?,
    )
    .map_err(output_io)
}

fn content_hash(request: &WatchRequest, path: &Path) -> Result<String, ToolError> {
    let mut file = crate::convert::open_regular_input(path)
        .map_err(|_| ToolError::InputIo("watch input is not a readable regular file".into()))?;
    if let Some(max) = request.recipe.limits.max_input_bytes
        && file.metadata().map_err(input_io)?.len() > max
    {
        return Err(ToolError::LimitExceeded);
    }
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut bytes_read = 0_u64;
    loop {
        if request.recipe.cancel.is_cancelled() {
            return Err(ToolError::Interrupted);
        }
        let count = file.read(&mut buffer).map_err(input_io)?;
        if count == 0 {
            break;
        }
        bytes_read = bytes_read.saturating_add(count as u64);
        if request
            .recipe
            .limits
            .max_input_bytes
            .is_some_and(|max| bytes_read > max)
        {
            return Err(ToolError::LimitExceeded);
        }
        check_quota(request.recipe.plan, 1, bytes_read)?;
        hasher.update(&buffer[..count]);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn check_quota(plan: Plan, files: usize, bytes: u64) -> Result<(), ToolError> {
    if within_run_quota(plan, RunSize { files, bytes }) {
        Ok(())
    } else {
        Err(ToolError::TierNotAllowed {
            tool: "watch".into(),
            tier: Tier::Pro,
            plan,
        })
    }
}
fn snapshot(dir: &Path) -> Result<BTreeMap<std::ffi::OsString, Fingerprint>, ToolError> {
    let mut entries = BTreeMap::new();
    for entry in fs::read_dir(dir).map_err(input_io)? {
        let entry = entry.map_err(input_io)?;
        let name = entry.file_name();
        if name.to_string_lossy().starts_with('.') {
            continue;
        }
        if let Some(stamp) = fingerprint(&entry.path())? {
            entries.insert(name, stamp);
        }
    }
    Ok(entries)
}
fn fingerprint(path: &Path) -> Result<Option<Fingerprint>, ToolError> {
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(input_io(e)),
    };
    if !meta.file_type().is_file() {
        return Ok(None);
    }
    Ok(Some(Fingerprint {
        size: meta.len(),
        modified: meta.modified().map_err(input_io)?,
    }))
}
fn ensure_private_subdir(path: &Path) -> Result<(), ToolError> {
    match fs::create_dir(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            let meta = fs::symlink_metadata(path).map_err(input_io)?;
            if meta.file_type().is_dir() {
                Ok(())
            } else {
                Err(ToolError::Confinement(
                    "watch staging paths must be real directories".into(),
                ))
            }
        }
        Err(e) => Err(output_io(e)),
    }
}
fn archive_input(input: &Path, dir: &Path, name: &std::ffi::OsStr) -> Result<PathBuf, ToolError> {
    let mut target = dir.join(name);
    let mut suffix = 1;
    while target.try_exists().map_err(output_io)? {
        let mut candidate = name.to_os_string();
        candidate.push(format!(".{suffix}"));
        target = dir.join(candidate);
        suffix += 1;
    }
    fs::rename(input, &target).map_err(output_io)?;
    Ok(target)
}
fn canonical_json(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(object) => {
            let sorted: BTreeMap<_, _> = object
                .into_iter()
                .map(|(key, value)| (key, canonical_json(value)))
                .collect();
            serde_json::Value::Object(sorted.into_iter().collect())
        }
        serde_json::Value::Array(values) => {
            serde_json::Value::Array(values.into_iter().map(canonical_json).collect())
        }
        other => other,
    }
}
fn persist_ledger(path: &Path, ledger: &Ledger) -> Result<(), ToolError> {
    let bytes =
        serde_json::to_vec_pretty(ledger).map_err(|e| ToolError::OutputIo(e.to_string()))?;
    let mut temp = tempfile::NamedTempFile::new_in(path.parent().expect("ledger has a parent"))
        .map_err(output_io)?;
    temp.write_all(&bytes).map_err(output_io)?;
    temp.as_file().sync_all().map_err(output_io)?;
    temp.persist(path).map_err(|e| output_io(e.error))?;
    Ok(())
}
fn input_io(error: std::io::Error) -> ToolError {
    ToolError::InputIo(error.to_string())
}
fn output_io(error: std::io::Error) -> ToolError {
    ToolError::OutputIo(error.to_string())
}

// OS locks release on crash and keep concurrent watchers from replacing claims or ledger updates.
fn lock_watch_dir(dir: &Path) -> Result<fs::File, ToolError> {
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(dir.join(".ariad-watch-lock"))
        .map_err(output_io)?;
    file.try_lock()
        .map_err(|e| ToolError::Usage(format!("watch directory lock unavailable: {e}")))?;
    Ok(file)
}
