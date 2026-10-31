pub mod common;
pub mod docling;
pub mod libreoffice;
pub mod pandoc;
pub mod pdf;
pub mod resolve;
pub mod sandbox;
pub mod typst;

pub use resolve::{ResolveError, resolve_pack, resolve_pack_in, resolve_pack_with_options};

use std::{
    collections::{BTreeMap, HashMap},
    ffi::OsString,
    path::Path,
    sync::{Mutex, OnceLock},
};

use ariad_core::{
    format::Format,
    protocol::{
        Event, PROTOCOL, ToolAvailability, ToolStatus,
        v2::{EventV2, PROTOCOL_V2, RequestV2, ToolCapability},
    },
};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use crate::runner::{self, RunError};

/// Wire protocol version spoken by an out-of-process engine.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum Protocol {
    /// `ariad-engine/1` (`convert` and `describe`).
    #[default]
    V1,
    /// `ariad-engine/2` (`convert`, `run_tool`, and `describe`).
    V2,
}

impl Protocol {
    /// Canonical protocol identifier string (`"ariad-engine/1"` or `"ariad-engine/2"`).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::V1 => PROTOCOL,
            Self::V2 => PROTOCOL_V2,
        }
    }
}

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
        other => {
            if let Some(spec) = pack_spec(other).filter(|spec| spec.protocol == Protocol::V2) {
                return serve_v2_pack_engine(spec, input, output);
            }
            eprintln!("Unknown built-in engine: {engine}");
            std::process::ExitCode::from(2)
        }
    }
}

