use std::{
    io::{self, IsTerminal, Write},
    path::PathBuf,
    process::ExitCode,
};

#[cfg(debug_assertions)]
use std::time::Duration;

use ariad_host::convert::{ConvertError, ConvertEvent, Profile};
use clap::{Args, Parser, Subcommand};
use tokio_util::sync::CancellationToken;

mod render;

/// Document conversion engine and CLI tool.
#[derive(Parser)]
#[command(
    name = "ashift",
    version,
    about = "Document conversion engine and CLI tool"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Convert a document between supported formats (Markdown, HTML, DOCX, EPUB).
    Convert(ConvertArgs),
    /// Inspect document format, metrics, structure counts, and reachable routes.
    Inspect(InspectArgs),
    /// Plan and explain conversion route, metrics score, and alternatives without converting.
    Plan(PlanArgs),
    /// List available transformation engines, licenses, statuses, and supported routes.
    Engines(EnginesArgs),
    /// Check toolchain dependencies, workspace permissions, and engine health.
    Doctor(DoctorArgs),
    #[command(name = "__ir", hide = true)]
    Ir(IrArgs),
    #[command(name = "__write", hide = true)]
    Write(WriteArgs),
    #[command(name = "__engine", hide = true)]
    Engine {
        #[command(subcommand)]
        command: EngineCommand,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, clap::ValueEnum)]
#[clap(rename_all = "lower")]
enum CliProfile {
    /// Balanced fidelity and editability (default).
    Editable,
    /// Preserves exact layout and structure where possible.
    Faithful,
    /// Fast execution speed.
    Fast,
    /// Prunes cloud edges; routes strictly locally.
    Private,
}

impl From<CliProfile> for Profile {
    fn from(p: CliProfile) -> Self {
        match p {
            CliProfile::Editable => Self::Editable,
            CliProfile::Faithful => Self::Faithful,
            CliProfile::Fast => Self::Fast,
            CliProfile::Private => Self::Private,
        }
    }
}

impl From<Profile> for CliProfile {
    fn from(p: Profile) -> Self {
        match p {
            Profile::Editable => Self::Editable,
            Profile::Faithful => Self::Faithful,
            Profile::Fast => Self::Fast,
            Profile::Private => Self::Private,
        }
    }
}

/// Arguments for converting a document between supported formats.
#[derive(Args)]
struct ConvertArgs {
    /// Input document file path.
    #[arg(value_name = "INPUT")]
    input: PathBuf,
    /// Target output format (md, html, docx, epub).
    #[arg(long = "to", value_name = "FORMAT")]
    target_format: String,
    /// Output file path (defaults to input stem with target extension).
    #[arg(short, long, value_name = "OUTPUT")]
    output: Option<PathBuf>,
    /// Optimization profile for route selection.
    #[arg(long, value_enum, default_value_t = CliProfile::Editable)]
    profile: CliProfile,
    /// Overwrite existing destination file if present.
    #[arg(long)]
    overwrite: bool,
    /// Output structured conversion report as JSON to stdout.
    #[arg(long)]
    json: bool,
}

/// Hidden command to convert an input document to AriadShift IR.
#[derive(Args)]
struct IrArgs {
    /// Input document file path.
    #[arg(value_name = "INPUT")]
    input: PathBuf,
    /// Output IR JSON file path.
    #[arg(short, long, value_name = "OUTPUT")]
    output: PathBuf,
    /// Overwrite destination file if present.
    #[arg(long)]
    overwrite: bool,
}

