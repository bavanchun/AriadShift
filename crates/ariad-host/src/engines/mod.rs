pub mod common;
pub mod docling;
pub mod libreoffice;
pub mod pandoc;
pub mod pdf;
pub mod sandbox;
pub mod typst;

use std::{
    collections::HashMap,
    ffi::OsString,
    path::Path,
    sync::{Mutex, OnceLock},
};

use ariad_core::{
    format::Format,
    protocol::{Event, ToolAvailability, ToolStatus},
};
use tokio_util::sync::CancellationToken;

use crate::runner::{self, RunError};

/// Serve a built-in engine protocol, shared by every native surface.
/// Add engines here so re-executing either surface uses the same dispatch.
pub fn serve_builtin(
    engine: &str,
    input: impl std::io::BufRead,
    output: impl std::io::Write,
) -> std::process::ExitCode {
    match engine {
        "pandoc" => pandoc::serve(input, output),
        "pdf" => pdf::serve(input, output),
        "typst" => typst::serve(input, output),
        "libreoffice" => libreoffice::serve(input, output),
        _ => {
            eprintln!("Unknown built-in engine: {engine}");
            std::process::ExitCode::from(2)
        }
    }
}

/// The in-process engine identifier for pure-Rust operations.
pub const IN_PROCESS_ENGINE: &str = "ariad-core";

/// Routes the in-process `ariad-core` engine executes.
pub const NATIVE_ROUTES: &[(Format, Format)] = &[
    (Format::DoclingJson, Format::AriadIrJson),
    (Format::Markdown, Format::AriadIrJson),
    (Format::Html, Format::AriadIrJson),
    (Format::AriadIrJson, Format::Markdown),
    (Format::AriadIrJson, Format::Html),
];

/// Static description of an engine that `ashift __engine <id>` serves out of process.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EngineSpec {
    /// Engine identifier used in capabilities, plans and `__engine <id>`.
    pub id: &'static str,
    /// SPDX licence of the engine or the tool it drives.
    pub license: &'static str,
    /// Supported `(input, output)` routes.
    pub routes: &'static [(Format, Format)],
    /// Human hint printed when the engine's tool is missing or has the wrong version.
    pub install_hint: &'static str,
    /// Supported tool versions, shown when the installed tool is out of range.
    pub supported: Option<&'static str>,
    /// Whether `doctor` fails when the engine is unavailable.
    pub required: bool,
    /// Whether the engine's tool is compiled into ashift rather than installed separately.
    pub builtin: bool,
}

/// Pandoc reads DOCX and EPUB and writes DOCX and EPUB.
pub const PANDOC: EngineSpec = EngineSpec {
    id: "pandoc",
    license: "GPL-2.0-or-later",
    routes: &[
        (Format::AriadIrJson, Format::Docx),
        (Format::AriadIrJson, Format::Epub),
        (Format::Docx, Format::AriadIrJson),
        (Format::Epub, Format::AriadIrJson),
    ],
    install_hint: "install Pandoc 3.12 or newer (<4) or set ASHIFT_PANDOC",
    supported: Some(">=3.12, <4"),
    required: true,
    builtin: false,
};

/// The built-in PDF text-layer reader.
pub const PDF: EngineSpec = EngineSpec {
    id: "pdf",
    license: "Apache-2.0",
    routes: &[(Format::Pdf, Format::AriadIrJson)],
    install_hint: "rebuild ashift with the default features to include the PDF reader",
    supported: None,
    required: true,
    builtin: true,
};

/// The built-in Typst PDF writer.
pub const TYPST: EngineSpec = EngineSpec {
    id: "typst",
    license: "Apache-2.0",
    routes: &[(Format::AriadIrJson, Format::Pdf)],
    install_hint: "rebuild ashift with the default features to include the Typst writer",
    supported: None,
    required: true,
    builtin: true,
};

