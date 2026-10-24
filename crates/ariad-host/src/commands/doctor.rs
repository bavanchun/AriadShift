//! Doctor command implementation.

use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use ariad_core::protocol::ToolAvailability;

use crate::{
    convert::ConvertError,
    engines::docling,
    packs::{self, PackContext, PackHealth},
    pandoc_bin::{self, PANDOC_SUPPORTED, PandocBinaryError},
    runner,
    sandbox::{self, SandboxMode, SandboxSupport},
    workspace::Workspace,
};

/// A single environment or dependency diagnostic check.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DoctorCheck {
    pub id: String,
    pub ok: bool,
    pub required: bool,
    pub message: String,
    pub hint: String,
}

/// The engine sandbox as `doctor` sees it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DoctorSandbox {
    /// The effective mode, or `null` when `ASHIFT_SANDBOX` holds an unknown value.
    pub mode: Option<SandboxMode>,
    #[serde(flatten)]
    pub support: SandboxSupport,
}

/// Aggregate doctor diagnostic report.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DoctorReport {
    pub checks: Vec<DoctorCheck>,
    #[serde(default)]
    pub stale_workspaces: usize,
    /// The engine sandbox mode and what the kernel supports.
    pub sandbox: DoctorSandbox,
    #[serde(skip)]
    pub tool_path: Option<PathBuf>,
    /// Engine pack data directory; printed by the human output only.
    #[serde(skip)]
    pub data_dir: Option<PathBuf>,
    #[serde(skip)]
    pub failure_error: Option<ConvertError>,
}

impl DoctorReport {
    /// Returns true if all required checks passed.
    #[must_use]
    pub fn all_required_ok(&self) -> bool {
        self.checks.iter().all(|c| !c.required || c.ok)
    }

    /// Returns the typed error representing the first failure, if any required check failed.
    #[must_use]
    pub fn failure(&self) -> Option<ConvertError> {
        if self.all_required_ok() {
            None
        } else {
            self.failure_error.clone().or(Some(ConvertError::Failed))
        }
    }
}

