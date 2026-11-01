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
    output_handle: Option<cap_std::fs::Dir>,
    backup_dir: PathBuf,
    backup_handle: Option<cap_std::fs::Dir>,
    work_dir: PathBuf,
    log_dir: PathBuf,
}

impl Workspace {
    pub fn new() -> Result<Self, WorkspaceError> {
        Self::with_temp_dir(
            tempfile::Builder::new()
                .prefix(WORKSPACE_PREFIX)
                .tempdir()?,
        )
    }

    /// Creates the workspace inside `parent`, for example a warm engine session's directory.
    pub fn new_in(parent: &Path) -> Result<Self, WorkspaceError> {
        Self::with_temp_dir(
            tempfile::Builder::new()
                .prefix(WORKSPACE_PREFIX)
                .tempdir_in(parent)?,
        )
    }

    fn with_temp_dir(temp_dir: TempDir) -> Result<Self, WorkspaceError> {
        let root = temp_dir.path().to_path_buf();
        let input_dir = root.join("in");
        let output_dir = root.join("out");
        let backup_dir = root.join("backup");
        let work_dir = root.join("tmp");
        let log_dir = root.join("log");

        for directory in [&input_dir, &output_dir, &backup_dir, &work_dir, &log_dir] {
            fs::create_dir(directory)?;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&backup_dir, fs::Permissions::from_mode(0o700))?;
        }

        let output_handle =
            cap_std::fs::Dir::open_ambient_dir(&output_dir, cap_std::ambient_authority())?;
        let backup_handle =
            cap_std::fs::Dir::open_ambient_dir(&backup_dir, cap_std::ambient_authority())?;

