pub mod isolated;
pub(crate) mod routing;

use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

pub use ariad_core::planner::Profile;
use ariad_core::{
    docling,
    format::Format,
    ir::Document,
    limits::Limits,
    planner::{self, Plan, PlanError, PlanStep},
    protocol::{ErrorCode, Event, Input, Output, PROTOCOL, Request},
    reader::{html as reader_html, markdown as reader_markdown},
    warning::Warning,
    writer::{html as writer_html, markdown as writer_markdown},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::{
    archive::{self, ArchiveError},
    assets::{self, AssetBase},
    engines::{self, EngineInvocation, IN_PROCESS_ENGINE},
    package_meta,
    runner::{self, EngineCommand, RunError},
    session::{Lease, SessionPool},
    workspace::{Workspace, WorkspaceError},
};

/// Where a conversion gets its engine processes from.
#[derive(Clone, Copy)]
enum Engines<'pool> {
    /// A fresh process per engine job.
    OneShot,
    /// Warm sessions from a pool for engines that support them.
    Pool(&'pool SessionPool),
}

impl<'pool> Engines<'pool> {
    fn lease(
        self,
        command: EngineCommand,
        cancel: &CancellationToken,
    ) -> Result<Lease<'pool>, ConvertError> {
        match self {
            Self::OneShot => Ok(Lease::one_shot(command)),
            Self::Pool(pool) => pool.lease(&command, cancel.clone()).map_err(map_run_error),
        }
    }
}

/// A planner route the host can execute: one or more steps from the input to the output format.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Route {
    pub input_format: Format,
    pub output_format: Format,
    pub steps: Vec<PlanStep>,
}

impl Route {
    /// Returns the sequence of planner steps of this route.
    #[must_use]
    pub fn steps(&self) -> Vec<PlanStep> {
        self.steps.clone()
    }
}

impl std::fmt::Display for Route {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} -> {}",
            self.input_format.user_name(),
            self.output_format.user_name()
        )
    }
}

/// Builds an executable route from a plan, or `None` when a step has no executor or the steps do
/// not chain.
#[must_use]
pub fn route_from_plan(plan: &Plan) -> Option<Route> {
    let first = plan.steps.first()?;
    let last = plan.steps.last()?;
    let chained = plan.steps.windows(2).all(|pair| pair[0].to == pair[1].from);
    let executable = plan
        .steps
        .iter()
        .all(|step| engines::is_executable(step.from, step.to, &step.engine));
    (chained && executable).then(|| Route {
        input_format: first.from,
        output_format: last.to,
        steps: plan.steps.clone(),
    })
}

/// Returns the capability edges supported by the host executor.
#[must_use]
pub fn executor_edges() -> Vec<(Format, Format, &'static str)> {
    let native = engines::NATIVE_ROUTES
        .iter()
        .map(|&(from, to)| (from, to, IN_PROCESS_ENGINE));
    let external = engines::REGISTRY
        .iter()
        .flat_map(|spec| spec.routes.iter().map(|&(from, to)| (from, to, spec.id)));
    native.chain(external).collect()
}

/// Detects a user-facing format from a path's extension.
#[must_use]
pub fn detect_format_from_path(path: &Path) -> Option<Format> {
    let ext = path.extension()?.to_str()?;
    Format::from_extension(ext)
}

#[must_use]
pub fn is_supported_route(input: Format, output: Format) -> bool {
    if input == output {
        return false;
    }
    let caps = planner::embedded();
    let engines = caps.registered_engines();
    planner::plan(caps, input, output, Profile::Editable, &engines).is_ok()
}

#[must_use]
pub fn paths_refer_to_same_file(input: &Path, output: &Path) -> bool {
    if let (Ok(in_canon), Ok(out_canon)) = (fs::canonicalize(input), fs::canonicalize(output))
        && in_canon == out_canon
    {
        return true;
    }
    let out_dir = output.parent().unwrap_or_else(|| Path::new("."));
    let in_dir = input.parent().unwrap_or_else(|| Path::new("."));
    if let (Ok(in_dir_canon), Ok(out_dir_canon)) =
        (fs::canonicalize(in_dir), fs::canonicalize(out_dir))
        && in_dir_canon == out_dir_canon
        && let (Some(in_name), Some(out_name)) = (input.file_name(), output.file_name())
        && in_name == out_name
    {
        return true;
    }
    input == output
}

