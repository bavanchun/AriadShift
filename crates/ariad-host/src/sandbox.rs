//! Engine process isolation.
//!
//! On Linux the runner starts every engine through the `__sandbox` launcher, a hidden subcommand
//! of `ashift`. The launcher restricts itself with Landlock (filesystem, TCP and scopes), a
//! seccomp filter that denies non-Unix sockets, and rlimits, then `exec`s the engine, which
//! inherits every restriction. macOS uses Seatbelt through `/usr/bin/sandbox-exec` when
//! available; Windows keeps Job Objects only.

use std::{
    ffi::OsString,
    fmt,
    io::{self, Write},
    path::{Path, PathBuf},
    process::ExitCode,
    sync::OnceLock,
};

use ariad_core::protocol::{EngineError, ErrorCode, Event};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;

/// Environment variable that overrides the sandbox mode: `off`, `best-effort` or `required`.
pub const MODE_ENV: &str = "ASHIFT_SANDBOX";

/// Exit status of a launcher that could not apply its restrictions or start the engine.
pub const LAUNCH_FAILURE_EXIT: u8 = 127;

/// Fixed prefix identifying a failure reported by the sandbox launcher itself.
pub const LAUNCH_FAILURE_PREFIX: &str = "The engine sandbox could not start the engine: ";

/// Remedy for Linux engines whose private temporary directory needs namespaces.
pub const NAMESPACE_HINT: &str = "allow user and mount namespaces for ashift; on Ubuntu use sudo sysctl -w kernel.apparmor_restrict_unprivileged_userns=0 or an AppArmor profile allowing userns; default container seccomp profiles also block this setup; ASHIFT_SANDBOX=off explicitly disables isolation";

/// How strictly the host isolates engine processes.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum SandboxMode {
    /// Engines run without the launcher.
    Off,
    /// Apply what the kernel supports and report what was skipped (CLI and desktop default).
    BestEffort,
    /// Refuse to start an engine unless filesystem isolation and network denial both apply
    /// (server workers).
    Required,
}

impl SandboxMode {
    /// Parses the `ASHIFT_SANDBOX` spelling of a mode.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "off" => Some(Self::Off),
            "best-effort" => Some(Self::BestEffort),
            "required" => Some(Self::Required),
            _ => None,
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::BestEffort => "best-effort",
            Self::Required => "required",
        }
    }
}

impl fmt::Display for SandboxMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

static DEFAULT_MODE: OnceLock<SandboxMode> = OnceLock::new();

/// Sets the mode used when `ASHIFT_SANDBOX` is unset; the first call wins.
pub fn set_default_mode(mode: SandboxMode) {
    let _ = DEFAULT_MODE.set(mode);
}

/// The mode used when `ASHIFT_SANDBOX` is unset.
#[must_use]
pub fn default_mode() -> SandboxMode {
    DEFAULT_MODE
        .get()
        .copied()
        .unwrap_or(SandboxMode::BestEffort)
}

/// The mode for the next engine: `ASHIFT_SANDBOX` when set, otherwise [`default_mode`].
///
/// An unknown value is an error so a typo can never weaken isolation silently.
pub fn effective_mode() -> Result<SandboxMode, String> {
    match std::env::var_os(MODE_ENV) {
        None => Ok(default_mode()),
        Some(value) if value.is_empty() => Ok(default_mode()),
        Some(value) => value
            .to_str()
            .and_then(SandboxMode::parse)
            .ok_or_else(|| format!("{MODE_ENV} must be off, best-effort or required")),
    }
}

/// Isolation features the running kernel offers to engine processes.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SandboxSupport {
    /// Whether this platform uses the sandbox launcher (Linux or macOS).
    pub platform: bool,
    /// Whether the system Seatbelt launcher exists (macOS only).
    #[cfg(target_os = "macos")]
    #[serde(default)]
    pub seatbelt_available: bool,
    /// Whether a disposable child could apply the Seatbelt profile (macOS only).
    #[cfg(target_os = "macos")]
    #[serde(default)]
    pub seatbelt_usable: bool,
    /// Highest Landlock ABI the kernel and the `landlock` crate share, if Landlock is enabled.
    pub landlock_abi: Option<u8>,
    /// Landlock filesystem rules (ABI 1).
    pub landlock_filesystem: bool,
    /// Landlock TCP bind and connect rules (ABI 4).
    pub landlock_network: bool,
    /// Landlock signal and abstract UNIX socket scoping (ABI 6).
    pub landlock_scopes: bool,
    /// Landlock control of connections to pathname UNIX sockets (ABI 9).
    pub landlock_unix_sockets: bool,
    /// Seccomp filters with errno actions.
    pub seccomp: bool,
}