fn serve_v2_pack_engine(
    spec: &EngineSpec,
    input: impl std::io::BufRead,
    output: impl std::io::Write,
) -> std::process::ExitCode {
    common::serve_v2(input, output, |request, out| match request {
        RequestV2::Describe { protocol, .. } => {
            let tools = tools_for_engine(spec.id);
            let capabilities = EventV2::Capabilities {
                protocol: PROTOCOL_V2.to_owned(),
                engine: spec.id.to_owned(),
                version: env!("CARGO_PKG_VERSION").to_owned(),
                tool: ToolStatus {
                    name: spec.id.to_owned(),
                    version: Some(env!("CARGO_PKG_VERSION").to_owned()),
                    status: ToolAvailability::Found,
                },
                license: spec.license.to_owned(),
                routes: Vec::new(),
                tools,
                enforces_memory_limit: false,
                supports_session: false,
                models: None,
            };
            common::emit_capabilities_v2(out, &protocol, capabilities)
        }
        RequestV2::Convert { .. } => Err(common::EngineFailure::unsupported_route()),
        RequestV2::RunTool {
            protocol,
            tool,
            inputs,
            output,
            params,
            params_file,
            work_dir,
            limits,
            ..
        } => {
            if params_file.is_some() && params.as_object().is_some_and(|map| !map.is_empty()) {
                return Err(common::EngineFailure::invalid_request());
            }
            let (paths, resolved_params) = common::validate_run_tool(
                &protocol,
                &inputs,
                &output,
                &params,
                params_file.as_deref(),
                &work_dir,
                &limits,
            )?;

            if resolved_params.get("probe_action") == Some(&json!("hang")) {
                loop {
                    std::thread::sleep(std::time::Duration::from_secs(60));
                }
            }
            if let Some(megabytes) = resolved_params
                .get("probe_memory_mb")
                .and_then(Value::as_u64)
            {
                let len = usize::try_from(megabytes)
                    .unwrap_or(64)
                    .saturating_mul(1024 * 1024);
                let mut buf = vec![0_u8; len];
                for page in buf.chunks_mut(4096) {
                    page[0] = 1;
                }
                loop {
                    std::hint::black_box(&mut buf);
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
            }
            if resolved_params.get("probe_sandbox") == Some(&json!(true)) {
                let mut metrics = BTreeMap::new();
                let first_input = &paths.inputs[0];
                metrics.insert(
                    "read_input".to_owned(),
                    json!(if std::fs::read(first_input).is_ok() {
                        "allowed"
                    } else {
                        "denied"
                    }),
                );
                let out_probe = paths.output_dir.join("probe-output.bin");
                metrics.insert(
                    "write_output".to_owned(),
                    json!(if std::fs::write(&out_probe, b"ok").is_ok() {
                        "allowed"
                    } else {
                        "denied"
                    }),
                );
                let work_probe = paths.work_dir.join("probe-work.bin");
                metrics.insert(
                    "write_work_dir".to_owned(),
                    json!(if std::fs::write(&work_probe, b"ok").is_ok() {
                        let _ = std::fs::remove_file(&work_probe);
                        "allowed"
                    } else {
                        "denied"
                    }),
                );
                if let Some(input_dir) = first_input.parent() {
                    let tamper_in = input_dir.join("tamper-input.bin");
                    let allowed = std::fs::write(&tamper_in, b"no").is_ok();
                    if allowed {
                        let _ = std::fs::remove_file(&tamper_in);
                    }
                    metrics.insert(
                        "write_input_dir".to_owned(),
                        json!(if allowed { "allowed" } else { "denied" }),
                    );
                }
                if let Some(outside) = resolved_params.get("outside").and_then(Value::as_str) {
                    let allowed = std::fs::write(outside, b"escape").is_ok();
                    if allowed {
                        let _ = std::fs::remove_file(outside);
                    }
                    metrics.insert(
                        "write_outside".to_owned(),
                        json!(if allowed { "allowed" } else { "denied" }),
                    );
                }
                #[cfg(unix)]
                if let Some(pf) = &params_file {
                    use std::os::unix::fs::PermissionsExt;
                    if let Ok(meta) = std::fs::metadata(pf) {
                        metrics.insert(
                            "params_file_mode".to_owned(),
                            json!(meta.permissions().mode() & 0o777),
                        );
                    }
                }
                return common::emit_artifacts_success(
                    out,
                    &[(out_probe.as_path(), "pdf")],
                    Some(metrics),
                );
            }

            for input_path in &paths.inputs {
                if std::fs::metadata(input_path).is_err() {
                    return Err(common::EngineFailure::invalid_request());
                }
            }
            common::emit_v2(
                out,
                EventV2::Progress {
                    stage: "run_tool".to_owned(),
                    done: Some(1),
                    total: Some(1),
                },
            )
            .map_err(|_| common::EngineFailure::io())?;
            if resolved_params.get("emit_warning") == Some(&json!(true)) {
                common::emit_v2(
                    out,
                    EventV2::Warning {
                        code: "tool_notice".to_owned(),
                        message: format!("executed {tool}"),
                    },
                )
                .map_err(|_| common::EngineFailure::io())?;
            }
            let out_format = ariad_core::tools::find(&tool)
                .map_or("pdf", |tool_spec| tool_spec.output_format)
                .to_owned();
            let artifact_count = resolved_params
                .get("artifact_count")
                .and_then(Value::as_u64)
                .unwrap_or(1)
                .clamp(1, 16);
            let mut written_paths = Vec::new();
            for idx in 0..artifact_count {
                let filename = if artifact_count == 1 {
                    format!("output.{out_format}")
                } else {
                    format!("output-{idx}.{out_format}")
                };
                let artifact_path = paths.output_dir.join(filename);
                std::fs::write(&artifact_path, b"%PDF-1.7\n")
                    .map_err(|_| common::EngineFailure::io())?;
                written_paths.push(artifact_path);
            }
            let mut metrics = BTreeMap::new();
            metrics.insert("inputs".to_owned(), json!(inputs.len()));
            metrics.insert("artifacts".to_owned(), json!(artifact_count));
            #[cfg(unix)]
            if let Some(pf) = &params_file {
                use std::os::unix::fs::PermissionsExt;
                if let Ok(meta) = std::fs::metadata(pf) {
                    metrics.insert(
                        "params_file_mode".to_owned(),
                        json!(meta.permissions().mode() & 0o777),
                    );
                }
            }
            if let Some(obj) = resolved_params.as_object() {
                metrics.insert("params_keys".to_owned(), json!(obj.len()));
            }
            let artifact_refs: Vec<(&Path, &str)> = written_paths
                .iter()
                .map(|p| (p.as_path(), out_format.as_str()))
                .collect();
            common::emit_artifacts_success(out, &artifact_refs, Some(metrics))
        }
    })
}

fn tools_for_engine(engine_id: &str) -> Vec<ToolCapability> {
    let mut tools: Vec<ToolCapability> = ariad_core::tools::all()
        .iter()
        .filter(|spec| spec.engine == engine_id)
        .map(|spec| ToolCapability {
            id: spec.id.to_owned(),
            version: spec.version.to_owned(),
        })
        .collect();
    if tools.is_empty() {
        let default_tool = match engine_id {
            "pdfium" => Some(("pdf.render", "1")),
            "qpdf" => Some(("pdf.linearize", "1")),
            "tesseract" => Some(("ocr.recognize", "1")),
            "sign" => Some(("pdf.sign", "1")),
            "verapdf" => Some(("pdf.validate_pdfa", "1")),
            "ai" => Some(("ai.summarize", "1")),
            "inpaint" => Some(("pdf.remove_watermark", "1")),
            _ => None,
        };
        if let Some((id, version)) = default_tool {
            tools.push(ToolCapability {
                id: id.to_owned(),
                version: version.to_owned(),
            });
        }
    }
    tools
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

/// Static description of an engine that `ashift __engine <id>` or an engine pack serves out of process.
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
    /// Wire protocol spoken by the engine (`ariad-engine/1` or `ariad-engine/2`).
    pub protocol: Protocol,
    /// Whether the engine must refuse to launch in `best-effort` mode when OS sandbox
    /// isolation is unavailable.
    pub sandbox_required: bool,
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
    install_hint: "run 'just pandoc' or install Pandoc >=3.12,<4 and set ASHIFT_PANDOC",
    supported: Some(">=3.12, <4"),
    required: true,
    builtin: false,
    protocol: Protocol::V1,
    sandbox_required: false,
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
    protocol: Protocol::V1,
    sandbox_required: false,
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
    protocol: Protocol::V1,
    sandbox_required: false,
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
    protocol: Protocol::V1,
    sandbox_required: false,
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
    protocol: Protocol::V1,
    sandbox_required: false,
};

/// PDFium rendering, text extraction and redaction pack (`ariad-engine/2`).
pub const PDFIUM: EngineSpec = EngineSpec {
    id: "pdfium",
    license: "Apache-2.0",
    routes: &[],
    install_hint: "run 'ashift engines install pdfium' to install the pdfium engine pack",
    supported: None,
    required: false,
    builtin: false,
    protocol: Protocol::V2,
    sandbox_required: true,
};

/// QPDF linearization, encryption and stream repair pack (`ariad-engine/2`).
pub const QPDF: EngineSpec = EngineSpec {
    id: "qpdf",
    license: "Apache-2.0",
    routes: &[],
    install_hint: "run 'ashift engines install qpdf' to install the qpdf engine pack",
    supported: None,
    required: false,
    builtin: false,
    protocol: Protocol::V2,
    sandbox_required: true,
};

/// Tesseract 5 OCR pack (`ariad-engine/2`).
pub const TESSERACT: EngineSpec = EngineSpec {
    id: "tesseract",
    license: "Apache-2.0",
    routes: &[],
    install_hint: "run 'ashift engines install tesseract' to install the tesseract engine pack",
    supported: None,
    required: false,
    builtin: false,
    protocol: Protocol::V2,
    sandbox_required: true,
};

/// PAdES digital signing and verification pack (`ariad-engine/2`).
pub const SIGN: EngineSpec = EngineSpec {
    id: "sign",
    license: "Apache-2.0 OR MIT",
    routes: &[],
    install_hint: "run 'ashift engines install sign' to install the sign engine pack",
    supported: None,
    required: false,
    builtin: false,
    protocol: Protocol::V2,
    sandbox_required: true,
};

/// veraPDF PDF/A validation pack (`ariad-engine/2`).
pub const VERAPDF: EngineSpec = EngineSpec {
    id: "verapdf",
    license: "MPL-2.0",
    routes: &[],
    install_hint: "run 'ashift engines install verapdf' to install the verapdf engine pack",
    supported: None,
    required: false,
    builtin: false,
    protocol: Protocol::V2,
    sandbox_required: true,
};

/// Local AI inference pack (`ariad-engine/2`).
pub const AI: EngineSpec = EngineSpec {
    id: "ai",
    license: "Apache-2.0",
    routes: &[],
    install_hint: "run 'ashift engines install ai' to install the ai engine pack",
    supported: None,
    required: false,
    builtin: false,
    protocol: Protocol::V2,
    sandbox_required: true,
};

/// Image watermark inpainting pack (`ariad-engine/2`).
pub const INPAINT: EngineSpec = EngineSpec {
    id: "inpaint",
    license: "Apache-2.0",
    routes: &[],
    install_hint: "run 'ashift engines install inpaint' to install the inpaint engine pack",
    supported: None,
    required: false,
    builtin: false,
    protocol: Protocol::V2,
    sandbox_required: true,
};

/// Every out-of-process conversion engine, in display order.
pub const REGISTRY: &[EngineSpec] = &[PANDOC, PDF, TYPST, LIBREOFFICE, DOCLING];

/// Every pack-backed out-of-process engine (`pack_name == engine_id`).
pub const PACK_ENGINES: &[EngineSpec] =
    &[DOCLING, PDFIUM, QPDF, TESSERACT, SIGN, VERAPDF, AI, INPAINT];

/// Looks up an out-of-process engine by identifier.
#[must_use]
pub fn spec(id: &str) -> Option<&'static EngineSpec> {
    REGISTRY
        .iter()
        .chain(PACK_ENGINES.iter())
        .find(|spec| spec.id == id)
}

/// Looks up a pack-backed engine specification by identifier (`pack_name == engine_id`).
#[must_use]
pub fn pack_spec(id: &str) -> Option<&'static EngineSpec> {
    PACK_ENGINES.iter().find(|spec| spec.id == id)
}

/// Returns `true` when `id` is a registered pack-backed engine (`pack_name == engine_id`).
#[must_use]
pub fn is_pack_engine(id: &str) -> bool {
    pack_spec(id).is_some()
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