fn stem_without_ir(path: &Path) -> Option<String> {
    let stem = path.file_stem()?.to_str()?;
    Some(stem.strip_suffix(".ir").unwrap_or(stem).to_owned())
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ConvertRequest {
    pub input: PathBuf,
    pub output: PathBuf,
    pub target_format: String,
    pub profile: Profile,
    pub overwrite: bool,
    pub engine_program: PathBuf,
    pub title_fallback: Option<String>,
    pub asset_base_dir: Option<PathBuf>,
    /// Optional engine that must read the input; later mapping and writing remain automatic.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reader_engine: Option<String>,
}

impl ConvertRequest {
    #[must_use]
    pub fn new(
        input: impl Into<PathBuf>,
        output: impl Into<PathBuf>,
        target_format: impl Into<String>,
        engine_program: impl Into<PathBuf>,
    ) -> Self {
        Self {
            input: input.into(),
            output: output.into(),
            target_format: target_format.into(),
            profile: Profile::Editable,
            overwrite: false,
            engine_program: engine_program.into(),
            title_fallback: None,
            asset_base_dir: None,
            reader_engine: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ConvertWarning {
    pub code: String,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ConvertReport {
    pub output: PathBuf,
    pub route: Vec<PlanStep>,
    pub warnings: Vec<ConvertWarning>,
    pub elapsed: Duration,
    pub output_bytes: u64,
    pub inline_markdown: Option<String>,
}

/// Serialized output for `convert --json`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ConvertOutput {
    pub output: String,
    pub route: Vec<PlanStep>,
    pub warnings: Vec<ConvertWarning>,
    pub elapsed_ms: u64,
}

impl From<&ConvertReport> for ConvertOutput {
    fn from(report: &ConvertReport) -> Self {
        Self {
            output: report.output.display().to_string(),
            route: report.route.clone(),
            warnings: report.warnings.clone(),
            elapsed_ms: u64::try_from(report.elapsed.as_millis()).unwrap_or(u64::MAX),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ConvertError {
    UnsupportedRoute {
        detail: Option<String>,
    },
    DestinationSameAsInput,
    InputIo,
    OutputIo,
    LimitExceeded,
    /// The engine process tree exceeded `limits.max_memory_mb`.
    MemoryLimitExceeded {
        limit_mb: u32,
    },
    /// Required sandbox mode could not isolate the engine.
    SandboxUnavailable(String),
    /// Pandoc is missing or has an unsupported version.
    ToolMissing,
    DoclingMissing,
    DoclingToolMissing,
    /// Another engine's tool is missing or has an unsupported version.
    EngineMissing {
        engine: String,
        hint: String,
    },
    DestinationExists,
    Interrupted,
    UnsupportedIrVersion,
    InvalidCapabilities(String),
    Failed,
}

impl std::fmt::Display for ConvertError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedRoute { detail } => match detail {
                Some(info) => write!(f, "unsupported conversion route; {info}"),
                None => write!(f, "unsupported conversion route"),
            },
            Self::DestinationSameAsInput => {
                write!(f, "the output path cannot be the same as the input file")
            }
            Self::InputIo => write!(f, "the input file could not be read"),
            Self::OutputIo => write!(f, "could not write to stdout"),
            Self::LimitExceeded => write!(f, "conversion limit exceeded"),
            Self::MemoryLimitExceeded { limit_mb } => write!(
                f,
                "conversion limit exceeded: the engine used more than max_memory_mb ({limit_mb} MB)"
            ),
            Self::SandboxUnavailable(detail) => {
                write!(f, "the engine sandbox is unavailable: {detail}")
            }
            Self::DoclingMissing => write!(
                f,
                "the docling engine is not installed; run `ashift engines install docling` or set ASHIFT_DOCLING_ENGINE for development"
            ),
            Self::DoclingToolMissing => write!(
                f,
                "the docling engine is missing Tesseract 5 or its models for this document; run `ashift doctor` for details"
            ),
            Self::ToolMissing => write!(
                f,
                "Pandoc is missing or unsupported. Run `just pandoc` or set ASHIFT_PANDOC."
            ),
            Self::EngineMissing { engine, hint } => {
                write!(f, "the {engine} engine is missing or unsupported; {hint}")
            }
            Self::DestinationExists => {
                write!(
                    f,
                    "the destination already exists; pass --overwrite to replace it"
                )
            }
            Self::Interrupted => write!(f, "conversion was interrupted"),
            Self::UnsupportedIrVersion => write!(f, "unsupported IR version"),
            Self::InvalidCapabilities(err) => write!(f, "invalid capabilities: {err}"),
            Self::Failed => write!(f, "conversion failed"),
        }
    }
}

impl std::error::Error for ConvertError {}

impl From<ArchiveError> for ConvertError {
    fn from(err: ArchiveError) -> Self {
        match err {
            ArchiveError::CentralDirectorySizeExceeded { .. }
            | ArchiveError::EntryCountExceeded { .. }
            | ArchiveError::DecompressedSizeExceeded { .. } => Self::LimitExceeded,
            ArchiveError::Encrypted
            | ArchiveError::DuplicateEntryName { .. }
            | ArchiveError::EntryCountMismatch { .. }
            | ArchiveError::InvalidEntryName { .. } => Self::Failed,
            ArchiveError::Io(_) | ArchiveError::Zip(_) => Self::InputIo,
        }
    }
}

impl ConvertError {
    #[must_use]
    pub const fn exit_code(&self) -> u8 {
        match self {
            Self::DestinationSameAsInput => 2,
            Self::UnsupportedRoute { .. } => 3,
            Self::LimitExceeded | Self::MemoryLimitExceeded { .. } => 4,
            Self::ToolMissing
            | Self::DoclingMissing
            | Self::DoclingToolMissing
            | Self::EngineMissing { .. }
            | Self::SandboxUnavailable(_) => 5,
            Self::DestinationExists => 6,
            Self::Interrupted => 130,
            Self::InputIo
            | Self::OutputIo
            | Self::UnsupportedIrVersion
            | Self::InvalidCapabilities(_)
            | Self::Failed => 1,
        }
    }

    /// Short machine-readable error code string.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::DestinationSameAsInput => "destination_same_as_input",
            Self::UnsupportedRoute { .. } => "unsupported_route",
            Self::LimitExceeded | Self::MemoryLimitExceeded { .. } => "limit_exceeded",
            Self::SandboxUnavailable(_) => "sandbox_unavailable",
            Self::ToolMissing
            | Self::DoclingMissing
            | Self::DoclingToolMissing
            | Self::EngineMissing { .. } => "tool_missing",
            Self::DestinationExists => "destination_exists",
            Self::Interrupted => "interrupted",
            Self::InputIo => "input_io",
            Self::OutputIo => "output_io",
            Self::UnsupportedIrVersion => "unsupported_ir_version",
            Self::InvalidCapabilities(_) => "invalid_capabilities",
            Self::Failed => "failed",
        }
    }

    /// The error reported when an engine's tool is unavailable.
    #[must_use]
    pub fn missing_engine(engine: &str) -> Self {
        if engine == engines::docling::ENGINE_ID {
            return Self::DoclingMissing;
        }
        match engines::spec(engine) {
            Some(spec) if spec.id != engines::PANDOC.id => Self::EngineMissing {
                engine: spec.id.to_owned(),
                hint: spec.install_hint.to_owned(),
            },
            _ => Self::ToolMissing,
        }
    }
}

pub fn reachable_targets_detail(from_str: &str, reachable: &[Format]) -> String {
    let mut sorted_targets: Vec<&'static str> = reachable
        .iter()
        .copied()
        .filter(|f| f.is_user_facing())
        .map(Format::user_name)
        .collect();
    sorted_targets.sort_unstable();
    sorted_targets.dedup();
    if sorted_targets.is_empty() {
        format!("no reachable targets from {from_str}")
    } else {
        format!(
            "reachable targets from {from_str}: {}",
            sorted_targets.join(", ")
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConvertEvent {
    Progress { stage: String },
}

/// Converts a supported input document with custom engine args and limits.
///
/// Non-empty `engine_args` replace `__engine <id>` for every out-of-process engine.
#[doc(hidden)]
#[allow(clippy::too_many_arguments)]
pub fn convert_custom<F>(
    input: &Path,
    output: &Path,
    target_format: &str,
    engine_program: &Path,
    engine_args: &[OsString],
    overwrite: bool,
    limits: Limits,
    cancel: CancellationToken,
    on_event: F,
) -> Result<ConvertReport, ConvertError>
where
    F: FnMut(ConvertEvent),
{
    let request = ConvertRequest {
        input: input.to_path_buf(),
        output: output.to_path_buf(),
        target_format: target_format.to_owned(),
        profile: Profile::Editable,
        overwrite,
        engine_program: engine_program.to_path_buf(),
        title_fallback: None,
        asset_base_dir: None,
        reader_engine: None,
    };
    let invocation = if engine_args.is_empty() {
        EngineInvocation::new(engine_program)
    } else {
        EngineInvocation::with_args(engine_program, engine_args)
    };
    convert_executor(
        &request,
        None,
        &invocation,
        None,
        limits,
        Engines::OneShot,
        cancel,
        on_event,
    )
}

/// Converts a supported document into the requested target format through the route executor.
pub fn convert<F>(
    request: &ConvertRequest,
    cancel: CancellationToken,
    on_event: F,
) -> Result<ConvertReport, ConvertError>
where
    F: FnMut(ConvertEvent),
{
    let invocation = EngineInvocation::new(&request.engine_program);
    convert_executor(
        request,
        None,
        &invocation,
        None,
        Limits::local(),
        Engines::OneShot,
        cancel,
        on_event,
    )
}

/// Converts like [`convert`], taking engine processes from `pool` so engines that support
/// sessions stay warm across calls.
pub fn convert_in<F>(
    pool: &SessionPool,
    request: &ConvertRequest,
    cancel: CancellationToken,
    on_event: F,
) -> Result<ConvertReport, ConvertError>
where
    F: FnMut(ConvertEvent),
{
    let invocation = EngineInvocation::new(&request.engine_program);
    convert_executor(
        request,
        None,
        &invocation,
        None,
        Limits::local(),
        Engines::Pool(pool),
        cancel,
        on_event,
    )
}

/// Converts under the cloud job's explicit resource limits.
pub fn convert_with_limits<F>(
    request: &ConvertRequest,
    limits: Limits,
    cancel: CancellationToken,
    on_event: F,
) -> Result<ConvertReport, ConvertError>
where
    F: FnMut(ConvertEvent),
{
    if limits.validate().is_err() {
        return Err(ConvertError::LimitExceeded);
    }
    convert_executor(
        request,
        None,
        &EngineInvocation::new(&request.engine_program),
        None,
        limits,
        Engines::OneShot,
        cancel,
        on_event,
    )
}

/// Converts a supported document into the requested target format using capability-based promotion.
pub fn convert_confined<F>(
    request: &ConvertRequest,
    destination_parent: &cap_std::fs::Dir,
    file_name: &Path,
    cancel: CancellationToken,
    on_event: F,
) -> Result<ConvertReport, ConvertError>
where
    F: FnMut(ConvertEvent),
{
    let invocation = EngineInvocation::new(&request.engine_program);
    convert_executor(
        request,
        Some(Confinement {
            destination_parent,
            file_name,
            asset_base: None,
        }),
        &invocation,
        None,
        Limits::local(),
        Engines::OneShot,
        cancel,
        on_event,
    )
}

/// Converts like [`convert_confined`], taking engine processes from `pool`.
///
/// Images referenced by a Markdown or HTML input resolve beneath `asset_base`, never through
/// `request.asset_base_dir` or the input's path, so they stay within the authorized directory.
pub fn convert_confined_in<F>(
    pool: &SessionPool,
    request: &ConvertRequest,
    destination_parent: &cap_std::fs::Dir,
    file_name: &Path,
    asset_base: &AssetBase,
    cancel: CancellationToken,
    on_event: F,
) -> Result<ConvertReport, ConvertError>
where
    F: FnMut(ConvertEvent),
{
    let invocation = EngineInvocation::new(&request.engine_program);
    convert_executor(
        request,
        Some(Confinement {
            destination_parent,
            file_name,
            asset_base: Some(asset_base),
        }),
        &invocation,
        None,
        Limits::local(),
        Engines::Pool(pool),
        cancel,
        on_event,
    )
}

pub use ariad_core::inspect::ANALYSIS_MAX_BYTES;

/// Maximum input bytes for parsing document body in plan summary (4 MiB).
pub const PLAN_SUMMARY_MAX_BYTES: u64 = 4 * 1024 * 1024;

pub const MAX_ZIP_ENTRIES: usize = 20_000;
pub const MAX_ZIP_ENTRY_NAME_BYTES: usize = 2048;

/// Classification of an input file's format.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileClassification {
    /// Detected format, or `None` if unknown.
    pub format: Option<Format>,
    /// Whether the file extension conflicts with the detected content format.
    pub extension_mismatch: bool,
    /// Total file size in bytes.
    pub bytes: u64,
}

/// Classifies a file's format from its content prefix, ZIP central directory, and extension.
pub fn classify_file(path: &Path) -> Result<FileClassification, std::io::Error> {
    let mut file = open_regular_input(path)
        .map_err(|_| std::io::Error::other("could not read regular input file"))?;
    let metadata = file.metadata()?;
    let bytes = metadata.len();

    let mut prefix = [0u8; 1024];
    let n = file.read(&mut prefix)?;
    let prefix = &prefix[..n];

    let ext = path.extension().and_then(|e| e.to_str());

    let container_entries: Option<Vec<String>> = if prefix
        .starts_with(ariad_core::format::ZIP_MAGIC)
    {
        crate::archive::read_zip_entry_names(&mut file, MAX_ZIP_ENTRIES, MAX_ZIP_ENTRY_NAME_BYTES)
    } else if prefix.starts_with(ariad_core::format::OLE_MAGIC) {
        crate::archive::read_cfb_entry_names(&mut file)
    } else {
        None
    };

    let entry_refs: Option<Vec<&str>> = container_entries
        .as_ref()
        .map(|entries| entries.iter().map(|s| s.as_str()).collect());

    let content_fmt = ariad_core::format::classify(prefix, entry_refs.as_deref(), None);
    let final_fmt = ariad_core::format::classify(prefix, entry_refs.as_deref(), ext);

    let ext_fmt = ext.and_then(Format::from_extension);
    let extension_mismatch = match (content_fmt, ext) {
        (Some(cf), Some(_)) => ext_fmt != Some(cf),
        _ => false,
    };

    Ok(FileClassification {
        format: final_fmt,
        extension_mismatch,
        bytes,
    })
}

/// Opens a regular input file for reading, rejecting directories, FIFOs,
/// sockets, and device special files before open and via fstat to prevent hangs.
pub fn open_regular_input(path: &Path) -> Result<fs::File, ConvertError> {
    let pre_meta = fs::metadata(path).map_err(|_| ConvertError::InputIo)?;
    if !pre_meta.file_type().is_file() {
        return Err(ConvertError::InputIo);
    }

    #[cfg(unix)]
    let file = {
        use std::os::unix::fs::OpenOptionsExt;
        let mut opts = fs::OpenOptions::new();
        opts.read(true);
        opts.custom_flags(libc::O_NONBLOCK);
        opts.open(path).map_err(|_| ConvertError::InputIo)?
    };

    #[cfg(not(unix))]
    let file = fs::File::open(path).map_err(|_| ConvertError::InputIo)?;

    let post_meta = file.metadata().map_err(|_| ConvertError::InputIo)?;
    if !post_meta.file_type().is_file() {
        return Err(ConvertError::InputIo);
    }

    Ok(file)
}

/// Reads a regular file up to `max_bytes`.
pub fn read_bounded_file(path: &Path, max_bytes: Option<u64>) -> Result<Vec<u8>, ConvertError> {
    let mut file = open_regular_input(path)?;
    if let Ok(metadata) = file.metadata()
        && max_bytes.is_some_and(|max| metadata.len() > max)
    {
        return Err(ConvertError::LimitExceeded);
    }
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buffer).map_err(|_| ConvertError::InputIo)?;
        if n == 0 {
            break;
        }
        if max_bytes.is_some_and(|max| (bytes.len() as u64).saturating_add(n as u64) > max) {
            return Err(ConvertError::LimitExceeded);
        }
        bytes.extend_from_slice(&buffer[..n]);
    }
    Ok(bytes)
}

/// Validates that a reader streams valid UTF-8 text using constant memory
/// (streaming in 64 KiB chunks) and carrying incomplete multibyte sequences across
/// chunk boundaries.
pub fn validate_utf8_reader_streaming<R: Read>(mut reader: R) -> Result<(), ConvertError> {
    const CHUNK_SIZE: usize = 64 * 1024;
    let mut buffer = [0u8; 4 + CHUNK_SIZE];
    let mut carry_len = 0usize;

    loop {
        let n = reader
            .read(&mut buffer[carry_len..carry_len + CHUNK_SIZE])
            .map_err(|_| ConvertError::InputIo)?;
        if n == 0 {
            if carry_len > 0 {
                return Err(ConvertError::InputIo);
            }
            return Ok(());
        }

        let total_len = carry_len + n;
        let slice = &buffer[..total_len];
        match std::str::from_utf8(slice) {
            Ok(_) => {
                carry_len = 0;
            }
            Err(e) => {
                let valid_up_to = e.valid_up_to();
                match e.error_len() {
                    Some(_) => return Err(ConvertError::InputIo),
                    None => {
                        let remaining = total_len - valid_up_to;
                        buffer.copy_within(valid_up_to..total_len, 0);
                        carry_len = remaining;
                    }
                }
            }
        }
    }
}

/// Validates that a regular file contains valid UTF-8 text using constant memory
/// (streaming in 64 KiB chunks) and carrying incomplete multibyte sequences across
/// chunk boundaries.
pub fn validate_utf8_file_streaming(path: &Path) -> Result<(), ConvertError> {
    let file = open_regular_input(path)?;
    validate_utf8_reader_streaming(file)
}

/// Result of reading a supported in-process document format to Ariad IR.
#[derive(Clone, Debug, PartialEq)]
pub struct ReadDocumentResult {
    pub document: ariad_core::ir::Document,
    pub warnings: Vec<ConvertWarning>,
}

/// Reads a supported document format (Markdown or HTML) to Ariad IR in-process
/// using strict bounds and UTF-8 verification.
pub fn read_document_to_ir(
    path: &Path,
    format: Format,
    limits: &Limits,
) -> Result<ReadDocumentResult, ConvertError> {
    let mut warnings = Vec::new();
    let document = match format {
        Format::Markdown => {
            let bytes = read_bounded_file(path, limits.max_input_bytes)?;
            let text = String::from_utf8(bytes).map_err(|_| ConvertError::InputIo)?;
            let parsed = reader_markdown::read(&text, limits).map_err(|err| match err {
                reader_markdown::ReadError::NestingTooDeep { .. }
                | reader_markdown::ReadError::TooManyBlocks { .. }
                | reader_markdown::ReadError::InputTooLarge { .. }
                | reader_markdown::ReadError::InvalidLimits(_) => ConvertError::LimitExceeded,
            })?;
            for w in &parsed.warnings {
                warnings.push(safe_warning(w));
            }
            parsed.document
        }
        Format::Html => {
            let bytes = read_bounded_file(path, limits.max_input_bytes)?;
            let parsed = reader_html::read(&bytes, limits).map_err(|err| match err {
                reader_html::ReadError::NestingTooDeep { .. }
                | reader_html::ReadError::TooManyBlocks { .. }
                | reader_html::ReadError::InputTooLarge { .. }
                | reader_html::ReadError::TooManyNodes { .. }
                | reader_html::ReadError::InvalidLimits(_) => ConvertError::LimitExceeded,
            })?;
            for w in &parsed.warnings {
                warnings.push(safe_warning(w));
            }
            parsed.document
        }
        _ => {
            return Err(ConvertError::UnsupportedRoute {
                detail: Some(format!(
                    "cannot read {} directly to IR in-process",
                    format.user_name()
                )),
            });
        }
    };
    Ok(ReadDocumentResult { document, warnings })
}

/// Sanitizes an identifier (e.g. extension or format name) by stripping control characters and bounding length.
pub fn sanitize_identifier(s: &str, max_len: usize) -> String {
    s.chars()
        .filter(|c| !c.is_control())
        .take(max_len)
        .collect()
}

/// Where the document lives between two route steps.
enum Stage {
    /// The user's input file, not yet copied anywhere.
    Source,
    /// A file an earlier step produced, kept alive with the workspace that holds it.
    File {
        workspace: Box<Workspace>,
        path: PathBuf,
    },
    /// An IR document in memory.
    Ir(Box<Document>),
}

/// Capability handles that tie a confined conversion's reads and writes to directories that
/// were authorized before it started.
#[derive(Clone, Copy)]
struct Confinement<'a> {
    destination_parent: &'a cap_std::fs::Dir,
    file_name: &'a Path,
    /// Base for images referenced by the source document; `None` resolves them by path.
    asset_base: Option<&'a AssetBase>,
}

/// State shared by the steps of one route execution.
struct RouteRun<'a, F> {
    request: &'a ConvertRequest,
    asset_base: Option<&'a AssetBase>,
    invocation: &'a EngineInvocation<'a>,
    engines: Engines<'a>,
    limits: &'a Limits,
    cancel: &'a CancellationToken,
    warnings: Vec<ConvertWarning>,
    on_event: F,
}

