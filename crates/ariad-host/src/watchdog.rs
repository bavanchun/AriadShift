//! Host-side memory limit for engine process trees.
//!
//! While a job runs with `limits.max_memory_mb`, the runner samples the resident memory of the
//! engine's whole process tree every [`SAMPLE_INTERVAL`] and kills the tree once the sum exceeds
//! the limit. Sampling can miss a spike shorter than the interval, so engines that enforce the
//! limit themselves (Pandoc `+RTS -M`, `RLIMIT_DATA` in Python adapters) still receive it.

use std::time::Duration;

/// How often the watchdog samples the engine process tree.
pub const SAMPLE_INTERVAL: Duration = Duration::from_millis(250);

/// Samples the resident memory of one engine process tree against a limit.
pub struct MemoryWatchdog {
    root: u32,
    limit_bytes: u64,
    #[cfg(any(target_os = "macos", windows))]
    system: sysinfo::System,
}

impl MemoryWatchdog {
    /// Watches the tree rooted at `root` (the engine process, which leads its process group on
    /// Unix) against `limit_mb` mebibytes.
    #[must_use]
    pub fn new(root: u32, limit_mb: u32) -> Self {
        Self {
            root,
            limit_bytes: u64::from(limit_mb).saturating_mul(1024 * 1024),
            #[cfg(any(target_os = "macos", windows))]
            system: sysinfo::System::new(),
        }
    }

    /// Returns the tree's resident bytes when they exceed the limit.
    pub fn exceeded(&mut self) -> Option<u64> {
        let resident = self.tree_resident_bytes();
        (resident > self.limit_bytes).then_some(resident)
    }

    /// Sums the resident memory of the engine and every process it started.
    pub fn tree_resident_bytes(&mut self) -> u64 {
        #[cfg(target_os = "linux")]
        {
            linux::tree_resident_bytes(self.root)
        }
        #[cfg(any(target_os = "macos", windows))]
        {
            portable::tree_resident_bytes(&mut self.system, self.root)
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
        {
            let _ = self.root;
            0
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
        let Ok(entries) = fs::read_dir("/proc") else {
            return 0;
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
        members.into_iter().map(resident_bytes).sum()
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
            .into_iter()
            .filter_map(|pid| system.process(Pid::from_u32(pid)))
            .map(sysinfo::Process::memory)
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::{MemoryWatchdog, descendants_of};

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
            assert!(resident > 0, "the test process must report resident memory");
            assert!(
                watchdog.exceeded().is_some(),
                "a 1 MiB limit is below any test process"
            );
        }
    }
}