impl SandboxSupport {
    /// Isolation the sandbox cannot provide on this kernel, as user-facing phrases.
    #[must_use]
    pub fn gaps(&self) -> Vec<&'static str> {
        #[cfg(target_os = "macos")]
        {
            macos::gaps(self)
        }
        #[cfg(not(target_os = "macos"))]
        {
            let mut gaps = Vec::new();
            if !self.landlock_filesystem {
                gaps.push(
                    "Landlock is unavailable, so the engine is not confined to its workspace",
                );
            }
            if !self.seccomp {
                gaps.push(if self.landlock_network {
                "seccomp is unavailable, so only TCP network access is blocked"
            } else {
                "neither Landlock network rules nor seccomp are available, so network access is not blocked"
            });
            }
            gaps
        }
    }
}

static SUPPORT: OnceLock<SandboxSupport> = OnceLock::new();

/// Probes the kernel once per process without restricting the caller.
#[must_use]
pub fn support() -> SandboxSupport {
    SUPPORT.get_or_init(probe_support).clone()
}

fn probe_support() -> SandboxSupport {
    #[cfg(debug_assertions)]
    if simulated_without_support() {
        return SandboxSupport {
            platform: cfg!(any(target_os = "linux", target_os = "macos")),
            ..SandboxSupport::default()
        };
    }
    #[cfg(target_os = "linux")]
    {
        linux::probe()
    }
    #[cfg(target_os = "macos")]
    {
        macos::probe()
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        SandboxSupport::default()
    }
}

/// Debug builds treat `ARIAD_TEST_SANDBOX_SUPPORT=none` as a kernel without Landlock or
/// seccomp, or a Mac without sandbox-exec. On macOS, `seatbelt-refused` instead keeps the
/// launcher available but makes every enforcement probe fail.
#[cfg(debug_assertions)]
fn simulated_without_support() -> bool {
    std::env::var_os("ARIAD_TEST_SANDBOX_SUPPORT").is_some_and(|value| value == "none")
}

#[cfg(all(debug_assertions, target_os = "macos"))]
fn simulated_seatbelt_refusal() -> bool {
    std::env::var_os("ARIAD_TEST_SANDBOX_SUPPORT").is_some_and(|value| value == "seatbelt-refused")
}

/// The isolation one engine process received.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SandboxOutcome {
    /// The mode is `off`.
    Off,
    /// The platform does not use the launcher (Windows).
    NotApplicable,
    /// Every isolation feature the sandbox relies on was applied.
    Applied,
    /// Best-effort mode skipped part of the isolation; the message names what is missing.
    Degraded(String),
}

impl SandboxOutcome {
    /// A user-facing description of the missing isolation, if any.
    #[must_use]
    pub fn gap(&self) -> Option<&str> {
        match self {
            Self::Degraded(message) => Some(message),
            Self::Off | Self::NotApplicable | Self::Applied => None,
        }
    }
}

/// Filesystem paths and limits the launcher applies to one engine process.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SandboxPolicy {
    /// Workspace backing private `/tmp` on Linux, or a dedicated IPC directory on macOS.
    pub private_tmp: Option<PathBuf>,
    /// Paths the engine may read.
    pub read_only: Vec<PathBuf>,
    /// Paths the engine may read and execute.
    pub exec: Vec<PathBuf>,
    /// Paths the engine may read, create, write and delete beneath, but not execute.
    pub read_write: Vec<PathBuf>,
    /// Deny TCP through Landlock and non-Unix sockets through seccomp.
    pub deny_network: bool,
    /// Extra resource limits as `(name, value)`; `core=0` is always applied.
    pub rlimits: Vec<(String, u64)>,
}