impl<F> RouteRun<'_, F>
where
    F: FnMut(ConvertEvent),
{
    fn title_fallback(&self) -> String {
        self.request
            .title_fallback
            .clone()
            .or_else(|| stem_without_ir(&self.request.input))
            .unwrap_or_else(|| "Untitled".to_owned())
    }

    fn step(&mut self, stage: Stage, step: &PlanStep) -> Result<Stage, ConvertError> {
        if self.cancel.is_cancelled() {
            return Err(ConvertError::Interrupted);
        }
        let stage = if step.from == Format::AriadIrJson {
            self.load_ir(stage)?
        } else {
            stage
        };
        if step.engine == IN_PROCESS_ENGINE {
            self.native_step(stage, step)
        } else {
            self.engine_step(stage, step)
        }
    }

    fn load_ir(&self, stage: Stage) -> Result<Stage, ConvertError> {
        let path = match &stage {
            Stage::Ir(_) => return Ok(stage),
            Stage::Source => &self.request.input,
            Stage::File { path, .. } => path,
        };
        let bytes = read_bounded_file(path, Some(self.limits.max_ir_json_bytes))?;
        let document =
            crate::ir_io::read_versioned(&bytes, self.limits).map_err(|err| match err {
                crate::ir_io::ReadError::ByteLimitExceeded { .. }
                | crate::ir_io::ReadError::DepthExceeded(_)
                | crate::ir_io::ReadError::BlockLimitExceeded { .. } => ConvertError::LimitExceeded,
                crate::ir_io::ReadError::UnsupportedVersion => ConvertError::UnsupportedIrVersion,
                crate::ir_io::ReadError::Io(_) => ConvertError::InputIo,
                crate::ir_io::ReadError::Json(_) => ConvertError::Failed,
            })?;
        Ok(Stage::Ir(Box::new(document)))
    }

    fn native_step(&mut self, stage: Stage, step: &PlanStep) -> Result<Stage, ConvertError> {
        match (step.from, step.to) {
            (Format::DoclingJson, Format::AriadIrJson) => {
                let read = match &stage {
                    Stage::Source => read_docling_to_ir(&self.request.input, self.limits)?,
                    Stage::File { path, .. } => {
                        read_docling_with_cap(path, self.limits, self.limits.max_ir_json_bytes)?
                    }
                    Stage::Ir(_) => return Err(ConvertError::Failed),
                };
                self.warnings.extend(read.warnings);
                Ok(Stage::Ir(Box::new(read.document)))
            }

            (Format::Markdown | Format::Html, Format::AriadIrJson) => {
                let (path, base_dir) = match &stage {
                    Stage::Source => (
                        self.request.input.as_path(),
                        self.request
                            .asset_base_dir
                            .as_deref()
                            .or_else(|| self.request.input.parent())
                            .unwrap_or_else(|| Path::new(".")),
                    ),
                    Stage::File { path, .. } => (
                        path.as_path(),
                        path.parent().unwrap_or_else(|| Path::new(".")),
                    ),
                    Stage::Ir(_) => return Err(ConvertError::Failed),
                };
                let confined_base = if matches!(stage, Stage::Source) {
                    self.asset_base
                } else {
                    None
                };
                let read = read_document_to_ir(path, step.from, self.limits)?;
                self.warnings.extend(read.warnings);
                let mut document = read.document;
                if self.cancel.is_cancelled() {
                    return Err(ConvertError::Interrupted);
                }
                // A confined source resolves images only through its authorized handle.
                let asset_warnings = match confined_base {
                    Some(base) => assets::resolve_in(&mut document, base, self.limits),
                    None => assets::resolve(&mut document, base_dir, self.limits)
                        .map_err(|_| ConvertError::InputIo)?,
                };
                self.warnings
                    .extend(asset_warnings.iter().map(safe_warning));
                Ok(Stage::Ir(Box::new(document)))
            }
            (Format::AriadIrJson, Format::Markdown | Format::Html) => {
                let Stage::Ir(document) = stage else {
                    return Err(ConvertError::Failed);
                };
                let output = if step.to == Format::Markdown {
                    writer_markdown::write(&document)
                } else {
                    writer_html::write_with_title_fallback(
                        &document,
                        Some(self.title_fallback().as_str()),
                    )
                };
                self.warnings
                    .extend(output.warnings.iter().map(safe_warning));
                let workspace = Workspace::new().map_err(|_| ConvertError::Failed)?;
                let path = workspace
                    .output_dir()
                    .join(format!("document.{}", step.to.user_name()));
                fs::write(&path, output.content.as_bytes()).map_err(|_| ConvertError::Failed)?;
                Ok(Stage::File {
                    workspace: Box::new(workspace),
                    path,
                })
            }
            _ => Err(ConvertError::UnsupportedRoute {
                detail: Some(format!(
                    "no reachable targets from {}",
                    step.from.user_name()
                )),
            }),
        }
    }

    fn engine_step(&mut self, stage: Stage, step: &PlanStep) -> Result<Stage, ConvertError> {
        let docling = if step.engine == engines::docling::ENGINE_ID {
            Some(engines::docling::resolve().ok_or(ConvertError::DoclingMissing)?)
        } else {
            None
        };
        let command = docling.as_ref().map_or_else(
            || self.invocation.command_for(&step.engine),
            |command| command.runner_command(self.invocation.program),
        );
        let mut lease = self.engines.lease(command, self.cancel)?;
        let mut workspace = lease.workspace().map_err(|_| ConvertError::Failed)?;
        let input_path = match stage {
            Stage::Ir(document) => write_ir_input(&document, &workspace)?,
            Stage::Source => copy_and_preflight_input(
                &self.request.input,
                step.from.id(),
                &workspace,
                self.limits,
            )?,
            Stage::File {
                path,
                workspace: previous,
            } => {
                let copied =
                    copy_and_preflight_input(&path, step.from.id(), &workspace, self.limits)?;
                drop(previous);
                copied
            }
        };

        let mut options = docling.map(|command| command.options).unwrap_or_default();
        options.insert(
            "profile".to_owned(),
            serde_json::Value::String(self.request.profile.as_str().to_owned()),
        );
        if step.to != Format::AriadIrJson {
            options.insert(
                "title_fallback".to_owned(),
                serde_json::Value::String(self.title_fallback()),
            );
        }
        // Writers stamp their output with the conversion time, which honours SOURCE_DATE_EPOCH.
        let stamps_package =
            step.engine == engines::PANDOC.id && matches!(step.to, Format::Docx | Format::Epub);
        let timestamp = if step.to == Format::AriadIrJson {
            None
        } else {
            Some(package_meta::conversion_time().map_err(|_| ConvertError::Failed)?)
        };
        if let Some(timestamp) = timestamp {
            options.insert(
                "timestamp".to_owned(),
                serde_json::Value::from(timestamp.as_second()),
            );
        }

        let artifact = run_engine(
            &mut lease,
            &step.engine,
            step.to,
            EngineInput {
                path: &input_path,
                format: step.from,
            },
            &workspace,
            self.limits,
            options,
            self.cancel,
            &mut self.warnings,
            &mut self.on_event,
        )?;

        if step.to == Format::AriadIrJson {
            let document = read_ir_artifact(&artifact, &workspace, self.limits)?;
            drop(lease);
            let _ = workspace.close();
            return Ok(Stage::Ir(Box::new(document)));
        }

        // A warm engine can still mutate its output. Read through the captured directory
        // handle into a host-only workspace before stamping or using the next route step.
        let snapshot = Workspace::new().map_err(map_workspace_error)?;
        let path = snapshot
            .output_dir()
            .join(format!("document.{}", step.to.user_name()));
        let source = workspace
            .open_artifact(&artifact)
            .map_err(map_workspace_error)?;
        let length = source.metadata().map_err(|_| ConvertError::Failed)?.len();
        if step.to == Format::DoclingJson && length > self.limits.max_ir_json_bytes {
            return Err(ConvertError::LimitExceeded);
        }
        let mut destination = fs::File::create(&path).map_err(|_| ConvertError::Failed)?;
        std::io::copy(&mut source.take(length), &mut destination)
            .map_err(|_| ConvertError::Failed)?;
        drop(destination);
        drop(lease);
        let _ = workspace.close();
        if let (true, Some(timestamp)) = (stamps_package, timestamp) {
            let stamped = if step.to == Format::Docx {
                package_meta::stamp_docx(&path, timestamp)
            } else {
                package_meta::stamp_epub(&path, timestamp)
            };
            stamped.map_err(|_| ConvertError::Failed)?;
        }
        Ok(Stage::File {
            workspace: Box::new(snapshot),
            path,
        })
    }
}

