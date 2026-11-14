//! Host-side memory limit for engine process trees.
//!
//! While a job runs with `limits.max_memory_mb`, the runner samples the resident memory of the
//! engine's whole process tree every [`SAMPLE_INTERVAL`] and kills the tree once the sum exceeds
//! the limit. Sampling can miss a spike shorter than the interval, so engines that enforce the
//! limit themselves (Pandoc `+RTS -M`, `RLIMIT_DATA` in Python adapters) still receive it.

use std::time::Duration;

/// How often the watchdog samples the engine process tree.
#[cfg(target_os = "macos")]
pub const SAMPLE_INTERVAL: Duration = Duration::from_millis(100);
#[cfg(not(target_os = "macos"))]
pub const SAMPLE_INTERVAL: Duration = Duration::from_millis(250);

#[cfg(any(target_os = "macos", windows))]
type WatchdogSystem = sysinfo::System;
#[cfg(not(any(target_os = "macos", windows)))]
type WatchdogSystem = ();

/// Samples the resident memory of one engine process tree against a limit.
pub struct MemoryWatchdog {
    root: u32,
    limit_mb: u32,
    limit_bytes: u64,
    system: Option<WatchdogSystem>,
}

impl MemoryWatchdog {
    /// Watches the tree rooted at `root` (the engine process, which leads its process group on
    /// Unix) against `limit_mb` mebibytes.
    #[must_use]
    pub fn new(root: u32, limit_mb: u32) -> Self {
        #[cfg(target_os = "linux")]
        {
            Self {
                root,
                limit_mb,
                limit_bytes: limit_bytes(limit_mb),
                system: Some(()),
            }
        }
        #[cfg(any(target_os = "macos", windows))]
        {
            Self::from_init(root, limit_mb, sysinfo::System::new(), |warning| {
                eprintln!("{warning}");
            })
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
        {
            Self {
                root,
                limit_mb,
                limit_bytes: limit_bytes(limit_mb),
                system: None,
            }
        }
    }

    /// Initializes a watchdog from a backend result, emitting a structured warning and falling
    /// back to an inactive watchdog when initialization fails.
    #[cfg(any(target_os = "macos", windows, test))]
    pub(crate) fn from_init<E>(
        root: u32,
        limit_mb: u32,
        system: Result<WatchdogSystem, E>,
        warn: impl FnOnce(String),
    ) -> Self
    where
        E: std::fmt::Display,
    {
        let system = unwrap_or_warn(
            system,
            |error| format_watchdog_unavailable_warning(root, limit_mb, error),
            warn,
        );
        Self {
            root,
            limit_mb,
            limit_bytes: limit_bytes(limit_mb),
            system,
        }
    }

    /// Reports whether host resident-memory polling is active for this watchdog.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.system.is_some()
    }

    /// Returns the configured memory limit in mebibytes.
    #[must_use]
    pub fn limit_mb(&self) -> u32 {
        self.limit_mb
    }

    /// Returns the tree's resident bytes when they exceed the limit.
    pub fn exceeded(&mut self) -> Option<u64> {
        if !self.is_active() {
            return None;
        }
        let resident = self.tree_resident_bytes();
        (resident > self.limit_bytes).then_some(resident)
    }

