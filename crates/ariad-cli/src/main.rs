use std::{
    io::{self, IsTerminal, Write},
    path::PathBuf,
    process::ExitCode,
};

#[cfg(debug_assertions)]
use std::time::Duration;

use ariad_host::convert::{ConvertError, ConvertEvent, ConvertReport, Profile};
use clap::{Args, Parser, Subcommand};
use tokio_util::sync::CancellationToken;

#[derive(Parser)]
#[command(name = "ashift", version)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    Convert(ConvertArgs),
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
    Editable,
    Faithful,
    Fast,
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

#[derive(Args)]
struct ConvertArgs {
    #[arg(value_name = "INPUT")]
    input: PathBuf,
    #[arg(long = "to", value_name = "FORMAT")]
    target_format: String,
    #[arg(short, long, value_name = "OUTPUT")]
    output: Option<PathBuf>,
    #[arg(long, value_enum, default_value_t = CliProfile::Editable)]
    profile: CliProfile,
    #[arg(long)]
    overwrite: bool,
}

#[derive(Args)]
struct IrArgs {
    #[arg(value_name = "INPUT")]
    input: PathBuf,
    #[arg(short, long, value_name = "OUTPUT")]
    output: PathBuf,
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
    #[arg(value_name = "INPUT")]
    input: PathBuf,
    #[arg(long = "to", value_name = "FORMAT")]
    target_format: String,
    #[arg(short, long, value_name = "OUTPUT")]
    output: PathBuf,
    #[arg(long, value_enum, default_value_t = CliProfile::Editable)]
    profile: CliProfile,
    #[arg(long)]
    overwrite: bool,
}

#[derive(Subcommand)]
enum EngineCommand {
    Pandoc,
}

fn main() -> ExitCode {
    match Cli::try_parse() {
        Ok(cli) => match cli.command {
            Some(Command::Convert(args)) => run_convert(args),
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

fn run_blocking<F>(f: F) -> ExitCode
where
    F: FnOnce(CancellationToken) -> Result<ConvertReport, ConvertError> + Send + 'static,
{
    let cancel = CancellationToken::new();
    let task_cancel = cancel.clone();
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => {
            eprintln!("ashift: conversion failed");
            return ExitCode::from(1);
        }
    };

    runtime.block_on(async move {
        let task = tokio::task::spawn_blocking(move || f(task_cancel));
        tokio::pin!(task);

        tokio::select! {
            result = &mut task => finish_conversion(result),
            signal = interrupt() => {
                cancel.cancel();
                let task_result = task.await;
                match signal {
                    Ok(()) => {
                        match task_result {
                            Ok(Ok(report)) => finish_conversion(Ok(Ok(report))),
                            Ok(Err(ConvertError::Interrupted)) => {
                                eprintln!("ashift: conversion was interrupted");
                                ExitCode::from(130)
                            }
                            other => finish_conversion(other),
                        }
                    }
                    Err(_) => {
                        eprintln!("ashift: could not install the interrupt handler");
                        ExitCode::from(1)
                    }
                }
            }
        }
    })
}

fn run_convert(args: ConvertArgs) -> ExitCode {
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
            eprintln!("ashift: conversion failed");
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

    run_blocking(move |cancel| {
        ariad_host::convert::convert(&request, cancel, |event| match event {
            ConvertEvent::Progress { stage } if stderr_is_terminal => {
                eprintln!("progress: {stage}");
            }
            ConvertEvent::Progress { .. } => {}
        })
    })
}

fn run_ir(args: IrArgs) -> ExitCode {
    let engine_program = match std::env::current_exe() {
        Ok(path) => path,
        Err(_) => {
            eprintln!("ashift: conversion failed");
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

    run_blocking(move |cancel| {
        ariad_host::convert::convert_to_ir(&request, cancel, |event| match event {
            ConvertEvent::Progress { stage } if stderr_is_terminal => {
                eprintln!("progress: {stage}");
            }
            ConvertEvent::Progress { .. } => {}
        })
    })
}

fn run_write(args: WriteArgs) -> ExitCode {
    let engine_program = match std::env::current_exe() {
        Ok(path) => path,
        Err(_) => {
            eprintln!("ashift: conversion failed");
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

    run_blocking(move |cancel| {
        ariad_host::convert::write_from_ir(&request, cancel, |event| match event {
            ConvertEvent::Progress { stage } if stderr_is_terminal => {
                eprintln!("progress: {stage}");
            }
            ConvertEvent::Progress { .. } => {}
        })
    })
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

fn finish_conversion(
    result: Result<Result<ConvertReport, ConvertError>, tokio::task::JoinError>,
) -> ExitCode {
    match result {
        Ok(Ok(report)) => {
            for warning in report.warnings {
                eprintln!("warning[{}]: {}", warning.code, warning.message);
            }
            match writeln!(io::stdout().lock(), "{}", report.output.display()) {
                Ok(()) => ExitCode::SUCCESS,
                Err(_) => ExitCode::from(1),
            }
        }
        Ok(Err(error)) => {
            eprintln!("ashift: {error}");
            ExitCode::from(error.exit_code())
        }
        Err(_) => {
            eprintln!("ashift: conversion failed");
            ExitCode::from(1)
        }
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