/// System paths every engine may execute from (libraries, the dynamic loader, shells).
/// On macOS the Seatbelt profile owns the system allow-list instead.
#[cfg(not(target_os = "macos"))]
const BASE_EXEC: &[&str] = &[
    "/usr",
    "/bin",
    "/sbin",
    "/lib",
    "/lib32",
    "/lib64",
    "/libx32",
    "/nix/store",
];

#[cfg(target_os = "macos")]
const BASE_EXEC: &[&str] = &[];

/// System configuration and devices every engine may read.
const BASE_READ_ONLY: &[&str] = &[
    "/etc",
    "/dev/urandom",
    "/dev/random",
    "/dev/zero",
    "/sys/devices/system/cpu",
];

/// Devices every engine may write.
const BASE_READ_WRITE: &[&str] = &["/dev/null"];

impl SandboxPolicy {
    /// The policy for one engine: the base system allow-list, the engine's declared extra paths,
    /// execute access to its program, and read-write access to `writable` (the job workspace or
    /// the session directory). Network access is denied.
    #[must_use]
    pub fn for_engine(engine: &str, program: &Path, writable: &[PathBuf]) -> Self {
        let extra = crate::engines::sandbox::paths_for(engine);
        let mut exec: Vec<PathBuf> = BASE_EXEC.iter().map(PathBuf::from).collect();
        exec.push(std::fs::canonicalize(program).unwrap_or_else(|_| program.to_path_buf()));
        exec.extend(extra.exec);
        let mut read_only: Vec<PathBuf> = BASE_READ_ONLY.iter().map(PathBuf::from).collect();
        read_only.extend(extra.read_only);
        let mut read_write: Vec<PathBuf> = BASE_READ_WRITE.iter().map(PathBuf::from).collect();
        read_write.extend(writable.iter().cloned());
        Self {
            private_tmp: if extra.private_tmp {
                writable.first().cloned()
            } else {
                None
            },
            read_only,
            exec,
            read_write,
            deny_network: true,
            rlimits: Vec::new(),
        }
    }

    /// Arguments for `<launcher> __sandbox …` that start `program args…` under this policy.
    #[must_use]
    pub fn launcher_args(
        &self,
        required: bool,
        program: &Path,
        args: &[OsString],
    ) -> Vec<OsString> {
        let mut out = vec![OsString::from("__sandbox")];
        if let Some(root) = &self.private_tmp {
            out.push("--private-tmp".into());
            out.push(root.clone().into_os_string());
        }
        if required {
            out.push("--required".into());
        }
        if self.deny_network {
            out.push("--deny-net".into());
        }
        for (name, value) in &self.rlimits {
            out.push("--rlimit".into());
            out.push(format!("{name}={value}").into());
        }
        for (flag, paths) in [
            ("--ro", &self.read_only),
            ("--exec", &self.exec),
            ("--rw", &self.read_write),
        ] {
            for path in paths {
                out.push(flag.into());
                out.push(path.clone().into_os_string());
            }
        }
        out.push("--".into());
        out.push(program.as_os_str().to_owned());
        out.extend(args.iter().cloned());
        out
    }
}

/// A parsed `__sandbox` command line.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LaunchSpec {
    pub limits_only: bool,
    pub required: bool,
    pub policy: SandboxPolicy,
    pub program: PathBuf,
    pub args: Vec<OsString>,
}

/// Resource limit names accepted by `--rlimit`.
pub const RLIMIT_NAMES: &[&str] = &[
    "core", "cpu", "fsize", "nofile", "nproc", "data", "as", "stack",
];

