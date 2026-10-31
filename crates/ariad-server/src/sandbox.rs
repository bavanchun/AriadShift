//! The worker's engine sandbox policy.
//!
//! Engines run out of process through `ariad_host`. The host owns the Linux
//! launcher (Landlock, seccomp and rlimits); this module is the seam that
//! turns the worker's configured mode into the host's policy and refuses to
//! start when the configuration cannot be honoured.

use std::{fmt, str::FromStr};

use thiserror::Error;

/// Denies same-user procfs/ptrace access to worker memory, environment and
/// descriptors. Unlike namespaces or remounting procfs, this needs no container
/// capabilities. Apply before starting any conversion helpers.
pub fn protect_worker_credentials() -> std::io::Result<()> {
    #[cfg(target_os = "linux")]
    nix::sys::prctl::set_dumpable(false).map_err(std::io::Error::from)?;
    Ok(())
}

/// Blocks network socket creation in the single-threaded helper before it
/// starts the host runtime. Children inherit the filter across exec; UNIX
/// sockets remain available for local runtime IPC. Filesystem and procfs
/// access remain unchanged so engine launch and RSS monitoring still work.
pub fn deny_helper_network() -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        use nix::libc;
        use seccompiler::{
            BpfProgram, SeccompAction, SeccompCmpArgLen, SeccompCmpOp, SeccompCondition,
            SeccompFilter, SeccompRule,
        };
        let condition = SeccompCondition::new(
            0,
            SeccompCmpArgLen::Dword,
            SeccompCmpOp::Ne,
            libc::AF_UNIX as u64,
        )
        .map_err(|e| e.to_string())?;
        let rule = SeccompRule::new(vec![condition]).map_err(|e| e.to_string())?;
        #[allow(unused_mut)]
        let mut rules = std::collections::BTreeMap::from([
            (libc::SYS_socket, vec![rule.clone()]),
            (libc::SYS_io_uring_setup, Vec::new()),
        ]);
        #[cfg(target_arch = "x86_64")]
        {
            // Cover the x32 syscall encoding as well as native x86_64.
            const X32: i64 = 0x4000_0000;
            rules.insert(X32 + libc::SYS_socket, vec![rule]);
            rules.insert(X32 + libc::SYS_io_uring_setup, Vec::new());
        }
        let program: BpfProgram = SeccompFilter::new(
            rules,
            SeccompAction::Allow,
            SeccompAction::Errno(libc::EPERM as u32),
            std::env::consts::ARCH
                .try_into()
                .map_err(|e: seccompiler::BackendError| e.to_string())?,
        )
        .map_err(|e| e.to_string())?
        .try_into()
        .map_err(|e: seccompiler::BackendError| e.to_string())?;
        seccompiler::apply_filter(&program).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SandboxMode {
    /// A missing isolation feature is an error. The default for Linux workers.
    Required,
    /// Apply what the platform supports and report the rest.
    BestEffort,
    /// No engine isolation beyond separate processes.
    Off,
}

impl SandboxMode {
    /// `required` on Linux, where the cloud workers run; `best-effort` elsewhere.
    #[must_use]
    pub const fn platform_default() -> Self {
        if cfg!(target_os = "linux") {
            Self::Required
        } else {
            Self::BestEffort
        }
    }

    #[must_use]
    pub const fn host_mode(self) -> ariad_host::sandbox::SandboxMode {
        match self {
            Self::Required => ariad_host::sandbox::SandboxMode::Required,
            Self::BestEffort => ariad_host::sandbox::SandboxMode::BestEffort,
            Self::Off => ariad_host::sandbox::SandboxMode::Off,
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Required => "required",
            Self::BestEffort => "best-effort",
            Self::Off => "off",
        }
    }
}

