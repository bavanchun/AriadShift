use std::{
    io::{self, IsTerminal, Write},
    path::PathBuf,
    process::ExitCode,
};

#[cfg(debug_assertions)]
use std::time::Duration;

use ariad_host::convert::{ConvertError, ConvertEvent, ConvertReport};
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
    #[command(name = "__engine", hide = true)]
    Engine {
        #[command(subcommand)]
        command: EngineCommand,
    },
}

#[derive(Args)]
struct ConvertArgs {
    #[arg(value_name = "INPUT")]
    input: PathBuf,
    #[arg(long = "to", value_name = "FORMAT")]
    target_format: String,
    #[arg(short, long, value_name = "OUTPUT")]
    output: Option<PathBuf>,
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

fn run_convert(args: ConvertArgs) -> ExitCode {
    let output = args
        .output
        .unwrap_or_else(|| args.input.with_extension("docx"));
    let engine_program = match std::env::current_exe() {
        Ok(path) => path,
        Err(_) => {
            eprintln!("ashift: conversion failed");
            return ExitCode::from(1);
        }
    };
    let cancel = CancellationToken::new();
    let task_cancel = cancel.clone();
    let input = args.input;
    let target_format = args.target_format;
    let overwrite = args.overwrite;
    let stderr_is_terminal = io::stderr().is_terminal();

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
        let task = tokio::task::spawn_blocking(move || {
            ariad_host::convert::convert(
                &input,
                &output,
                &target_format,
                &engine_program,
                overwrite,
                task_cancel,
                |event| match event {
                    ConvertEvent::Progress { stage } if stderr_is_terminal => {
                        eprintln!("progress: {stage}");
                    }
                    ConvertEvent::Progress { .. } => {}
                },
            )
        });
        tokio::pin!(task);

        tokio::select! {
            result = &mut task => finish_conversion(result),
            signal = interrupt() => {
                cancel.cancel();
                match signal {
                    Ok(()) => {
                        let _ = task.await;
                        eprintln!("ashift: conversion was interrupted");
                        ExitCode::from(130)
                    }
                    Err(_) => {
                        let _ = task.await;
                        eprintln!("ashift: could not install the interrupt handler");
                        ExitCode::from(1)
                    }
                }
            }
        }
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
        return test_hang();
    }
    ariad_host::engines::pandoc::serve(io::stdin().lock(), io::stdout().lock())
}

#[cfg(debug_assertions)]
fn test_engine_override() -> bool {
    std::env::var_os("ARIAD_TEST_ENGINE").is_some_and(|value| value == "hang")
}

#[cfg(debug_assertions)]
fn test_hang() -> ExitCode {
    loop {
        std::thread::sleep(Duration::from_secs(60));
    }
}
