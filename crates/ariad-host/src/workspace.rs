use std::{
    fs, io,
    path::{Component, Path, PathBuf},
};

#[cfg(windows)]
use std::{
    thread,
    time::{Duration, Instant},
};

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
}

impl Drop for Workspace {
    fn drop(&mut self) {
        if self.temp_dir.is_some() && self.close().is_err() {
            eprintln!("warning[workspace_cleanup_failed]: could not remove temporary workspace");
        }
    }
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
