use std::{
    fs, io,
    path::{Component, Path, PathBuf},
    time::Duration,
};

#[cfg(windows)]
use std::{thread, time::Instant};

use tempfile::{NamedTempFile, TempDir};
use thiserror::Error;

const WORKSPACE_PREFIX: &str = "ariadshift-";

#[derive(Debug, Error)]
pub enum WorkspaceError {
    #[error("workspace I/O failed")]
    Io(#[from] io::Error),
    #[error("workspace cleanup failed")]
    Cleanup(#[source] io::Error),
    #[error("artifact is outside the workspace output directory")]
    ArtifactOutsideOutput,
    #[error("artifact is not a regular file")]
    ArtifactNotFile,
    #[error("destination already exists")]
    DestinationExists,
    #[error("atomic promotion failed")]
    Promotion(#[source] io::Error),
}

/// A private workspace with the protocol's standard directory layout.
pub struct Workspace {
    temp_dir: Option<TempDir>,
    root: PathBuf,
    input_dir: PathBuf,
    output_dir: PathBuf,
    work_dir: PathBuf,
    log_dir: PathBuf,
}

impl Workspace {
    pub fn new() -> Result<Self, WorkspaceError> {
        let temp_dir = tempfile::Builder::new()
            .prefix(WORKSPACE_PREFIX)
            .tempdir()?;
        let root = temp_dir.path().to_path_buf();
        let input_dir = root.join("in");
        let output_dir = root.join("out");
        let work_dir = root.join("tmp");
        let log_dir = root.join("log");

        for directory in [&input_dir, &output_dir, &work_dir, &log_dir] {
            fs::create_dir(directory)?;
        }

        Ok(Self {
            temp_dir: Some(temp_dir),
            root,
            input_dir,
            output_dir,
            work_dir,
            log_dir,
        })
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    #[must_use]
    pub fn input_dir(&self) -> &Path {
        &self.input_dir
    }

    #[must_use]
    pub fn output_dir(&self) -> &Path {
        &self.output_dir
    }

    #[must_use]
    pub fn work_dir(&self) -> &Path {
        &self.work_dir
    }

    #[must_use]
    pub fn log_dir(&self) -> &Path {
        &self.log_dir
    }

    /// Removes the workspace, retrying Windows sharing violations for up to five seconds.
    pub fn close(&mut self) -> Result<(), WorkspaceError> {
        let Some(temp_dir) = self.temp_dir.as_ref() else {
            return Ok(());
        };

        remove_workspace(temp_dir.path()).map_err(WorkspaceError::Cleanup)?;
        self.temp_dir.take();
        Ok(())
    }

    /// Atomically promotes a completed output artifact into the destination directory.
    pub fn promote(
        &self,
        artifact: &Path,
        destination: &Path,
        overwrite: bool,
    ) -> Result<(), WorkspaceError> {
        let canonical_output = fs::canonicalize(&self.output_dir)?;
        let canonical_artifact = fs::canonicalize(artifact)?;
        let relative = canonical_artifact
            .strip_prefix(&canonical_output)
            .map_err(|_| WorkspaceError::ArtifactOutsideOutput)?;
        if !relative
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
        {
            return Err(WorkspaceError::ArtifactOutsideOutput);
        }
        if !fs::metadata(&canonical_artifact)?.is_file() {
            return Err(WorkspaceError::ArtifactNotFile);
        }

        if !overwrite && destination_exists(destination)? {
            return Err(WorkspaceError::DestinationExists);
        }

        let destination_parent = destination
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let mut temporary = NamedTempFile::new_in(destination_parent)?;
        let mut source = fs::File::open(canonical_artifact)?;
        io::copy(&mut source, temporary.as_file_mut())?;
        temporary.as_file().sync_all()?;

        let result = if overwrite {
            temporary.persist(destination)
        } else {
            temporary.persist_noclobber(destination)
        };
        result.map(|_| ()).map_err(|error| {
            if !overwrite && error.error.kind() == io::ErrorKind::AlreadyExists {
                WorkspaceError::DestinationExists
            } else {
                WorkspaceError::Promotion(error.error)
            }
        })
    }

    /// Atomically promotes a completed output artifact into a destination directory capability handle.
    pub fn promote_into(
        &self,
        artifact: &Path,
        parent_dir: &cap_std::fs::Dir,
        file_name: &Path,
        overwrite: bool,
    ) -> Result<(), WorkspaceError> {
        let canonical_output = fs::canonicalize(&self.output_dir)?;
        let canonical_artifact = fs::canonicalize(artifact)?;
        let relative = canonical_artifact
            .strip_prefix(&canonical_output)
            .map_err(|_| WorkspaceError::ArtifactOutsideOutput)?;
        if !relative
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
        {
            return Err(WorkspaceError::ArtifactOutsideOutput);
        }
        if !fs::metadata(&canonical_artifact)?.is_file() {
            return Err(WorkspaceError::ArtifactNotFile);
        }

        // Validate that file_name is a single normal component
        if file_name.components().count() != 1
            || !matches!(file_name.components().next(), Some(Component::Normal(_)))
        {
            return Err(WorkspaceError::Promotion(io::Error::new(
                io::ErrorKind::InvalidInput,
                "file_name must be a single path component",
            )));
        }

        match parent_dir.symlink_metadata(file_name) {
            Ok(meta) => {
                if meta.is_symlink() {
                    return Err(WorkspaceError::Promotion(io::Error::new(
                        io::ErrorKind::AlreadyExists,
                        "destination is a symlink",
                    )));
                }
                if !overwrite {
                    return Err(WorkspaceError::DestinationExists);
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(WorkspaceError::Io(error)),
        }

        // Sweep stale promote tmp files in parent_dir older than 24h
        let _ = sweep_stale_promote_tmp_in_dir(parent_dir, Duration::from_secs(24 * 3600));

        let temp_name = format!(".ariadshift-promote-{}.tmp", uuid::Uuid::new_v4());
        let write_result = (|| -> io::Result<()> {
            let mut temporary = parent_dir.create(&temp_name)?;
            let mut source = fs::File::open(&canonical_artifact)?;
            io::copy(&mut source, &mut temporary)?;
            temporary.sync_all()?;
            drop(temporary);

            if overwrite {
                parent_dir.rename(&temp_name, parent_dir, file_name)
            } else {
                parent_dir.hard_link(&temp_name, parent_dir, file_name)?;
                let _ = parent_dir.remove_file(&temp_name);
                Ok(())
            }
        })();

        if let Err(err) = write_result {
            let _ = parent_dir.remove_file(&temp_name);
            if err.kind() == io::ErrorKind::AlreadyExists {
                return Err(WorkspaceError::DestinationExists);
            }
            return Err(WorkspaceError::Promotion(err));
        }

        Ok(())
    }
}

fn sweep_stale_promote_tmp_in_dir(
    dir: &cap_std::fs::Dir,
    older_than: Duration,
) -> io::Result<usize> {
    let mut count = 0;
    if let Ok(entries) = dir.entries() {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.starts_with(".ariadshift-promote-")
                && name_str.ends_with(".tmp")
                && let Ok(meta) = entry.metadata()
            {
                let is_stale = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.into_std().elapsed().ok())
                    .is_some_and(|elapsed| elapsed >= older_than);
                if is_stale && dir.remove_file(&name).is_ok() {
                    count += 1;
                }
            }
        }
    }
    Ok(count)
}

impl Drop for Workspace {
    fn drop(&mut self) {
        if self.temp_dir.is_some() && self.close().is_err() {
            eprintln!("warning[workspace_cleanup_failed]: could not remove temporary workspace");
        }
    }
}

/// Counts stale `ariadshift-*` workspaces in `dir` that are older than `older_than`
/// and owned by the current user without removing them.
pub fn count_stale_in(
    dir: &Path,
    older_than: std::time::Duration,
) -> Result<usize, WorkspaceError> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(WorkspaceError::Io(e)),
    };

    #[cfg(unix)]
    let current_uid = {
        use std::os::unix::fs::MetadataExt;
        match tempfile::NamedTempFile::new_in(dir)
            .ok()
            .and_then(|t| t.path().metadata().ok())
            .map(|m| m.uid())
        {
            Some(uid) => uid,
            None => return Ok(0),
        }
    };

    let mut count = 0;
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let name_str = file_name.to_string_lossy();
        if !name_str.starts_with(WORKSPACE_PREFIX) {
            continue;
        }

        let path = entry.path();
        let metadata = match fs::symlink_metadata(&path) {
            Ok(meta) => meta,
            Err(_) => continue,
        };

        if metadata.is_symlink() || !metadata.is_dir() {
            continue;
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if metadata.uid() != current_uid {
                continue;
            }
        }

        let is_stale = metadata
            .modified()
            .or_else(|_| metadata.created())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|elapsed| elapsed >= older_than);

        if is_stale {
            count += 1;
        }
    }

