use std::{env, ffi::OsStr, path::PathBuf, time::Duration};

use thiserror::Error;

/// Pandoc versions accepted by the engine protocol implementation.
pub const PANDOC_SUPPORTED: &str = ">= 3.12, < 4";

/// Version used when producing reproducible golden documents.
pub const PANDOC_GOLDEN_VERSION: &str = "3.12";

/// Default timeout for `pandoc --version` probes.
///
/// Sized at `15s` from measured `macos-26` CI telemetry in run `43605221838`: in job
/// `132509183838`, the first cold `pandoc` invocation (`cli_interrupt_signal_handling`,
/// `20/2885`) took `5.364s` under concurrent nextest load inside `sandbox-exec` while warm runs
/// took `0.608s`–`2.011s`, and attempt 1 (`job 132478519346`) hit the old `5.000s` cliff
/// (`5.00s` + `~2.99s` fallback probes = `8.059s`). `15s` provides ~2.8x headroom over the
/// measured `5.364s` cold start while staying at half of `runner::DESCRIBE_TIMEOUT` (`30s`), so
/// a stalled probe still yields `PandocBinaryError::TimedOut` cleanly before the outer host
/// watchdog kills the engine.
const DEFAULT_VERSION_TIMEOUT: Duration = Duration::from_secs(15);

/// Test-only environment variable overriding the Pandoc version probe timeout (milliseconds).
#[cfg(any(test, feature = "test-probe"))]
pub(crate) const TEST_TIMEOUT_ENV: &str = "ARIAD_TEST_PANDOC_PROBE_TIMEOUT_MS";

fn default_version_timeout() -> Duration {
    #[cfg(any(test, feature = "test-probe"))]
    if let Some(raw) = env::var_os(TEST_TIMEOUT_ENV)
        && let Some(ms) = raw.to_str().and_then(|value| value.parse::<u64>().ok())
    {
        return Duration::from_millis(ms);
    }
    DEFAULT_VERSION_TIMEOUT
}

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
    locate_with_timeout(cancel, own_group, default_version_timeout())
}

