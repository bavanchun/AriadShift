//! Seatbelt confinement through Apple's system launcher, without in-process FFI.

use std::{os::unix::process::CommandExt, path::Path, process::Command};

use ariad_core::protocol::{EngineError, ErrorCode, Event};

use super::{LaunchSpec, SandboxPolicy, SandboxSupport};

#[path = "macos_profile.rs"]
mod profile;

const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";

pub(super) fn probe() -> SandboxSupport {
    let available = Path::new(SANDBOX_EXEC).is_file();
    let usable = available
        && profile::Profile::new(&SandboxPolicy {
            deny_network: true,
            ..SandboxPolicy::default()
        })
        .and_then(|profile| check_profile(&profile))
        .is_ok();
    SandboxSupport {
        platform: true,
        seatbelt_available: available,
        seatbelt_usable: usable,
        ..SandboxSupport::default()
    }
}

pub(super) fn gaps(support: &SandboxSupport) -> Vec<&'static str> {
    if !support.seatbelt_available {
        vec![
            "Seatbelt is unavailable (/usr/bin/sandbox-exec is missing), so filesystem and network access are not confined",
        ]
    } else if !support.seatbelt_usable {
        vec!["Seatbelt is present but the system refused its enforcement probe"]
    } else {
        Vec::new()
    }
}

/// Validate the exact profile before replacing the launcher, so policy compilation or
/// sandbox_init failures retain the launcher's typed error and never retry without isolation.
fn check_profile(profile: &profile::Profile) -> Result<(), String> {
    #[cfg(debug_assertions)]
    if super::simulated_seatbelt_refusal() {
        return Err("Seatbelt could not apply the profile: simulated refusal".to_owned());
    }
    let output = profile
        .command()
        .arg("/usr/bin/true")
        .output()
        .map_err(|error| format!("could not start Seatbelt: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "Seatbelt could not apply the profile: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

pub(super) fn apply_and_exec(spec: &LaunchSpec) -> String {
    // Preserve caller-relative executables without resolving Python venv symlinks.
    let program = match std::path::absolute(&spec.program) {
        Ok(program) => program,
        Err(_) => return "could not resolve the engine program".to_owned(),
    };
    if !spec.program.is_file() {
        return "could not execute the engine program: file not found".to_owned();
    }
    #[cfg(debug_assertions)]
    let simulated = super::simulated_without_support();
    #[cfg(not(debug_assertions))]
    let simulated = false;
    let available = !simulated && Path::new(SANDBOX_EXEC).is_file();
    if spec.required && !available {
        return "Seatbelt is unavailable (/usr/bin/sandbox-exec is missing)".to_owned();
    }
    let target = if available {
        let profile = match profile::Profile::new(&spec.policy) {
            Ok(profile) => profile,
            Err(error) => return error,
        };
        if let Err(error) = check_profile(&profile) {
            return error;
        }
        let mut command = profile.command();
        command.arg(&program).args(&spec.args);
        command
    } else {
        // The parent reports reduced isolation in best-effort mode, as on Linux.
        let mut command = Command::new(&program);
        command.args(&spec.args);
        command
    };
    // macOS's nix resource feature is not enabled. The noninteractive system Bash
    // sets limits before exec instead of adding unsafe calls or another dependency.
    let script = match limit_script(spec) {
        Ok(script) => script,
        Err(error) => return error,
    };
    let event = Event::Result {
        ok: false,
        metrics: None,
        error: Some(EngineError {
            code: ErrorCode::EngineFailure,
            message: format!(
                "{}could not apply engine resource limits",
                super::LAUNCH_FAILURE_PREFIX
            ),
        }),
    };
    let failure = match serde_json::to_string(&event) {
        Ok(line) => line,
        Err(error) => return format!("could not encode launcher failure: {error}"),
    };
    // Runtime startup may inspect cwd; keep it inside a granted directory.
    let cwd = spec
        .policy
        .read_write
        .iter()
        .find(|path| path.is_dir())
        .map_or(Path::new("/"), |path| path.as_path());
    let mut command = Command::new("/bin/bash");
    // Built-in adapters resolve these explicit tool paths after the cwd change.
    for name in ["ASHIFT_PANDOC", crate::engines::libreoffice::SOFFICE_ENV] {
        if let Some(value) = std::env::var_os(name).filter(|value| !value.is_empty()) {
            let path = match std::path::absolute(Path::new(&value)) {
                Ok(path) => path,
                Err(_) => return "could not resolve the engine tool override".to_owned(),
            };
            command.env(name, path);
        }
    }
    command.env_remove("OSL_SOCKET_PATH");
    if let Some(ipc) = &spec.policy.private_tmp {
        command.env("OSL_SOCKET_PATH", ipc);
    }
    let error = command
        .current_dir(cwd)
        .args([
            "--noprofile",
            "--norc",
            "-c",
            &script,
            "ashift-sandbox",
            &failure,
        ])
        .env_remove("BASH_ENV")
        .env_remove("ENV")
        .env_remove("POSIXLY_CORRECT")
        .env_remove("SHELLOPTS")
        .env_remove("BASHOPTS")
        .arg(target.get_program())
        .args(target.get_args())
        .exec();
    // Keep the original process group and inherited protocol pipes across both execs.
    format!("could not execute the engine program: {error}")
}

fn limit_script(spec: &LaunchSpec) -> Result<String, String> {
    let mut script = String::from("failure=$1; shift\n");
    let limits = std::iter::once(("core", 0)).chain(
        spec.policy
            .rlimits
            .iter()
            .map(|(name, value)| (name.as_str(), *value)),
    );
    for (name, value) in limits {
        // Bash outside POSIX mode uses KiB for memory and 1024-byte blocks for files.
        let (flag, unit) = match name {
            "core" => ("c", 1024),
            "cpu" => ("t", 1),
            "fsize" => ("f", 1024),
            "nofile" => ("n", 1),
            "nproc" => ("u", 1),
            "data" => ("d", 1024),
            "as" => ("v", 1024),
            "stack" => ("s", 1024),
            other => return Err(format!("unknown rlimit {other}")),
        };
        // Only fixed flags and parsed integers enter shell source; paths stay in "$@".
        script.push_str(&format!("ulimit -S -{flag} {} && ulimit -H -{flag} {} || {{ printf '%s\\n' \"$failure\"; exit {}; }}\n", value / unit, value / unit, super::LAUNCH_FAILURE_EXIT));
    }
    script.push_str("exec \"$@\"\n");
    Ok(script)
}
