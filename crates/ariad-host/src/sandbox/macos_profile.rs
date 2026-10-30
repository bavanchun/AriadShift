//! Deny-by-default Seatbelt policy. Paths are command-line parameters, never SBPL source.

use std::{ffi::OsString, fs, io, path::Path, process::Command};

use super::super::SandboxPolicy;

const BASE: &str = r#"(version 1)
(deny default)
; macOS 27 dyld opens the volume root during startup; no descendants are granted.
(allow file-read-data (literal "/"))
(allow process-fork)
(allow signal (target same-sandbox))
(allow process-info* (target same-sandbox))
(allow sysctl-read
    (sysctl-name-prefix "hw.")
    (sysctl-name-prefix "machdep.cpu.")
    (sysctl-name "kern.argmax")
    (sysctl-name "kern.hostname")
    (sysctl-name "kern.osproductversion")
    (sysctl-name "kern.osrelease")
    (sysctl-name "kern.ostype")
    (sysctl-name "kern.osversion")
    (sysctl-name "kern.version")
    (sysctl-name "kern.maxfilesperproc")
    (sysctl-name "kern.usrstack64"))
(allow mach-lookup
    (global-name "com.apple.system.logger")
    (global-name "com.apple.logd")
    (global-name "com.apple.system.opendirectoryd.libinfo"))
"#;

/// System frameworks, fonts, loader caches and package-manager runtimes.
/// Package-manager state (var and etc) is never granted by this system allow-list.
const SYSTEM_EXEC: &[&str] = &[
    "/usr/bin",
    "/usr/sbin",
    "/usr/lib",
    "/usr/libexec",
    "/usr/share",
    "/bin",
    "/sbin",
    // Never grant /System itself: /System/Volumes/Data aliases the user's data volume.
    "/System/Library",
    "/System/Volumes/Preboot/Cryptexes/OS/usr/lib",
    "/Library/Apple",
    "/opt/homebrew/bin",
    "/opt/homebrew/sbin",
    "/opt/homebrew/lib",
    "/opt/homebrew/libexec",
    "/opt/homebrew/opt",
    "/opt/homebrew/Cellar",
    "/opt/homebrew/share",
    "/opt/homebrew/Frameworks",
    "/usr/local/bin",
    "/usr/local/sbin",
    "/usr/local/lib",
    "/usr/local/libexec",
    "/usr/local/opt",
    "/usr/local/Cellar",
    "/usr/local/share",
    "/usr/local/Frameworks",
];
const SYSTEM_READ: &[&str] = &[
    "/Library/Fonts",
    "/Library/Preferences/Logging",
    "/private/var/db/dyld",
    "/private/var/db/timezone",
    "/private/var/db/SystemPolicyConfiguration",
];
const IPC_RESOLUTION_FAILURE: &str = "could not resolve the Seatbelt IPC directory";

pub(super) struct Profile {
    source: String,
    parameters: Vec<OsString>,
}

impl Profile {
    pub(super) fn new(policy: &SandboxPolicy) -> Result<Self, String> {
        let ipc_directory = policy.private_tmp.as_deref();
        let mut profile = Self {
            source: BASE.to_owned(),
            parameters: Vec::new(),
        };
        for path in SYSTEM_EXEC
            .iter()
            .map(Path::new)
            .chain(policy.exec.iter().map(AsRef::as_ref))
        {
            profile.grant(
                path,
                "file-read* file-map-executable process-exec",
                false,
                ipc_directory,
            )?;
            // System Office apps keep libraries and helpers beside Contents/MacOS/soffice.
            if let Some(bundle) = path
                .ancestors()
                .find(|path| path.extension().is_some_and(|ext| ext == "app"))
            {
                profile.grant(
                    bundle,
                    "file-read* file-map-executable process-exec",
                    false,
                    ipc_directory,
                )?;
            }
        }
        for path in SYSTEM_READ.iter().map(Path::new) {
            profile.grant(path, "file-read*", false, ipc_directory)?;
        }
        // Engine libraries need executable mappings, without permission to execute programs.
        for path in &policy.read_only {
            profile.grant(path, "file-read* file-map-executable", false, ipc_directory)?;
        }
        for path in &policy.read_write {
            profile.grant(path, "file-read* file-write*", true, ipc_directory)?;
        }
        if !policy.deny_network {
            profile.source.push_str("(allow network*)\n");
        }
        if let Some(ipc) = ipc_directory {
            let ipc = fs::canonicalize(ipc).map_err(|_| IPC_RESOLUTION_FAILURE.to_owned())?;
            if !ipc.is_dir() {
                return Err("Seatbelt IPC path is not a directory".to_owned());
            }
            let key = profile.grant_path(&ipc, "file-read* file-write*", true);
            profile.source.push_str(&format!(
                "(allow network-bind network-inbound (local unix-socket (subpath (param \"{key}\"))))\n\
                 (allow network-outbound (remote unix-socket (subpath (param \"{key}\"))))\n"
            ));
            // Native spell checking registers the headless Office process with LaunchServices.
            profile.source.push_str(
                "(allow mach-lookup (global-name \"com.apple.coreservices.launchservicesd\"))\n",
            );
        }
        // Other engines and sessions use inherited stdin/stdout, without socket grants.
        Ok(profile)
    }

