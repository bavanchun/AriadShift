//! `ashift convert` with several inputs.

use std::{
    collections::HashSet,
    io::{self, IsTerminal, Write},
    path::PathBuf,
    process::ExitCode,
};

use ariad_host::{
    convert::{
        ConvertError, ConvertEvent, ConvertOutput, ConvertReport, ConvertRequest, convert_in,
    },
    session::{PoolConfig, SessionPool},
};
use serde::Serialize;

use crate::{ConvertArgs, render::escape_control_chars, run_blocking_exit, target_extension};

/// The JSON document `ashift convert --json` prints for several inputs.
#[derive(Serialize)]
struct BatchOutput {
    results: Vec<BatchResult>,
}

/// One input's outcome: the single-input report fields on success, an error otherwise.
#[derive(Serialize)]
struct BatchResult {
    input: String,
    ok: bool,
    #[serde(flatten)]
    report: Option<ConvertOutput>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<BatchError>,
}

#[derive(Serialize)]
struct BatchError {
    code: &'static str,
    exit_code: u8,
    message: String,
}

pub(crate) fn run(args: ConvertArgs) -> ExitCode {
    if let Some(directory) = &args.output
        && !directory.is_dir()
    {
        let _ = writeln!(
            io::stderr(),
            "ashift: with several inputs, -o must be an existing directory"
        );
        return ExitCode::from(2);
    }
    let extension = target_extension(&args.target_format).to_owned();
    let outputs: Vec<PathBuf> = args
        .inputs
        .iter()
        .map(|input| match &args.output {
            // Same name as the single-input default, placed in the directory.
            Some(directory) => directory
                .join(input.file_name().unwrap_or(input.as_os_str()))
                .with_extension(&extension),
            None => input.with_extension(&extension),
        })
        .collect();
    let mut seen = HashSet::new();
    if let Some(duplicate) = outputs.iter().find(|output| !seen.insert(*output)) {
        let _ = writeln!(
            io::stderr(),
            "ashift: several inputs would write {}",
            escape_control_chars(&duplicate.display().to_string())
        );
        return ExitCode::from(2);
    }
    let engine_program = match std::env::current_exe() {
        Ok(path) => path,
        Err(_) => {
            let _ = writeln!(io::stderr(), "ashift: conversion failed");
            return ExitCode::from(1);
        }
    };
    let requests: Vec<ConvertRequest> = args
        .inputs
        .iter()
        .zip(outputs)
        .map(|(input, output)| ConvertRequest {
            input: input.clone(),
            output,
            target_format: args.target_format.clone(),
            profile: args.profile.into(),
            overwrite: args.overwrite,
            engine_program: engine_program.clone(),
            title_fallback: None,
            asset_base_dir: None,
        })
        .collect();
    let is_json = args.json;
    let stderr_is_terminal = io::stderr().is_terminal();

    run_blocking_exit(
        "conversion",
        move |cancel| {
            let pool = SessionPool::new(PoolConfig::default());
            let mut results = Vec::with_capacity(requests.len());
            for request in &requests {
                let result = if cancel.is_cancelled() {
                    Err(ConvertError::Interrupted)
                } else {
                    convert_in(&pool, request, cancel.clone(), |event| match event {
                        ConvertEvent::Progress { stage } if stderr_is_terminal => {
                            let _ = writeln!(io::stderr(), "progress: {stage}");
                        }
                        ConvertEvent::Progress { .. } => {}
                    })
                };
                report_one(request, &result, is_json);
                let interrupted = matches!(result, Err(ConvertError::Interrupted));
                results.push((request.input.clone(), result));
                if interrupted {
                    break;
                }
            }
            pool.shutdown();
            Ok(results)
        },
        move |results| finish(&results, is_json),
    )
}

/// Prints one input's warnings and, in human mode, its output path or error as it completes.
fn report_one(
    request: &ConvertRequest,
    result: &Result<ConvertReport, ConvertError>,
    is_json: bool,
) {
    let input = escape_control_chars(&request.input.display().to_string());
    match result {
        Ok(report) => {
            for warning in &report.warnings {
                let _ = writeln!(
                    io::stderr(),
                    "warning[{}]: {input}: {}",
                    warning.code,
                    warning.message
                );
            }
            if !is_json {
                let _ = writeln!(
                    io::stdout().lock(),
                    "{}",
                    escape_control_chars(&report.output.display().to_string())
                );
            }
        }
        Err(error) if !is_json => {
            let _ = writeln!(io::stderr(), "ashift: {input}: {}", error_message(error));
        }
        Err(_) => {}
    }
}

fn error_message(error: &ConvertError) -> String {
    match error {
        ConvertError::Failed => "conversion failed".to_owned(),
        error => error.to_string(),
    }
}

/// Prints the JSON document and picks the exit code: 130 after an interruption, otherwise the
/// first failure's code, or 0 when every input converted.
fn finish(results: &[(PathBuf, Result<ConvertReport, ConvertError>)], is_json: bool) -> ExitCode {
    if is_json {
        let output = BatchOutput {
            results: results
                .iter()
                .map(|(input, result)| BatchResult {
                    input: input.display().to_string(),
                    ok: result.is_ok(),
                    report: result.as_ref().ok().map(ConvertOutput::from),
                    error: result.as_ref().err().map(|error| BatchError {
                        code: error.code(),
                        exit_code: error.exit_code(),
                        message: error_message(error),
                    }),
                })
                .collect(),
        };
        let written = serde_json::to_vec_pretty(&output)
            .map_err(io::Error::other)
            .and_then(|mut bytes| {
                bytes.push(b'\n');
                let mut stdout = io::stdout().lock();
                stdout.write_all(&bytes)?;
                stdout.flush()
            });
        if written.is_err() {
            let _ = writeln!(io::stderr(), "ashift: could not write to stdout");
            return ExitCode::from(ConvertError::OutputIo.exit_code());
        }
    }
    let errors = results
        .iter()
        .filter_map(|(_, result)| result.as_ref().err());
    let mut first = None;
    for error in errors {
        if matches!(error, ConvertError::Interrupted) {
            return ExitCode::from(error.exit_code());
        }
        first.get_or_insert(error.exit_code());
    }
    ExitCode::from(first.unwrap_or(0))
}