/// The system LibreOffice, used for legacy Office, ODF and Office-to-PDF routes.
pub const LIBREOFFICE: EngineSpec = EngineSpec {
    id: "libreoffice",
    license: "MPL-2.0",
    routes: &[
        (Format::Doc, Format::Docx),
        (Format::Rtf, Format::Docx),
        (Format::Odt, Format::Docx),
        (Format::Xls, Format::Xlsx),
        (Format::Ods, Format::Xlsx),
        (Format::Ppt, Format::Pptx),
        (Format::Odp, Format::Pptx),
        (Format::Doc, Format::Pdf),
        (Format::Rtf, Format::Pdf),
        (Format::Odt, Format::Pdf),
        (Format::Docx, Format::Pdf),
        (Format::Xls, Format::Pdf),
        (Format::Ods, Format::Pdf),
        (Format::Xlsx, Format::Pdf),
        (Format::Ppt, Format::Pdf),
        (Format::Odp, Format::Pdf),
        (Format::Pptx, Format::Pdf),
    ],
    install_hint: "install LibreOffice 24.2 or newer from libreoffice.org or set ASHIFT_SOFFICE",
    supported: Some(">=24.2"),
    required: false,
    builtin: false,
};

/// Optional layout and OCR extraction through the Docling pack.
pub const DOCLING: EngineSpec = EngineSpec {
    id: docling::ENGINE_ID,
    license: "MIT",
    routes: &[
        (Format::Pdf, Format::DoclingJson),
        (Format::Docx, Format::DoclingJson),
        (Format::Pptx, Format::DoclingJson),
        (Format::Xlsx, Format::DoclingJson),
        (Format::Png, Format::DoclingJson),
        (Format::Jpeg, Format::DoclingJson),
        (Format::Tiff, Format::DoclingJson),
        (Format::Webp, Format::DoclingJson),
    ],
    install_hint: "run 'ashift engines install docling' or set ASHIFT_DOCLING_ENGINE for development",
    supported: None,
    required: false,
    builtin: false,
};

/// Every out-of-process engine, in display order.
pub const REGISTRY: &[EngineSpec] = &[PANDOC, PDF, TYPST, LIBREOFFICE, DOCLING];

/// Looks up an out-of-process engine by identifier.
#[must_use]
pub fn spec(id: &str) -> Option<&'static EngineSpec> {
    REGISTRY.iter().find(|spec| spec.id == id)
}

/// Whether the host can execute the step `from -> to` with `engine`.
#[must_use]
pub fn is_executable(from: Format, to: Format, engine: &str) -> bool {
    if engine == IN_PROCESS_ENGINE {
        NATIVE_ROUTES.contains(&(from, to))
    } else {
        spec(engine).is_some_and(|spec| spec.routes.contains(&(from, to)))
    }
}

/// How the host starts out-of-process engines.
#[derive(Clone, Copy, Debug)]
pub struct EngineInvocation<'a> {
    /// The executable serving engines, normally the running `ashift`.
    pub program: &'a Path,
    /// Arguments replacing `__engine <id>` for every engine; tests point this at a probe.
    pub args_override: Option<&'a [OsString]>,
}

impl<'a> EngineInvocation<'a> {
    /// Starts `program __engine <id>` for each engine.
    #[must_use]
    pub const fn new(program: &'a Path) -> Self {
        Self {
            program,
            args_override: None,
        }
    }

    /// Starts `program <args>` for every engine.
    #[must_use]
    pub const fn with_args(program: &'a Path, args: &'a [OsString]) -> Self {
        Self {
            program,
            args_override: Some(args),
        }
    }

    /// The command shared by describe, one-shot execution and warm sessions.
    #[must_use]
    pub fn command_for(&self, id: &str) -> crate::runner::EngineCommand {
        match self.args_override {
            Some(args) => crate::runner::EngineCommand::from_parts(self.program, args),
            None => crate::runner::EngineCommand::builtin(self.program, id),
        }
    }