/// Runs environment and toolchain diagnostic checks.
pub fn doctor(
    engine_program: &Path,
    cancel: CancellationToken,
) -> Result<DoctorReport, ConvertError> {
    if cancel.is_cancelled() {
        return Err(ConvertError::Interrupted);
    }

    let mut checks = Vec::new();
    let mut failure_error: Option<ConvertError> = None;

    // 1. Pandoc check
    let pandoc_loc = pandoc_bin::locate_with_cancel(Some(&cancel));
    if cancel.is_cancelled() {
        return Err(ConvertError::Interrupted);
    }
    let (tool_path, pandoc_check) = match pandoc_loc {
        Ok(binary) => (
            Some(binary.path),
            DoctorCheck {
                id: "pandoc_version".to_owned(),
                ok: true,
                required: true,
                message: format!("Pandoc {} found in {PANDOC_SUPPORTED}", binary.version),
                hint: String::new(),
            },
        ),
        Err(PandocBinaryError::Missing) => {
            if failure_error.is_none() {
                failure_error = Some(ConvertError::ToolMissing);
            }
            (
                None,
                DoctorCheck {
                    id: "pandoc_version".to_owned(),
                    ok: false,
                    required: true,
                    message: "Pandoc executable was not found".to_owned(),
                    hint: "run 'just pandoc' or install Pandoc >=3.12,<4 and set ASHIFT_PANDOC"
                        .to_owned(),
                },
            )
        }
        Err(PandocBinaryError::UnsupportedVersion { found }) => {
            if failure_error.is_none() {
                failure_error = Some(ConvertError::ToolMissing);
            }
            let detail = match found {
                Some(ref v) => {
                    format!("Pandoc version {v} is unsupported; must be {PANDOC_SUPPORTED}")
                }
                None => format!("Pandoc version is unsupported; must be {PANDOC_SUPPORTED}"),
            };
            (
                None,
                DoctorCheck {
                    id: "pandoc_version".to_owned(),
                    ok: false,
                    required: true,
                    message: detail,
                    hint: "run 'just pandoc' or install Pandoc >=3.12,<4".to_owned(),
                },
            )
        }
        Err(PandocBinaryError::TimedOut) => {
            if failure_error.is_none() {
                failure_error = Some(ConvertError::ToolMissing);
            }
            (
                None,
                DoctorCheck {
                    id: "pandoc_version".to_owned(),
                    ok: false,
                    required: true,
                    message: "Pandoc timed out while checking version".to_owned(),
                    hint: "ensure Pandoc is responsive or reinstall it with 'just pandoc'"
                        .to_owned(),
                },
            )
        }
        Err(PandocBinaryError::Interrupted) => return Err(ConvertError::Interrupted),
    };
    checks.push(pandoc_check);

    if cancel.is_cancelled() {
        return Err(ConvertError::Interrupted);
    }

    // 2. Temp directory and workspace check
    let stale_found =
        crate::workspace::count_stale(std::time::Duration::from_secs(24 * 3600)).unwrap_or(0);
    let workspace_check = match Workspace::new() {
        Ok(mut ws) => match ws.close() {
            Ok(()) => DoctorCheck {
                id: "workspace".to_owned(),
                ok: true,
                required: true,
                message: if stale_found > 0 {
                    format!(
                        "temp directory is writable and workspace can be created and removed; found {stale_found} stale workspace(s)"
                    )
                } else {
                    "temp directory is writable and workspace can be created and removed".to_owned()
                },
                hint: String::new(),
            },
            Err(e) => {
                if failure_error.is_none() {
                    failure_error = Some(ConvertError::Failed);
                }
                DoctorCheck {
                    id: "workspace".to_owned(),
                    ok: false,
                    required: true,
                    message: format!("could not remove temporary workspace: {e}"),
                    hint:
                        "ensure temp directory permissions allow creating and deleting directories"
                            .to_owned(),
                }
            }
        },
        Err(e) => {
            if failure_error.is_none() {
                failure_error = Some(ConvertError::Failed);
            }
            DoctorCheck {
                id: "workspace".to_owned(),
                ok: false,
                required: true,
                message: format!("could not create temporary workspace: {e}"),
                hint: "ensure temp directory is writable and has free space".to_owned(),
            }
        }
    };
    checks.push(workspace_check);

    if cancel.is_cancelled() {
        return Err(ConvertError::Interrupted);
    }

    // 3. Engine describe round trip
    let default_args = [
        std::ffi::OsString::from("__engine"),
        std::ffi::OsString::from("pandoc"),
    ];
    let describe_check = match runner::describe(engine_program, &default_args, cancel.clone()) {
        Ok(ariad_core::protocol::Event::Capabilities {
            ref tool,
            ref engine,
            ..
        }) => {
            if tool.status == ariad_core::protocol::ToolAvailability::Found {
                DoctorCheck {
                    id: "engine_describe".to_owned(),
                    ok: true,
                    required: true,
                    message: format!("engine '{engine}' passes describe round trip"),
                    hint: String::new(),
                }
            } else {
                if failure_error.is_none() {
                    failure_error = Some(ConvertError::ToolMissing);
                }
                let status_desc = match tool.status {
                    ariad_core::protocol::ToolAvailability::WrongVersion => {
                        "unsupported tool version"
                    }
                    ariad_core::protocol::ToolAvailability::Missing => "tool is missing",
                    _ => "tool is not ready",
                };
                DoctorCheck {
                    id: "engine_describe".to_owned(),
                    ok: false,
                    required: true,
                    message: format!("engine '{engine}' describe reported {status_desc}"),
                    hint: "run 'just pandoc' to install Pandoc >=3.12,<4".to_owned(),
                }
            }
        }
        Ok(_) => {
            if failure_error.is_none() {
                failure_error = Some(ConvertError::Failed);
            }
            DoctorCheck {
                id: "engine_describe".to_owned(),
                ok: false,
                required: true,
                message: "engine returned unexpected response to describe".to_owned(),
                hint: "verify engine implementation conforms to engine protocol".to_owned(),
            }
        }
        Err(e) => {
            if failure_error.is_none() {
                failure_error = Some(match &e {
                    runner::RunError::SandboxUnavailable(detail)
                    | runner::RunError::SandboxLaunchFailed(detail) => {
                        ConvertError::SandboxUnavailable(detail.clone())
                    }
                    _ => ConvertError::Failed,
                });
            }
            let startup_failure = match &e {
                runner::RunError::SandboxLaunchFailed(detail) => Some(detail.as_str()),
                _ => None,
            };
            let sanitized_msg = match &e {
                runner::RunError::SandboxUnavailable(_) => "the engine sandbox is unavailable",
                runner::RunError::Spawn { .. } => "could not spawn engine process",
                runner::RunError::Io(_) => "engine communication I/O failure",
                runner::RunError::Timeout => "engine describe timed out",
                runner::RunError::Crash { .. } => "engine process crashed during describe",
                _ => "engine describe round trip failed",
            };
            DoctorCheck {
                id: "engine_describe".to_owned(),
                ok: false,
                required: true,
                message: startup_failure.map_or_else(
                    || format!("engine describe round trip failed: {sanitized_msg}"),
                    |detail| format!("engine 'pandoc' is installed but cannot start: {detail}"),
                ),
                hint: startup_failure.map_or_else(
                    || {
                        "run 'just pandoc' or verify ASHIFT_PANDOC path and execution permissions"
                            .to_owned()
                    },
                    |detail| launcher_failure_hint(detail).to_owned(),
                ),
            }
        }
    };
    checks.push(describe_check);

    // 4. Built-in and system engines beyond Pandoc
    for spec in crate::engines::REGISTRY
        .iter()
        .filter(|spec| spec.id != crate::engines::PANDOC.id && spec.id != docling::ENGINE_ID)
    {
        if cancel.is_cancelled() {
            return Err(ConvertError::Interrupted);
        }
        let check = engine_check(engine_program, spec, cancel.clone());
        if spec.required && !check.ok && failure_error.is_none() {
            failure_error = Some(ConvertError::missing_engine(spec.id));
        }
        checks.push(check);
    }

    // 5. Engine packs: data directory, trusted keys, installed packs. None are required.
    let data_dir = pack_checks(&mut checks);
    let (sandbox_check, sandbox_report) = sandbox_check();
    if !sandbox_check.ok && sandbox_check.required && failure_error.is_none() {
        failure_error = Some(ConvertError::SandboxUnavailable(
            sandbox_check.message.clone(),
        ));
    }
    checks.push(sandbox_check);

    // 5. The Docling engine and the Tesseract it runs. Neither is required.
    if cancel.is_cancelled() {
        return Err(ConvertError::Interrupted);
    }
    checks.extend(docling_checks(engine_program, cancel));

    Ok(DoctorReport {
        checks,
        stale_workspaces: stale_found,
        sandbox: sandbox_report,
        tool_path,
        data_dir,
        failure_error,
    })
}