impl LaunchSpec {
    /// Parses the arguments that follow `__sandbox`.
    pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Self, String> {
        let mut spec = Self::default();
        let mut args = args.into_iter();
        loop {
            let Some(arg) = args.next() else {
                return Err("missing `-- <program>`".to_owned());
            };
            let flag = arg.to_str().unwrap_or_default();
            match flag {
                "--limits-only" => spec.limits_only = true,
                "--required" => spec.required = true,
                "--deny-net" => spec.policy.deny_network = true,
                "--ro" | "--exec" | "--rw" | "--private-tmp" => {
                    let path = args
                        .next()
                        .map(PathBuf::from)
                        .ok_or_else(|| format!("{flag} needs a path"))?;
                    match flag {
                        "--private-tmp" => spec.policy.private_tmp = Some(path),
                        "--ro" => spec.policy.read_only.push(path),
                        "--exec" => spec.policy.exec.push(path),
                        _ => spec.policy.read_write.push(path),
                    }
                }
                "--rlimit" => {
                    let value = args
                        .next()
                        .and_then(|value| value.into_string().ok())
                        .ok_or("--rlimit needs NAME=VALUE")?;
                    spec.policy.rlimits.push(parse_rlimit(&value)?);
                }
                "--" => {
                    if spec.limits_only
                        && (spec.required
                            || spec.policy.deny_network
                            || spec.policy.private_tmp.is_some()
                            || !spec.policy.read_only.is_empty()
                            || !spec.policy.read_write.is_empty()
                            || !spec.policy.exec.is_empty())
                    {
                        return Err(
                            "limits-only cannot be combined with sandbox restrictions".to_owned()
                        );
                    }
                    spec.program = args
                        .next()
                        .map(PathBuf::from)
                        .ok_or("missing program after `--`")?;
                    spec.args = args.collect();
                    return Ok(spec);
                }
                _ => return Err(format!("unknown argument {}", arg.to_string_lossy())),
            }
        }
    }
}

fn parse_rlimit(value: &str) -> Result<(String, u64), String> {
    let (name, limit) = value.split_once('=').ok_or("--rlimit needs NAME=VALUE")?;
    if !RLIMIT_NAMES.contains(&name) {
        return Err(format!("unknown rlimit {name}"));
    }
    let limit = limit
        .parse::<u64>()
        .map_err(|_| format!("rlimit {name} needs a number"))?;
    Ok((name.to_owned(), limit))
}

/// Entry point of `ashift __sandbox`: applies the restrictions and `exec`s the engine.
///
/// Returns only when something failed. A failure writes one protocol `result` with
/// `engine_failure` to stdout, so the host reports a typed error, and exits with
/// [`LAUNCH_FAILURE_EXIT`].
pub fn launch(args: impl IntoIterator<Item = OsString>) -> ExitCode {
    let spec = match LaunchSpec::parse(args) {
        Ok(spec) => spec,
        Err(message) => return launch_failure(&format!("invalid sandbox arguments: {message}")),
    };
    #[cfg(debug_assertions)]
    if spec.policy.private_tmp.is_some()
        && std::env::var_os("ARIAD_TEST_SANDBOX_SUPPORT")
            .is_some_and(|value| value == "namespace-denied")
    {
        return launch_failure(
            "private /tmp requires user and mount namespaces: Permission denied",
        );
    }
    #[cfg(target_os = "linux")]
    {
        let error = linux::apply_and_exec(&spec);
        launch_failure(&error)
    }
    #[cfg(target_os = "macos")]
    {
        let error = macos::apply_and_exec(&spec);
        launch_failure(&error)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = spec;
        launch_failure("the engine sandbox is only available on Linux and macOS")
    }
}

fn launch_failure(message: &str) -> ExitCode {
    let _ = writeln!(io::stderr(), "ashift sandbox: {message}");
    let event = Event::Result {
        ok: false,
        metrics: None,
        error: Some(EngineError {
            code: ErrorCode::EngineFailure,
            message: format!("{LAUNCH_FAILURE_PREFIX}{message}"),
        }),
    };
    if let Ok(mut line) = serde_json::to_vec(&event) {
        line.push(b'\n');
        let mut stdout = io::stdout().lock();
        let _ = stdout.write_all(&line);
        let _ = stdout.flush();
    }
    ExitCode::from(LAUNCH_FAILURE_EXIT)
}

#[cfg(test)]
mod tests {
    use std::{ffi::OsString, path::PathBuf};

    #[cfg(target_os = "linux")]
    use super::SandboxSupport;
    use super::{LaunchSpec, SandboxMode, SandboxPolicy};

    #[test]
    fn modes_parse_their_documented_spellings_only() {
        assert_eq!(SandboxMode::parse("off"), Some(SandboxMode::Off));
        assert_eq!(
            SandboxMode::parse("best-effort"),
            Some(SandboxMode::BestEffort)
        );
        assert_eq!(SandboxMode::parse("required"), Some(SandboxMode::Required));
        assert_eq!(SandboxMode::parse("Required"), None);
        assert_eq!(SandboxMode::parse("besteffort"), None);
        for mode in [
            SandboxMode::Off,
            SandboxMode::BestEffort,
            SandboxMode::Required,
        ] {
            assert_eq!(SandboxMode::parse(mode.as_str()), Some(mode));
        }
    }