    /// Sums the resident memory of the engine and every process it started.
    ///
    /// Returns `0` when host resident-memory polling is disabled or unsupported.
    pub fn tree_resident_bytes(&mut self) -> u64 {
        #[cfg(target_os = "linux")]
        {
            if !self.is_active() {
                return 0;
            }
            linux::tree_resident_bytes(self.root)
        }
        #[cfg(any(target_os = "macos", windows))]
        {
            self.system
                .as_mut()
                .map_or(0, |system| portable::tree_resident_bytes(system, self.root))
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
        {
            let _ = (&self.root, &self.system);
            0
        }
    }
}

fn limit_bytes(limit_mb: u32) -> u64 {
    u64::from(limit_mb).saturating_mul(1024 * 1024)
}

#[cfg(any(target_os = "macos", windows, test))]
fn unwrap_or_warn<S, E>(
    init: Result<S, E>,
    format_warning: impl FnOnce(&E) -> String,
    warn: impl FnOnce(String),
) -> Option<S>
where
    E: std::fmt::Display,
{
    match init {
        Ok(value) => Some(value),
        Err(error) => {
            warn(format_warning(&error));
            None
        }
    }
}

#[cfg(any(target_os = "macos", windows, test))]
fn format_watchdog_unavailable_warning(
    root: u32,
    limit_mb: u32,
    error: &dyn std::fmt::Display,
) -> String {
    format!(
        "warning[memory_watchdog_unavailable]: could not initialize host memory watchdog for pid {root} (limit {limit_mb} MiB): {error}; falling back without host resident-memory polling (engine-side limits such as Pandoc +RTS -M or RLIMIT_DATA still apply where supported)"
    )
}

#[cfg(any(target_os = "macos", test))]
fn format_descendants_unavailable_warning(root: u32, error: &dyn std::fmt::Display) -> String {
    format!(
        "warning[process_tree_unavailable]: could not enumerate descendants for pid {root}: {error}; falling back to stopping root process only"
    )
}

#[cfg(any(target_os = "macos", test))]
fn tree_processes_or_root<S, E>(
    root: u32,
    system: Result<S, E>,
    collect: impl FnOnce(&mut S, u32) -> Vec<u32>,
    warn: impl FnOnce(String),
) -> Vec<u32>
where
    E: std::fmt::Display,
{
    match unwrap_or_warn(
        system,
        |error| format_descendants_unavailable_warning(root, error),
        warn,
    ) {
        Some(mut system) => collect(&mut system, root),
        None => vec![root],
    }
}

/// Stops descendants before their parent is reaped, including nested engines
/// that lead separate process groups. The root is stopped by process-wrap.
#[cfg(unix)]
pub(crate) fn kill_descendants(root: u32) {
    use nix::{
        sys::signal::{Signal, kill},
        unistd::Pid,
    };
    #[cfg(target_os = "linux")]
    let members = linux::tree_processes(root);
    #[cfg(target_os = "macos")]
    let members = tree_processes_or_root(
        root,
        sysinfo::System::new(),
        portable::tree_processes,
        |warning| eprintln!("{warning}"),
    );
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    let members = vec![root];
    for pid in members.into_iter().rev().filter(|&pid| pid != root) {
        if let Ok(pid) = i32::try_from(pid) {
            let _ = kill(Pid::from_raw(pid), Signal::SIGKILL);
        }
    }
}

/// Collects `root` and its descendants from `(pid, parent)` pairs.
fn descendants_of(root: u32, processes: &[(u32, u32)]) -> Vec<u32> {
    let mut members = vec![root];
    let mut index = 0;
    while index < members.len() {
        let parent = members[index];
        for &(pid, ppid) in processes {
            if ppid == parent && pid != parent && !members.contains(&pid) {
                members.push(pid);
            }
        }
        index += 1;
    }
    members
}

#[cfg(target_os = "linux")]
mod linux {
    use std::fs;

    /// Members are the engine's process group (the engine leads it) plus every descendant by
    /// parent PID, so a child that left the group or was reparented still counts while a
    /// member remains its ancestor.
    pub(super) fn tree_resident_bytes(root: u32) -> u64 {
        tree_processes(root).into_iter().map(resident_bytes).sum()
    }

    pub(super) fn tree_processes(root: u32) -> Vec<u32> {
        let Ok(entries) = fs::read_dir("/proc") else {
            return Vec::new();
        };
        let mut processes = Vec::new();
        let mut group = Vec::new();
        for entry in entries.flatten() {
            let Some(pid) = entry
                .file_name()
                .to_str()
                .and_then(|name| name.parse::<u32>().ok())
            else {
                continue;
            };
            let Some((ppid, pgrp)) = read_stat(pid) else {
                continue;
            };
            processes.push((pid, ppid));
            if pgrp == root {
                group.push(pid);
            }
        }
        let mut members = super::descendants_of(root, &processes);
        for pid in group {
            if !members.contains(&pid) {
                members.extend(super::descendants_of(pid, &processes));
            }
        }
        members.sort_unstable();
        members.dedup();
        members
    }

    /// Reads the parent PID and process group from `/proc/<pid>/stat`.
    fn read_stat(pid: u32) -> Option<(u32, u32)> {
        let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        // The command name is parenthesised and may contain spaces or parentheses.
        let after_name = &stat[stat.rfind(')')? + 1..];
        let mut fields = after_name.split_whitespace();
        let _state = fields.next()?;
        let ppid = fields.next()?.parse().ok()?;
        let pgrp = fields.next()?.parse().ok()?;
        Some((ppid, pgrp))
    }

    /// `VmRSS` from `/proc/<pid>/status`, in bytes; zero for zombies and vanished processes.
    fn resident_bytes(pid: u32) -> u64 {
        let Ok(status) = fs::read_to_string(format!("/proc/{pid}/status")) else {
            return 0;
        };
        status
            .lines()
            .find_map(|line| line.strip_prefix("VmRSS:"))
            .and_then(|value| value.split_whitespace().next())
            .and_then(|kib| kib.parse::<u64>().ok())
            .map_or(0, |kib| kib.saturating_mul(1024))
    }
}

#[cfg(any(target_os = "macos", windows))]
mod portable {
    use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

    pub(super) fn tree_resident_bytes(system: &mut System, root: u32) -> u64 {
        tree_processes(system, root)
            .into_iter()
            .filter_map(|pid| system.process(Pid::from_u32(pid)))
            .map(sysinfo::Process::memory)
            .sum()
    }

    pub(super) fn tree_processes(system: &mut System, root: u32) -> Vec<u32> {
        system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing().with_memory(),
        );
        let processes: Vec<(u32, u32)> = system
            .processes()
            .iter()
            .filter(|(_, process)| process.thread_kind().is_none())
            .filter_map(|(pid, process)| Some((pid.as_u32(), process.parent()?.as_u32())))
            .collect();
        super::descendants_of(root, &processes)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        MemoryWatchdog, descendants_of, format_watchdog_unavailable_warning,
        tree_processes_or_root, unwrap_or_warn,
    };