fn optional_check(id: String, ok: bool, message: String, hint: &str) -> DoctorCheck {
    DoctorCheck {
        id,
        ok,
        required: false,
        message,
        hint: if ok { String::new() } else { hint.to_owned() },
    }
}

/// Adds the engine pack checks and returns the data directory when it is usable.
fn pack_checks(checks: &mut Vec<DoctorCheck>) -> Option<PathBuf> {
    let context = match PackContext::from_env() {
        Ok(context) => context,
        Err(_) => {
            checks.push(optional_check(
                "data_dir".to_owned(),
                false,
                "engine pack data directory could not be determined".to_owned(),
                "set ASHIFT_DATA_DIR to a writable directory",
            ));
            return None;
        }
    };
    let packs_dir = context.packs_dir();
    let writable =
        std::fs::create_dir_all(&packs_dir).is_ok() && tempfile::tempfile_in(&packs_dir).is_ok();
    checks.push(optional_check(
        "data_dir".to_owned(),
        writable,
        if writable {
            "engine pack directory is writable".to_owned()
        } else {
            "engine pack directory is not writable".to_owned()
        },
        "set ASHIFT_DATA_DIR to a writable directory",
    ));

    let ids = context.keys.ids();
    checks.push(optional_check(
        "pack_keys".to_owned(),
        !ids.is_empty(),
        if ids.is_empty() {
            "no engine pack signing keys are built into this ashift".to_owned()
        } else {
            format!("engine pack signing keys: {}", ids.join(", "))
        },
        "this build cannot install engine packs; use an official release build",
    ));

    for installed in packs::list_installed(&context).unwrap_or_default() {
        let label = format!(
            "engine pack {} {} ({})",
            installed.name, installed.version, installed.target
        );
        let (ok, message) = match packs::check(&context, &installed.name, &installed.version) {
            PackHealth::Ok if installed.active => {
                (true, format!("{label} is installed and active"))
            }
            PackHealth::Ok => (true, format!("{label} is installed")),
            PackHealth::Problem(problem) => (false, format!("{label}: {problem}")),
        };
        checks.push(optional_check(
            format!("pack_{}", installed.name),
            ok,
            message,
            &format!(
                "run 'ashift engines verify {0}', or reinstall with 'ashift engines remove {0}' and 'ashift engines install {0}'",
                installed.name
            ),
        ));
    }
    for name in packs::override_names() {
        let variable = packs::override_var(&name);
        let ok = matches!(packs::try_resolve(&name), Ok(Some(_)));
        checks.push(optional_check(
            format!("pack_{name}"),
            ok,
            if ok {
                format!("engine pack {name} comes from the development override {variable}")
            } else {
                format!("the development override {variable} is unusable")
            },
            "point it at a directory holding pack.toml and the unpacked pack",
        ));
    }
    writable.then_some(context.data_dir)
}