        Ok(Self {
            temp_dir: Some(temp_dir),
            root,
            input_dir,
            output_dir,
            output_handle: Some(output_handle),
            backup_dir,
            backup_handle: Some(backup_handle),
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
    pub fn backup_dir(&self) -> &Path {
        &self.backup_dir
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

        self.output_handle.take();
        self.backup_handle.take();
        remove_workspace(temp_dir.path()).map_err(WorkspaceError::Cleanup)?;
        self.temp_dir.take();
        Ok(())
    }

    /// Opens an artifact relative to the output handle captured before the engine starts.
    /// Directory replacements cannot redirect this read outside that original directory.
    pub fn open_artifact(&self, artifact: &Path) -> Result<fs::File, WorkspaceError> {
        if !artifact.is_absolute()
            || artifact
                .components()
                .any(|component| matches!(component, Component::ParentDir))
        {
            return Err(WorkspaceError::ArtifactOutsideOutput);
        }
        let parent = artifact
            .parent()
            .ok_or(WorkspaceError::ArtifactOutsideOutput)?;
        let name = artifact
            .file_name()
            .ok_or(WorkspaceError::ArtifactOutsideOutput)?;
        let canonical_output = fs::canonicalize(&self.output_dir)?;
        let canonical_parent = fs::canonicalize(parent)?;
        let relative_parent = canonical_parent
            .strip_prefix(&canonical_output)
            .map_err(|_| WorkspaceError::ArtifactOutsideOutput)?;
        let relative = relative_parent.join(name);
        let directory = self
            .output_handle
            .as_ref()
            .ok_or_else(|| io::Error::other("workspace is closed"))?;
        let file = crate::confine::open_file_nofollow(directory, &relative)?;
        if !file.metadata()?.is_file() {
            return Err(WorkspaceError::ArtifactNotFile);
        }
        Ok(file)
    }

    /// Atomically promotes a completed output artifact into the destination directory.
    pub fn promote(
        &self,
        artifact: &Path,
        destination: &Path,
        overwrite: bool,
    ) -> Result<(), WorkspaceError> {
        let mut source = self.open_artifact(artifact)?;

        if !overwrite && destination_exists(destination)? {
            return Err(WorkspaceError::DestinationExists);
        }

        let destination_parent = destination
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let mut temporary = NamedTempFile::new_in(destination_parent)?;
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
        let mut source = self.open_artifact(artifact)?;

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

    /// Moves an existing regular file `file_name` from `parent_dir` into the workspace's private
    /// backup directory before it is overwritten, returning the backup filename when a file existed.
    pub fn backup_existing_into(
        &self,
        parent_dir: &cap_std::fs::Dir,
        file_name: &Path,
    ) -> Result<Option<String>, WorkspaceError> {
        ensure_single_normal_component(file_name)?;

        let meta = match parent_dir.symlink_metadata(file_name) {
            Ok(meta) => meta,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(WorkspaceError::Io(error)),
        };

        if meta.is_symlink() {
            return Err(WorkspaceError::Promotion(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "destination is a symlink",
            )));
        }
        if !meta.is_file() {
            return Err(WorkspaceError::Promotion(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "destination is not a regular file",
            )));
        }

        let backup_dir = self
            .backup_handle
            .as_ref()
            .ok_or_else(|| io::Error::other("workspace is closed"))?;
        let backup_name = format!("backup-{}.bak", uuid::Uuid::new_v4());

        match parent_dir.rename(file_name, backup_dir, &backup_name) {
            Ok(()) => Ok(Some(backup_name)),
            Err(err) if is_cross_device(&err) => {
                let mut src = crate::confine::open_file_nofollow(parent_dir, file_name)
                    .map_err(WorkspaceError::Promotion)?;
                let src_meta = src.metadata().map_err(WorkspaceError::Promotion)?;
                if !src_meta.is_file() {
                    return Err(WorkspaceError::Promotion(io::Error::new(
                        io::ErrorKind::AlreadyExists,
                        "destination is not a regular file",
                    )));
                }
                let copy_res = (|| -> io::Result<()> {
                    let mut dst = backup_dir.create(&backup_name)?;
                    io::copy(&mut src, &mut dst)?;
                    let _ = dst.set_permissions(cap_std::fs::Permissions::from_std(
                        src_meta.permissions(),
                    ));
                    dst.sync_all()?;
                    drop(dst);
                    drop(src);
                    parent_dir.remove_file(file_name)?;
                    Ok(())
                })();
                if let Err(copy_err) = copy_res {
                    let _ = backup_dir.remove_file(&backup_name);
                    return Err(WorkspaceError::Promotion(copy_err));
                }
                Ok(Some(backup_name))
            }
            Err(err) => Err(WorkspaceError::Promotion(err)),
        }
    }

    /// Restores a previously backed-up file `backup_name` from the workspace's private backup
    /// directory back to `file_name` inside `parent_dir`.
    pub fn restore_backup_into(
        &self,
        backup_name: &str,
        parent_dir: &cap_std::fs::Dir,
        file_name: &Path,
    ) -> Result<(), WorkspaceError> {
        ensure_single_normal_component(Path::new(backup_name))?;
        ensure_single_normal_component(file_name)?;

        let backup_dir = self
            .backup_handle
            .as_ref()
            .ok_or_else(|| io::Error::other("workspace is closed"))?;

        if backup_dir
            .rename(backup_name, parent_dir, file_name)
            .is_ok()
        {
            return Ok(());
        }

        let temp_name = format!(".ariadshift-restore-{}.tmp", uuid::Uuid::new_v4());
        let restore_res = (|| -> io::Result<()> {
            let mut src = crate::confine::open_file_nofollow(backup_dir, Path::new(backup_name))?;
            let src_meta = src.metadata()?;
            let mut dst = parent_dir.create(&temp_name)?;
            io::copy(&mut src, &mut dst)?;
            let _ = dst.set_permissions(cap_std::fs::Permissions::from_std(src_meta.permissions()));
            dst.sync_all()?;
            drop(dst);
            drop(src);

            #[cfg(windows)]
            let _ = parent_dir.remove_file(file_name);
            parent_dir.rename(&temp_name, parent_dir, file_name)?;
            let _ = backup_dir.remove_file(backup_name);
            Ok(())
        })();

        if let Err(err) = restore_res {
            let _ = parent_dir.remove_file(&temp_name);
            return Err(WorkspaceError::Promotion(err));
        }

        Ok(())
    }

    /// Removes a backup file from the workspace's private backup directory after batch commit.
    pub fn remove_backup(&self, backup_name: &str) -> Result<(), WorkspaceError> {
        ensure_single_normal_component(Path::new(backup_name))?;
        let backup_dir = self
            .backup_handle
            .as_ref()
            .ok_or_else(|| io::Error::other("workspace is closed"))?;
        match backup_dir.remove_file(backup_name) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(WorkspaceError::Cleanup(err)),
        }
    }
}

fn ensure_single_normal_component(path: &Path) -> Result<(), WorkspaceError> {
    if path.components().count() != 1
        || !matches!(path.components().next(), Some(Component::Normal(_)))
    {
        return Err(WorkspaceError::Promotion(io::Error::new(
            io::ErrorKind::InvalidInput,
            "file_name must be a single path component",
        )));
    }
    Ok(())
}

fn is_cross_device(err: &io::Error) -> bool {
    err.kind() == io::ErrorKind::CrossesDevices
        || err.raw_os_error() == Some(18)
        || err.raw_os_error() == Some(17)
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
pub(crate) fn remove_workspace(path: &Path) -> io::Result<()> {
    match fs::remove_dir_all(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}

#[cfg(windows)]
pub(crate) fn remove_workspace(path: &Path) -> io::Result<()> {
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