    /// Arguments that start the engine `id`.
    #[must_use]
    pub fn args_for(&self, id: &str) -> Vec<OsString> {
        match self.args_override {
            Some(args) => args.to_vec(),
            None => vec![OsString::from("__engine"), OsString::from(id)],
        }
    }
}

/// Availability status for an engine.
#[derive(Clone, Debug, PartialEq)]
pub struct EngineAvailability {
    /// Engine identifier (e.g. `"ariad-core"` or `"pandoc"`).
    pub engine: String,
    /// Whether the engine is available for conversions.
    pub available: bool,
    /// Detailed tool status if available.
    pub tool_status: Option<ToolStatus>,
    /// Engine package version if known.
    pub version: Option<String>,
    /// Fixed launcher diagnostic when the installed engine could not start.
    pub startup_failure: Option<String>,
}

/// Aggregated availability report for all known engines.
#[derive(Clone, Debug, PartialEq)]
pub struct AvailabilityReport {
    /// List of engine statuses.
    pub engines: Vec<EngineAvailability>,
}

impl AvailabilityReport {
    /// Returns the identifiers of all engines that are currently available.
    #[must_use]
    pub fn available_engine_ids(&self) -> Vec<String> {
        self.engines
            .iter()
            .filter(|e| e.available)
            .map(|e| e.engine.clone())
            .collect()
    }

    /// Checks if a specific engine is available.
    #[must_use]
    pub fn is_available(&self, engine: &str) -> bool {
        engine == IN_PROCESS_ENGINE
            || self
                .engines
                .iter()
                .any(|e| e.engine == engine && e.available)
    }

    /// Returns the status entry of an engine.
    #[must_use]
    pub fn get(&self, engine: &str) -> Option<&EngineAvailability> {
        self.engines.iter().find(|e| e.engine == engine)
    }
}

type AvailabilityCacheKey = crate::runner::EngineCommand;
type AvailabilityCache = Mutex<HashMap<AvailabilityCacheKey, EngineAvailability>>;

static AVAILABILITY_CACHE: OnceLock<AvailabilityCache> = OnceLock::new();

fn availability_cache() -> &'static AvailabilityCache {
    AVAILABILITY_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn in_process_availability() -> EngineAvailability {
    EngineAvailability {
        engine: IN_PROCESS_ENGINE.to_owned(),
        available: true,
        tool_status: Some(ToolStatus {
            name: IN_PROCESS_ENGINE.to_owned(),
            version: Some(env!("CARGO_PKG_VERSION").to_owned()),
            status: ToolAvailability::Found,
        }),
        version: Some(env!("CARGO_PKG_VERSION").to_owned()),
        startup_failure: None,
    }
}

/// Reports `ariad-core` plus every registered out-of-process engine.
///
/// An empty `engine_args` starts `engine_program __engine <id>` for each engine; non-empty
/// arguments replace that invocation for every engine (tests use a probe). Results are cached
/// per program and arguments for the life of the process.
pub fn availability(
    engine_program: &Path,
    engine_args: &[OsString],
    cancel: CancellationToken,
) -> Result<AvailabilityReport, RunError> {
    let invocation = if engine_args.is_empty() {
        EngineInvocation::new(engine_program)
    } else {
        EngineInvocation::with_args(engine_program, engine_args)
    };
    let ids: Vec<&str> = REGISTRY.iter().map(|spec| spec.id).collect();
    availability_of(&invocation, &ids, cancel)
}

/// Reports `ariad-core` plus the listed out-of-process engines, querying each with `describe`.
///
/// A missing executable or a failed `describe` marks the engine unavailable. Launcher failures
/// retain their diagnostic in the entry; cancellation and unavailable required isolation
/// return an error.
pub fn availability_of(
    invocation: &EngineInvocation<'_>,
    engines: &[&str],
    cancel: CancellationToken,
) -> Result<AvailabilityReport, RunError> {
    let mut report = AvailabilityReport {
        engines: vec![in_process_availability()],
    };
    for &id in engines {
        if id == IN_PROCESS_ENGINE || report.get(id).is_some() {
            continue;
        }
        report
            .engines
            .push(engine_availability(invocation, id, cancel.clone())?);
    }
    Ok(report)
}

