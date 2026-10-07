//! Doctor command implementation.

use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::{
    convert::ConvertError,
    pandoc_bin::{self, PANDOC_SUPPORTED, PandocBinaryError},
    runner,
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

/// Aggregate doctor diagnostic report.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DoctorReport {
    pub checks: Vec<DoctorCheck>,
    #[serde(skip)]
    pub tool_path: Option<PathBuf>,
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
    let workspace_check = match Workspace::new() {
        Ok(mut ws) => match ws.close() {
            Ok(()) => DoctorCheck {
                id: "workspace".to_owned(),
                ok: true,
                required: true,
                message: "temp directory is writable and workspace can be created and removed"
                    .to_owned(),
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
    let describe_check = match runner::describe(engine_program, &default_args, cancel) {
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
                failure_error = Some(ConvertError::Failed);
            }
            let sanitized_msg = match e {
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
                message: format!("engine describe round trip failed: {sanitized_msg}"),
                hint: "run 'just pandoc' or verify ASHIFT_PANDOC path and execution permissions"
                    .to_owned(),
            }
        }
    };
    checks.push(describe_check);

    Ok(DoctorReport {
        checks,
        tool_path,
        failure_error,
    })
}

#[cfg(test)]
mod tests {
    use super::doctor;
    use crate::convert::ConvertError;
    use tokio_util::sync::CancellationToken;

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