/// Reports the sandbox mode and kernel support. The check is required only in required mode.
fn sandbox_check() -> (DoctorCheck, DoctorSandbox) {
    let support = sandbox::support();
    let mode = sandbox::effective_mode();
    let check = |ok: bool, required: bool, message: String, hint: &str| DoctorCheck {
        id: "sandbox".to_owned(),
        ok,
        required,
        message,
        hint: hint.to_owned(),
    };
    let check = match &mode {
        Err(message) => check(
            false,
            true,
            format!("engine sandbox: {message}"),
            "unset ASHIFT_SANDBOX or set it to off, best-effort or required",
        ),
        Ok(SandboxMode::Off) => check(
            true,
            false,
            "engine sandbox: off (ASHIFT_SANDBOX=off); engines run without isolation".to_owned(),
            "",
        ),
        Ok(mode) if !support.platform => check(
            *mode != SandboxMode::Required,
            *mode == SandboxMode::Required,
            format!(
                "engine sandbox: {mode}; not used on this platform, engines run in their own process group or Job Object"
            ),
            "required mode needs Linux",
        ),
        Ok(mode) => {
            let gaps = support.gaps();
            let mut features = Vec::new();
            if let Some(abi) = support.landlock_abi {
                let mut covered = vec!["filesystem"];
                if support.landlock_network {
                    covered.push("TCP");
                }
                if support.landlock_scopes {
                    covered.push("scopes");
                }
                if support.landlock_unix_sockets {
                    covered.push("UNIX sockets");
                }
                features.push(format!("Landlock ABI {abi} ({})", covered.join(", ")));
            } else {
                features.push("no Landlock".to_owned());
            }
            features.push(if support.seccomp {
                "seccomp available".to_owned()
            } else {
                "no seccomp".to_owned()
            });
            if support.landlock_filesystem && !support.landlock_unix_sockets {
                features.push("pathname UNIX socket isolation needs Landlock ABI 9".to_owned());
            }
            let mut message = format!("engine sandbox: {mode}; {}", features.join(", "));
            if !gaps.is_empty() {
                message.push_str("; ");
                message.push_str(&gaps.join("; "));
            }
            check(
                gaps.is_empty(),
                *mode == SandboxMode::Required,
                message,
                "use a kernel with Landlock enabled (Linux 5.13 or newer with landlock in the LSM list)",
            )
        }
    };
    (
        check,
        DoctorSandbox {
            mode: mode.ok(),
            support,
        },
    )
}

/// Optional checks for the Docling engine and the Tesseract it runs for Vietnamese OCR.
fn docling_checks(engine_program: &Path, cancel: CancellationToken) -> Vec<DoctorCheck> {
    let Some(command) = docling::resolve() else {
        return vec![DoctorCheck {
            id: "docling_engine".to_owned(),
            ok: false,
            required: false,
            message:
                "docling engine is not installed; OCR, layout extraction and image routes need it"
                    .to_owned(),
            hint: format!(
                "run 'ashift engines install docling', or set {} to a development engine",
                docling::OVERRIDE_ENV
            ),
        }];
    };
    match docling::describe(&command, engine_program, cancel) {
        Ok(description) => {
            let models = if description.models.is_empty() {
                "no models found".to_owned()
            } else {
                format!("models: {}", description.models.join(", "))
            };
            let tesseract = &description.tesseract;
            let tesseract_check = match tesseract.status {
                ToolAvailability::Found => DoctorCheck {
                    id: "tesseract".to_owned(),
                    ok: true,
                    required: false,
                    message: format!(
                        "Tesseract {} found for Vietnamese OCR",
                        tesseract.version.as_deref().unwrap_or("5")
                    ),
                    hint: String::new(),
                },
                status => DoctorCheck {
                    id: "tesseract".to_owned(),
                    ok: false,
                    required: false,
                    message: if status == ToolAvailability::WrongVersion {
                        "Tesseract is older than 5; Vietnamese OCR is unavailable".to_owned()
                    } else {
                        "Tesseract was not found; Vietnamese OCR is unavailable".to_owned()
                    },
                    hint: "install Tesseract 5 from your package manager (apt install tesseract-ocr, brew install tesseract) or the UB Mannheim installer on Windows, and put it on PATH".to_owned(),
                },
            };
            vec![
                DoctorCheck {
                    id: "docling_engine".to_owned(),
                    ok: true,
                    required: false,
                    message: format!(
                        "docling engine (Docling {}) passes describe round trip; {models}",
                        description.version
                    ),
                    hint: String::new(),
                },
                tesseract_check,
            ]
        }
        Err(reason) => {
            let launcher_failed = reason.starts_with(crate::sandbox::LAUNCH_FAILURE_PREFIX);
            vec![DoctorCheck {
                id: "docling_engine".to_owned(),
                ok: false,
                required: false,
                message: if launcher_failed {
                    format!("docling engine is installed but cannot start: {reason}")
                } else {
                    format!("docling engine describe round trip failed: {reason}")
                },
                hint: if launcher_failed {
                    launcher_failure_hint(&reason).to_owned()
                } else {
                    format!(
                        "run 'ashift engines verify docling', reinstall the pack, or check {}",
                        docling::OVERRIDE_ENV
                    )
                },
            }]
        }
    }
}

