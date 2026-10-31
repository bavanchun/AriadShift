//! Private system temporary directory for tools whose IPC ignores TMPDIR.

use std::{
    collections::BTreeMap,
    fs, io,
    path::{Component, Path, PathBuf},
};

use nix::{
    mount::{MsFlags, mount},
    sched::{CloneFlags, unshare},
    unistd::{getgid, getuid},
};

use crate::sandbox::SandboxPolicy;

/// Runs only in the single-threaded launcher, before Landlock and seccomp.
pub(super) fn isolate(workspace: &Path, program: &Path, policy: &SandboxPolicy) -> io::Result<()> {
    let workspace = fs::canonicalize(workspace)?;
    let system_tmp = fs::canonicalize("/tmp")?;
    if workspace.parent().is_none() || workspace == system_tmp {
        return Err(io::Error::other("invalid private temporary workspace"));
    }
    let private = workspace.join(".sandbox-tmp");
    fs::create_dir(&private)?;

    let mut paths = BTreeMap::new();
    let mut aliases = Vec::new();
    let program = program.to_path_buf();
    for (path, writable) in policy
        .exec
        .iter()
        .chain(&policy.read_only)
        .chain(std::iter::once(&program))
        .map(|path| (path, false))
        .chain(policy.read_write.iter().map(|path| (path, true)))
    {
        let Ok(canonical) = fs::canonicalize(path) else {
            continue;
        };
        if canonical == system_tmp {
            return Err(io::Error::other("cannot expose the host temporary root"));
        }
        paths
            .entry(canonical.clone())
            .and_modify(|write| *write |= writable)
            .or_insert(writable);
        if path != &canonical
            && let Some(relative) = relative_tmp(path, &system_tmp)
        {
            aliases.push((relative, canonical));
        }
    }
    paths.insert(workspace.clone(), true);
    let mut paths: Vec<_> = paths.into_iter().collect();
    paths.sort_by_key(|(path, _)| path.components().count());

    let uid = getuid().as_raw();
    let gid = getgid().as_raw();
    unshare(CloneFlags::CLONE_NEWUSER | CloneFlags::CLONE_NEWNS)?;
    // A non-root namespace identity loses its mount capabilities at exec, including when
    // the host caller is root. Files still belong to the original host uid/gid.
    fs::write("/proc/self/setgroups", "deny")?;
    fs::write("/proc/self/uid_map", format!("65534 {uid} 1\n"))?;
    fs::write("/proc/self/gid_map", format!("65534 {gid} 1\n"))?;
    mount::<str, _, str, str>(None, "/", None, MsFlags::MS_REC | MsFlags::MS_PRIVATE, None)?;

    // Keep declared paths, including temporary installations, at their original locations.
    // Read-only bind mounts prevent writable /tmp from granting writes to host installations.
    let mut bound: Vec<(PathBuf, bool)> = Vec::new();
    for (source, writable) in paths {
        let Some(relative) = relative_tmp(&source, &system_tmp) else {
            continue;
        };
        if bound
            .iter()
            .any(|(parent, write)| source.starts_with(parent) && (*write || !writable))
        {
            continue;
        }
        let preserved = private.join(relative);
        if source.is_dir() {
            fs::create_dir_all(&preserved)?;
        } else {
            fs::create_dir_all(
                preserved
                    .parent()
                    .ok_or_else(|| io::Error::other("invalid preserved path"))?,
            )?;
            fs::File::create(&preserved)?;
        }
        mount::<_, _, str, str>(Some(&source), &preserved, None, MsFlags::MS_BIND, None)?;
        if !writable {
            let flags = readonly_flags(&source)?;
            mount::<str, _, str, str>(None, &preserved, None, flags, None)?;
        }
        bound.push((source, writable));
    }
    aliases.sort_by_key(|(path, _)| path.components().count());
    let mut linked: Vec<PathBuf> = Vec::new();
    for (relative, canonical) in aliases {
        let logical = system_tmp.join(&relative);
        if bound.iter().any(|(path, _)| logical.starts_with(path))
            || linked.iter().any(|path| relative.starts_with(path))
        {
            continue;
        }
        let destination = private.join(&relative);
        fs::create_dir_all(
            destination
                .parent()
                .ok_or_else(|| io::Error::other("invalid executable alias"))?,
        )?;
        if !destination.try_exists()? {
            std::os::unix::fs::symlink(canonical, &destination)?;
        }
        linked.push(relative);
    }
    mount::<_, _, str, str>(
        Some(&private),
        &system_tmp,
        None,
        MsFlags::MS_BIND | MsFlags::MS_REC,
        None,
    )?;
    Ok(())
}

fn relative_tmp(path: &Path, system_tmp: &Path) -> Option<PathBuf> {
    let relative = path
        .strip_prefix(system_tmp)
        .or_else(|_| path.strip_prefix("/tmp"))
        .ok()?;
    (!relative.as_os_str().is_empty()
        && relative
            .components()
            .all(|part| matches!(part, Component::Normal(_))))
    .then(|| relative.to_path_buf())
}

/// Preserve locked mount attributes when making a bind mount read-only in a user namespace.
fn readonly_flags(source: &Path) -> io::Result<MsFlags> {
    let mut selected = None;
    for line in fs::read_to_string("/proc/self/mountinfo")?.lines() {
        let fields: Vec<_> = line.split_whitespace().collect();
        let (Some(path), Some(options)) = (fields.get(4), fields.get(5)) else {
            continue;
        };
        let path = path
            .replace("\\040", " ")
            .replace("\\011", "\t")
            .replace("\\012", "\n")
            .replace("\\134", "\\");
        let path = Path::new(&path);
        if !source.starts_with(path)
            || selected
                .as_ref()
                .is_some_and(|(depth, _)| *depth >= path.components().count())
        {
            continue;
        }
        let mut flags = MsFlags::MS_BIND | MsFlags::MS_REMOUNT | MsFlags::MS_RDONLY;
        for option in options.split(',') {
            flags |= match option {
                "nosuid" => MsFlags::MS_NOSUID,
                "nodev" => MsFlags::MS_NODEV,
                "noexec" => MsFlags::MS_NOEXEC,
                "noatime" => MsFlags::MS_NOATIME,
                "nodiratime" => MsFlags::MS_NODIRATIME,
                "relatime" => MsFlags::MS_RELATIME,
                "strictatime" => MsFlags::MS_STRICTATIME,
                _ => MsFlags::empty(),
            };
        }
        selected = Some((path.components().count(), flags));
    }
    selected
        .map(|(_, flags)| flags)
        .ok_or_else(|| io::Error::other("mount attributes unavailable"))
}