    fn grant(
        &mut self,
        path: &Path,
        operations: &str,
        required: bool,
        ipc_directory: Option<&Path>,
    ) -> Result<(), String> {
        let path = match fs::canonicalize(path) {
            Ok(path) => path,
            // IPC can also be an ordinary grant; every resolution failure stays path-free.
            Err(_) if ipc_directory == Some(path) => return Err(IPC_RESOLUTION_FAILURE.to_owned()),
            Err(error) if !required && error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(format!("Seatbelt path {}: {error}", path.display())),
        };
        // Seatbelt also checks the spelling used by open/exec. Keep Apple's fixed aliases
        // for these canonical roots; arbitrary symlinks never create additional grants.
        for (canonical, alias) in [
            ("/private/var", "/var"),
            ("/private/tmp", "/tmp"),
            ("/private/etc", "/etc"),
        ] {
            if let Ok(suffix) = path.strip_prefix(canonical) {
                self.grant_path(&Path::new(alias).join(suffix), operations, path.is_dir());
            }
        }
        self.grant_path(&path, operations, path.is_dir());
        Ok(())
    }

    fn grant_path(&mut self, path: &Path, operations: &str, directory: bool) -> String {
        let key = format!("PATH_{}", self.parameters.len());
        let mut parameter = OsString::from(format!("{key}="));
        parameter.push(path.as_os_str());
        self.parameters.push(parameter);
        let filter = if directory { "subpath" } else { "literal" };
        self.source.push_str(&format!(
            "(allow {operations} ({filter} (param \"{key}\")))\n"
        ));
        // Traversal can stat parent components, but cannot read their contents.
        self.source.push_str(&format!(
            "(allow file-read-metadata (path-ancestors (param \"{key}\")))\n"
        ));
        key
    }

    pub(super) fn command(&self) -> Command {
        let mut command = Command::new(super::SANDBOX_EXEC);
        command.arg("-p").arg(&self.source);
        for parameter in &self.parameters {
            command.arg("-D").arg(parameter);
        }
        command
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ipc_resolution_failures_are_path_free_in_every_policy_route() {
        let root = tempfile::tempdir().unwrap();
        let missing = root.path().join("missing-ipc");
        let looped = root.path().join("looped-ipc");
        std::os::unix::fs::symlink(&looped, &looped).unwrap();
        for ipc in [missing, looped] {
            for route in ["read_write", "private_tmp", "read_only", "exec"] {
                let mut policy = SandboxPolicy {
                    private_tmp: Some(ipc.clone()),
                    ..SandboxPolicy::default()
                };
                match route {
                    "read_write" => policy.read_write.push(ipc.clone()),
                    "read_only" => policy.read_only.push(ipc.clone()),
                    "exec" => policy.exec.push(ipc.clone()),
                    _ => {}
                }
                assert_eq!(
                    Profile::new(&policy).err().as_deref(),
                    Some("could not resolve the Seatbelt IPC directory"),
                    "{route}"
                );
            }
        }
    }

    #[test]
    fn paths_are_parameters_and_never_become_profile_syntax() {
        let root = tempfile::tempdir().unwrap();
        let path = root
            .path()
            .join("quotes\" (allow default) ; backslash\\ newline\n");
        fs::create_dir(&path).unwrap();
        let policy = SandboxPolicy {
            read_write: vec![path.clone()],
            deny_network: true,
            ..SandboxPolicy::default()
        };
        let profile = Profile::new(&policy).unwrap();
        assert!(!profile.source.contains("allow default"));
        let parameter = profile.parameters.last().unwrap();
        let mut expected = OsString::from(format!("PATH_{}=", profile.parameters.len() - 1));
        expected.push(path.canonicalize().unwrap());
        assert_eq!(parameter, &expected);
        assert!(!profile.source.contains("(allow network*)"));
    }

    #[test]
    fn system_grants_leave_package_manager_state_out() {
        let policy = SandboxPolicy::for_engine("libreoffice", Path::new("/usr/bin/true"), &[]);
        let profile = Profile::new(&policy).unwrap();
        let granted: Vec<&str> = profile
            .parameters
            .iter()
            .filter_map(|parameter| parameter.to_str()?.split_once('=').map(|(_, path)| path))
            .collect();
        for state in [
            "/usr/local/var",
            "/usr/local/etc",
            "/opt/homebrew/var",
            "/opt/homebrew/etc",
        ] {
            assert!(
                !granted
                    .iter()
                    .any(|path| Path::new(state).starts_with(path)),
                "{state} is reachable through a broader grant: {granted:?}"
            );
        }
    }
}

#[cfg(test)]
#[path = "macos_profile_tests.rs"]
mod library_tests;
