//! Locating and querying the Docling engine.
//!
//! The engine is a Python program speaking `ariad-engine/1`. It comes from the
//! `ASHIFT_DOCLING_ENGINE` development override, which holds a command line such as
//! `.tools/docling/python-env/bin/python -m ariad_docling`, or from the `docling` engine pack:
//! an installed pack or an unpacked one named by `ASHIFT_PACK_DOCLING`.

use std::{
    collections::BTreeMap,
    ffi::OsString,
    path::PathBuf,
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

type DescribeCache = Mutex<Option<(EngineCommand, Result<Description, String>)>>;

static DESCRIBE_CACHE: OnceLock<DescribeCache> = OnceLock::new();

/// Queries the engine once per process and command.
///
/// `Ok` means the engine answered `describe`; `Err` carries a short, path-free reason.
pub fn describe(command: &EngineCommand, cancel: CancellationToken) -> Result<Description, String> {
    let cache = DESCRIBE_CACHE.get_or_init(|| Mutex::new(None));
    {
        let guard = cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some((cached, result)) = guard.as_ref()
            && cached == command
        {
            return result.clone();
        }
    }
    let result = match runner::describe_with_env(
        &command.program,
        &command.args,
        &command.env,
        cancel.clone(),
    ) {
        Ok(Event::Capabilities {
            version,
            tool,
            models,
            ..
        }) => Ok(Description {
            version,
            tesseract: tool,
            models: models.unwrap_or_default(),
        }),
        Ok(_) => Err("the engine answered describe with an unexpected event".to_owned()),
        Err(runner::RunError::Spawn(_)) => Err("the engine could not be started".to_owned()),
        Err(runner::RunError::Timeout) => {
            Err("the engine did not answer describe in time".to_owned())
        }
        Err(runner::RunError::Cancelled) => return Err("describe was cancelled".to_owned()),
        Err(_) => Err("the engine failed its describe round trip".to_owned()),
    };
    if !cancel.is_cancelled() {
        let mut guard = cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *guard = Some((command.clone(), result.clone()));
    }
    result
}

/// Availability of the Docling engine; `None` when no engine is installed or configured.
///
/// The engine is available when it answers `describe`. Tesseract is reported as its tool, but a
/// missing Tesseract only disables Vietnamese OCR, so it does not make the engine unavailable.
#[must_use]
pub fn availability(cancel: CancellationToken) -> Option<EngineAvailability> {
    let command = resolve()?;
    Some(match describe(&command, cancel) {
        Ok(description) => EngineAvailability {
            engine: ENGINE_ID.to_owned(),
            available: true,
            tool_status: Some(description.tesseract),
            version: Some(description.version),
        },
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
    })
}

#[cfg(test)]
mod tests {
    use super::split_command_line;

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
