//! The worker's engine sandbox policy.
//!
//! Engines run out of process through `ariad_host`. The host owns the Linux
//! launcher (Landlock, seccomp and rlimits); this module is the seam that
//! turns the worker's configured mode into the host's policy and refuses to
//! start when the configuration cannot be honoured.

use std::{fmt, str::FromStr};

use thiserror::Error;

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
    #[error(
        "ARIAD_SANDBOX=required, but this build cannot isolate engines ({0}); \
         use a build with the engine sandbox or set ARIAD_SANDBOX=best-effort"
    )]
    Unavailable(String),
}

/// Isolation features that `ariad_host` applies to engine processes in this
/// build.
fn host_features() -> Vec<&'static str> {
    Vec::new()
}

/// Isolation features a worker needs for `required`.
const REQUIRED_FEATURES: [&str; 3] = ["landlock", "seccomp", "rlimits"];

/// Checks the configured mode against what this build provides.
pub fn preflight(mode: SandboxMode) -> Result<SandboxStatus, SandboxError> {
    let available = host_features();
    let missing: Vec<&'static str> = REQUIRED_FEATURES
        .into_iter()
        .filter(|feature| !available.contains(feature))
        .collect();
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
    fn required_fails_closed_without_host_isolation() {
        if host_features().len() == REQUIRED_FEATURES.len() {
            assert!(preflight(SandboxMode::Required).is_ok());
        } else {
            assert!(matches!(
                preflight(SandboxMode::Required),
                Err(SandboxError::Unavailable(_))
            ));
        }
        let best = preflight(SandboxMode::BestEffort).expect("best effort");
        assert_eq!(best.mode, SandboxMode::BestEffort);
        assert!(preflight(SandboxMode::Off).expect("off").missing.is_empty());
    }
}