fn engine_availability(
    invocation: &EngineInvocation<'_>,
    id: &str,
    cancel: CancellationToken,
) -> Result<EngineAvailability, RunError> {
    if id == docling::ENGINE_ID {
        return Ok(
            docling::availability(invocation.program, cancel)?.unwrap_or(EngineAvailability {
                engine: id.to_owned(),
                available: false,
                tool_status: None,
                version: None,
                startup_failure: None,
            }),
        );
    }
    let command = invocation.command_for(id);
    let key = command.clone();
    {
        let cache = availability_cache()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(entry) = cache.get(&key) {
            return Ok(entry.clone());
        }
    }

    let entry = match runner::describe_command(&command, runner::DESCRIBE_TIMEOUT, cancel) {
        Ok(Event::Capabilities { version, tool, .. }) => EngineAvailability {
            engine: id.to_owned(),
            available: tool.status == ToolAvailability::Found,
            tool_status: Some(tool),
            version: Some(version),
            startup_failure: None,
        },
        Err(error @ (RunError::Cancelled | RunError::SandboxUnavailable(_))) => return Err(error),
        Err(RunError::SandboxLaunchFailed(message)) => EngineAvailability {
            engine: id.to_owned(),
            available: false,
            tool_status: None,
            version: None,
            startup_failure: Some(message),
        },
        Ok(_) | Err(_) => EngineAvailability {
            engine: id.to_owned(),
            available: false,
            tool_status: None,
            version: None,
            startup_failure: None,
        },
    };
    let mut cache = availability_cache()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    cache.insert(key, entry.clone());
    Ok(entry)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use ariad_core::format::Format;

    use super::{IN_PROCESS_ENGINE, NATIVE_ROUTES, REGISTRY, is_executable, spec};

    #[test]
    fn registry_ids_are_unique_and_routes_are_not_self_loops() {
        let ids: BTreeSet<&str> = REGISTRY.iter().map(|spec| spec.id).collect();
        assert_eq!(ids.len(), REGISTRY.len());
        assert!(!ids.contains(IN_PROCESS_ENGINE));
        for spec in REGISTRY {
            for (from, to) in spec.routes {
                assert_ne!(from, to, "{} declares a self loop", spec.id);
            }
        }
        for (from, to) in NATIVE_ROUTES {
            assert_ne!(from, to);
        }
    }

    #[test]
    fn executable_steps_follow_the_registry() {
        assert!(is_executable(
            Format::Markdown,
            Format::AriadIrJson,
            IN_PROCESS_ENGINE
        ));
        assert!(is_executable(Format::Docx, Format::AriadIrJson, "pandoc"));
        assert!(is_executable(Format::Doc, Format::Docx, "libreoffice"));
        assert!(is_executable(Format::Pdf, Format::AriadIrJson, "pdf"));
        assert!(is_executable(Format::AriadIrJson, Format::Pdf, "typst"));
        assert!(!is_executable(Format::Docx, Format::AriadIrJson, "typst"));
        assert!(!is_executable(Format::Docx, Format::Pdf, "unknown"));
        assert!(spec("libreoffice").is_some_and(|spec| !spec.required));
    }
}

#[cfg(test)]
mod dispatch_tests {
    #[test]
    fn unknown_engine_fails_without_writing_protocol_output() {
        let mut output = Vec::new();
        assert_eq!(
            super::serve_builtin("unknown", std::io::Cursor::new([]), &mut output),
            std::process::ExitCode::from(2),
        );
        assert!(output.is_empty());
    }
}