    Ok(count)
}

/// Counts stale `ariadshift-*` workspaces in the temporary directory that are older than `older_than`
/// and owned by the current user without removing them.
pub fn count_stale(older_than: std::time::Duration) -> Result<usize, WorkspaceError> {
    count_stale_in(&std::env::temp_dir(), older_than)
}

/// Sweeps stale `ariadshift-*` workspaces in `dir` that are older than `older_than`
/// and owned by the current user. Returns the count of removed workspaces.
pub fn sweep_stale_in(
    dir: &Path,
    older_than: std::time::Duration,
) -> Result<usize, WorkspaceError> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(WorkspaceError::Io(e)),
    };

    #[cfg(unix)]
    let current_uid = {
        use std::os::unix::fs::MetadataExt;
        match tempfile::NamedTempFile::new_in(dir)
            .ok()
            .and_then(|t| t.path().metadata().ok())
            .map(|m| m.uid())
        {
            Some(uid) => uid,
            None => return Ok(0), // Fail-safe: refuse to sweep if process identity cannot be proven
        }
    };

    let mut swept = 0;
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let name_str = file_name.to_string_lossy();

        let path = entry.path();
        let metadata = match fs::symlink_metadata(&path) {
            Ok(meta) => meta,
            Err(_) => continue,
        };

        // Sweep promote tmp leftovers
        if name_str.starts_with(".ariadshift-promote-") && name_str.ends_with(".tmp") {
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                if metadata.uid() != current_uid {
                    continue;
                }
            }
            if metadata.is_symlink() || !metadata.is_file() {
                continue;
            }
            let is_stale = metadata
                .modified()
                .or_else(|_| metadata.created())
                .ok()
                .and_then(|t| t.elapsed().ok())
                .is_some_and(|elapsed| elapsed >= older_than);
            if is_stale && fs::remove_file(&path).is_ok() {
                swept += 1;
            }
            continue;
        }

        if !name_str.starts_with(WORKSPACE_PREFIX) {
            continue;
        }

        if metadata.is_symlink() || !metadata.is_dir() {
            continue;
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if metadata.uid() != current_uid {
                continue;
            }
        }

        let is_stale = metadata
            .modified()
            .or_else(|_| metadata.created())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|elapsed| elapsed >= older_than);

        if is_stale && remove_workspace(&path).is_ok() {
            swept += 1;
        }
    }

    Ok(swept)
}

/// Sweeps stale `ariadshift-*` workspaces in the temporary directory that are older than `older_than`
/// and owned by the current user. Returns the count of removed workspaces.
pub fn sweep_stale(older_than: std::time::Duration) -> Result<usize, WorkspaceError> {
    sweep_stale_in(&std::env::temp_dir(), older_than)
}

fn destination_exists(destination: &Path) -> Result<bool, WorkspaceError> {
    match fs::symlink_metadata(destination) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(WorkspaceError::Io(error)),
    }
}

#[cfg(not(windows))]
fn remove_workspace(path: &Path) -> io::Result<()> {
    match fs::remove_dir_all(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}

#[cfg(windows)]
fn remove_workspace(path: &Path) -> io::Result<()> {
    let started = Instant::now();
    let timeout = Duration::from_secs(5);
    let mut backoff = Duration::from_millis(25);
    loop {
        match fs::remove_dir_all(path) {
            Ok(()) => return Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) if started.elapsed() >= timeout => return Err(error),
            Err(_) => {
                let remaining = timeout.saturating_sub(started.elapsed());
                thread::sleep(backoff.min(remaining));
                backoff = backoff.saturating_mul(2).min(Duration::from_millis(500));
            }
        }
    }
}