/// Arguments for writing an AriadShift IR document to a target format.
///
/// If the IR document does not contain an explicit metadata title, target formats that
/// support or require titles (including HTML, DOCX, and EPUB) fall back to using the input filename
/// stem (stripping any `.ir` suffix, e.g. `doc.ir.json` -> `doc`).
#[derive(Args)]
struct WriteArgs {
    /// Input AriadShift IR document file path.
    #[arg(value_name = "INPUT")]
    input: PathBuf,
    /// Target format to write (md, html, docx, epub).
    #[arg(long = "to", value_name = "FORMAT")]
    target_format: String,
    /// Output file path.
    #[arg(short, long, value_name = "OUTPUT")]
    output: PathBuf,
    /// Optimization profile.
    #[arg(long, value_enum, default_value_t = CliProfile::Editable)]
    profile: CliProfile,
    /// Overwrite destination file if present.
    #[arg(long)]
    overwrite: bool,
}

/// Arguments for inspecting an input document file.
#[derive(Args)]
struct InspectArgs {
    /// Input document file path to inspect.
    #[arg(value_name = "INPUT")]
    input: PathBuf,
    /// Output structured inspection report as JSON to stdout.
    #[arg(long)]
    json: bool,
}

/// Arguments for generating a conversion plan.
#[derive(Args)]
struct PlanArgs {
    /// Input document file path.
    #[arg(value_name = "INPUT")]
    input: PathBuf,
    /// Target output format to plan conversion route towards.
    #[arg(long = "to", value_name = "FORMAT")]
    target_format: String,
    /// Optimization profile for plan route selection.
    #[arg(long, value_enum, default_value_t = CliProfile::Editable)]
    profile: CliProfile,
    /// Output structured plan as JSON to stdout.
    #[arg(long)]
    json: bool,
}

/// Arguments for listing engine statuses and supported routes.
#[derive(Args)]
struct EnginesArgs {
    /// Output engine statuses and routes as JSON to stdout.
    #[arg(long)]
    json: bool,
}

/// Arguments for running environment and dependency diagnostics.
#[derive(Args)]
struct DoctorArgs {
    /// Output diagnostic checks as JSON to stdout.
    #[arg(long)]
    json: bool,
}

#[derive(Subcommand)]
enum EngineCommand {
    Pandoc,
}

fn main() -> ExitCode {
    match Cli::try_parse() {
        Ok(cli) => match cli.command {
            Some(Command::Convert(args)) => run_convert(args),
            Some(Command::Inspect(args)) => run_inspect(args),
            Some(Command::Plan(args)) => run_plan(args),
            Some(Command::Engines(args)) => run_engines(args),
            Some(Command::Doctor(args)) => run_doctor(args),
            Some(Command::Ir(args)) => run_ir(args),
            Some(Command::Write(args)) => run_write(args),
            Some(Command::Engine {
                command: EngineCommand::Pandoc,
            }) => serve_pandoc(),
            None => ExitCode::SUCCESS,
        },
        Err(error) => {
            let code = error.exit_code().try_into().unwrap_or(2);
            let _ = error.print();
            ExitCode::from(code)
        }
    }
}