fn launcher_failure_hint(detail: &str) -> &'static str {
    if detail.to_ascii_lowercase().contains("namespace") {
        crate::sandbox::NAMESPACE_HINT
    } else {
        "check the launcher failure detail and ASHIFT_SANDBOX configuration; see docs/cli.md"
    }
}

/// Runs `describe` against one registered engine and reports its tool status.
fn engine_check(
    engine_program: &Path,
    spec: &crate::engines::EngineSpec,
    cancel: CancellationToken,
) -> DoctorCheck {
    let invocation = crate::engines::EngineInvocation::new(engine_program);
    let id = format!("engine_{}", spec.id);
    let mut startup_hint = None;
    let (ok, message) =
        match runner::describe(engine_program, &invocation.args_for(spec.id), cancel) {
            Ok(ariad_core::protocol::Event::Capabilities { tool, .. }) => match tool.status {
                ariad_core::protocol::ToolAvailability::Found => {
                    let version = tool
                        .version
                        .filter(|_| !spec.builtin)
                        .map(|version| format!(" ({} {version})", tool.name))
                        .unwrap_or_default();
                    (true, format!("engine '{}' is ready{version}", spec.id))
                }
                ariad_core::protocol::ToolAvailability::WrongVersion => (
                    false,
                    format!(
                        "engine '{}' found {} {}, outside the supported range {}",
                        spec.id,
                        tool.name,
                        tool.version.as_deref().unwrap_or("of unknown version"),
                        spec.supported.unwrap_or("-")
                    ),
                ),
                ariad_core::protocol::ToolAvailability::Missing => (
                    false,
                    format!("engine '{}' could not find {}", spec.id, tool.name),
                ),
            },
            Ok(_) => (
                false,
                format!(
                    "engine '{}' returned an unexpected describe response",
                    spec.id
                ),
            ),
            Err(runner::RunError::SandboxLaunchFailed(detail)) => {
                startup_hint = Some(launcher_failure_hint(&detail));
                (
                    false,
                    format!(
                        "engine '{}' is installed but cannot start: {detail}",
                        spec.id
                    ),
                )
            }
            Err(_) => (
                false,
                format!("engine '{}' describe round trip failed", spec.id),
            ),
        };
    DoctorCheck {
        id,
        ok,
        required: spec.required,
        message,
        hint: if ok {
            String::new()
        } else if let Some(hint) = startup_hint {
            hint.to_owned()
        } else {
            spec.install_hint.to_owned()
        },
    }
}

#[cfg(test)]
mod tests {
    use super::doctor;
    use crate::convert::ConvertError;
    use tokio_util::sync::CancellationToken;

    #[test]
    fn launcher_hints_match_the_failure_cause() {
        assert!(
            super::launcher_failure_hint("mount namespaces: Permission denied")
                .contains("kernel.apparmor_restrict_unprivileged_userns=0")
        );
        let generic = super::launcher_failure_hint("seccomp filter could not be installed");
        assert!(generic.contains("ASHIFT_SANDBOX"));
        assert!(!generic.contains("userns"));
    }

    #[test]
    fn doctor_describe_failure_produces_failed_report() {
        let temp = tempfile::tempdir().unwrap();
        let bad_engine = temp.path().join("definitely-nonexistent-engine-binary");
        let report = doctor(&bad_engine, CancellationToken::new()).expect("doctor run");
        let describe_check = report
            .checks
            .iter()
            .find(|c| c.id == "engine_describe")
            .expect("engine_describe check exists");
        assert!(!describe_check.ok);
        assert!(describe_check.required);
        assert_eq!(report.failure(), Some(ConvertError::Failed));
    }
}
