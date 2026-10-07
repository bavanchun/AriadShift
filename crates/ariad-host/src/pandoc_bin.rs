use std::{env, ffi::OsStr, path::PathBuf};

use thiserror::Error;

/// Pandoc versions accepted by the engine protocol implementation.
pub const PANDOC_SUPPORTED: &str = ">= 3.12, < 4";

/// Version used when producing reproducible golden documents.
pub const PANDOC_GOLDEN_VERSION: &str = "3.12";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PandocBinary {
    pub path: PathBuf,
    pub version: String,
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum PandocBinaryError {
    #[error("Pandoc was not found. Install Pandoc with `just pandoc` or set ASHIFT_PANDOC.")]
    Missing,
    #[error(
        "Pandoc must be {PANDOC_SUPPORTED}; install it with `just pandoc` or set ASHIFT_PANDOC."
    )]
    UnsupportedVersion { found: Option<String> },
    #[error("Pandoc timed out while checking version")]
    TimedOut,
    #[error("operation was interrupted")]
    Interrupted,
}

/// Resolves Pandoc from an explicit override or the process PATH and checks its version.
pub fn locate() -> Result<PandocBinary, PandocBinaryError> {
    locate_with(None, true)
}

/// Resolves Pandoc from an explicit override or the process PATH with optional cancellation.
pub fn locate_with_cancel(
    cancel: Option<&tokio_util::sync::CancellationToken>,
) -> Result<PandocBinary, PandocBinaryError> {
    locate_with(cancel, true)
}

/// Resolves Pandoc from an explicit override or the process PATH with optional cancellation and group control.
///
/// Host-side callers (such as `doctor`) pass `own_group = true` so the probe leads its own process
/// group and timeout kills the probe tree without affecting the host.
/// In-engine callers (`ashift __engine pandoc`) pass `own_group = false` so the probe remains
/// in the engine process group managed and reaped by `runner`.
pub fn locate_with(
    cancel: Option<&tokio_util::sync::CancellationToken>,
    own_group: bool,
) -> Result<PandocBinary, PandocBinaryError> {
    let path = match env::var_os("ASHIFT_PANDOC").filter(|value| !value.is_empty()) {
        Some(explicit) => {
            let path = PathBuf::from(explicit);
            if !path.is_file() {
                return Err(PandocBinaryError::Missing);
            }
            path
        }
        None => find_in_path(env::var_os("PATH").as_deref()).ok_or(PandocBinaryError::Missing)?,
    };

    use process_wrap::std::CommandWrap;
    #[cfg(windows)]
    use process_wrap::std::JobObject;
    #[cfg(unix)]
    use process_wrap::std::ProcessGroup;
    use std::{
        io::Read,
        process::Stdio,
        time::{Duration, Instant},
    };

    let mut command = CommandWrap::with_new(&path, |cmd| {
        cmd.arg("--version").env_clear();
        if let Some(path_value) = env::var_os("PATH") {
            cmd.env("PATH", path_value);
        }
        #[cfg(windows)]
        if let Some(system_root) = env::var_os("SYSTEMROOT") {
            cmd.env("SYSTEMROOT", system_root);
        }
        cmd.stdout(Stdio::piped()).stderr(Stdio::null());
    });
    if own_group {
        #[cfg(unix)]
        command.wrap(ProcessGroup::leader());
        #[cfg(windows)]
        command.wrap(JobObject);
    }

    let mut child = command.spawn().map_err(|_| PandocBinaryError::Missing)?;
    let start = Instant::now();
    let timeout = Duration::from_secs(5);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if cancel.is_some_and(|c| c.is_cancelled()) {
                    let _ = child.kill();
                    return Err(PandocBinaryError::Interrupted);
                }
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    return Err(PandocBinaryError::TimedOut);
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(_) => {
                let _ = child.kill();
                return Err(PandocBinaryError::Missing);
            }
        }
    };
    if !status.success() {
        return Err(PandocBinaryError::UnsupportedVersion { found: None });
    }
    let mut stdout = Vec::new();
    if let Some(out) = child.stdout().take() {
        let _ = out.take(4096).read_to_end(&mut stdout);
    }
    let version = String::from_utf8_lossy(&stdout)
        .lines()
        .next()
        .and_then(parse_version_line)
        .ok_or(PandocBinaryError::UnsupportedVersion { found: None })?;
    if !is_supported(&version) {
        return Err(PandocBinaryError::UnsupportedVersion {
            found: Some(version),
        });
    }

    Ok(PandocBinary { path, version })
}

fn find_in_path(path: Option<&OsStr>) -> Option<PathBuf> {
    let path = path.filter(|value| !value.is_empty())?;
    #[cfg(windows)]
    let candidates = ["pandoc.exe", "pandoc"];
    #[cfg(not(windows))]
    let candidates = ["pandoc"];

    env::split_paths(path)
        .flat_map(|directory| candidates.iter().map(move |name| directory.join(name)))
        .find(|candidate| candidate.is_file())
}

fn parse_version_line(line: &str) -> Option<String> {
    let mut words = line.split_whitespace();
    if words.next()? != "pandoc" {
        return None;
    }
    let version = words.next()?;
    if version.is_empty()
        || version.split('.').any(|component| {
            component.is_empty() || !component.bytes().all(|byte| byte.is_ascii_digit())
        })
    {
        return None;
    }
    Some(version.to_owned())
}

fn is_supported(version: &str) -> bool {
    let Some((major, minor)) = version.split_once('.') else {
        return false;
    };
    let Ok(major) = major.parse::<u32>() else {
        return false;
    };
    let Ok(minor) = minor.split('.').next().unwrap_or_default().parse::<u32>() else {
        return false;
    };
    major == 3 && minor >= 12
}

#[cfg(test)]
mod tests {
    use super::{PANDOC_GOLDEN_VERSION, PANDOC_SUPPORTED, is_supported, parse_version_line};

    #[test]
    fn exposes_the_runtime_and_golden_version_contracts() {
        assert_eq!(PANDOC_SUPPORTED, ">= 3.12, < 4");
        assert_eq!(PANDOC_GOLDEN_VERSION, "3.12");
    }

    #[test]
    fn parses_only_a_stable_pandoc_version_line() {
        assert_eq!(parse_version_line("pandoc 3.12\n"), Some("3.12".to_owned()));
        assert_eq!(
            parse_version_line("pandoc 3.12.1\n"),
            Some("3.12.1".to_owned())
        );
        assert_eq!(parse_version_line("Pandoc 3.12"), None);
        assert_eq!(parse_version_line("pandoc 3.12-rc1"), None);
    }

    #[test]
    fn enforces_the_supported_version_range() {
        assert!(is_supported("3.12"));
        assert!(is_supported("3.12.1"));
        assert!(is_supported("3.99.0"));
        assert!(!is_supported("3.11.9"));
        assert!(!is_supported("4.0"));
        assert!(!is_supported("4.0.0"));
        assert!(!is_supported("3"));
    }
}
