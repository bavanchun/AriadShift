//! Landlock, seccomp and rlimit enforcement for the `__sandbox` launcher.

use std::{collections::BTreeMap, fs, os::unix::process::CommandExt, process::Command};

use landlock::{
    ABI, Access, AccessFs, AccessNet, BitFlags, CompatLevel, Compatible, RulesetAttr,
    RulesetCreatedAttr, RulesetError, Scope, path_beneath_rules,
};
use nix::sys::resource::{Resource, setrlimit};
use seccompiler::{
    BpfProgram, SeccompAction, SeccompCmpArgLen, SeccompCmpOp, SeccompCondition, SeccompFilter,
    SeccompRule, TargetArch,
};

use super::{LaunchSpec, SandboxSupport};

/// The newest Landlock ABI this launcher knows; older kernels get the subset they support.
const NEWEST_ABI: ABI = ABI::V9;

/// ABIs from newest to oldest, with the number the kernel reports for each.
const ABIS: [(ABI, u8); 9] = [
    (ABI::V9, 9),
    (ABI::V8, 8),
    (ABI::V7, 7),
    (ABI::V6, 6),
    (ABI::V5, 5),
    (ABI::V4, 4),
    (ABI::V3, 3),
    (ABI::V2, 2),
    (ABI::V1, 1),
];

/// Offset the x32 ABI adds to x86_64 system call numbers.
#[cfg(target_arch = "x86_64")]
const X32_SYSCALL_BIT: i64 = 0x4000_0000;

pub(super) fn probe() -> SandboxSupport {
    let landlock_abi = ABIS
        .iter()
        .find(|(abi, _)| kernel_supports(*abi))
        .map(|(_, number)| *number);
    let at_least = |minimum: u8| landlock_abi.is_some_and(|abi| abi >= minimum);
    SandboxSupport {
        platform: true,
        landlock_abi,
        landlock_filesystem: at_least(1),
        landlock_network: at_least(4),
        landlock_scopes: at_least(6),
        landlock_unix_sockets: at_least(9),
        seccomp: seccomp_available(),
    }
}

/// Creating (not applying) a ruleset with a hard requirement on every right of `abi` succeeds
/// only when the kernel supports that ABI.
fn kernel_supports(abi: ABI) -> bool {
    landlock::Ruleset::default()
        .set_compatibility(CompatLevel::HardRequirement)
        .handle_access(AccessFs::from_all(abi))
        .and_then(landlock::Ruleset::create)
        .is_ok()
}

fn seccomp_available() -> bool {
    fs::read_to_string("/proc/sys/kernel/seccomp/actions_avail")
        .is_ok_and(|actions| actions.split_whitespace().any(|action| action == "errno"))
}

/// Applies every restriction in `spec` to the current process and `exec`s the engine.
///
/// Returns a description of the failure; on success the process image is replaced.
pub(super) fn apply_and_exec(spec: &LaunchSpec) -> String {
    if let Err(error) = apply_rlimits(spec) {
        return error;
    }
    #[cfg(debug_assertions)]
    let simulated = super::simulated_without_support();
    #[cfg(not(debug_assertions))]
    let simulated = false;
    if !simulated {
        // Without seccomp support the host already reports the gap; when the kernel offers it,
        // a failure to install the filter is an error in every mode. Check before Landlock
        // hides `/proc`.
        let seccomp = spec.policy.deny_network && (spec.required || seccomp_available());
        if let Err(error) = apply_landlock(spec) {
            return format!("Landlock: {error}");
        }
        if seccomp && let Err(error) = apply_seccomp() {
            return format!("seccomp: {error}");
        }
    } else if spec.required {
        return "Landlock is unavailable".to_owned();
    }
    let error = Command::new(&spec.program).args(&spec.args).exec();
    format!("could not execute the engine program: {error}")
}