impl fmt::Display for SandboxMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for SandboxMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "required" => Ok(Self::Required),
            "best-effort" => Ok(Self::BestEffort),
            "off" => Ok(Self::Off),
            other => Err(format!(
                "unknown sandbox mode '{other}'; expected required, best-effort or off"
            )),
        }
    }
}

/// What the worker could apply.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SandboxStatus {
    pub mode: SandboxMode,
    /// Isolation features the platform could not provide.
    pub missing: Vec<&'static str>,
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum SandboxError {
    #[error("ARIAD_SANDBOX=required, but this host cannot isolate engines ({0})")]
    Unavailable(String),
    #[error(
        "ARIAD_WORKER_CONCURRENCY must be 1 in best-effort mode without filesystem confinement"
    )]
    UnsafeConcurrency,
}

/// Checks the configured mode against the host's actual isolation contract.
pub fn preflight(mode: SandboxMode) -> Result<SandboxStatus, SandboxError> {
    preflight_with_support(mode, &ariad_host::sandbox::support())
}

/// Checks worker parallelism against actual kernel support. Off is an explicit
/// weaker boundary; required still refuses every missing isolation feature.
pub fn preflight_worker_with_support(
    mode: SandboxMode,
    concurrency: usize,
    support: &ariad_host::sandbox::SandboxSupport,
) -> Result<SandboxStatus, SandboxError> {
    let status = preflight_with_support(mode, support)?;
    #[cfg(target_os = "macos")]
    let filesystem = support.seatbelt_available && support.seatbelt_usable;
    #[cfg(not(target_os = "macos"))]
    let filesystem = support.landlock_filesystem;
    if mode == SandboxMode::BestEffort && concurrency > 1 && !filesystem {
        return Err(SandboxError::UnsafeConcurrency);
    }
    Ok(status)
}

fn preflight_with_support(
    mode: SandboxMode,
    support: &ariad_host::sandbox::SandboxSupport,
) -> Result<SandboxStatus, SandboxError> {
    let missing = support.gaps();
    match mode {
        SandboxMode::Required if !missing.is_empty() => {
            Err(SandboxError::Unavailable(missing.join(", ")))
        }
        SandboxMode::Off => Ok(SandboxStatus {
            mode,
            missing: Vec::new(),
        }),
        _ => Ok(SandboxStatus { mode, missing }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modes_round_trip() {
        for mode in [
            SandboxMode::Required,
            SandboxMode::BestEffort,
            SandboxMode::Off,
        ] {
            assert_eq!(mode.as_str().parse::<SandboxMode>(), Ok(mode));
        }
        assert!("strict".parse::<SandboxMode>().is_err());
    }

    #[test]
    fn required_uses_the_host_confinement_contract() {
        use ariad_host::sandbox::SandboxSupport;
        let unsupported = SandboxSupport::default();
        assert!(matches!(
            preflight_with_support(SandboxMode::Required, &unsupported),
            Err(SandboxError::Unavailable(_))
        ));
        let best = preflight_with_support(SandboxMode::BestEffort, &unsupported).unwrap();
        assert_eq!(best.missing, unsupported.gaps());
        assert!(
            preflight_with_support(SandboxMode::Off, &unsupported)
                .unwrap()
                .missing
                .is_empty()
        );
        let supported = SandboxSupport {
            platform: true,
            #[cfg(target_os = "macos")]
            seatbelt_available: true,
            #[cfg(target_os = "macos")]
            seatbelt_usable: true,
            landlock_filesystem: true,
            seccomp: true,
            ..SandboxSupport::default()
        };
        assert!(
            supported.gaps().is_empty(),
            "fixture covers the platform's isolation contract"
        );
        assert!(
            preflight_with_support(SandboxMode::Required, &supported)
                .unwrap()
                .missing
                .is_empty()
        );
        for mode in [
            SandboxMode::Required,
            SandboxMode::BestEffort,
            SandboxMode::Off,
        ] {
            assert_eq!(mode.as_str(), mode.host_mode().as_str());
        }
    }
}