/// Reads bounded DoclingDocument JSON without starting an external engine.
pub fn read_docling_to_ir(
    input: &Path,
    limits: &Limits,
) -> Result<ReadDocumentResult, ConvertError> {
    let cap = limits
        .max_input_bytes
        .map_or(limits.max_ir_json_bytes, |max| {
            max.min(limits.max_ir_json_bytes)
        });
    read_docling_with_cap(input, limits, cap)
}

fn read_docling_with_cap(
    input: &Path,
    limits: &Limits,
    cap: u64,
) -> Result<ReadDocumentResult, ConvertError> {
    let bytes = read_bounded_file(input, Some(cap))?;
    let mut warnings = Vec::new();
    let output = docling::read(&bytes, limits).map_err(|error| match error {
        docling::ReadError::InvalidLimits(_)
        | docling::ReadError::InputTooLarge { .. }
        | docling::ReadError::JsonTooDeep(_)
        | docling::ReadError::NestingTooDeep { .. }
        | docling::ReadError::TooManyBlocks { .. }
        | docling::ReadError::ContentTooLarge { .. } => ConvertError::LimitExceeded,
        docling::ReadError::InvalidJson { .. }
        | docling::ReadError::NotDoclingDocument
        | docling::ReadError::UnsupportedVersion => ConvertError::Failed,
    })?;
    warnings.extend(output.warnings.iter().map(safe_warning));
    Ok(ReadDocumentResult {
        document: output.document,
        warnings,
    })
}

fn write_ir_input(document: &Document, workspace: &Workspace) -> Result<PathBuf, ConvertError> {
    let path = workspace.input_dir().join("document.ir.json");
    let file = fs::File::create(&path).map_err(|_| ConvertError::Failed)?;
    let mut writer = std::io::BufWriter::new(file);
    serde_json::to_writer(&mut writer, document).map_err(|_| ConvertError::Failed)?;
    writer.flush().map_err(|_| ConvertError::Failed)?;
    Ok(path)
}

fn read_ir_artifact(
    path: &Path,
    workspace: &Workspace,
    limits: &Limits,
) -> Result<Document, ConvertError> {
    let file = workspace.open_artifact(path).map_err(map_workspace_error)?;
    crate::ir_io::read(std::io::BufReader::new(file), limits).map_err(|err| match err {
        crate::ir_io::ReadError::ByteLimitExceeded { .. }
        | crate::ir_io::ReadError::DepthExceeded(_)
        | crate::ir_io::ReadError::BlockLimitExceeded { .. } => ConvertError::LimitExceeded,
        _ => ConvertError::Failed,
    })
}

/// The file an engine step reads, already inside its workspace `in/`.
struct EngineInput<'a> {
    path: &'a Path,
    format: Format,
}

/// Runs one out-of-process engine step and returns the path of its `output` artifact.
#[allow(clippy::too_many_arguments)]
fn run_engine<F>(
    lease: &mut Lease<'_>,
    engine: &str,
    output: Format,
    input: EngineInput<'_>,
    workspace: &Workspace,
    limits: &Limits,
    options: BTreeMap<String, serde_json::Value>,
    cancel: &CancellationToken,
    warnings: &mut Vec<ConvertWarning>,
    on_event: &mut F,
) -> Result<PathBuf, ConvertError>
where
    F: FnMut(ConvertEvent),
{
    let request = Request::Convert {
        protocol: PROTOCOL.to_owned(),
        job: Uuid::new_v4().simple().to_string(),
        input: Input {
            path: input.path.to_string_lossy().into_owned(),
            format: input.format.id().to_owned(),
        },
        output: Output {
            dir: workspace.output_dir().to_string_lossy().into_owned(),
            format: output.id().to_owned(),
        },
        work_dir: workspace.work_dir().to_string_lossy().into_owned(),
        options,
        limits: limits.clone(),
        session_id: None,
    };
    let outcome = lease
        .run(&request, cancel.clone(), |event| match event {
            Event::Progress { stage, .. } => on_event(ConvertEvent::Progress { stage }),
            Event::Warning { code, message } => warnings.push(sanitize_warning(&code, &message)),
            Event::Artifact { .. } | Event::Result { .. } | Event::Capabilities { .. } => {}
        })
        .map_err(|error| map_engine_error(engine, error))?;
    push_sandbox_warning(warnings, &outcome);
    if cancel.is_cancelled() {
        return Err(ConvertError::Interrupted);
    }
    outcome
        .artifacts
        .iter()
        .find(|artifact| artifact.format == output.id())
        .map(|artifact| artifact.path.clone())
        .ok_or(ConvertError::Failed)
}