    #[test]
    fn descendants_follow_parent_links_from_the_root_only() {
        let processes = [(10, 1), (11, 10), (12, 11), (13, 1), (14, 13), (15, 12)];
        let mut members = descendants_of(10, &processes);
        members.sort_unstable();
        assert_eq!(members, vec![10, 11, 12, 15]);
        assert_eq!(descendants_of(99, &processes), vec![99]);
    }

    #[test]
    fn the_current_process_has_resident_memory() {
        let mut watchdog = MemoryWatchdog::new(std::process::id(), 1);
        let resident = watchdog.tree_resident_bytes();
        if cfg!(any(target_os = "linux", target_os = "macos", windows)) {
            assert!(
                watchdog.is_active(),
                "supported platforms must activate the memory watchdog"
            );
            assert!(resident > 0, "the test process must report resident memory");
            assert!(
                watchdog.exceeded().is_some(),
                "a 1 MiB limit is below any test process"
            );
        } else {
            assert!(!watchdog.is_active());
            assert_eq!(resident, 0);
            assert_eq!(watchdog.exceeded(), None);
        }
    }

    #[test]
    fn init_helper_handles_success_and_error_paths() {
        let mut warnings = Vec::new();
        let ok = unwrap_or_warn(
            Ok::<_, &str>(42_u32),
            |_| unreachable!("success path must not format a warning"),
            |warning| warnings.push(warning),
        );
        assert_eq!(ok, Some(42));
        assert!(warnings.is_empty());

        let err = unwrap_or_warn(
            Err::<u32, _>("permission denied"),
            |error| format_watchdog_unavailable_warning(4321, 512, error),
            |warning| warnings.push(warning),
        );
        assert_eq!(err, None);
        assert_eq!(warnings.len(), 1);
        assert!(
            warnings[0].starts_with("warning[memory_watchdog_unavailable]: "),
            "{}",
            warnings[0]
        );
        assert!(warnings[0].contains("4321"), "{}", warnings[0]);
        assert!(warnings[0].contains("512 MiB"), "{}", warnings[0]);
        assert!(warnings[0].contains("permission denied"), "{}", warnings[0]);
    }

    #[test]
    fn failed_sysinfo_init_disables_watchdog_and_logs_structured_warning() {
        let pid = std::process::id();
        let mut warnings = Vec::new();
        let mut watchdog =
            MemoryWatchdog::from_init(pid, 1, Err("simulated sysinfo failure"), |warning| {
                warnings.push(warning);
            });
        assert_eq!(warnings.len(), 1);
        assert!(
            warnings[0].starts_with("warning[memory_watchdog_unavailable]: "),
            "{}",
            warnings[0]
        );
        assert!(warnings[0].contains(&pid.to_string()), "{}", warnings[0]);
        assert!(warnings[0].contains("1 MiB"), "{}", warnings[0]);
        assert!(
            warnings[0].contains("simulated sysinfo failure"),
            "{}",
            warnings[0]
        );
        assert!(!watchdog.is_active());
        assert_eq!(watchdog.limit_mb(), 1);
        assert_eq!(watchdog.tree_resident_bytes(), 0);
        assert_eq!(watchdog.exceeded(), None);

        let mut zero_warnings = Vec::new();
        let mut disabled_zero =
            MemoryWatchdog::from_init(pid, 0, Err("zero-limit error"), |warning| {
                zero_warnings.push(warning);
            });
        assert_eq!(zero_warnings.len(), 1);
        assert!(!disabled_zero.is_active());
        assert_eq!(disabled_zero.tree_resident_bytes(), 0);
        assert_eq!(disabled_zero.exceeded(), None);
    }

    #[test]
    fn descendant_collection_falls_back_to_root_and_logs_when_init_fails() {
        let mut warnings = Vec::new();
        let members = tree_processes_or_root(
            77,
            Ok::<_, &str>(vec![77, 88, 99]),
            |state, root| {
                assert_eq!(root, 77);
                state.clone()
            },
            |warning| warnings.push(warning),
        );
        assert_eq!(members, vec![77, 88, 99]);
        assert!(warnings.is_empty());

        let fallback = tree_processes_or_root::<Vec<u32>, _>(
            77,
            Err("mach port unavailable"),
            |_, _| unreachable!("collector must not run when initialization fails"),
            |warning| warnings.push(warning),
        );
        assert_eq!(fallback, vec![77]);
        assert_eq!(warnings.len(), 1);
        assert!(
            warnings[0].starts_with("warning[process_tree_unavailable]: "),
            "{}",
            warnings[0]
        );
        assert!(warnings[0].contains("77"), "{}", warnings[0]);
        assert!(
            warnings[0].contains("mach port unavailable"),
            "{}",
            warnings[0]
        );
    }
}
