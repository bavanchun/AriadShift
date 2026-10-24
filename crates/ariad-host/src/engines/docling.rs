//! Locating and querying the Docling engine.
//!
//! The engine is a Python program speaking `ariad-engine/1`. It comes from the
//! `ASHIFT_DOCLING_ENGINE` development override, which holds a command line such as
//! `.tools/docling/python-env/bin/python -m ariad_docling`, or from the `docling` engine pack:
//! an installed pack or an unpacked one named by `ASHIFT_PACK_DOCLING`.

use std::{
    collections::BTreeMap,
    ffi::OsString,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

use ariad_core::protocol::{Event, ToolAvailability, ToolStatus};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::{engines::EngineAvailability, packs, runner};

/// The engine identifier used in `capabilities.json`.
pub const ENGINE_ID: &str = "docling";

/// Development override: the command line that starts the engine.
pub const OVERRIDE_ENV: &str = "ASHIFT_DOCLING_ENGINE";

/// How to start the Docling engine and the request options it needs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EngineCommand {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    /// Settings a pack adds to the cleared engine environment.
    pub env: Vec<(OsString, OsString)>,
    /// Request options such as `artifacts_path` and `tessdata_path`.
    pub options: BTreeMap<String, Value>,
    pub install_root: Option<PathBuf>,
}

impl EngineCommand {
    /// Carries pack identity and sandbox access into the common runner and session pool.
    #[must_use]
    pub fn runner_command(&self, ashift: &Path) -> runner::EngineCommand {
        runner::EngineCommand {
            engine: ENGINE_ID.to_owned(),
            program: self.program.clone(),
            args: self.args.clone(),
            env: self.env.clone(),
            install_root: self.install_root.clone(),
            launcher: Some(ashift.to_path_buf()),
            sandbox: None,
        }
    }
}

/// Finds the engine: the development override first, then an installed pack.
#[must_use]
pub fn resolve() -> Option<EngineCommand> {
    match std::env::var_os(OVERRIDE_ENV) {
        Some(value) => from_override(&value),
        None => installed_pack(),
    }
}

/// The engine of the `docling` pack, from the installed pack or the `ASHIFT_PACK_DOCLING`
/// directory. A pack that cannot be used resolves to nothing; `ashift engines` and
/// `ashift doctor` report why.
fn installed_pack() -> Option<EngineCommand> {
    let pack = packs::resolve(ENGINE_ID)?;
    Some(EngineCommand {
        install_root: Some(pack.root),
        program: pack.program,
        args: pack.args,
        env: pack.env,
        options: pack.options.into_iter().collect(),
    })
}

fn from_override(value: &std::ffi::OsStr) -> Option<EngineCommand> {
    let words = split_command_line(value.to_str()?)?;
    let (program, args) = words.split_first()?;
    Some(EngineCommand {
        install_root: None,
        program: PathBuf::from(program),
        args: args.iter().map(OsString::from).collect(),
        env: Vec::new(),
        // The engine finds its models and OCR data next to its own runtime.
        options: BTreeMap::new(),
    })
}

/// Splits a command line on whitespace; double quotes group words that contain spaces.
///
/// Returns `None` for an empty line or an unterminated quote.
#[must_use]
pub fn split_command_line(line: &str) -> Option<Vec<String>> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut in_word = false;
    let mut quoted = false;
    for character in line.chars() {
        match character {
            '"' => {
                quoted = !quoted;
                in_word = true;
            }
            character if character.is_whitespace() && !quoted => {
                if in_word {
                    words.push(std::mem::take(&mut current));
                    in_word = false;
                }
            }
            character => {
                current.push(character);
                in_word = true;
            }
        }
    }
    if quoted {
        return None;
    }
    if in_word {
        words.push(current);
    }
    (!words.is_empty()).then_some(words)
}

/// What `describe` reported about the engine.
#[derive(Clone, Debug, PartialEq)]
pub struct Description {
    /// The Docling version.
    pub version: String,
    /// Tesseract, which the engine runs for Vietnamese OCR.
    pub tesseract: ToolStatus,
    /// Installed model and OCR data sets.
    pub models: Vec<String>,
}

type DescribeCache = Mutex<Option<(EngineCommand, PathBuf, Description)>>;

static DESCRIBE_CACHE: OnceLock<DescribeCache> = OnceLock::new();

/// Queries the engine once per process and command.
///
/// `Ok` means the engine answered `describe`; `Err` carries a short, path-free reason.
pub fn describe(
    command: &EngineCommand,
    ashift: &Path,
    cancel: CancellationToken,
) -> Result<Description, String> {
    query(command, ashift, cancel).map_err(describe_failure)
}

