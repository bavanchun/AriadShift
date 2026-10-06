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
    format::Format,
    limits::Limits,
    planner::{self, Plan, PlanError},
    protocol::{ErrorCode, Event, Input, Output, PROTOCOL, Request},
    reader::{html as reader_html, markdown as reader_markdown},
    warning::Warning,
    writer::{html as writer_html, markdown as writer_markdown},
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::{
    archive::{self, ArchiveError},
    assets, engines, package_meta,
    runner::{self, RunError},
    workspace::{Workspace, WorkspaceError},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReaderEdge {
    NativeMarkdown,
    NativeHtml,
    PandocDocx,
    PandocEpub,
}

impl ReaderEdge {
    /// All reader edges supported by the host executor.
    pub const ALL: &'static [Self] = &[
        Self::NativeMarkdown,
        Self::NativeHtml,
        Self::PandocDocx,
        Self::PandocEpub,
    ];

    /// The format transformation and engine capability represented by this edge.
    #[must_use]
    pub const fn capability(self) -> (Format, Format, &'static str) {
        match self {
            Self::NativeMarkdown => (
                Format::Markdown,
                Format::AriadIrJson,
                engines::IN_PROCESS_ENGINE,
            ),
            Self::NativeHtml => (
                Format::Html,
                Format::AriadIrJson,
                engines::IN_PROCESS_ENGINE,
            ),
            Self::PandocDocx => (Format::Docx, Format::AriadIrJson, "pandoc"),
            Self::PandocEpub => (Format::Epub, Format::AriadIrJson, "pandoc"),
        }
    }

    /// Finds the reader edge corresponding to a specific format pair and engine.
    #[must_use]
    pub fn for_capability(from: Format, to: Format, engine: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|e| {
            let (f, t, eng) = e.capability();
            f == from && t == to && eng == engine
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WriterEdge {
    NativeMarkdown,
    NativeHtml,
    PandocDocx,
    PandocEpub,
}

impl WriterEdge {
    /// All writer edges supported by the host executor.
    pub const ALL: &'static [Self] = &[
        Self::NativeMarkdown,
        Self::NativeHtml,
        Self::PandocDocx,
        Self::PandocEpub,
    ];

    /// The format transformation and engine capability represented by this edge.
    #[must_use]
    pub const fn capability(self) -> (Format, Format, &'static str) {
        match self {
            Self::NativeMarkdown => (
                Format::AriadIrJson,
                Format::Markdown,
                engines::IN_PROCESS_ENGINE,
            ),
            Self::NativeHtml => (
                Format::AriadIrJson,
                Format::Html,
                engines::IN_PROCESS_ENGINE,
            ),
            Self::PandocDocx => (Format::AriadIrJson, Format::Docx, "pandoc"),
            Self::PandocEpub => (Format::AriadIrJson, Format::Epub, "pandoc"),
        }
    }

    /// Finds the writer edge corresponding to a specific format pair and engine.
    #[must_use]
    pub fn for_capability(from: Format, to: Format, engine: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|e| {
            let (f, t, eng) = e.capability();
            f == from && t == to && eng == engine
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Route {
    pub input_format: DocumentFormat,
    pub output_format: DocumentFormat,
    pub reader: ReaderEdge,
    pub writer: WriterEdge,
}

impl std::fmt::Display for Route {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} -> {}",
            self.input_format.as_str(),
            self.output_format.as_str()
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum DocumentFormat {
    Markdown,
    Html,
    Docx,
    Epub,
}

impl DocumentFormat {
    #[must_use]
    pub fn from_extension(ext: &str) -> Option<Self> {
        if ext.eq_ignore_ascii_case("md") || ext.eq_ignore_ascii_case("markdown") {
            Some(Self::Markdown)
        } else if ext.eq_ignore_ascii_case("html") || ext.eq_ignore_ascii_case("htm") {
            Some(Self::Html)
        } else if ext.eq_ignore_ascii_case("docx") {
            Some(Self::Docx)
        } else if ext.eq_ignore_ascii_case("epub") {
            Some(Self::Epub)
        } else {
            None
        }
    }

    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Self::from_extension(s)
    }

    #[must_use]
    pub const fn default_extension(self) -> &'static str {
        match self {
            Self::Markdown => "md",
            Self::Html => "html",
            Self::Docx => "docx",
            Self::Epub => "epub",
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Markdown => "md",
            Self::Html => "html",
            Self::Docx => "docx",
            Self::Epub => "epub",
        }
    }
}

impl From<DocumentFormat> for Format {
    fn from(doc_fmt: DocumentFormat) -> Self {
        match doc_fmt {
            DocumentFormat::Markdown => Self::Markdown,
            DocumentFormat::Html => Self::Html,
            DocumentFormat::Docx => Self::Docx,
            DocumentFormat::Epub => Self::Epub,
        }
    }
}

impl TryFrom<Format> for DocumentFormat {
    type Error = ();

    fn try_from(fmt: Format) -> Result<Self, Self::Error> {
        match fmt {
            Format::Markdown => Ok(Self::Markdown),
            Format::Html => Ok(Self::Html),
            Format::Docx => Ok(Self::Docx),
            Format::Epub => Ok(Self::Epub),
            _ => Err(()),
        }
    }
}

#[must_use]
pub fn route_from_plan(plan: &Plan) -> Option<Route> {
    if plan.steps.len() != 2 {
        return None;
    }
    let reader_step = &plan.steps[0];
    let writer_step = &plan.steps[1];

    if reader_step.to != Format::AriadIrJson || writer_step.from != Format::AriadIrJson {
        return None;
    }

    let input_format = DocumentFormat::try_from(reader_step.from).ok()?;
    let output_format = DocumentFormat::try_from(writer_step.to).ok()?;

    let reader = ReaderEdge::for_capability(reader_step.from, reader_step.to, &reader_step.engine)?;
    let writer = WriterEdge::for_capability(writer_step.from, writer_step.to, &writer_step.engine)?;

    Some(Route {
        input_format,
        output_format,
        reader,
        writer,
    })
}

/// Returns the capability edges supported by the host executor.
#[must_use]
pub fn executor_edges() -> Vec<(Format, Format, &'static str)> {
    let mut edges = Vec::with_capacity(ReaderEdge::ALL.len() + WriterEdge::ALL.len());
    for r in ReaderEdge::ALL {
        edges.push(r.capability());
    }
    for w in WriterEdge::ALL {
        edges.push(w.capability());
    }
    edges
}

#[must_use]
pub fn detect_format_from_path(path: &Path) -> Option<DocumentFormat> {
    let ext = path.extension()?.to_str()?;
    DocumentFormat::from_extension(ext)
}

#[must_use]
pub fn is_supported_route(input: DocumentFormat, output: DocumentFormat) -> bool {
    if input == output {
        return false;
    }
    let caps = planner::embedded();
    let from = Format::from(input);
    let to = Format::from(output);
    let engines = caps.registered_engines();
    planner::plan(caps, from, to, Profile::Editable, &engines).is_ok()
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConvertRequest {
    pub input: PathBuf,
    pub output: PathBuf,
    pub target_format: String,
    pub profile: Profile,
    pub overwrite: bool,
    pub engine_program: PathBuf,
    pub title_fallback: Option<String>,
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
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConvertWarning {
    pub code: String,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ConvertReport {
    pub output: PathBuf,
    pub warnings: Vec<ConvertWarning>,
    pub elapsed: Duration,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConvertError {
    UnsupportedRoute { detail: Option<String> },
    DestinationSameAsInput,
    InputIo,
    LimitExceeded,
    ToolMissing,
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
            Self::LimitExceeded => write!(f, "conversion limit exceeded"),
            Self::ToolMissing => write!(
                f,
                "Pandoc is missing or unsupported. Run `just pandoc` or set ASHIFT_PANDOC."
            ),
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

impl ConvertError {
    #[must_use]
    pub const fn exit_code(&self) -> u8 {
        match self {
            Self::DestinationSameAsInput => 2,
            Self::UnsupportedRoute { .. } => 3,
            Self::LimitExceeded => 4,
            Self::ToolMissing => 5,
            Self::DestinationExists => 6,
            Self::Interrupted => 130,
            Self::InputIo
            | Self::UnsupportedIrVersion
            | Self::InvalidCapabilities(_)
            | Self::Failed => 1,
        }
    }
}

fn reachable_targets_detail(from_str: &str, reachable: &[Format]) -> String {
    let mut sorted_targets: Vec<&'static str> = reachable
        .iter()
        .copied()
        .filter(|&f| f != Format::AriadIrJson)
        .map(|f| DocumentFormat::try_from(f).map_or(f.id(), DocumentFormat::as_str))
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
    };
    convert_executor(&request, engine_args, limits, cancel, on_event)
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
    let args = [OsString::from("__engine"), OsString::from("pandoc")];
    convert_executor(request, &args, Limits::local(), cancel, on_event)
}

fn read_bounded_file(path: &Path, max_bytes: Option<u64>) -> Result<Vec<u8>, ConvertError> {
    let mut file = fs::File::open(path).map_err(|_| ConvertError::InputIo)?;
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

/// Converts a supported input document with custom engine args and limits for an explicit Route.
pub(crate) fn convert_for_route<F>(
    request: &ConvertRequest,
    route: &Route,
    engine_args: &[OsString],
    limits: Limits,
    cancel: CancellationToken,
    mut on_event: F,
) -> Result<ConvertReport, ConvertError>
where
    F: FnMut(ConvertEvent),
{
    if cancel.is_cancelled() {
        return Err(ConvertError::Interrupted);
    }

    if route.input_format == route.output_format {
        return Err(ConvertError::UnsupportedRoute {
            detail: Some(format!(
                "no reachable targets from {}",
                route.input_format.as_str()
            )),
        });
    }

    if paths_refer_to_same_file(&request.input, &request.output) {
        return Err(ConvertError::DestinationSameAsInput);
    }

    let started = Instant::now();

    // Step 1: Reader dispatched via route.reader
    if cancel.is_cancelled() {
        return Err(ConvertError::Interrupted);
    }

    let mut warnings = Vec::new();
    let (mut document, asset_base_dir) = match route.reader {
        ReaderEdge::NativeMarkdown => {
            let bytes = read_bounded_file(&request.input, limits.max_input_bytes)?;
            let text = String::from_utf8(bytes).map_err(|_| ConvertError::InputIo)?;
            let parsed = reader_markdown::read(&text, &limits).map_err(|err| match err {
                reader_markdown::ReadError::NestingTooDeep { .. }
                | reader_markdown::ReadError::TooManyBlocks { .. }
                | reader_markdown::ReadError::InputTooLarge { .. }
                | reader_markdown::ReadError::InvalidLimits(_) => ConvertError::LimitExceeded,
            })?;
            for w in &parsed.warnings {
                warnings.push(safe_warning(w));
            }
            let base_dir = request.input.parent().unwrap_or_else(|| Path::new("."));
            (parsed.document, Some(base_dir))
        }
        ReaderEdge::NativeHtml => {
            let bytes = read_bounded_file(&request.input, limits.max_input_bytes)?;
            let parsed = reader_html::read(&bytes, &limits).map_err(|err| match err {
                reader_html::ReadError::NestingTooDeep { .. }
                | reader_html::ReadError::TooManyBlocks { .. }
                | reader_html::ReadError::InputTooLarge { .. }
                | reader_html::ReadError::TooManyNodes { .. }
                | reader_html::ReadError::InvalidLimits(_) => ConvertError::LimitExceeded,
            })?;
            for w in &parsed.warnings {
                warnings.push(safe_warning(w));
            }
            let base_dir = request.input.parent().unwrap_or_else(|| Path::new("."));
            (parsed.document, Some(base_dir))
        }
        ReaderEdge::PandocDocx | ReaderEdge::PandocEpub => {
            let format_str = if route.reader == ReaderEdge::PandocDocx {
                "docx"
            } else {
                "epub"
            };
            let mut archive_workspace = Workspace::new().map_err(|_| ConvertError::Failed)?;
            let res = read_archive_to_ir(
                &request.input,
                format_str,
                &mut archive_workspace,
                &limits,
                &request.engine_program,
                cancel.clone(),
            );
            let _ = archive_workspace.close();
            let archive_out = res?;
            warnings.extend(archive_out.warnings);
            (archive_out.document, None)
        }
    };

    // Step 2: Asset resolution for md and html input
    if cancel.is_cancelled() {
        return Err(ConvertError::Interrupted);
    }
    if let Some(base_dir) = asset_base_dir {
        let asset_warnings =
            assets::resolve(&mut document, base_dir, &limits).map_err(|_| ConvertError::InputIo)?;
        for w in &asset_warnings {
            warnings.push(safe_warning(w));
        }
    }

    // Step 3, 4, 5: Workspace execution (writer dispatched via route.writer, stamp, promote)
    if cancel.is_cancelled() {
        return Err(ConvertError::Interrupted);
    }
    let mut workspace = Workspace::new().map_err(|_| ConvertError::Failed)?;
    let result = write_in_workspace(
        &mut workspace,
        &document,
        route.writer,
        request,
        engine_args,
        limits,
        cancel,
        &mut warnings,
        &mut on_event,
    );
    let cleanup = workspace.close();
    result?;
    if cleanup.is_err() {
        warnings.push(ConvertWarning {
            code: "workspace_cleanup".to_owned(),
            message: "Temporary workspace cleanup failed.".to_owned(),
        });
    }

    Ok(ConvertReport {
        output: request.output.clone(),
        warnings,
        elapsed: started.elapsed(),
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

/// Converts a document to AriadShift IR (single reader edge).
pub fn convert_to_ir<F>(
    request: &ConvertToIrRequest,
    cancel: CancellationToken,
    _on_event: F,
) -> Result<ConvertReport, ConvertError>
where
    F: FnMut(ConvertEvent),
{
    if cancel.is_cancelled() {
        return Err(ConvertError::Interrupted);
    }

    if paths_refer_to_same_file(&request.input, &request.output) {
        return Err(ConvertError::DestinationSameAsInput);
    }

    let ext = request
        .input
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("unknown");
    let input_format =
        detect_format_from_path(&request.input).ok_or_else(|| ConvertError::UnsupportedRoute {
            detail: Some(format!("no reachable targets from {ext}")),
        })?;

    let limits = Limits::local();
    let started = Instant::now();
    let mut warnings = Vec::new();

    let reader = ReaderEdge::ALL
        .iter()
        .copied()
        .find(|e| {
            let (from, to, _) = e.capability();
            from == Format::from(input_format) && to == Format::AriadIrJson
        })
        .ok_or_else(|| ConvertError::UnsupportedRoute {
            detail: Some(format!(
                "no reachable targets from {}",
                input_format.as_str()
            )),
        })?;

    let (_, _, engine) = reader.capability();
    let engine_args = [OsString::from("__engine"), OsString::from(engine)];
    if engine != engines::IN_PROCESS_ENGINE {
        let avail = engines::availability(&request.engine_program, &engine_args, cancel.clone())
            .map_err(map_run_error)?;
        if !avail.is_available(engine) {
            return Err(ConvertError::ToolMissing);
        }
    }

    let (mut document, asset_base_dir) = match reader {
        ReaderEdge::NativeMarkdown => {
            let bytes = read_bounded_file(&request.input, limits.max_input_bytes)?;
            let text = String::from_utf8(bytes).map_err(|_| ConvertError::InputIo)?;
            let parsed = reader_markdown::read(&text, &limits).map_err(|err| match err {
                reader_markdown::ReadError::NestingTooDeep { .. }
                | reader_markdown::ReadError::TooManyBlocks { .. }
                | reader_markdown::ReadError::InputTooLarge { .. }
                | reader_markdown::ReadError::InvalidLimits(_) => ConvertError::LimitExceeded,
            })?;
            for w in &parsed.warnings {
                warnings.push(safe_warning(w));
            }
            let base_dir = request.input.parent().unwrap_or_else(|| Path::new("."));
            (parsed.document, Some(base_dir))
        }
        ReaderEdge::NativeHtml => {
            let bytes = read_bounded_file(&request.input, limits.max_input_bytes)?;
            let parsed = reader_html::read(&bytes, &limits).map_err(|err| match err {
                reader_html::ReadError::NestingTooDeep { .. }
                | reader_html::ReadError::TooManyBlocks { .. }
                | reader_html::ReadError::InputTooLarge { .. }
                | reader_html::ReadError::TooManyNodes { .. }
                | reader_html::ReadError::InvalidLimits(_) => ConvertError::LimitExceeded,
            })?;
            for w in &parsed.warnings {
                warnings.push(safe_warning(w));
            }
            let base_dir = request.input.parent().unwrap_or_else(|| Path::new("."));
            (parsed.document, Some(base_dir))
        }
        ReaderEdge::PandocDocx | ReaderEdge::PandocEpub => {
            let format_str = if reader == ReaderEdge::PandocDocx {
                "docx"
            } else {
                "epub"
            };
            let mut archive_workspace = Workspace::new().map_err(|_| ConvertError::Failed)?;
            let res = read_archive_to_ir(
                &request.input,
                format_str,
                &mut archive_workspace,
                &limits,
                &request.engine_program,
                cancel.clone(),
            );
            let _ = archive_workspace.close();
            let archive_out = res?;
            warnings.extend(archive_out.warnings);
            (archive_out.document, None)
        }
    };

    if cancel.is_cancelled() {
        return Err(ConvertError::Interrupted);
    }
    if let Some(base_dir) = asset_base_dir {
        let asset_warnings =
            assets::resolve(&mut document, base_dir, &limits).map_err(|_| ConvertError::InputIo)?;
        for w in &asset_warnings {
            warnings.push(safe_warning(w));
        }
    }

    if cancel.is_cancelled() {
        return Err(ConvertError::Interrupted);
    }

    let mut workspace = Workspace::new().map_err(|_| ConvertError::Failed)?;
    let ir_path = workspace.output_dir().join("document.ir.json");
    let ir_file = fs::File::create(&ir_path).map_err(|_| ConvertError::Failed)?;
    let mut buf_writer = std::io::BufWriter::new(ir_file);
    serde_json::to_writer_pretty(&mut buf_writer, &document).map_err(|_| ConvertError::Failed)?;
    buf_writer.flush().map_err(|_| ConvertError::Failed)?;

    workspace
        .promote(&ir_path, &request.output, request.overwrite)
        .map_err(|error| match error {
            WorkspaceError::DestinationExists => ConvertError::DestinationExists,
            other => map_workspace_error(other),
        })?;

    let cleanup = workspace.close();
    if cleanup.is_err() {
        warnings.push(ConvertWarning {
            code: "workspace_cleanup".to_owned(),
            message: "Temporary workspace cleanup failed.".to_owned(),
        });
    }

    Ok(ConvertReport {
        output: request.output.clone(),
        warnings,
        elapsed: started.elapsed(),
    })
}

/// Writes an AriadShift IR document to a target format (single writer edge).
pub fn write_from_ir<F>(
    request: &WriteFromIrRequest,
    cancel: CancellationToken,
    mut on_event: F,
) -> Result<ConvertReport, ConvertError>
where
    F: FnMut(ConvertEvent),
{
    if cancel.is_cancelled() {
        return Err(ConvertError::Interrupted);
    }

    if paths_refer_to_same_file(&request.input, &request.output) {
        return Err(ConvertError::DestinationSameAsInput);
    }

    let target_format = DocumentFormat::parse(&request.target_format).ok_or_else(|| {
        let clean_target: String = request
            .target_format
            .chars()
            .filter(|c| !c.is_control())
            .take(64)
            .collect();
        ConvertError::UnsupportedRoute {
            detail: Some(format!("unknown target format '{clean_target}'")),
        }
    })?;

    let limits = Limits::local();
    let started = Instant::now();

    let bytes = read_bounded_file(&request.input, Some(limits.max_ir_json_bytes))?;
    let document = crate::ir_io::read_versioned(&bytes, &limits).map_err(|err| match err {
        crate::ir_io::ReadError::ByteLimitExceeded { .. }
        | crate::ir_io::ReadError::DepthExceeded(_)
        | crate::ir_io::ReadError::BlockLimitExceeded { .. } => ConvertError::LimitExceeded,
        crate::ir_io::ReadError::UnsupportedVersion => ConvertError::UnsupportedIrVersion,
        crate::ir_io::ReadError::Io(_) => ConvertError::InputIo,
        _ => ConvertError::Failed,
    })?;

    let writer = WriterEdge::ALL
        .iter()
        .copied()
        .find(|e| {
            let (from, to, _) = e.capability();
            from == Format::AriadIrJson && to == Format::from(target_format)
        })
        .ok_or_else(|| {
            let clean_target: String = request
                .target_format
                .chars()
                .filter(|c| !c.is_control())
                .take(64)
                .collect();
            ConvertError::UnsupportedRoute {
                detail: Some(format!("unknown target format '{clean_target}'")),
            }
        })?;

    let (_, _, engine) = writer.capability();
    let engine_args = [OsString::from("__engine"), OsString::from(engine)];
    if engine != engines::IN_PROCESS_ENGINE {
        let avail = engines::availability(&request.engine_program, &engine_args, cancel.clone())
            .map_err(map_run_error)?;
        if !avail.is_available(engine) {
            return Err(ConvertError::ToolMissing);
        }
    }

    let convert_req = ConvertRequest {
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
    };

    let mut warnings = Vec::new();

    let mut workspace = Workspace::new().map_err(|_| ConvertError::Failed)?;
    let result = write_in_workspace(
        &mut workspace,
        &document,
        writer,
        &convert_req,
        &engine_args,
        limits,
        cancel,
        &mut warnings,
        &mut on_event,
    );
    let cleanup = workspace.close();
    result?;
    if cleanup.is_err() {
        warnings.push(ConvertWarning {
            code: "workspace_cleanup".to_owned(),
            message: "Temporary workspace cleanup failed.".to_owned(),
        });
    }

    Ok(ConvertReport {
        output: request.output.clone(),
        warnings,
        elapsed: started.elapsed(),
    })
}

fn convert_executor<F>(
    request: &ConvertRequest,
    engine_args: &[OsString],
    limits: Limits,
    cancel: CancellationToken,
    on_event: F,
) -> Result<ConvertReport, ConvertError>
where
    F: FnMut(ConvertEvent),
{
    if cancel.is_cancelled() {
        return Err(ConvertError::Interrupted);
    }

    let ext = request
        .input
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("unknown");
    let clean_ext: String = ext.chars().filter(|c| !c.is_control()).take(32).collect();

    let input_format = match detect_format_from_path(&request.input) {
        Some(fmt) => fmt,
        None => {
            return Err(ConvertError::UnsupportedRoute {
                detail: Some(format!("no reachable targets from {clean_ext}")),
            });
        }
    };

    let caps = planner::embedded();
    let from_fmt = Format::from(input_format);

    let target_format = match DocumentFormat::parse(&request.target_format) {
        Some(fmt) => fmt,
        None => {
            let clean_target: String = request
                .target_format
                .chars()
                .filter(|c| !c.is_control())
                .take(64)
                .collect();
            let registered = caps.registered_engines();
            let reachable =
                planner::reachable_formats(caps, from_fmt, request.profile, &registered);
            let detail = if reachable.is_empty() {
                format!(
                    "unknown target format '{clean_target}'; no reachable targets from {}",
                    input_format.as_str()
                )
            } else {
                format!(
                    "unknown target format '{clean_target}'; {}",
                    reachable_targets_detail(input_format.as_str(), &reachable)
                )
            };
            return Err(ConvertError::UnsupportedRoute {
                detail: Some(detail),
            });
        }
    };

    let to_fmt = Format::from(target_format);

    if input_format == target_format {
        let registered = caps.registered_engines();
        let reachable = planner::reachable_formats(caps, from_fmt, request.profile, &registered);
        let detail = reachable_targets_detail(input_format.as_str(), &reachable);
        return Err(ConvertError::UnsupportedRoute {
            detail: Some(detail),
        });
    }

    let registered_engines = caps.registered_engines();
    let initial_plan =
        match planner::plan(caps, from_fmt, to_fmt, request.profile, &registered_engines) {
            Ok(p) => p,
            Err(PlanError::NoRoute { reachable, .. }) => {
                let detail = reachable_targets_detail(input_format.as_str(), &reachable);
                return Err(ConvertError::UnsupportedRoute {
                    detail: Some(detail),
                });
            }
            Err(PlanError::InvalidCapabilities(err)) => {
                return Err(ConvertError::InvalidCapabilities(err.to_string()));
            }
            Err(PlanError::SameFormat { .. }) => {
                let detail = format!("no reachable targets from {}", input_format.as_str());
                return Err(ConvertError::UnsupportedRoute {
                    detail: Some(detail),
                });
            }
        };

    let is_pure_core = initial_plan
        .steps
        .iter()
        .all(|s| s.engine == engines::IN_PROCESS_ENGINE);
    let plan = if is_pure_core {
        initial_plan
    } else {
        let avail = engines::availability(&request.engine_program, engine_args, cancel.clone())
            .map_err(map_run_error)?;
        let available_engine_ids = avail.available_engine_ids();

        let all_needed_available = initial_plan
            .steps
            .iter()
            .all(|s| available_engine_ids.contains(&s.engine));

        if all_needed_available {
            initial_plan
        } else {
            match planner::plan(
                caps,
                from_fmt,
                to_fmt,
                request.profile,
                &available_engine_ids,
            ) {
                Ok(p) => p,
                Err(PlanError::NoRoute { .. }) => {
                    return Err(ConvertError::ToolMissing);
                }
                Err(PlanError::InvalidCapabilities(err)) => {
                    return Err(ConvertError::InvalidCapabilities(err.to_string()));
                }
                Err(PlanError::SameFormat { .. }) => {
                    let detail = format!("no reachable targets from {}", input_format.as_str());
                    return Err(ConvertError::UnsupportedRoute {
                        detail: Some(detail),
                    });
                }
            }
        }
    };

    let route = route_from_plan(&plan).ok_or_else(|| {
        let detail = format!("no reachable targets from {}", input_format.as_str());
        ConvertError::UnsupportedRoute {
            detail: Some(detail),
        }
    })?;
    convert_for_route(request, &route, engine_args, limits, cancel, on_event)
}

// Helper coordinating writer invocation, stamping, and atomic promotion.
#[allow(clippy::too_many_arguments)]
fn write_in_workspace<F>(
    workspace: &mut Workspace,
    document: &ariad_core::ir::Document,
    writer: WriterEdge,
    request: &ConvertRequest,
    engine_args: &[OsString],
    limits: Limits,
    cancel: CancellationToken,
    warnings: &mut Vec<ConvertWarning>,
    on_event: &mut F,
) -> Result<(), ConvertError>
where
    F: FnMut(ConvertEvent),
{
    if cancel.is_cancelled() {
        return Err(ConvertError::Interrupted);
    }

    let artifact_path = match writer {
        WriterEdge::NativeMarkdown => {
            let write_output = writer_markdown::write(document);
            for w in &write_output.warnings {
                warnings.push(safe_warning(w));
            }
            let path = workspace.output_dir().join("document.md");
            fs::write(&path, write_output.content.as_bytes()).map_err(|_| ConvertError::Failed)?;
            path
        }
        WriterEdge::NativeHtml => {
            let fallback_stem = stem_without_ir(&request.input);
            let title_fallback = request
                .title_fallback
                .as_deref()
                .or(fallback_stem.as_deref());
            let write_output = writer_html::write_with_title_fallback(document, title_fallback);
            for w in &write_output.warnings {
                warnings.push(safe_warning(w));
            }
            let path = workspace.output_dir().join("document.html");
            fs::write(&path, write_output.content.as_bytes()).map_err(|_| ConvertError::Failed)?;
            path
        }
        WriterEdge::PandocDocx | WriterEdge::PandocEpub => {
            let ir_path = workspace.input_dir().join("document.ir.json");
            let ir_file = fs::File::create(&ir_path).map_err(|_| ConvertError::Failed)?;
            let mut buf_writer = std::io::BufWriter::new(ir_file);
            serde_json::to_writer(&mut buf_writer, document).map_err(|_| ConvertError::Failed)?;
            buf_writer.flush().map_err(|_| ConvertError::Failed)?;

            let target_str = if writer == WriterEdge::PandocDocx {
                "docx"
            } else {
                "epub"
            };
            let mut options = BTreeMap::new();
            options.insert(
                "profile".to_owned(),
                serde_json::Value::String(request.profile.as_str().to_owned()),
            );
            let title_fallback = request.title_fallback.clone().unwrap_or_else(|| {
                stem_without_ir(&request.input).unwrap_or_else(|| "Untitled".to_owned())
            });
            options.insert(
                "title_fallback".to_owned(),
                serde_json::Value::String(title_fallback),
            );

            let engine_req = Request::Convert {
                protocol: PROTOCOL.to_owned(),
                job: Uuid::new_v4().simple().to_string(),
                input: Input {
                    path: ir_path.to_string_lossy().into_owned(),
                    format: "ariad-ir+json".to_owned(),
                },
                output: Output {
                    dir: workspace.output_dir().to_string_lossy().into_owned(),
                    format: target_str.to_owned(),
                },
                work_dir: workspace.work_dir().to_string_lossy().into_owned(),
                options,
                limits,
            };

            let outcome = runner::run(
                &request.engine_program,
                engine_args,
                &engine_req,
                cancel.clone(),
                |event| match event {
                    Event::Progress { stage, .. } => on_event(ConvertEvent::Progress { stage }),
                    Event::Warning { code, message } => {
                        warnings.push(sanitize_warning(&code, &message));
                    }
                    Event::Artifact { .. } | Event::Result { .. } | Event::Capabilities { .. } => {}
                },
            )
            .map_err(map_run_error)?;

            if cancel.is_cancelled() {
                return Err(ConvertError::Interrupted);
            }

            let artifact = outcome
                .artifacts
                .iter()
                .find(|artifact| artifact.format == target_str)
                .ok_or(ConvertError::Failed)?;

            let artifact_path = PathBuf::from(&artifact.path);

            // Step 4: Metadata stamp for docx and epub
            let timestamp = package_meta::conversion_time().map_err(|_| ConvertError::Failed)?;
            if writer == WriterEdge::PandocDocx {
                package_meta::stamp_docx(&artifact_path, timestamp)
                    .map_err(|_| ConvertError::Failed)?;
            } else if writer == WriterEdge::PandocEpub {
                package_meta::stamp_epub(&artifact_path, timestamp)
                    .map_err(|_| ConvertError::Failed)?;
            }

            artifact_path
        }
    };

    // Step 5: Promotion (Commit point)
    if cancel.is_cancelled() {
        return Err(ConvertError::Interrupted);
    }
    workspace
        .promote(&artifact_path, &request.output, request.overwrite)
        .map_err(|error| match error {
            WorkspaceError::DestinationExists => ConvertError::DestinationExists,
            other => map_workspace_error(other),
        })?;

    Ok(())
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

    let mut source = fs::File::open(input).map_err(|_| ConvertError::InputIo)?;
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

    let is_archive = input_format.eq_ignore_ascii_case("docx")
        || input_format.eq_ignore_ascii_case("epub")
        || input_format.eq_ignore_ascii_case("zip");

    if is_archive {
        archive::preflight_archive(&dest_path, limits).map_err(|err| match err {
            ArchiveError::EntryCountExceeded { .. }
            | ArchiveError::DecompressedSizeExceeded { .. } => {
                let _ = fs::remove_file(&dest_path);
                ConvertError::LimitExceeded
            }
            ArchiveError::Encrypted
            | ArchiveError::DuplicateEntryName { .. }
            | ArchiveError::EntryCountMismatch { .. }
            | ArchiveError::InvalidEntryName { .. } => {
                let _ = fs::remove_file(&dest_path);
                ConvertError::Failed
            }
            ArchiveError::Io(_) | ArchiveError::Zip(_) => {
                let _ = fs::remove_file(&dest_path);
                ConvertError::InputIo
            }
        })?;
    }

    Ok(dest_path)
}

/// The outcome of converting an archive input to AriadShift IR.
#[derive(Clone, Debug, PartialEq)]
pub struct ArchiveToIrOutput {
    pub document: ariad_core::ir::Document,
    pub warnings: Vec<ConvertWarning>,
}

/// Converts an untrusted archive input (DOCX or EPUB) to AriadShift IR.
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
    if cancel.is_cancelled() {
        return Err(ConvertError::Interrupted);
    }

    let copied_path = copy_and_preflight_input(input, input_format, workspace, limits)?;

    let request = Request::Convert {
        protocol: PROTOCOL.to_owned(),
        job: Uuid::new_v4().simple().to_string(),
        input: Input {
            path: copied_path.to_string_lossy().into_owned(),
            format: input_format.to_owned(),
        },
        output: Output {
            dir: workspace.output_dir().to_string_lossy().into_owned(),
            format: "ariad-ir+json".to_owned(),
        },
        work_dir: workspace.work_dir().to_string_lossy().into_owned(),
        options: BTreeMap::new(),
        limits: limits.clone(),
    };

    let mut warnings = Vec::new();
    let engine_args = [OsString::from("__engine"), OsString::from("pandoc")];
    let outcome = runner::run(
        engine_program,
        &engine_args,
        &request,
        cancel.clone(),
        |event| {
            if let Event::Warning { code, message } = event {
                warnings.push(sanitize_warning(&code, &message));
            }
        },
    )
    .map_err(map_run_error)?;

    if cancel.is_cancelled() {
        return Err(ConvertError::Interrupted);
    }

    let artifact = outcome
        .artifacts
        .iter()
        .find(|a| a.format == "ariad-ir+json")
        .ok_or(ConvertError::Failed)?;

    let ir_file = fs::File::open(&artifact.path).map_err(|_| ConvertError::Failed)?;
    let buf_reader = std::io::BufReader::new(ir_file);
    let doc = crate::ir_io::read(buf_reader, limits).map_err(|err| match err {
        crate::ir_io::ReadError::ByteLimitExceeded { .. }
        | crate::ir_io::ReadError::DepthExceeded(_)
        | crate::ir_io::ReadError::BlockLimitExceeded { .. } => ConvertError::LimitExceeded,
        _ => ConvertError::Failed,
    })?;

    Ok(ArchiveToIrOutput {
        document: doc,
        warnings,
    })
}

fn map_run_error(error: RunError) -> ConvertError {
    match error {
        RunError::Cancelled => ConvertError::Interrupted,
        RunError::EngineFailed {
            code: ErrorCode::ToolMissing | ErrorCode::ToolVersion,
            ..
        } => ConvertError::ToolMissing,
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

fn safe_warning(warning: &Warning) -> ConvertWarning {
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
        _ => "The document could not be converted without a warning.",
    };
    msg.to_owned()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{
        ConvertError, DocumentFormat, detect_format_from_path, is_supported_route,
        paths_refer_to_same_file, safe_warning, sanitize_warning,
    };
    use ariad_core::warning::{Warning, WarningCode};

    #[test]
    fn detects_supported_document_formats() {
        assert_eq!(
            detect_format_from_path(Path::new("notes.md")),
            Some(DocumentFormat::Markdown)
        );
        assert_eq!(
            detect_format_from_path(Path::new("notes.MARKDOWN")),
            Some(DocumentFormat::Markdown)
        );
        assert_eq!(
            detect_format_from_path(Path::new("page.html")),
            Some(DocumentFormat::Html)
        );
        assert_eq!(
            detect_format_from_path(Path::new("page.HTM")),
            Some(DocumentFormat::Html)
        );
        assert_eq!(
            detect_format_from_path(Path::new("doc.docx")),
            Some(DocumentFormat::Docx)
        );
        assert_eq!(
            detect_format_from_path(Path::new("book.epub")),
            Some(DocumentFormat::Epub)
        );
        assert_eq!(detect_format_from_path(Path::new("file.pdf")), None);
    }

    #[test]
    fn supports_all_distinct_format_pairs_and_refuses_same_format() {
        let all_formats = [
            DocumentFormat::Markdown,
            DocumentFormat::Html,
            DocumentFormat::Docx,
            DocumentFormat::Epub,
        ];
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
        use super::{
            ConvertRequest, DocumentFormat, ReaderEdge, Route, WriterEdge, convert_for_route,
        };

        let temp_dir = tempfile::tempdir().unwrap();
        let in_file = temp_dir.path().join("input.md");
        std::fs::write(&in_file, b"# Custom Dispatch Test\n\nSome text here.\n").unwrap();

        let out_file = temp_dir.path().join("output.txt");
        let cancel = tokio_util::sync::CancellationToken::new();

        // Target format string says "html", but route specifies WriterEdge::NativeMarkdown
        let mut req = ConvertRequest::new(&in_file, &out_file, "html", "ashift");
        req.overwrite = true;

        let swapped_route = Route {
            input_format: DocumentFormat::Markdown,
            output_format: DocumentFormat::Html,
            reader: ReaderEdge::NativeMarkdown,
            writer: WriterEdge::NativeMarkdown, // Swapped writer edge!
        };

        let report = convert_for_route(
            &req,
            &swapped_route,
            &[],
            ariad_core::limits::Limits::local(),
            cancel,
            |_| {},
        )
        .expect("conversion through swapped route succeeds");

        let content = std::fs::read_to_string(&report.output).unwrap();
        // Since writer edge is NativeMarkdown, output is Markdown (# Custom Dispatch Test), NOT HTML (<!DOCTYPE html>)
        assert!(content.contains("# Custom Dispatch Test"));
        assert!(!content.contains("<!DOCTYPE html>"));
    }

    #[test]
    fn convert_for_route_rejects_same_format_route() {
        use super::{
            ConvertError, ConvertRequest, DocumentFormat, ReaderEdge, Route, WriterEdge,
            convert_for_route,
        };

        let temp_dir = tempfile::tempdir().unwrap();
        let in_file = temp_dir.path().join("input.md");
        std::fs::write(&in_file, b"# Same format\n").unwrap();
        let out_file = temp_dir.path().join("output.out");

        let req = ConvertRequest::new(&in_file, &out_file, "md", "ashift");
        let same_route = Route {
            input_format: DocumentFormat::Markdown,
            output_format: DocumentFormat::Markdown,
            reader: ReaderEdge::NativeMarkdown,
            writer: WriterEdge::NativeMarkdown,
        };

        let result = convert_for_route(
            &req,
            &same_route,
            &[],
            ariad_core::limits::Limits::local(),
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
}