    #[test]
    fn launcher_arguments_round_trip_through_the_parser() {
        let policy = SandboxPolicy {
            private_tmp: Some(PathBuf::from("/tmp/ariadshift-x")),
            read_only: vec![PathBuf::from("/etc")],
            exec: vec![PathBuf::from("/usr"), PathBuf::from("/opt/engine")],
            read_write: vec![PathBuf::from("/tmp/ariadshift-x")],
            deny_network: true,
            rlimits: vec![("nofile".to_owned(), 256)],
        };
        let args = policy.launcher_args(
            true,
            &PathBuf::from("/opt/engine/bin/run"),
            &[OsString::from("--flag"), OsString::from("--")],
        );
        assert_eq!(args[0], "__sandbox");
        let spec = LaunchSpec::parse(args.into_iter().skip(1)).expect("parse launcher arguments");
        assert!(spec.required);
        assert_eq!(spec.policy, policy);
        assert_eq!(spec.program, PathBuf::from("/opt/engine/bin/run"));
        assert_eq!(
            spec.args,
            vec![OsString::from("--flag"), OsString::from("--")]
        );
    }

    #[test]
    fn the_parser_rejects_malformed_launcher_arguments() {
        let parse = |args: &[&str]| LaunchSpec::parse(args.iter().map(OsString::from));
        assert!(parse(&[]).is_err());
        assert!(parse(&["--ro"]).is_err());
        assert!(parse(&["--rlimit", "core"]).is_err());
        assert!(parse(&["--rlimit", "memory=1", "--", "x"]).is_err());
        assert!(parse(&["--rlimit", "core=-1", "--", "x"]).is_err());
        assert!(parse(&["--unknown", "--", "x"]).is_err());
        assert!(parse(&["--limits-only", "--required", "--", "x"]).is_err());
        assert!(parse(&["--limits-only", "--deny-net", "--", "x"]).is_err());
        assert!(parse(&["--limits-only", "--rlimit", "as=1024", "--", "x"]).is_ok());
        assert!(parse(&["--"]).is_err());
        assert!(parse(&["--rlimit", "core=0", "--", "x"]).is_ok());
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn gaps_name_missing_filesystem_and_network_isolation() {
        let full = SandboxSupport {
            platform: true,
            landlock_abi: Some(9),
            landlock_filesystem: true,
            landlock_network: true,
            landlock_scopes: true,
            landlock_unix_sockets: true,
            seccomp: true,
        };
        assert!(full.gaps().is_empty());

        let old_kernel = SandboxSupport {
            landlock_abi: Some(3),
            landlock_network: false,
            landlock_scopes: false,
            landlock_unix_sockets: false,
            ..full.clone()
        };
        assert!(
            old_kernel.gaps().is_empty(),
            "seccomp still blocks the network when Landlock lacks TCP rules"
        );

        let tcp_only = SandboxSupport {
            seccomp: false,
            ..full
        };
        assert_eq!(tcp_only.gaps().len(), 1);
        assert!(tcp_only.gaps()[0].contains("only TCP"));

        let nothing = SandboxSupport {
            platform: true,
            ..SandboxSupport::default()
        };
        assert_eq!(nothing.gaps().len(), 2);
    }

    #[test]
    fn engine_policy_grants_the_program_and_the_writable_paths() {
        let policy = SandboxPolicy::for_engine(
            "probe",
            &PathBuf::from("/nonexistent/bin/engine"),
            &[PathBuf::from("/tmp/ariadshift-job")],
        );
        assert!(policy.deny_network);
        assert!(
            policy
                .exec
                .contains(&PathBuf::from("/nonexistent/bin/engine"))
        );
        assert!(
            policy
                .read_write
                .contains(&PathBuf::from("/tmp/ariadshift-job"))
        );
        assert!(policy.read_write.contains(&PathBuf::from("/dev/null")));
        assert!(policy.read_only.contains(&PathBuf::from("/etc")));
        assert!(!policy.exec.contains(&PathBuf::from("/tmp/ariadshift-job")));
    }
}
