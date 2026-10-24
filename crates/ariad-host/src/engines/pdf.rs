//! `ashift __engine pdf`: reads the text layer of a PDF into IR JSON.
//!
//! The parser runs here, in a child process, so a malformed PDF can never take the host down.

use std::{
    io::{BufRead, Write},
    process::ExitCode,
};

use ariad_core::{
    format::Format,
    protocol::{ErrorCode, Event, Request, RouteCapability, ToolAvailability, ToolStatus},
};

use super::common::{self, EngineFailure};

const UNSUPPORTED_ROUTE: EngineFailure = EngineFailure::new(
    ErrorCode::UnsupportedRoute,
    "The PDF engine converts PDF to IR JSON only.",
);

/// Reads one engine request, emits NDJSON events, and returns the process exit status.
pub fn serve<R: BufRead, W: Write>(input: R, output: W) -> ExitCode {
    common::serve(input, output, execute)
}

fn capabilities() -> Event {
    Event::Capabilities {
        engine: super::PDF.id.to_owned(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
        tool: ToolStatus {
            name: super::PDF.id.to_owned(),
            version: Some(env!("CARGO_PKG_VERSION").to_owned()),
            status: if cfg!(feature = "pdf") {
                ToolAvailability::Found
            } else {
                ToolAvailability::Missing
            },
        },
        license: super::PDF.license.to_owned(),
        routes: super::PDF
            .routes
            .iter()
            .map(|(input, output)| RouteCapability {
                input: input.id().to_owned(),
                output: output.id().to_owned(),
            })
            .collect(),
        // The reader bounds its work through the request limits but cannot cap its own resident
        // memory.
        supports_session: false,
        enforces_memory_limit: false,
        models: None,
    }
}

fn execute<W: Write>(request: Request, output: &mut W) -> Result<(), EngineFailure> {
    match request {
        Request::Describe { protocol, .. } => {
            common::emit_capabilities(output, &protocol, capabilities())
        }
        Request::Convert {
            protocol,
            input,
            output: request_output,
            work_dir,
            limits,
            ..
        } => {
            let paths = common::validate_convert_paths(
                &protocol,
                &input,
                &request_output,
                &work_dir,
                &limits,
            )?;
            if input.format != Format::Pdf.id() || request_output.format != Format::AriadIrJson.id()
            {
                return Err(UNSUPPORTED_ROUTE);
            }
            convert(&paths, &limits, output)
        }
    }
}

#[cfg(feature = "pdf")]
fn convert<W: Write>(
    paths: &common::ConvertPaths,
    limits: &ariad_core::limits::Limits,
    output: &mut W,
) -> Result<(), EngineFailure> {
    use std::{
        fs::{self, File},
        io,
    };

    use ariad_core::reader::pdf::{self, ReadError};

    let bytes = crate::convert::read_bounded_file(&paths.input, limits.max_input_bytes).map_err(
        |error| match error {
            crate::convert::ConvertError::LimitExceeded => EngineFailure::limit_exceeded(),
            _ => EngineFailure::io(),
        },
    )?;
    let read = pdf::read(&bytes, limits).map_err(|error| match error {
        ReadError::Malformed => {
            EngineFailure::new(ErrorCode::EngineFailure, "The file is not a readable PDF.")
        }
        ReadError::Encrypted => EngineFailure::new(
            ErrorCode::EngineFailure,
            "The PDF is encrypted with a password; password-protected PDFs are not supported.",
        ),
        ReadError::InvalidLimits(_) => EngineFailure::invalid_request(),
        ReadError::InputTooLarge { .. }
        | ReadError::TooManyPages { .. }
        | ReadError::TooManyObjects { .. }
        | ReadError::DecompressedTooLarge { .. }
        | ReadError::NestingTooDeep { .. }
        | ReadError::TooManyBlocks { .. } => {
            EngineFailure::owned(ErrorCode::LimitExceeded, error.to_string())
        }
    })?;
    for warning in &read.warnings {
        common::emit_warning(output, warning)?;
    }

    let artifact = paths.output_dir.join("document.ir.json");
    let file = File::create(&artifact).map_err(|_| EngineFailure::io())?;
    let mut writer = io::BufWriter::new(file);
    serde_json::to_writer(&mut writer, &read.document).map_err(|_| EngineFailure::io())?;
    writer.flush().map_err(|_| EngineFailure::io())?;
    drop(writer);
    if !fs::metadata(&artifact).is_ok_and(|meta| meta.is_file()) {
        return Err(EngineFailure::io());
    }
    common::emit_success(output, &artifact, Format::AriadIrJson.id())
}

#[cfg(not(feature = "pdf"))]
fn convert<W: Write>(
    _paths: &common::ConvertPaths,
    _limits: &ariad_core::limits::Limits,
    _output: &mut W,
) -> Result<(), EngineFailure> {
    Err(EngineFailure::new(
        ErrorCode::ToolMissing,
        "This ashift build does not include the PDF reader.",
    ))
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use ariad_core::protocol::{Event, PROTOCOL, Request, ToolAvailability};

    #[test]
    fn describe_reports_the_pdf_route() {
        let request = Request::Describe {
            protocol: PROTOCOL.to_owned(),
            job: "describe".to_owned(),
        };
        let mut output = Vec::new();
        let exit = super::serve(
            Cursor::new(serde_json::to_vec(&request).unwrap()),
            &mut output,
        );
        assert_eq!(exit, std::process::ExitCode::SUCCESS);
        let first: Event =
            serde_json::from_slice(output.split(|b| *b == b'\n').next().unwrap()).unwrap();
        let Event::Capabilities {
            engine,
            tool,
            routes,
            ..
        } = first
        else {
            panic!("expected capabilities");
        };
        assert_eq!(engine, "pdf");
        assert_eq!(tool.status, ToolAvailability::Found);
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].input, "pdf");
        assert_eq!(routes[0].output, "ariad-ir+json");
    }

    #[cfg(feature = "pdf")]
    #[test]
    fn convert_reports_limits_and_warnings_through_the_protocol() {
        use crate::workspace::Workspace;
        use ariad_core::{
            limits::Limits,
            protocol::{ErrorCode, Input, Output},
        };
        use std::{collections::BTreeMap, path::Path};
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/pdf");
        for (name, pages, expected) in [
            (
                "pd-vi-qd-1922-2026.pdf",
                Some(1),
                Some(ErrorCode::LimitExceeded),
            ),
            ("vi-garden-image-only.pdf", None, None),
            ("malformed", None, Some(ErrorCode::EngineFailure)),
        ] {
            let workspace = Workspace::new().unwrap();
            let input = workspace.input_dir().join("document.pdf");
            if name == "malformed" {
                std::fs::write(&input, b"%PDF-1.7 invalid").unwrap();
            } else {
                std::fs::copy(root.join(name), &input).unwrap();
            }
            let request = Request::Convert {
                session_id: None,
                protocol: PROTOCOL.to_owned(),
                job: "pdf-convert".to_owned(),
                input: Input {
                    path: input.to_string_lossy().into_owned(),
                    format: "pdf".to_owned(),
                },
                output: Output {
                    dir: workspace.output_dir().to_string_lossy().into_owned(),
                    format: "ariad-ir+json".to_owned(),
                },
                work_dir: workspace.work_dir().to_string_lossy().into_owned(),
                options: BTreeMap::new(),
                limits: Limits {
                    max_pages: pages,
                    ..Limits::local()
                },
            };
            let mut output = Vec::new();
            let exit = super::serve(
                Cursor::new(serde_json::to_vec(&request).unwrap()),
                &mut output,
            );
            let events = output
                .split(|byte| *byte == b'\n')
                .filter(|line| !line.is_empty())
                .map(|line| serde_json::from_slice::<Event>(line).unwrap())
                .collect::<Vec<_>>();
            if let Some(expected) = expected {
                assert_eq!(exit, std::process::ExitCode::from(1));
                assert!(
                    matches!(events.last(), Some(Event::Result { ok: false, error: Some(error), .. }) if error.code == expected)
                );
                assert!(
                    !events
                        .iter()
                        .any(|event| matches!(event, Event::Artifact { .. }))
                );
            } else {
                assert_eq!(exit, std::process::ExitCode::SUCCESS);
                let warning = events.iter().position(|event| matches!(event, Event::Warning { code, .. } if code == "ir_ocr_required")).unwrap();
                let artifact = events
                    .iter()
                    .position(|event| matches!(event, Event::Artifact { .. }))
                    .unwrap();
                assert!(warning < artifact);
                assert!(matches!(
                    events.last(),
                    Some(Event::Result { ok: true, .. })
                ));
            }
        }
    }
}