/// Converts a supported input document through an explicit route.
#[allow(clippy::too_many_arguments)]
fn convert_for_route<F>(
    request: &ConvertRequest,
    confinement: Option<Confinement<'_>>,
    route: &Route,
    pdf_read: Option<ArchiveToIrOutput>,
    invocation: &EngineInvocation<'_>,
    limits: Limits,
    engines: Engines<'_>,
    cancel: CancellationToken,
    on_event: F,
) -> Result<ConvertReport, ConvertError>
where
    F: FnMut(ConvertEvent),
{
    if cancel.is_cancelled() {
        return Err(ConvertError::Interrupted);
    }

    if route.input_format == route.output_format || route.steps.is_empty() {
        return Err(ConvertError::UnsupportedRoute {
            detail: Some(format!(
                "no reachable targets from {}",
                route.input_format.user_name()
            )),
        });
    }

    if paths_refer_to_same_file(&request.input, &request.output) {
        return Err(ConvertError::DestinationSameAsInput);
    }

    let started = Instant::now();
    let (mut stage, start_step, warnings) = match pdf_read {
        Some(read)
            if route.steps.first().is_some_and(|step| {
                step.engine == engines::PDF.id && step.to == Format::AriadIrJson
            }) =>
        {
            (Stage::Ir(Box::new(read.document)), 1, read.warnings)
        }
        _ => (Stage::Source, 0, Vec::new()),
    };
    let mut run = RouteRun {
        request,
        asset_base: confinement.and_then(|confinement| confinement.asset_base),
        invocation,
        engines,
        limits: &limits,
        cancel: &cancel,
        warnings,
        on_event,
    };
    for step in route.steps.iter().skip(start_step) {
        stage = run.step(stage, step)?;
    }
    let mut warnings = run.warnings;

    if cancel.is_cancelled() {
        return Err(ConvertError::Interrupted);
    }
    let (mut workspace, artifact_path) = match stage {
        Stage::File { workspace, path } => (*workspace, path),
        Stage::Ir(document) => {
            let workspace = Workspace::new().map_err(|_| ConvertError::Failed)?;
            let path = workspace.output_dir().join("document.ir.json");
            let file = fs::File::create(&path).map_err(|_| ConvertError::Failed)?;
            let mut writer = std::io::BufWriter::new(file);
            serde_json::to_writer_pretty(&mut writer, &document)
                .map_err(|_| ConvertError::Failed)?;
            writer.flush().map_err(|_| ConvertError::Failed)?;
            (workspace, path)
        }
        Stage::Source => return Err(ConvertError::Failed),
    };

    let output_bytes = fs::metadata(&artifact_path).map(|m| m.len()).unwrap_or(0);
    let inline_markdown = if route.output_format == Format::Markdown && output_bytes <= 256 * 1024 {
        fs::read_to_string(&artifact_path).ok()
    } else {
        None
    };

    // Promotion is the commit point.
    if cancel.is_cancelled() {
        return Err(ConvertError::Interrupted);
    }
    let promoted = match confinement {
        Some(confinement) => workspace.promote_into(
            &artifact_path,
            confinement.destination_parent,
            confinement.file_name,
            request.overwrite,
        ),
        None => workspace.promote(&artifact_path, &request.output, request.overwrite),
    };
    promoted.map_err(map_workspace_error)?;

    if workspace.close().is_err() {
        warnings.push(ConvertWarning {
            code: "workspace_cleanup".to_owned(),
            message: "Temporary workspace cleanup failed.".to_owned(),
        });
    }

    Ok(ConvertReport {
        output: request.output.clone(),
        route: route.steps(),
        warnings,
        elapsed: started.elapsed(),
        output_bytes,
        inline_markdown,
    })
}

/// Request for converting an input document to AriadShift IR.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConvertToIrRequest {
    pub input: PathBuf,
    pub output: PathBuf,
    pub overwrite: bool,
    pub engine_program: PathBuf,
}

impl ConvertToIrRequest {
    #[must_use]
    pub fn new(
        input: impl Into<PathBuf>,
        output: impl Into<PathBuf>,
        engine_program: impl Into<PathBuf>,
    ) -> Self {
        Self {
            input: input.into(),
            output: output.into(),
            overwrite: false,
            engine_program: engine_program.into(),
        }
    }
}

/// Request for writing an AriadShift IR document to a target format.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WriteFromIrRequest {
    pub input: PathBuf,
    pub output: PathBuf,
    pub target_format: String,
    pub profile: Profile,
    pub overwrite: bool,
    pub engine_program: PathBuf,
    pub title_fallback: Option<String>,
}

impl WriteFromIrRequest {
    #[must_use]
    pub fn new(
        input: impl Into<PathBuf>,
        output: impl Into<PathBuf>,
        target_format: impl Into<String>,
        engine_program: impl Into<PathBuf>,
    ) -> Self {
        Self {
            input: input.into(),
            output: output.into(),
            target_format: target_format.into(),
            profile: Profile::Editable,
            overwrite: false,
            engine_program: engine_program.into(),
            title_fallback: None,
        }
    }
}

/// Converts a document to AriadShift IR JSON.
pub fn convert_to_ir<F>(
    request: &ConvertToIrRequest,
    cancel: CancellationToken,
    on_event: F,
) -> Result<ConvertReport, ConvertError>
where
    F: FnMut(ConvertEvent),
{
    let convert_request = ConvertRequest {
        input: request.input.clone(),
        output: request.output.clone(),
        target_format: Format::AriadIrJson.id().to_owned(),
        profile: Profile::Editable,
        overwrite: request.overwrite,
        engine_program: request.engine_program.clone(),
        title_fallback: None,
        asset_base_dir: None,
        reader_engine: None,
    };
    let invocation = EngineInvocation::new(&request.engine_program);
    convert_executor(
        &convert_request,
        None,
        &invocation,
        None,
        Limits::local(),
        Engines::OneShot,
        cancel,
        on_event,
    )
}

/// Writes an AriadShift IR document to a target format.
///
/// The input is read as IR JSON whatever its extension.
pub fn write_from_ir<F>(
    request: &WriteFromIrRequest,
    cancel: CancellationToken,
    on_event: F,
) -> Result<ConvertReport, ConvertError>
where
    F: FnMut(ConvertEvent),
{
    let convert_request = ConvertRequest {
        input: request.input.clone(),
        output: request.output.clone(),
        target_format: request.target_format.clone(),
        profile: request.profile,
        overwrite: request.overwrite,
        engine_program: request.engine_program.clone(),
        title_fallback: request
            .title_fallback
            .clone()
            .or_else(|| stem_without_ir(&request.input)),
        asset_base_dir: None,
        reader_engine: None,
    };
    let invocation = EngineInvocation::new(&request.engine_program);
    convert_executor(
        &convert_request,
        None,
        &invocation,
        Some(Format::AriadIrJson),
        Limits::local(),
        Engines::OneShot,
        cancel,
        on_event,
    )
}

/// Resolves the input format of a conversion from the file's content and extension.
fn classify_input(request: &ConvertRequest) -> Result<(Format, bool), ConvertError> {
    let clean_ext = || {
        let ext = request
            .input
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("unknown");
        sanitize_identifier(ext, 32)
    };
    match classify_file(&request.input) {
        Ok(classification) => match classification.format {
            Some(format) => Ok((format, classification.extension_mismatch)),
            None => Err(ConvertError::UnsupportedRoute {
                detail: Some(format!("no reachable targets from {}", clean_ext())),
            }),
        },
        Err(_) => {
            if request.input.is_dir() {
                return Err(ConvertError::InputIo);
            }
            let ext = clean_ext();
            let Some(format) = Format::from_extension(&ext) else {
                return Err(ConvertError::UnsupportedRoute {
                    detail: Some(format!("no reachable targets from {ext}")),
                });
            };
            if Format::from_user_name(&request.target_format) == Some(format) {
                let caps = planner::embedded();
                let registered = caps.registered_engines();
                let reachable =
                    planner::reachable_formats(caps, format, request.profile, &registered);
                return Err(ConvertError::UnsupportedRoute {
                    detail: Some(reachable_targets_detail(format.user_name(), &reachable)),
                });
            }
            Err(ConvertError::InputIo)
        }
    }
}

fn plan_error(error: PlanError, input: Format) -> ConvertError {
    match error {
        PlanError::NoRoute { reachable, .. } => ConvertError::UnsupportedRoute {
            detail: Some(reachable_targets_detail(input.user_name(), &reachable)),
        },
        PlanError::InvalidCapabilities(err) => ConvertError::InvalidCapabilities(err.to_string()),
        PlanError::SameFormat { .. } => ConvertError::UnsupportedRoute {
            detail: Some(format!("no reachable targets from {}", input.user_name())),
        },
    }
}

/// Keeps the planned route when its engines are available; otherwise plans again over the
/// engines that are, and names the first missing engine when nothing else reaches the target.
fn plan_with_available_engines(
    caps: &planner::Capabilities,
    initial: Plan,
    profile: Profile,
    invocation: &EngineInvocation<'_>,
    cancel: &CancellationToken,
) -> Result<Plan, ConvertError> {
    let needed: Vec<&str> = initial
        .steps
        .iter()
        .map(|step| step.engine.as_str())
        .filter(|engine| *engine != IN_PROCESS_ENGINE)
        .collect();
    if needed.is_empty() {
        return Ok(initial);
    }
    let report =
        engines::availability_of(invocation, &needed, cancel.clone()).map_err(map_run_error)?;
    for engine in &needed {
        if let Some(detail) = report
            .get(engine)
            .and_then(|entry| entry.startup_failure.as_ref())
        {
            return Err(ConvertError::SandboxUnavailable(detail.clone()));
        }
    }
    let Some(missing) = needed
        .iter()
        .find(|engine| !report.is_available(engine))
        .map(|engine| (*engine).to_owned())
    else {
        return Ok(initial);
    };

    let registered = caps.registered_engines();
    let registered: Vec<&str> = registered.iter().map(String::as_str).collect();
    let report =
        engines::availability_of(invocation, &registered, cancel.clone()).map_err(map_run_error)?;
    let (Some(first), Some(last)) = (initial.steps.first(), initial.steps.last()) else {
        return Ok(initial);
    };
    match planner::plan(
        caps,
        first.from,
        last.to,
        profile,
        &report.available_engine_ids(),
    ) {
        Ok(plan) => Ok(plan),
        Err(PlanError::NoRoute { .. }) => Err(ConvertError::missing_engine(&missing)),
        Err(error) => Err(plan_error(error, first.from)),
    }
}

#[allow(clippy::too_many_arguments)]
fn convert_executor<F>(
    request: &ConvertRequest,
    confinement: Option<Confinement<'_>>,
    invocation: &EngineInvocation<'_>,
    forced_input: Option<Format>,
    limits: Limits,
    engines: Engines<'_>,
    cancel: CancellationToken,
    on_event: F,
) -> Result<ConvertReport, ConvertError>
where
    F: FnMut(ConvertEvent),
{
    if cancel.is_cancelled() {
        return Err(ConvertError::Interrupted);
    }

    let started = Instant::now();
    let (input_format, extension_mismatch) = match forced_input {
        Some(format) => (format, false),
        None => classify_input(request)?,
    };

    let caps = planner::embedded();
    let registered_engines = caps.registered_engines();
    let reachable_detail = || {
        let reachable =
            planner::reachable_formats(caps, input_format, request.profile, &registered_engines);
        reachable_targets_detail(input_format.user_name(), &reachable)
    };

    let Some(target_format) = Format::from_user_name(&request.target_format) else {
        let clean_target = sanitize_identifier(&request.target_format, 64);
        return Err(ConvertError::UnsupportedRoute {
            detail: Some(format!(
                "unknown target format '{clean_target}'; {}",
                reachable_detail()
            )),
        });
    };

    if input_format == target_format {
        return Err(ConvertError::UnsupportedRoute {
            detail: Some(reachable_detail()),
        });
    }

    let selected = routing::select(
        &request.input,
        input_format,
        target_format,
        request.profile,
        caps,
        invocation,
        request.reader_engine.as_deref(),
        &limits,
        &cancel,
    )?;
    let plan = plan_with_available_engines(
        &selected.capabilities,
        selected.plan,
        request.profile,
        invocation,
        &cancel,
    )?;

    let route = route_from_plan(&plan).ok_or_else(|| ConvertError::UnsupportedRoute {
        detail: Some(format!(
            "no reachable targets from {}",
            input_format.user_name()
        )),
    })?;
    let mut report = convert_for_route(
        request,
        confinement,
        &route,
        selected.pdf,
        invocation,
        limits,
        engines,
        cancel,
        on_event,
    )?;
    report.elapsed = started.elapsed();
    if extension_mismatch {
        report.warnings.insert(
            0,
            ConvertWarning {
                code: "format_mismatch".to_owned(),
                message: "The input file extension does not match the detected format.".to_owned(),
            },
        );
    }
    Ok(report)
}