fn describe_failure(error: runner::RunError) -> String {
    const STATUS_DLL_NOT_FOUND: i32 = -1_073_741_515;
    let missing_dll = match &error {
        runner::RunError::Crash { exit, stderr_tail } => {
            *exit == Some(STATUS_DLL_NOT_FOUND)
                || stderr_tail.contains("ImportError: DLL load failed")
        }
        runner::RunError::EngineFailed { message, .. } => {
            message.contains("ImportError: DLL load failed")
        }
        runner::RunError::Spawn(error) => error.raw_os_error() == Some(126),
        _ => false,
    };
    if cfg!(windows) && missing_dll {
        return "Windows could not load a required DLL; install the latest Microsoft Visual C++ Redistributable (x64, 2015-2022 family) and retry".to_owned();
    }
    match error {
        runner::RunError::Spawn(_) => "the engine could not be started".to_owned(),
        runner::RunError::Timeout => "the engine did not answer describe in time".to_owned(),
        runner::RunError::Cancelled => "describe was cancelled".to_owned(),
        runner::RunError::SandboxUnavailable(_) => "the engine sandbox is unavailable".to_owned(),
        _ => "the engine failed its describe round trip".to_owned(),
    }
}

fn query(
    command: &EngineCommand,
    ashift: &Path,
    cancel: CancellationToken,
) -> Result<Description, runner::RunError> {
    let cache = DESCRIBE_CACHE.get_or_init(|| Mutex::new(None));
    {
        let guard = cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some((cached, launcher, description)) = guard.as_ref()
            && cached == command
            && launcher == ashift
        {
            return Ok(description.clone());
        }
    }
    let description = match runner::describe_command(
        &command.runner_command(ashift),
        runner::DESCRIBE_TIMEOUT,
        cancel.clone(),
    )? {
        Event::Capabilities {
            version,
            tool,
            models,
            ..
        } => Description {
            version,
            tesseract: tool,
            models: models.unwrap_or_default(),
        },
        _ => {
            return Err(runner::RunError::ProtocolViolation(
                "expected capabilities from describe",
            ));
        }
    };
    if !cancel.is_cancelled() {
        let mut guard = cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *guard = Some((command.clone(), ashift.to_path_buf(), description.clone()));
    }
    Ok(description)
}

/// Availability of the Docling engine; `None` when no engine is installed or configured.
///
/// The engine is available when it answers `describe`. Tesseract is reported as its tool, but a
/// missing Tesseract only disables Vietnamese OCR, so it does not make the engine unavailable.
pub fn availability(
    ashift: &Path,
    cancel: CancellationToken,
) -> Result<Option<EngineAvailability>, runner::RunError> {
    let Some(command) = resolve() else {
        return Ok(None);
    };
    Ok(Some(match query(&command, ashift, cancel) {
        Ok(description) => EngineAvailability {
            engine: ENGINE_ID.to_owned(),
            available: true,
            tool_status: Some(description.tesseract),
            version: Some(description.version),
        },
        Err(error @ (runner::RunError::Cancelled | runner::RunError::SandboxUnavailable(_))) => {
            return Err(error);
        }
        Err(_) => EngineAvailability {
            engine: ENGINE_ID.to_owned(),
            available: false,
            tool_status: Some(ToolStatus {
                name: ENGINE_ID.to_owned(),
                version: None,
                status: ToolAvailability::Missing,
            }),
            version: None,
        },
    }))
}

#[cfg(test)]
mod tests {
    use super::split_command_line;

    #[test]
    fn dll_load_failures_name_the_windows_redistributable_prerequisite() {
        use crate::runner::RunError;

        for error in [
            RunError::Crash {
                exit: Some(-1_073_741_515),
                stderr_tail: String::new(),
            },
            RunError::Spawn(std::io::Error::from_raw_os_error(126)),
            RunError::EngineFailed {
                code: ariad_core::protocol::ErrorCode::EngineFailure,
                message: "ImportError: DLL load failed while importing torch".to_owned(),
            },
            RunError::Crash {
                exit: Some(1),
                stderr_tail: "Traceback:\nImportError: DLL load failed while importing torch"
                    .to_owned(),
            },
        ] {
            let message = super::describe_failure(error);
            assert_eq!(
                message.contains("Visual C++ Redistributable (x64"),
                cfg!(windows)
            );
        }
        for error in [
            RunError::Spawn(std::io::Error::from_raw_os_error(2)),
            RunError::EngineFailed {
                code: ariad_core::protocol::ErrorCode::EngineFailure,
                message: "ImportError: No module named torch".to_owned(),
            },
            RunError::Crash {
                exit: Some(1),
                stderr_tail: "unrelated engine failure".to_owned(),
            },
            RunError::Timeout,
        ] {
            assert!(!super::describe_failure(error).contains("Redistributable"));
        }
    }

    #[test]
    fn splits_command_lines_with_quoted_words() {
        assert_eq!(
            split_command_line(".tools/docling/python-env/bin/python -m ariad_docling"),
            Some(vec![
                ".tools/docling/python-env/bin/python".to_owned(),
                "-m".to_owned(),
                "ariad_docling".to_owned()
            ])
        );
        assert_eq!(
            split_command_line(r#""C:\Program Files\Python\python.exe"  -I  -m ariad_docling"#),
            Some(vec![
                r"C:\Program Files\Python\python.exe".to_owned(),
                "-I".to_owned(),
                "-m".to_owned(),
                "ariad_docling".to_owned()
            ])
        );
        assert_eq!(
            split_command_line(r#"python "" x"#),
            Some(vec!["python".to_owned(), String::new(), "x".to_owned()])
        );
        assert_eq!(split_command_line("   "), None);
        assert_eq!(split_command_line(r#"python "unterminated"#), None);
    }
}