fn run_blocking<T, F, R>(command_name: &'static str, f: F, render: R) -> ExitCode
where
    T: Send + 'static,
    F: FnOnce(CancellationToken) -> Result<T, ConvertError> + Send + 'static,
    R: FnOnce(T) -> Result<(), ConvertError> + Send + 'static,
{
    let cancel = CancellationToken::new();
    let task_cancel = cancel.clone();
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => {
            let _ = writeln!(io::stderr(), "ashift: {command_name} failed");
            return ExitCode::from(1);
        }
    };

    runtime.block_on(async move {
        let task = tokio::task::spawn_blocking(move || f(task_cancel));
        tokio::pin!(task);

        tokio::select! {
            result = &mut task => {
                match result {
                    Ok(Ok(val)) => match render(val) {
                        Ok(()) => ExitCode::SUCCESS,
                        Err(ConvertError::Failed) => {
                            let _ = writeln!(io::stderr(), "ashift: {command_name} failed");
                            ExitCode::from(1)
                        }
                        Err(err) => {
                            let _ = writeln!(io::stderr(), "ashift: {err}");
                            ExitCode::from(err.exit_code())
                        }
                    },
                    Ok(Err(ConvertError::Interrupted)) => {
                        let _ = writeln!(io::stderr(), "ashift: {command_name} was interrupted");
                        ExitCode::from(130)
                    }
                    Ok(Err(ConvertError::Failed)) => {
                        let _ = writeln!(io::stderr(), "ashift: {command_name} failed");
                        ExitCode::from(1)
                    }
                    Ok(Err(err)) => {
                        let _ = writeln!(io::stderr(), "ashift: {err}");
                        ExitCode::from(err.exit_code())
                    }
                    Err(_) => {
                        let _ = writeln!(io::stderr(), "ashift: {command_name} failed");
                        ExitCode::from(1)
                    }
                }
            }
            signal = interrupt() => {
                cancel.cancel();
                let task_res = task.await;
                if let Ok(Ok(val)) = task_res {
                    match render(val) {
                        Ok(()) => ExitCode::SUCCESS,
                        Err(ConvertError::Failed) => {
                            let _ = writeln!(io::stderr(), "ashift: {command_name} failed");
                            ExitCode::from(1)
                        }
                        Err(err) => {
                            let _ = writeln!(io::stderr(), "ashift: {err}");
                            ExitCode::from(err.exit_code())
                        }
                    }
                } else {
                    match signal {
                        Ok(()) => {
                            let _ = writeln!(io::stderr(), "ashift: {command_name} was interrupted");
                            ExitCode::from(130)
                        }
                        Err(_) => {
                            let _ = writeln!(io::stderr(), "ashift: could not install the interrupt handler");
                            ExitCode::from(1)
                        }
                    }
                }
            }
        }
    })
}

fn run_convert(args: ConvertArgs) -> ExitCode {
    let is_json = args.json;
    let output = match args.output {
        Some(path) => path,
        None => {
            let ext = match ariad_host::convert::DocumentFormat::parse(&args.target_format) {
                Some(fmt) => fmt.default_extension(),
                None => args.target_format.as_str(),
            };
            args.input.with_extension(ext)
        }
    };
    let engine_program = match std::env::current_exe() {
        Ok(path) => path,
        Err(_) => {
            let _ = writeln!(io::stderr(), "ashift: conversion failed");
            return ExitCode::from(1);
        }
    };
    let stderr_is_terminal = io::stderr().is_terminal();
    let request = ariad_host::convert::ConvertRequest {
        input: args.input,
        output,
        target_format: args.target_format,
        profile: args.profile.into(),
        overwrite: args.overwrite,
        engine_program,
        title_fallback: None,
    };

    run_blocking(
        "conversion",
        move |cancel| {
            ariad_host::convert::convert(&request, cancel, |event| match event {
                ConvertEvent::Progress { stage } if stderr_is_terminal => {
                    let _ = writeln!(io::stderr(), "progress: {stage}");
                }
                ConvertEvent::Progress { .. } => {}
            })
        },
        move |report| {
            for warning in &report.warnings {
                let _ = writeln!(
                    io::stderr(),
                    "warning[{}]: {}",
                    warning.code,
                    warning.message
                );
            }
            if is_json {
                render::render_convert_json(&report).map_err(|_| ConvertError::OutputIo)
            } else {
                writeln!(
                    io::stdout().lock(),
                    "{}",
                    render::escape_control_chars(&report.output.display().to_string())
                )
                .map_err(|_| ConvertError::OutputIo)?;
                Ok(())
            }
        },
    )
}