/// Resolves Pandoc with an explicit probe timeout, cancellation token, and group control.
pub fn locate_with_timeout(
    cancel: Option<&tokio_util::sync::CancellationToken>,
    own_group: bool,
    timeout: Duration,
) -> Result<PandocBinary, PandocBinaryError> {
    let path = resolve_path().ok_or(PandocBinaryError::Missing)?;

    use process_wrap::std::CommandWrap;
    #[cfg(windows)]
    use process_wrap::std::JobObject;
    #[cfg(unix)]
    use process_wrap::std::ProcessGroup;
    use std::{io::Read, process::Stdio, time::Instant};

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
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if cancel.is_some_and(|c| c.is_cancelled()) {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(PandocBinaryError::Interrupted);
                }
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(PandocBinaryError::TimedOut);
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
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

/// Resolves the Pandoc executable from `ASHIFT_PANDOC` or `PATH` without running it.
#[must_use]
pub fn resolve_path() -> Option<PathBuf> {
    match env::var_os("ASHIFT_PANDOC").filter(|value| !value.is_empty()) {
        Some(explicit) => Some(PathBuf::from(explicit)).filter(|path| path.is_file()),
        None => find_in_path(env::var_os("PATH").as_deref()),
    }
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

    #[test]
    fn default_version_timeout_is_15s_and_honors_test_override() {
        use std::{env, process::Command, time::Duration};

        const CHILD: &str = "ARIAD_TEST_PANDOC_TIMEOUT_DEFAULT_CHILD";
        if env::var_os(CHILD).is_some() {
            assert_eq!(super::default_version_timeout(), Duration::from_millis(123));
            return;
        }
        assert_eq!(super::DEFAULT_VERSION_TIMEOUT, Duration::from_secs(15));
        let status = Command::new(env::current_exe().unwrap())
            .args([
                "--exact",
                "pandoc_bin::tests::default_version_timeout_is_15s_and_honors_test_override",
            ])
            .env(CHILD, "1")
            .env(super::TEST_TIMEOUT_ENV, "123")
            .status()
            .unwrap();
        assert!(status.success());

        // When unset, default_version_timeout falls back to 15 seconds.
        const UNSET_CHILD: &str = "ARIAD_TEST_PANDOC_TIMEOUT_UNSET_CHILD";
        let unset_status = Command::new(env::current_exe().unwrap())
            .args([
                "--exact",
                "pandoc_bin::tests::default_version_timeout_returns_15s_when_unset",
            ])
            .env(UNSET_CHILD, "1")
            .env_remove(super::TEST_TIMEOUT_ENV)
            .status()
            .unwrap();
        assert!(unset_status.success());
    }

    #[test]
    fn default_version_timeout_returns_15s_when_unset() {
        if std::env::var_os("ARIAD_TEST_PANDOC_TIMEOUT_UNSET_CHILD").is_none() {
            return;
        }
        assert_eq!(
            super::default_version_timeout(),
            std::time::Duration::from_secs(15)
        );
    }

    #[cfg(unix)]
    fn write_executable_stub(dir: &std::path::Path, name: &str, body: &str) -> std::path::PathBuf {
        use std::os::unix::fs::PermissionsExt;

        let path = dir.join(name);
        std::fs::write(&path, body).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[cfg(unix)]
    #[test]
    fn locate_with_timeout_accepts_slow_stub_within_15s() {
        use std::{env, process::Command, time::Duration};

        const CHILD: &str = "ARIAD_TEST_SLOW_PANDOC_CHILD";
        if env::var_os(CHILD).is_some() {
            let binary = super::locate_with_timeout(None, true, Duration::from_secs(15))
                .expect("slow stub must succeed within 15s");
            assert_eq!(binary.version, "3.12");
            let via_default = super::locate().expect("default 15s timeout must also succeed");
            assert_eq!(via_default.version, "3.12");
            return;
        }

        let dir = tempfile::tempdir().unwrap();
        let stub = write_executable_stub(
            dir.path(),
            "pandoc-slow",
            "#!/bin/sh\nsleep 0.15\nprintf 'pandoc 3.12\\n'\n",
        );
        let status = Command::new(env::current_exe().unwrap())
            .args([
                "--exact",
                "pandoc_bin::tests::locate_with_timeout_accepts_slow_stub_within_15s",
            ])
            .env(CHILD, "1")
            .env("ASHIFT_PANDOC", &stub)
            .env_remove(super::TEST_TIMEOUT_ENV)
            .status()
            .unwrap();
        assert!(status.success());
    }

    #[cfg(unix)]
    #[test]
    fn locate_with_timeout_enforces_short_timeout_and_reaps_killed_child() {
        use std::{
            env, fs,
            process::Command,
            time::{Duration, Instant},
        };

        const CHILD: &str = "ARIAD_TEST_HANG_PANDOC_CHILD";
        if let Some(pid_path) = env::var_os(CHILD) {
            let pid_path = std::path::PathBuf::from(pid_path);
            let started = Instant::now();
            let err = super::locate_with_timeout(None, true, Duration::from_millis(100))
                .expect_err("hanging stub must time out");
            assert_eq!(err, super::PandocBinaryError::TimedOut);
            assert!(
                started.elapsed() < Duration::from_secs(1),
                "timeout must return promptly, took {:?}",
                started.elapsed()
            );
            if let Ok(raw_pid) = fs::read_to_string(&pid_path)
                && let Some(pid) = raw_pid.trim().parse::<u32>().ok().map(|n| n.to_string())
            {
                let alive = Command::new("kill")
                    .args(["-0", &pid])
                    .status()
                    .map(|status| status.success())
                    .unwrap_or(false);
                assert!(!alive, "timed-out probe child {pid} was not reaped");
            }

            // Deterministic reap check via cancellation once pid file is written.
            let _ = fs::remove_file(&pid_path);
            let token = tokio_util::sync::CancellationToken::new();
            let cancel_token = token.clone();
            let watch_path = pid_path.clone();
            let watcher = std::thread::spawn(move || {
                let deadline = Instant::now() + Duration::from_secs(2);
                while Instant::now() < deadline {
                    if let Ok(text) = fs::read_to_string(&watch_path)
                        && text.contains('\n')
                    {
                        cancel_token.cancel();
                        return true;
                    }
                    std::thread::sleep(Duration::from_millis(2));
                }
                cancel_token.cancel();
                false
            });
            let cancel_err =
                super::locate_with_timeout(Some(&token), false, Duration::from_secs(5))
                    .expect_err("cancelled stub must interrupt");
            assert_eq!(cancel_err, super::PandocBinaryError::Interrupted);
            assert!(watcher.join().unwrap(), "stub should have recorded its pid");
            let raw_pid = fs::read_to_string(&pid_path).unwrap();
            let pid = raw_pid.trim().parse::<u32>().unwrap().to_string();
            let alive = Command::new("kill")
                .args(["-0", &pid])
                .status()
                .map(|status| status.success())
                .unwrap_or(false);
            assert!(!alive, "cancelled probe child {pid} was not reaped");
            return;
        }

        let dir = tempfile::tempdir().unwrap();
        let pid_path = dir.path().join("hang.pid");
        let stub = write_executable_stub(
            dir.path(),
            "pandoc-hang",
            &format!(
                "#!/bin/sh\necho $$ > '{}'\nexec sleep 30\n",
                pid_path.display()
            ),
        );
        let status = Command::new(env::current_exe().unwrap())
            .args([
                "--exact",
                "pandoc_bin::tests::locate_with_timeout_enforces_short_timeout_and_reaps_killed_child",
            ])
            .env(CHILD, &pid_path)
            .env("ASHIFT_PANDOC", &stub)
            .status()
            .unwrap();
        assert!(status.success());
    }
}
