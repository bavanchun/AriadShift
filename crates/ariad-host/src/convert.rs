use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use ariad_core::{
    limits::Limits,
    protocol::{ErrorCode, Event, Input, Output, PROTOCOL, Request},
    reader::markdown,
    warning::{Warning, WarningCode},
};
use thiserror::Error;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::{
    assets, docx_meta,
    runner::{self, RunError},
    workspace::{Workspace, WorkspaceError},
};

const SUPPORTED_ROUTE: &str = "md/markdown -> docx";

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

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum ConvertError {
    #[error("unsupported conversion route; supported route: {SUPPORTED_ROUTE}")]
    UnsupportedRoute,
    #[error("the input file could not be read")]
    InputIo,
    #[error("conversion limit exceeded")]
    LimitExceeded,
    #[error("Pandoc is missing or unsupported. Run `just pandoc` or set ASHIFT_PANDOC.")]
    ToolMissing,
    #[error("the destination already exists; pass --overwrite to replace it")]
    DestinationExists,
    #[error("conversion was interrupted")]
    Interrupted,
    #[error("conversion failed")]
    Failed,
}

impl ConvertError {
    #[must_use]
    pub const fn exit_code(&self) -> u8 {
        match self {
            Self::UnsupportedRoute => 3,
            Self::LimitExceeded => 4,
            Self::ToolMissing => 5,
            Self::DestinationExists => 6,
            Self::Interrupted => 130,
            Self::InputIo | Self::Failed => 1,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConvertEvent {
    Progress { stage: String },
}

/// Converts a supported Markdown input with custom engine args and limits.
#[doc(hidden)]
// Internal test hook exposing engine execution parameters for custom harness tests.
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
    mut on_event: F,
) -> Result<ConvertReport, ConvertError>
where
    F: FnMut(ConvertEvent),
{
    if cancel.is_cancelled() {
        return Err(ConvertError::Interrupted);
    }
    if !is_markdown_docx_route(input, target_format) {
        return Err(ConvertError::UnsupportedRoute);
    }

    let started = Instant::now();
    let input_text = fs::read_to_string(input).map_err(|_| ConvertError::InputIo)?;
    let mut parsed = markdown::read(&input_text, &limits).map_err(|error| match error {
        markdown::ReadError::NestingTooDeep { .. }
        | markdown::ReadError::TooManyBlocks { .. }
        | markdown::ReadError::InputTooLarge { .. }
        | markdown::ReadError::InvalidLimits(_) => ConvertError::LimitExceeded,
    })?;

    let base_dir = input.parent().unwrap_or_else(|| Path::new("."));
    let asset_warnings = assets::resolve(&mut parsed.document, base_dir, &limits)
        .map_err(|_| ConvertError::InputIo)?;
    let mut warnings = parsed
        .warnings
        .iter()
        .chain(&asset_warnings)
        .map(safe_warning)
        .collect::<Vec<_>>();

    let mut workspace = Workspace::new().map_err(|_| ConvertError::Failed)?;
    let result = convert_in_workspace(
        &mut workspace,
        &parsed.document,
        output,
        engine_program,
        engine_args,
        overwrite,
        limits,
        cancel,
        &mut warnings,
        &mut on_event,
    );
    let cleanup = workspace.close();
    result?;
    cleanup.map_err(|_| ConvertError::Failed)?;

    Ok(ConvertReport {
        output: output.to_path_buf(),
        warnings,
        elapsed: started.elapsed(),
    })
}

/// Converts a supported Markdown input into a DOCX through the configured engine process.
pub fn convert<F>(
    input: &Path,
    output: &Path,
    target_format: &str,
    engine_program: &Path,
    overwrite: bool,
    cancel: CancellationToken,
    on_event: F,
) -> Result<ConvertReport, ConvertError>
where
    F: FnMut(ConvertEvent),
{
    let args = [OsString::from("__engine"), OsString::from("pandoc")];
    convert_custom(
        input,
        output,
        target_format,
        engine_program,
        &args,
        overwrite,
        Limits::local(),
        cancel,
        on_event,
    )
}

fn is_markdown_docx_route(input: &Path, target_format: &str) -> bool {
    let source_is_markdown = input
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("md") || extension.eq_ignore_ascii_case("markdown")
        });
    source_is_markdown && target_format.eq_ignore_ascii_case("docx")
}