/// Copies an untrusted input file into the workspace `in/` directory, bounded by `max_input_bytes`.
/// If the requested input format is an archive format (e.g. DOCX or EPUB), runs archive preflight on the workspace copy.
///
/// This closes the time-of-check/time-of-use gap by ensuring that subsequent engine operations
/// read only the verified workspace copy, even if the source file is modified afterwards.
pub fn copy_and_preflight_input(
    input: &Path,
    input_format: &str,
    workspace: &Workspace,
    limits: &Limits,
) -> Result<PathBuf, ConvertError> {
    let file_name = input
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty() && !name.contains('/') && !name.contains('\\'))
        .unwrap_or("input.bin");
    let dest_path = workspace.input_dir().join(file_name);

    let mut source = open_regular_input(input)?;
    let mut dest = fs::File::create(&dest_path).map_err(|_| ConvertError::Failed)?;

    let mut buffer = [0u8; 64 * 1024];
    let mut total_copied: u64 = 0;

    loop {
        let n = source
            .read(&mut buffer)
            .map_err(|_| ConvertError::InputIo)?;
        if n == 0 {
            break;
        }
        total_copied = total_copied.saturating_add(n as u64);
        if limits.max_input_bytes.is_some_and(|max| total_copied > max) {
            let _ = fs::remove_file(&dest_path);
            return Err(ConvertError::LimitExceeded);
        }
        dest.write_all(&buffer[..n])
            .map_err(|_| ConvertError::Failed)?;
    }
    dest.flush().map_err(|_| ConvertError::Failed)?;
    drop(dest);

    let is_archive = input_format.eq_ignore_ascii_case("zip")
        || Format::ALL.iter().any(|format| {
            format.is_zip_container() && input_format.eq_ignore_ascii_case(format.id())
        });

    if is_archive {
        archive::preflight_archive(&dest_path, limits).map_err(|err| {
            let _ = fs::remove_file(&dest_path);
            ConvertError::from(err)
        })?;
    }

    Ok(dest_path)
}

/// The outcome of converting an input to AriadShift IR through an engine.
#[derive(Clone, Debug, PartialEq)]
pub struct ArchiveToIrOutput {
    pub document: ariad_core::ir::Document,
    pub warnings: Vec<ConvertWarning>,
}

/// Converts an untrusted archive input (DOCX or EPUB) to AriadShift IR through Pandoc.
///
/// Order of operations (structural TOCTOU defense):
/// 1. The host copies the source file into workspace `in/`, bounded by `max_input_bytes`.
/// 2. Runs the archive preflight on that workspace copy.
/// 3. Sends a `convert` request to the engine pointing at the copy.
/// 4. Reads `document.ir.json` back using the bounded depth-scanning `ir_io::read`.
///
/// Nothing reads the user's path after step 1.
pub fn read_archive_to_ir(
    input: &Path,
    input_format: &str,
    workspace: &mut Workspace,
    limits: &Limits,
    engine_program: &Path,
    cancel: CancellationToken,
) -> Result<ArchiveToIrOutput, ConvertError> {
    let format = Format::from_user_name(input_format)
        .ok_or(ConvertError::UnsupportedRoute { detail: None })?;
    read_to_ir_with_engine(
        input,
        format,
        engines::PANDOC.id,
        workspace,
        limits,
        &EngineInvocation::new(engine_program),
        cancel,
    )
}

/// Converts an untrusted input to AriadShift IR through the out-of-process `engine`, with the
/// same copy-then-read order as [`read_archive_to_ir`].
pub fn read_to_ir_with_engine(
    input: &Path,
    format: Format,
    engine: &str,
    workspace: &mut Workspace,
    limits: &Limits,
    invocation: &EngineInvocation<'_>,
    cancel: CancellationToken,
) -> Result<ArchiveToIrOutput, ConvertError> {
    if cancel.is_cancelled() {
        return Err(ConvertError::Interrupted);
    }

    let copied_path = copy_and_preflight_input(input, format.id(), workspace, limits)?;
    let mut warnings = Vec::new();
    let mut lease = Lease::one_shot(invocation.command_for(engine));
    let artifact = run_engine(
        &mut lease,
        engine,
        Format::AriadIrJson,
        EngineInput {
            path: &copied_path,
            format,
        },
        workspace,
        limits,
        BTreeMap::new(),
        &cancel,
        &mut warnings,
        &mut |_| {},
    )?;
    let document = read_ir_artifact(&artifact, workspace, limits)?;
    Ok(ArchiveToIrOutput { document, warnings })
}

/// Records reduced engine isolation once per conversion.
fn push_sandbox_warning(warnings: &mut Vec<ConvertWarning>, outcome: &runner::RunOutcome) {
    let Some(gap) = outcome.sandbox.gap() else {
        return;
    };
    let warning = ConvertWarning {
        code: "sandbox_unavailable".to_owned(),
        message: format!("The engine ran with reduced isolation: {gap}."),
    };
    if !warnings.contains(&warning) {
        warnings.push(warning);
    }
}

pub(crate) fn map_run_error(error: RunError) -> ConvertError {
    map_engine_error(engines::PANDOC.id, error)
}

fn map_engine_error(engine: &str, error: RunError) -> ConvertError {
    if engine == engines::docling::ENGINE_ID {
        match &error {
            RunError::EngineFailed {
                code: ErrorCode::ToolMissing | ErrorCode::ToolVersion,
                ..
            } => return ConvertError::DoclingToolMissing,
            RunError::Spawn(_) => return ConvertError::DoclingMissing,
            _ => {}
        }
    }
    match error {
        RunError::Cancelled => ConvertError::Interrupted,
        RunError::MemoryLimitExceeded { limit_mb } => {
            ConvertError::MemoryLimitExceeded { limit_mb }
        }
        RunError::SandboxUnavailable(detail) | RunError::SandboxLaunchFailed(detail) => {
            ConvertError::SandboxUnavailable(detail)
        }
        RunError::Timeout => ConvertError::LimitExceeded,
        RunError::EngineFailed {
            code: ErrorCode::ToolMissing | ErrorCode::ToolVersion,
            ..
        } => ConvertError::missing_engine(engine),
        RunError::EngineFailed {
            code: ErrorCode::LimitExceeded,
            ..
        } => ConvertError::LimitExceeded,
        _ => ConvertError::Failed,
    }
}

fn map_workspace_error(error: WorkspaceError) -> ConvertError {
    match error {
        WorkspaceError::DestinationExists => ConvertError::DestinationExists,
        _ => ConvertError::Failed,
    }
}

pub(crate) fn safe_warning(warning: &Warning) -> ConvertWarning {
    let code = serde_json::to_value(warning.code)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "conversion_warning".to_owned());
    sanitize_warning(&code, &warning.message)
}

fn sanitize_warning(code: &str, raw_message: &str) -> ConvertWarning {
    let sanitized_code = code.strip_prefix("ir_").unwrap_or(code).to_owned();
    let message = if raw_message.to_lowercase().contains("underline") {
        "Underline is not supported and was mapped to emphasis.".to_owned()
    } else {
        safe_warning_message(&sanitized_code)
    };
    ConvertWarning {
        code: sanitized_code,
        message,
    }
}