fn apply_rlimits(spec: &LaunchSpec) -> Result<(), String> {
    let mut limits = vec![("core", 0)];
    limits.extend(
        spec.policy
            .rlimits
            .iter()
            .map(|(name, value)| (name.as_str(), *value)),
    );
    for (name, value) in limits {
        let resource = match name {
            "core" => Resource::RLIMIT_CORE,
            "cpu" => Resource::RLIMIT_CPU,
            "fsize" => Resource::RLIMIT_FSIZE,
            "nofile" => Resource::RLIMIT_NOFILE,
            "nproc" => Resource::RLIMIT_NPROC,
            "data" => Resource::RLIMIT_DATA,
            "as" => Resource::RLIMIT_AS,
            "stack" => Resource::RLIMIT_STACK,
            other => return Err(format!("unknown rlimit {other}")),
        };
        setrlimit(resource, value, value).map_err(|error| format!("rlimit {name}: {error}"))?;
    }
    Ok(())
}

fn read_access() -> BitFlags<AccessFs> {
    AccessFs::ReadFile | AccessFs::ReadDir
}

fn exec_access() -> BitFlags<AccessFs> {
    AccessFs::from_read(NEWEST_ABI)
}

fn write_access() -> BitFlags<AccessFs> {
    AccessFs::from_all(NEWEST_ABI) & !AccessFs::Execute
}

/// Builds one ruleset: filesystem rights of the newest ABI, TCP bind and connect, and scopes.
///
/// In required mode the ABI 1 filesystem rights are a hard requirement; newer rights, network
/// rules and scopes are best effort because seccomp also blocks the network.
fn apply_landlock(spec: &LaunchSpec) -> Result<(), RulesetError> {
    let base_level = if spec.required {
        CompatLevel::HardRequirement
    } else {
        CompatLevel::BestEffort
    };
    let mut ruleset = landlock::Ruleset::default()
        .set_compatibility(base_level)
        .handle_access(AccessFs::from_all(ABI::V1))?
        .set_compatibility(CompatLevel::BestEffort)
        .handle_access(AccessFs::from_all(NEWEST_ABI))?;
    if spec.policy.deny_network {
        ruleset = ruleset.handle_access(AccessNet::from_all(NEWEST_ABI))?;
    }
    ruleset
        .scope(Scope::from_all(NEWEST_ABI))?
        .create()?
        .set_compatibility(CompatLevel::BestEffort)
        .add_rules(path_beneath_rules(&spec.policy.read_only, read_access()))?
        .add_rules(path_beneath_rules(&spec.policy.exec, exec_access()))?
        .add_rules(path_beneath_rules(&spec.policy.read_write, write_access()))?
        .restrict_self()?;
    Ok(())
}

/// Returns `EPERM` for `socket` with any family but `AF_UNIX` and for `io_uring_setup`, whose
/// operations could otherwise create sockets without the `socket` system call.
fn apply_seccomp() -> Result<(), String> {
    let arch = TargetArch::try_from(std::env::consts::ARCH).map_err(|error| error.to_string())?;
    let not_unix = || {
        SeccompCondition::new(
            0,
            SeccompCmpArgLen::Dword,
            SeccompCmpOp::Ne,
            u64::try_from(libc::AF_UNIX).unwrap_or_default(),
        )
        .and_then(|condition| SeccompRule::new(vec![condition]))
    };
    let mut rules: BTreeMap<i64, Vec<SeccompRule>> = BTreeMap::new();
    rules.insert(
        libc::SYS_socket,
        vec![not_unix().map_err(|error| error.to_string())?],
    );
    rules.insert(libc::SYS_io_uring_setup, Vec::new());
    #[cfg(target_arch = "x86_64")]
    {
        rules.insert(
            X32_SYSCALL_BIT + libc::SYS_socket,
            vec![not_unix().map_err(|error| error.to_string())?],
        );
        rules.insert(X32_SYSCALL_BIT + libc::SYS_io_uring_setup, Vec::new());
    }
    let filter = SeccompFilter::new(
        rules,
        SeccompAction::Allow,
        SeccompAction::Errno(u32::try_from(libc::EPERM).unwrap_or(1)),
        arch,
    )
    .map_err(|error| error.to_string())?;
    let program: BpfProgram = filter.try_into().map_err(|error| format!("{error}"))?;
    seccompiler::apply_filter(&program).map_err(|error| error.to_string())
}