// Private workspace execution helper coordinating individual paths, buffers, and flags.
#[allow(clippy::too_many_arguments)]
fn convert_in_workspace<F>(
    workspace: &mut Workspace,
    document: &ariad_core::ir::Document,
    output: &Path,
    engine_program: &Path,
    engine_args: &[OsString],
    overwrite: bool,
    limits: Limits,
    cancel: CancellationToken,
    warnings: &mut Vec<ConvertWarning>,
    on_event: &mut F,
) -> Result<(), ConvertError>
where
    F: FnMut(ConvertEvent),
{
    let ir_path = workspace.input_dir().join("document.ir.json");
    let ir_file = fs::File::create(&ir_path).map_err(|_| ConvertError::Failed)?;
    serde_json::to_writer(ir_file, document).map_err(|_| ConvertError::Failed)?;

    let request = Request::Convert {
        protocol: PROTOCOL.to_owned(),
        job: Uuid::new_v4().simple().to_string(),
        input: Input {
            path: ir_path.to_string_lossy().into_owned(),
            format: "ariad-ir+json".to_owned(),
        },
        output: Output {
            dir: workspace.output_dir().to_string_lossy().into_owned(),
            format: "docx".to_owned(),
        },
        work_dir: workspace.work_dir().to_string_lossy().into_owned(),
        options: BTreeMap::new(),
        limits,
    };
    let outcome = runner::run(
        engine_program,
        engine_args,
        &request,
        cancel.clone(),
        |event| match event {
            Event::Progress { stage, .. } => on_event(ConvertEvent::Progress { stage }),
            Event::Warning { code, message: _ } => {
                let code = code.strip_prefix("ir_").unwrap_or(&code).to_owned();
                warnings.push(ConvertWarning {
                    code,
                    message: "The document could not be converted without a warning.".to_owned(),
                });
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
        .find(|artifact| artifact.format == "docx")
        .ok_or(ConvertError::Failed)?;
    let timestamp = docx_meta::conversion_time().map_err(|_| ConvertError::Failed)?;
    docx_meta::stamp(&artifact.path, timestamp).map_err(|_| ConvertError::Failed)?;
    workspace
        .promote(&artifact.path, output, overwrite)
        .map_err(|error| match error {
            WorkspaceError::DestinationExists => ConvertError::DestinationExists,
            other => map_workspace_error(other),
        })?;
    Ok(())
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
    let message = match warning.code {
        WarningCode::FrontMatterInvalid => "Invalid document metadata was ignored.",
        WarningCode::FrontMatterKeyIgnored => "Unsupported document metadata was ignored.",
        WarningCode::FrontMatterAliasRejected => "Unsafe document metadata was ignored.",
        WarningCode::UnsupportedNode => "Unsupported Markdown content was omitted.",
        WarningCode::RawDropped => "Raw Markdown content is not preserved in DOCX.",
        WarningCode::LinkDropped => "An unsupported link was omitted.",
        WarningCode::ImageNotEmbedded => "An image was not embedded in the DOCX.",
        WarningCode::FootnoteMissing => "A footnote reference could not be resolved.",
        WarningCode::FootnoteUnused => "An unused footnote was omitted.",
    };
    ConvertWarning {
        code,
        message: message.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{ConvertError, is_markdown_docx_route};

    #[test]
    fn accepts_only_the_phase_zero_markdown_to_docx_route() {
        assert!(is_markdown_docx_route(Path::new("notes.md"), "docx"));
        assert!(is_markdown_docx_route(Path::new("notes.MARKDOWN"), "DOCX"));
        assert!(!is_markdown_docx_route(Path::new("notes.html"), "docx"));
        assert!(!is_markdown_docx_route(Path::new("notes.md"), "html"));
    }

    #[test]
    fn maps_conversion_errors_to_the_documented_exit_codes() {
        let cases = [
            (ConvertError::InputIo, 1),
            (ConvertError::Failed, 1),
            (ConvertError::UnsupportedRoute, 3),
            (ConvertError::LimitExceeded, 4),
            (ConvertError::ToolMissing, 5),
            (ConvertError::DestinationExists, 6),
            (ConvertError::Interrupted, 130),
        ];
        for (error, expected) in cases {
            assert_eq!(error.exit_code(), expected);
        }
    }
}