fn safe_warning_message(code: &str) -> String {
    let msg = match code {
        "front_matter_invalid" => "Invalid document metadata was ignored.",
        "front_matter_key_ignored" => "Unsupported document metadata was ignored.",
        "front_matter_alias_rejected" => "Unsafe document metadata was ignored.",
        "unsupported_node" => "Unsupported document content was omitted.",
        "raw_dropped" => "Unsafe or unsupported raw markup was omitted or sanitized.",
        "link_dropped" => "An unsupported link was omitted.",
        "image_not_embedded" => "An image was not embedded in the output document.",
        "footnote_missing" => "A footnote reference could not be resolved.",
        "footnote_unused" => "An unused footnote was omitted.",
        "format_mismatch" => "The input file extension does not match the detected format.",
        "ocr_fallback" => "OCR was skipped because no suitable OCR engine is installed.",
        "ocr_language_unsupported" => {
            "OCR is not available for a requested language; another language was used."
        }
        "partial_conversion" => "Some pages or elements could not be converted.",
        "ocr_required" => "A page has no text layer; reading it needs OCR.",
        "text_unmapped" => "Some characters have no Unicode mapping and were replaced.",
        "math_not_converted" => "Math could not be converted and was kept as TeX text.",
        _ => "The document could not be converted without a warning.",
    };
    msg.to_owned()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{
        ConvertError, detect_format_from_path, is_supported_route, open_regular_input,
        paths_refer_to_same_file, read_bounded_file, read_document_to_ir, safe_warning,
        sanitize_warning,
    };
    use ariad_core::{
        format::Format,
        limits::Limits,
        warning::{Warning, WarningCode},
    };

    #[cfg(unix)]
    #[test]
    fn engine_ir_artifacts_cannot_escape_or_block_on_special_files() {
        use super::{Workspace, read_ir_artifact};
        use std::os::unix::fs::symlink;
        let workspace = Workspace::new().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let secret = outside.path().join("private.ir.json");
        std::fs::write(
            &secret,
            serde_json::to_vec(&ariad_core::ir::Document::default()).unwrap(),
        )
        .unwrap();
        let artifact = workspace.output_dir().join("document.ir.json");
        symlink(&secret, &artifact).unwrap();
        assert_eq!(
            read_ir_artifact(&artifact, &workspace, &Limits::local()).err(),
            Some(ConvertError::Failed)
        );
        std::fs::remove_file(&artifact).unwrap();
        assert!(
            std::process::Command::new("mkfifo")
                .arg(&artifact)
                .status()
                .unwrap()
                .success()
        );
        let started = std::time::Instant::now();
        assert_eq!(
            read_ir_artifact(&artifact, &workspace, &Limits::local()).err(),
            Some(ConvertError::Failed)
        );
        assert!(started.elapsed() < std::time::Duration::from_secs(1));
    }

    #[test]
    fn detects_supported_document_formats() {
        assert_eq!(
            detect_format_from_path(Path::new("notes.md")),
            Some(Format::Markdown)
        );
        assert_eq!(
            detect_format_from_path(Path::new("notes.MARKDOWN")),
            Some(Format::Markdown)
        );
        assert_eq!(
            detect_format_from_path(Path::new("page.html")),
            Some(Format::Html)
        );
        assert_eq!(
            detect_format_from_path(Path::new("page.HTM")),
            Some(Format::Html)
        );
        assert_eq!(
            detect_format_from_path(Path::new("doc.docx")),
            Some(Format::Docx)
        );
        assert_eq!(
            detect_format_from_path(Path::new("book.epub")),
            Some(Format::Epub)
        );
        assert_eq!(
            detect_format_from_path(Path::new("file.pdf")),
            Some(Format::Pdf)
        );
        assert_eq!(detect_format_from_path(Path::new("file.unknown")), None);
    }

    #[test]
    fn supports_all_distinct_format_pairs_and_refuses_same_format() {
        let all_formats = [Format::Markdown, Format::Html, Format::Docx, Format::Epub];
        for from in all_formats {
            for to in all_formats {
                if from == to {
                    assert!(
                        !is_supported_route(from, to),
                        "same-format route {:?} -> {:?} must be refused",
                        from,
                        to
                    );
                } else {
                    assert!(
                        is_supported_route(from, to),
                        "cross-format route {:?} -> {:?} must be supported",
                        from,
                        to
                    );
                }
            }
        }
    }

    #[test]
    fn detects_identical_or_canonical_paths() {
        let temp_dir = tempfile::tempdir().unwrap();
        let file_a = temp_dir.path().join("file_a.md");
        std::fs::write(&file_a, b"test").unwrap();

        assert!(paths_refer_to_same_file(&file_a, &file_a));
        assert!(paths_refer_to_same_file(
            &file_a,
            &temp_dir.path().join("./file_a.md")
        ));
        assert!(!paths_refer_to_same_file(
            &file_a,
            &temp_dir.path().join("file_b.docx")
        ));
    }

    #[test]
    fn extracted_docling_json_uses_the_ir_budget_instead_of_the_upload_budget() {
        use super::{read_docling_to_ir, read_docling_with_cap};
        let input = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/docling/en-map-room.docling.json");
        let mut limits = Limits::cloud();
        limits.max_input_bytes = Some(64);
        assert_eq!(
            read_docling_to_ir(&input, &limits),
            Err(ConvertError::LimitExceeded)
        );
        let output = read_docling_with_cap(&input, &limits, limits.max_ir_json_bytes)
            .expect("extracted JSON can be larger than the uploaded document");
        assert!(!output.document.body.is_empty());
        limits.max_ir_json_bytes = 64;
        assert_eq!(
            read_docling_with_cap(&input, &limits, limits.max_ir_json_bytes),
            Err(ConvertError::LimitExceeded)
        );
    }

    #[test]
    fn maps_conversion_errors_to_the_documented_exit_codes() {
        let cases = [
            (ConvertError::InputIo, 1),
            (ConvertError::Failed, 1),
            (ConvertError::DestinationSameAsInput, 2),
            (ConvertError::UnsupportedRoute { detail: None }, 3),
            (ConvertError::LimitExceeded, 4),
            (ConvertError::ToolMissing, 5),
            (ConvertError::DestinationExists, 6),
            (ConvertError::Interrupted, 130),
        ];
        for (error, expected) in cases {
            assert_eq!(error.exit_code(), expected);
        }
    }

    #[test]
    fn warning_sanitization_removes_prefixes_and_contains_no_doc_content() {
        let warning = Warning::new(
            WarningCode::ImageNotEmbedded,
            "secret_file.png was not found on /home/user/private",
        );
        let sanitized = safe_warning(&warning);
        assert_eq!(sanitized.code, "image_not_embedded");
        assert!(!sanitized.code.starts_with("ir_"));
        assert!(!sanitized.message.contains("secret_file"));
        assert!(!sanitized.message.contains("/home/user"));

        // Test stripping ir_ prefix on engine warning code using sanitize_warning
        let sanitized =
            sanitize_warning("ir_font_missing", "secret font Helvetica on /private/path");
        assert_eq!(sanitized.code, "font_missing");
        assert!(!sanitized.message.contains("secret"));
        assert!(!sanitized.message.contains("/private"));
    }

    #[test]
    fn mutating_source_after_copy_converts_checked_bytes() {
        use super::{Limits, Workspace, copy_and_preflight_input};
        use ariad_core::protocol::{Input, Output, PROTOCOL, Request};
        use std::path::Path;

        let fixture_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("fixtures/docx/vi-styled-report.docx");
        assert!(
            fixture_path.is_file(),
            "fixture must exist: {}",
            fixture_path.display()
        );

        let temp_dir = tempfile::tempdir().unwrap();
        let temp_src = temp_dir.path().join("source.docx");
        std::fs::copy(&fixture_path, &temp_src).unwrap();

        let mut workspace = Workspace::new().unwrap();
        let limits = Limits::local();
        let copy_path = copy_and_preflight_input(&temp_src, "docx", &workspace, &limits).unwrap();

        // Mutate source file after copy: overwrite with garbage
        std::fs::write(&temp_src, b"MUTATED_CORRUPT_SOURCE_GARBAGE").unwrap();

        // Execute convert request through engine pointing to copy
        let request = Request::Convert {
            session_id: None,
            protocol: PROTOCOL.to_owned(),
            job: "test-toctou".to_owned(),
            input: Input {
                path: copy_path.to_string_lossy().into_owned(),
                format: "docx".to_owned(),
            },
            output: Output {
                dir: workspace.output_dir().to_string_lossy().into_owned(),
                format: "ariad-ir+json".to_owned(),
            },
            work_dir: workspace.work_dir().to_string_lossy().into_owned(),
            options: std::collections::BTreeMap::new(),
            limits: limits.clone(),
        };

        let mut input_bytes = serde_json::to_vec(&request).unwrap();
        input_bytes.push(b'\n');
        let mut output_bytes = Vec::new();

        let exit =
            crate::engines::pandoc::serve(std::io::Cursor::new(input_bytes), &mut output_bytes);
        assert_eq!(exit, std::process::ExitCode::SUCCESS);

        let out_doc_path = workspace.output_dir().join("document.ir.json");
        assert!(out_doc_path.is_file());

        let doc = crate::ir_io::read(std::fs::File::open(&out_doc_path).unwrap(), &limits)
            .expect("IR read must succeed");

        // The document reflects the checked bytes before source was mutated
        let json = serde_json::to_string(&doc).unwrap();
        assert!(json.contains("Khu") && json.contains("vườn"));

        let _ = workspace.close();
    }

    #[test]
    fn copy_and_preflight_input_enforces_max_input_bytes() {
        use super::{ConvertError, Limits, Workspace, copy_and_preflight_input};

        let temp_dir = tempfile::tempdir().unwrap();
        let temp_src = temp_dir.path().join("source.docx");
        std::fs::write(&temp_src, [0u8; 1024]).unwrap();

        let mut workspace = Workspace::new().unwrap();
        let mut limits = Limits::local();
        limits.max_input_bytes = Some(500);

        let err = copy_and_preflight_input(&temp_src, "docx", &workspace, &limits).unwrap_err();
        assert_eq!(err, ConvertError::LimitExceeded);

        let _ = workspace.close();
    }

    #[test]
    fn reader_edges_warning_sanitization_removes_prefixes_and_content() {
        // Markdown reader edge
        let md_warning = Warning::new(WarningCode::UnsupportedNode, "secret markdown syntax <raw>");
        let s_md = safe_warning(&md_warning);
        assert_eq!(s_md.code, "unsupported_node");
        assert!(!s_md.code.starts_with("ir_"));
        assert!(!s_md.message.contains("secret"));

        // HTML reader edge
        let html_warning = Warning::new(
            WarningCode::RawDropped,
            "<script>evil_confidential_token()</script>",
        );
        let s_html = safe_warning(&html_warning);
        assert_eq!(s_html.code, "raw_dropped");
        assert!(!s_html.code.starts_with("ir_"));
        assert!(!s_html.message.contains("evil"));
        assert!(!s_html.message.contains("token"));

        // Archive / engine reader edges (DOCX & EPUB)
        for code in [
            "ir_image_not_embedded",
            "ir_unsupported_node",
            "ir_footnote_missing",
            "ir_custom_code",
        ] {
            let sanitized = sanitize_warning(code, &format!("confidential detail in file {code}"));
            assert!(!sanitized.code.starts_with("ir_"));
            assert!(!sanitized.message.contains("confidential"));
            assert!(!sanitized.message.contains("detail"));
            assert!(!sanitized.message.contains('/'));
        }
    }

    #[test]
    fn convert_with_warning_emits_sanitized_warnings_without_private_details() {
        let temp_dir = tempfile::tempdir().unwrap();
        let in_file = temp_dir.path().join("input.md");
        std::fs::write(
            &in_file,
            b"# Title\n\n<script>secret_private_token()</script>\n",
        )
        .unwrap();

        let out_file = temp_dir.path().join("output.html");
        let cancel = tokio_util::sync::CancellationToken::new();

        let mut req = super::ConvertRequest::new(&in_file, &out_file, "html", "ashift");
        req.overwrite = true;

        let report = super::convert(&req, cancel, |_| {}).expect("conversion succeeds");
        assert!(
            report.warnings.iter().any(|w| w.code == "raw_dropped"),
            "must emit raw_dropped warning"
        );
        for w in &report.warnings {
            assert!(!w.code.starts_with("ir_"));
            assert!(!w.message.contains("secret"));
            assert!(!w.message.contains("token"));
            assert_eq!(
                w.message,
                "Unsafe or unsupported raw markup was omitted or sanitized."
            );
        }
    }

    #[test]
    fn executor_dispatches_through_route_edges_not_target_format() {
        use super::{ConvertRequest, Route, convert_for_route};
        use crate::engines::{EngineInvocation, IN_PROCESS_ENGINE};
        use ariad_core::planner::PlanStep;

        let temp_dir = tempfile::tempdir().unwrap();
        let in_file = temp_dir.path().join("input.md");
        std::fs::write(&in_file, b"# Custom Dispatch Test\n\nSome text here.\n").unwrap();

        let out_file = temp_dir.path().join("output.txt");
        let cancel = tokio_util::sync::CancellationToken::new();

        // Target format string says "html", but the route's last step writes Markdown.
        let mut req = ConvertRequest::new(&in_file, &out_file, "html", "ashift");
        req.overwrite = true;

        let step = |from, to| PlanStep {
            from,
            to,
            engine: IN_PROCESS_ENGINE.to_owned(),
        };
        let swapped_route = Route {
            input_format: Format::Markdown,
            output_format: Format::Html,
            steps: vec![
                step(Format::Markdown, Format::AriadIrJson),
                step(Format::AriadIrJson, Format::Markdown),
            ],
        };

        let program = Path::new("ashift");
        let report = convert_for_route(
            &req,
            None,
            &swapped_route,
            None,
            &EngineInvocation::new(program),
            ariad_core::limits::Limits::local(),
            super::Engines::OneShot,
            cancel,
            |_| {},
        )
        .expect("conversion through swapped route succeeds");

        let content = std::fs::read_to_string(&report.output).unwrap();
        // The last step writes Markdown, so the output is Markdown, not HTML.
        assert!(content.contains("# Custom Dispatch Test"));
        assert!(!content.contains("<!DOCTYPE html>"));
    }

    #[test]
    fn convert_for_route_rejects_same_format_route() {
        use super::{ConvertRequest, Route, convert_for_route};
        use crate::engines::{EngineInvocation, IN_PROCESS_ENGINE};
        use ariad_core::planner::PlanStep;

        let temp_dir = tempfile::tempdir().unwrap();
        let in_file = temp_dir.path().join("input.md");
        std::fs::write(&in_file, b"# Same format\n").unwrap();
        let out_file = temp_dir.path().join("output.out");

        let req = ConvertRequest::new(&in_file, &out_file, "md", "ashift");
        let same_route = Route {
            input_format: Format::Markdown,
            output_format: Format::Markdown,
            steps: vec![PlanStep {
                from: Format::Markdown,
                to: Format::AriadIrJson,
                engine: IN_PROCESS_ENGINE.to_owned(),
            }],
        };

        let result = convert_for_route(
            &req,
            None,
            &same_route,
            None,
            &EngineInvocation::new(Path::new("ashift")),
            ariad_core::limits::Limits::local(),
            super::Engines::OneShot,
            tokio_util::sync::CancellationToken::new(),
            |_| {},
        );

        assert!(matches!(result, Err(ConvertError::UnsupportedRoute { .. })));
    }

    #[test]
    #[cfg(unix)]
    fn archive_to_ir_warning_sanitization_runs_through_real_read_path() {
        use super::{Limits, Workspace, read_archive_to_ir};
        use std::os::unix::fs::PermissionsExt;
        use std::path::Path;

        let fixture_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("fixtures/docx/vi-styled-report.docx");
        assert!(fixture_path.is_file(), "fixture must exist");

        // Create an executable mock python script acting as the engine
        let temp_dir = tempfile::tempdir().unwrap();
        let script_path = temp_dir.path().join("mock_engine.py");
        let script_content = r#"#!/usr/bin/env python3
import sys, json, os

for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    req = json.loads(line)
    if "input" in req:
        out_dir = req["output"]["dir"]
        ir_path = os.path.join(out_dir, "document.ir.json")
        with open(ir_path, "w") as f:
            f.write('{"meta":{"authors":[]},"body":[]}')
        print(json.dumps({"type":"warning","code":"ir_image_not_embedded","message":"confidential asset at /home/user/secret.png was not found"}), flush=True)
        print(json.dumps({"type":"artifact","format":"ariad-ir+json","path":ir_path}), flush=True)
        print(json.dumps({"type":"result","ok":True}), flush=True)
        break
    else:
        print(json.dumps({
            "type":"capabilities",
            "engine":"mock",
            "version":"0.1.0",
            "tool":{"name":"mock","version":None,"status":"found"},
            "license":"MIT",
            "routes":[],
            "enforces_memory_limit":True
        }), flush=True)
        print(json.dumps({"type":"result","ok":True}), flush=True)
"#;
        std::fs::write(&script_path, script_content).unwrap();
        let mut perms = std::fs::metadata(&script_path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script_path, perms).unwrap();

        let mut workspace = Workspace::new().unwrap();
        let cancel = tokio_util::sync::CancellationToken::new();

        let out = read_archive_to_ir(
            &fixture_path,
            "docx",
            &mut workspace,
            &Limits::local(),
            &script_path,
            cancel,
        )
        .expect("read_archive_to_ir must succeed");

        assert_eq!(out.warnings.len(), 1);
        let warn = &out.warnings[0];
        // Must strip ir_ prefix
        assert_eq!(warn.code, "image_not_embedded");
        assert!(!warn.code.starts_with("ir_"));
        // Must not contain leaked private path or secret words
        assert!(!warn.message.contains("/home/user"));
        assert!(!warn.message.contains("secret"));
        assert!(!warn.message.contains("confidential"));
        // Must use sanitized generic safe message
        assert_eq!(
            warn.message,
            "An image was not embedded in the output document."
        );

        let _ = workspace.close();
    }

    #[test]
    fn open_regular_input_rejects_directory() {
        let temp = tempfile::tempdir().unwrap();
        let err = open_regular_input(temp.path()).unwrap_err();
        assert_eq!(err, ConvertError::InputIo);
    }

    #[cfg(unix)]
    #[test]
    fn open_regular_input_rejects_fifo_without_blocking() {
        let temp = tempfile::tempdir().unwrap();
        let fifo_path = temp.path().join("test.fifo");
        if let Ok(status) = std::process::Command::new("mkfifo")
            .arg(&fifo_path)
            .status()
            && status.success()
        {
            let err = open_regular_input(&fifo_path).unwrap_err();
            assert_eq!(err, ConvertError::InputIo);
        }
    }

    #[cfg(unix)]
    #[test]
    fn open_regular_input_rejects_device_nodes() {
        let dev_zero = Path::new("/dev/zero");
        if dev_zero.exists() {
            let err = open_regular_input(dev_zero).unwrap_err();
            assert_eq!(err, ConvertError::InputIo);

            let temp = tempfile::tempdir().unwrap();
            let symlink = temp.path().join("symlink_to_zero");
            if std::os::unix::fs::symlink(dev_zero, &symlink).is_ok() {
                let err = open_regular_input(&symlink).unwrap_err();
                assert_eq!(err, ConvertError::InputIo);
            }
        }
    }

    #[test]
    fn read_bounded_file_enforces_cap() {
        let temp = tempfile::tempdir().unwrap();
        let file_path = temp.path().join("data.bin");
        std::fs::write(&file_path, vec![0u8; 1024]).unwrap();

        let err = read_bounded_file(&file_path, Some(512)).unwrap_err();
        assert_eq!(err, ConvertError::LimitExceeded);

        let data = read_bounded_file(&file_path, Some(2048)).unwrap();
        assert_eq!(data.len(), 1024);
    }

    #[test]
    fn read_document_to_ir_rejects_invalid_utf8_markdown() {
        let temp = tempfile::tempdir().unwrap();
        let file_path = temp.path().join("bad.md");
        std::fs::write(&file_path, b"# Title\n\nHello \xff\xfe world\n").unwrap();

        let err = read_document_to_ir(&file_path, Format::Markdown, &Limits::local()).unwrap_err();
        assert_eq!(err, ConvertError::InputIo);
    }

    #[test]
    fn sanitize_identifier_filters_control_characters_and_esc() {
        let raw = "foo\x1b[31mbar\x00baz\r\nqux";
        let sanitized = super::sanitize_identifier(raw, 64);
        assert_eq!(sanitized, "foo[31mbarbazqux");
        assert!(!sanitized.contains('\x1b'));
        assert!(!sanitized.contains('\x00'));
        assert!(!sanitized.contains('\r'));
        assert!(!sanitized.contains('\n'));
    }

    #[test]
    fn validate_utf8_streaming_accepts_valid_content() {
        let valid_text = "Hello, world! This is valid UTF-8 with € and ¢ and emojis 🚀.\n";
        super::validate_utf8_reader_streaming(valid_text.as_bytes()).unwrap();
    }

    #[test]
    fn validate_utf8_streaming_rejects_invalid_bytes() {
        let invalid = b"Hello \xff\xfe world";
        let err = super::validate_utf8_reader_streaming(&invalid[..]).unwrap_err();
        assert_eq!(err, ConvertError::InputIo);
    }

    #[test]
    fn validate_utf8_streaming_handles_boundary_crossing() {
        // 1. Valid 2-byte UTF-8 character '¢' (0xC2, 0xA2) split across 64 KiB boundary
        let mut buf = vec![b'a'; 64 * 1024 + 10];
        buf[64 * 1024 - 1] = 0xC2;
        buf[64 * 1024] = 0xA2;
        super::validate_utf8_reader_streaming(&buf[..]).unwrap();

        // 2. Invalid continuation byte across boundary (0xC2 followed by 0x20)
        buf[64 * 1024] = 0x20;
        let err = super::validate_utf8_reader_streaming(&buf[..]).unwrap_err();
        assert_eq!(err, ConvertError::InputIo);

        // 3. Trailing incomplete sequence at EOF (0xC2 with no continuation byte)
        let truncated = &buf[..64 * 1024];
        let err = super::validate_utf8_reader_streaming(truncated).unwrap_err();
        assert_eq!(err, ConvertError::InputIo);
    }
}