fn run_inspect(args: InspectArgs) -> ExitCode {
    let engine_program = match std::env::current_exe() {
        Ok(path) => path,
        Err(_) => {
            let _ = writeln!(io::stderr(), "ashift: inspection failed");
            return ExitCode::from(1);
        }
    };
    let is_json = args.json;
    let input = args.input;
    let caps = match ariad_host::commands::active_capabilities() {
        Ok(c) => c,
        Err(e) => {
            let _ = writeln!(io::stderr(), "ashift: {e}");
            return ExitCode::from(e.exit_code());
        }
    };

    run_blocking(
        "inspection",
        move |cancel| {
            ariad_host::commands::inspect::inspect(&input, Some(&engine_program), &caps, cancel)
        },
        move |output| {
            for warning in &output.warnings {
                let _ = writeln!(
                    io::stderr(),
                    "warning[{}]: {}",
                    warning.code,
                    warning.message
                );
            }
            if is_json {
                render::render_inspect_json(&output).map_err(|_| ConvertError::OutputIo)
            } else {
                render::render_inspect_human(&output).map_err(|_| ConvertError::OutputIo)
            }
        },
    )
}

fn run_plan(args: PlanArgs) -> ExitCode {
    let engine_program = match std::env::current_exe() {
        Ok(path) => path,
        Err(_) => {
            let _ = writeln!(io::stderr(), "ashift: planning failed");
            return ExitCode::from(1);
        }
    };
    let is_json = args.json;
    let input = args.input;
    let target = args.target_format;
    let profile = args.profile.into();
    let caps = match ariad_host::commands::active_capabilities() {
        Ok(c) => c,
        Err(e) => {
            let _ = writeln!(io::stderr(), "ashift: {e}");
            return ExitCode::from(e.exit_code());
        }
    };

    run_blocking(
        "planning",
        move |cancel| {
            ariad_host::commands::plan::plan(
                &input,
                &target,
                profile,
                &caps,
                Some(&engine_program),
                cancel,
            )
        },
        move |output| {
            if is_json {
                render::render_plan_json(&output).map_err(|_| ConvertError::OutputIo)
            } else {
                render::render_plan_human(&output).map_err(|_| ConvertError::OutputIo)
            }
        },
    )
}

fn run_engines(args: EnginesArgs) -> ExitCode {
    let engine_program = match std::env::current_exe() {
        Ok(path) => path,
        Err(_) => {
            let _ = writeln!(io::stderr(), "ashift: engines query failed");
            return ExitCode::from(1);
        }
    };
    let is_json = args.json;
    let caps = match ariad_host::commands::active_capabilities() {
        Ok(c) => c,
        Err(e) => {
            let _ = writeln!(io::stderr(), "ashift: {e}");
            return ExitCode::from(e.exit_code());
        }
    };

    run_blocking(
        "engines query",
        move |cancel| ariad_host::commands::engines::engines(&engine_program, &caps, cancel),
        move |rows| {
            if is_json {
                render::render_engines_json(&rows).map_err(|_| ConvertError::OutputIo)
            } else {
                render::render_engines_human(&rows).map_err(|_| ConvertError::OutputIo)
            }
        },
    )
}

fn run_doctor(args: DoctorArgs) -> ExitCode {
    let engine_program = match std::env::current_exe() {
        Ok(path) => path,
        Err(_) => {
            let _ = writeln!(io::stderr(), "ashift: doctor check failed");
            return ExitCode::from(1);
        }
    };
    let is_json = args.json;

    run_blocking(
        "doctor check",
        move |cancel| ariad_host::commands::doctor::doctor(&engine_program, cancel),
        move |report| {
            let render_res = if is_json {
                render::render_doctor_json(&report)
            } else {
                render::render_doctor_human(&report)
            };
            if render_res.is_err() {
                return Err(ConvertError::OutputIo);
            }
            if let Some(failure) = report.failure() {
                return Err(failure);
            }
            Ok(())
        },
    )
}

fn run_ir(args: IrArgs) -> ExitCode {
    let engine_program = match std::env::current_exe() {
        Ok(path) => path,
        Err(_) => {
            let _ = writeln!(io::stderr(), "ashift: conversion failed");
            return ExitCode::from(1);
        }
    };
    let stderr_is_terminal = io::stderr().is_terminal();
    let request = ariad_host::convert::ConvertToIrRequest {
        input: args.input,
        output: args.output,
        overwrite: args.overwrite,
        engine_program,
    };

    run_blocking(
        "conversion",
        move |cancel| {
            ariad_host::convert::convert_to_ir(&request, cancel, |event| match event {
                ConvertEvent::Progress { stage } if stderr_is_terminal => {
                    let _ = writeln!(io::stderr(), "progress: {stage}");
                }
                ConvertEvent::Progress { .. } => {}
            })
        },
        move |report| {
            for warning in &report.warnings {
                let _ = writeln!(
                    io::stderr(),
                    "warning[{}]: {}",
                    warning.code,
                    warning.message
                );
            }
            writeln!(
                io::stdout().lock(),
                "{}",
                render::escape_control_chars(&report.output.display().to_string())
            )
            .map_err(|_| ConvertError::OutputIo)?;
            Ok(())
        },
    )
}

fn run_write(args: WriteArgs) -> ExitCode {
    let engine_program = match std::env::current_exe() {
        Ok(path) => path,
        Err(_) => {
            let _ = writeln!(io::stderr(), "ashift: conversion failed");
            return ExitCode::from(1);
        }
    };
    let stderr_is_terminal = io::stderr().is_terminal();
    let request = ariad_host::convert::WriteFromIrRequest {
        input: args.input,
        output: args.output,
        target_format: args.target_format,
        profile: args.profile.into(),
        overwrite: args.overwrite,
        engine_program,
        title_fallback: None,
    };

    run_blocking(
        "conversion",
        move |cancel| {
            ariad_host::convert::write_from_ir(&request, cancel, |event| match event {
                ConvertEvent::Progress { stage } if stderr_is_terminal => {
                    let _ = writeln!(io::stderr(), "progress: {stage}");
                }
                ConvertEvent::Progress { .. } => {}
            })
        },
        move |report| {
            for warning in &report.warnings {
                let _ = writeln!(
                    io::stderr(),
                    "warning[{}]: {}",
                    warning.code,
                    warning.message
                );
            }
            writeln!(
                io::stdout().lock(),
                "{}",
                render::escape_control_chars(&report.output.display().to_string())
            )
            .map_err(|_| ConvertError::OutputIo)?;
            Ok(())
        },
    )
}

#[cfg(not(windows))]
async fn interrupt() -> io::Result<()> {
    tokio::signal::ctrl_c().await
}

#[cfg(windows)]
async fn interrupt() -> io::Result<()> {
    let mut ctrl_break = tokio::signal::windows::ctrl_break()?;
    tokio::select! {
        signal = tokio::signal::ctrl_c() => signal,
        _ = ctrl_break.recv() => Ok(()),
    }
}

fn serve_pandoc() -> ExitCode {
    #[cfg(debug_assertions)]
    if test_engine_override() {
        return serve_test_override(io::stdin().lock(), io::stdout().lock());
    }
    ariad_host::engines::pandoc::serve(io::stdin().lock(), io::stdout().lock())
}

#[cfg(debug_assertions)]
fn test_engine_override() -> bool {
    std::env::var_os("ARIAD_TEST_ENGINE").is_some_and(|value| value == "hang")
}

#[cfg(debug_assertions)]
fn serve_test_override(mut input: impl io::BufRead, output: impl io::Write) -> ExitCode {
    let mut line = String::new();
    if input.read_line(&mut line).is_err() {
        return ExitCode::from(1);
    }
    if line.contains("\"describe\"") {
        let cursor = io::Cursor::new(line.into_bytes());
        return ariad_host::engines::pandoc::serve(cursor, output);
    }
    test_hang()
}

#[cfg(debug_assertions)]
fn test_hang() -> ExitCode {
    loop {
        std::thread::sleep(Duration::from_secs(60));
    }
}
