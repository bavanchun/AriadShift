use std::{
    io::{self, IsTerminal, Write},
    path::PathBuf,
    process::ExitCode,
};

#[cfg(debug_assertions)]
use std::time::Duration;

use ariad_host::{
    convert::{ConvertError, ConvertEvent, Profile},
    packs::{self, PackError},
};
use clap::{Args, Parser, Subcommand};
use tokio_util::sync::CancellationToken;

mod mcp;
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
    /// List engines and installed engine packs, or install, remove and verify packs.
    Engines(EnginesArgs),
    /// Check toolchain dependencies, workspace permissions, and engine health.
    Doctor(DoctorArgs),
    /// Serve Model Context Protocol (MCP) over stdio.
    Mcp(mcp::McpArgs),
    #[command(name = "__ir", hide = true)]
    Ir(IrArgs),
    #[command(name = "__write", hide = true)]
    Write(WriteArgs),
    #[command(name = "__engine", hide = true)]
    Engine {
        #[command(subcommand)]
        command: EngineCommand,
    },
    #[command(name = "__pack", hide = true)]
    Pack {
        #[command(subcommand)]
        command: PackCommand,
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

/// Arguments for listing engines or managing engine packs.
#[derive(Args)]
#[command(args_conflicts_with_subcommands = true)]
struct EnginesArgs {
    #[command(subcommand)]
    action: Option<EnginesAction>,
    /// Output engine statuses and routes as JSON to stdout.
    #[arg(long)]
    json: bool,
}

#[derive(Subcommand)]
enum EnginesAction {
    /// Download, verify and install a signed engine pack.
    Install(PackInstallArgs),
    /// Remove an installed engine pack (every version unless --version is given).
    Remove(PackRemoveArgs),
    /// Re-hash an installed engine pack's files and re-check its signature.
    Verify(PackVerifyArgs),
}

/// Arguments for installing an engine pack.
#[derive(Args)]
struct PackInstallArgs {
    /// Pack name, e.g. docling.
    #[arg(value_name = "NAME")]
    name: String,
    /// Pack version (defaults to the recommended version, or the manifest's version).
    #[arg(long, value_name = "VERSION")]
    version: Option<String>,
    /// Manifest URL or path instead of the built-in index; its signature is <MANIFEST>.minisig.
    #[arg(long, value_name = "URL_OR_PATH", conflicts_with = "from")]
    manifest: Option<String>,
    /// Install offline from a directory holding pack.toml, pack.toml.minisig and the components.
    #[arg(long, value_name = "DIR")]
    from: Option<PathBuf>,
    /// Output the install report as JSON to stdout.
    #[arg(long)]
    json: bool,
}

/// Arguments for removing an engine pack.
#[derive(Args)]
struct PackRemoveArgs {
    /// Pack name.
    #[arg(value_name = "NAME")]
    name: String,
    /// Remove only this version.
    #[arg(long, value_name = "VERSION")]
    version: Option<String>,
    /// Output the removal report as JSON to stdout.
    #[arg(long)]
    json: bool,
}

/// Arguments for verifying an engine pack.
#[derive(Args)]
struct PackVerifyArgs {
    /// Pack name.
    #[arg(value_name = "NAME")]
    name: String,
    /// Verify this version instead of the active one.
    #[arg(long, value_name = "VERSION")]
    version: Option<String>,
    /// Output the verification report as JSON to stdout.
    #[arg(long)]
    json: bool,
}

#[derive(Subcommand)]
enum PackCommand {
    /// Write an unsigned pack manifest describing built component files.
    Manifest(PackManifestArgs),
}

/// Arguments for generating a pack manifest.
#[derive(Args)]
struct PackManifestArgs {
    /// Pack name.
    #[arg(long, value_name = "NAME")]
    name: String,
    /// Pack version.
    #[arg(long, value_name = "VERSION")]
    version: String,
    /// Target triple of the component files.
    #[arg(long, value_name = "TRIPLE")]
    target: String,
    /// Existing manifest or template: pack fields and other targets are kept.
    #[arg(long, value_name = "FILE")]
    base: Option<PathBuf>,
    /// Entrypoint argument; repeat once per argv element, the program first.
    #[arg(long = "entrypoint", value_name = "ARG", allow_hyphen_values = true)]
    entrypoint: Vec<String>,
    /// Prefix for component URLs (default: the file name, relative to the manifest).
    #[arg(long, value_name = "URL")]
    url_prefix: Option<String>,
    /// Install directory of a component inside the pack root.
    #[arg(long = "dest", value_name = "COMPONENT=DIR")]
    dest: Vec<String>,
    /// Output file (default: stdout).
    #[arg(short, long, value_name = "FILE")]
    output: Option<PathBuf>,
    /// Component files, optionally named: NAME=FILE.
    #[arg(value_name = "[NAME=]FILE", required = true)]
    files: Vec<String>,
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
            Some(Command::Mcp(args)) => run_mcp_entry(args),
            Some(Command::Ir(args)) => run_ir(args),
            Some(Command::Write(args)) => run_write(args),
            Some(Command::Engine {
                command: EngineCommand::Pandoc,
            }) => serve_pandoc(),
            Some(Command::Pack {
                command: PackCommand::Manifest(args),
            }) => run_pack_manifest(args),
            None => ExitCode::SUCCESS,
        },
        Err(error) => {
            let code = error.exit_code().try_into().unwrap_or(2);
            let _ = error.print();
            ExitCode::from(code)
        }
    }
}

fn run_mcp_entry(args: mcp::McpArgs) -> ExitCode {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(_) => {
            let _ = writeln!(io::stderr(), "ashift: mcp server failed to start runtime");
            return ExitCode::from(1);
        }
    };
    let exit_code = runtime.block_on(mcp::run_mcp(args));
    runtime.shutdown_background();
    exit_code
}

/// An error a blocking command can end with.
trait CommandError: std::fmt::Display + Send + 'static {
    fn exit_code(&self) -> u8;
    fn is_interrupted(&self) -> bool;
    /// Generic failures print "<command> failed" instead of a message.
    fn is_generic(&self) -> bool;
}

impl CommandError for ConvertError {
    fn exit_code(&self) -> u8 {
        ConvertError::exit_code(self)
    }
    fn is_interrupted(&self) -> bool {
        matches!(self, Self::Interrupted)
    }
    fn is_generic(&self) -> bool {
        matches!(self, Self::Failed)
    }
}

impl CommandError for PackError {
    fn exit_code(&self) -> u8 {
        PackError::exit_code(self)
    }
    fn is_interrupted(&self) -> bool {
        matches!(self, Self::Interrupted)
    }
    fn is_generic(&self) -> bool {
        false
    }
}

fn report_error<E: CommandError>(command_name: &str, err: &E) -> ExitCode {
    if err.is_interrupted() {
        let _ = writeln!(io::stderr(), "ashift: {command_name} was interrupted");
        ExitCode::from(130)
    } else if err.is_generic() {
        let _ = writeln!(io::stderr(), "ashift: {command_name} failed");
        ExitCode::from(1)
    } else {
        let _ = writeln!(io::stderr(), "ashift: {err}");
        ExitCode::from(err.exit_code())
    }
}

fn run_blocking<T, E, F, R>(command_name: &'static str, f: F, render: R) -> ExitCode
where
    T: Send + 'static,
    E: CommandError,
    F: FnOnce(CancellationToken) -> Result<T, E> + Send + 'static,
    R: FnOnce(T) -> Result<(), E> + Send + 'static,
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
                        Err(err) => report_error(command_name, &err),
                    },
                    Ok(Err(err)) => report_error(command_name, &err),
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
                        Err(err) => report_error(command_name, &err),
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
        asset_base_dir: None,
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
    match args.action {
        Some(EnginesAction::Install(args)) => return run_pack_install(args),
        Some(EnginesAction::Remove(args)) => return run_pack_remove(args),
        Some(EnginesAction::Verify(args)) => return run_pack_verify(args),
        None => {}
    }
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

fn pack_progress(stderr_is_terminal: bool) -> impl FnMut(packs::PackEvent) {
    let mut last_percent: Option<(String, u64)> = None;
    move |event| {
        if !stderr_is_terminal {
            return;
        }
        match event {
            packs::PackEvent::Stage(stage) => {
                let _ = writeln!(
                    io::stderr(),
                    "progress: {}",
                    render::escape_control_chars(&stage)
                );
            }
            packs::PackEvent::Download {
                component,
                done,
                total,
            } => {
                let percent = done.saturating_mul(100).checked_div(total).unwrap_or(100) / 10 * 10;
                let changed = last_percent
                    .as_ref()
                    .is_none_or(|(name, last)| name != &component || *last != percent);
                if changed {
                    let _ = writeln!(io::stderr(), "progress: download {component} {percent}%");
                    last_percent = Some((component, percent));
                }
            }
        }
    }
}

fn run_pack_install(args: PackInstallArgs) -> ExitCode {
    let source = match (args.manifest, args.from) {
        (Some(manifest), _) => packs::InstallSource::Manifest(manifest),
        (None, Some(dir)) => packs::InstallSource::Directory(dir),
        (None, None) => packs::InstallSource::Index,
    };
    let request = packs::InstallRequest {
        name: args.name,
        version: args.version,
        source,
    };
    let is_json = args.json;
    let stderr_is_terminal = io::stderr().is_terminal();
    run_blocking(
        "pack install",
        move |cancel| {
            let context = packs::PackContext::from_env()?;
            packs::install(
                &context,
                &request,
                &cancel,
                &mut pack_progress(stderr_is_terminal),
            )
        },
        move |report| {
            if is_json {
                render::render_json(&report).map_err(stdout_error)
            } else {
                render::render_pack_install_human(&report).map_err(stdout_error)
            }
        },
    )
}

fn run_pack_remove(args: PackRemoveArgs) -> ExitCode {
    let is_json = args.json;
    run_blocking(
        "pack removal",
        move |cancel| {
            if cancel.is_cancelled() {
                return Err(PackError::Interrupted);
            }
            let context = packs::PackContext::from_env()?;
            packs::remove(&context, &args.name, args.version.as_deref())
        },
        move |report| {
            if is_json {
                render::render_json(&report).map_err(stdout_error)
            } else {
                render::render_pack_remove_human(&report).map_err(stdout_error)
            }
        },
    )
}

fn run_pack_verify(args: PackVerifyArgs) -> ExitCode {
    let is_json = args.json;
    run_blocking(
        "pack verification",
        move |cancel| {
            let context = packs::PackContext::from_env()?;
            packs::verify(&context, &args.name, args.version.as_deref(), &cancel)
        },
        move |report| {
            if is_json {
                render::render_json(&report).map_err(stdout_error)?;
            } else {
                render::render_pack_verify_human(&report).map_err(stdout_error)?;
            }
            if report.ok {
                Ok(())
            } else {
                Err(PackError::VerificationFailed {
                    detail: format!(
                        "{} modified, {} missing, {} other problem(s)",
                        report.modified.len(),
                        report.missing.len(),
                        report.problems.len()
                    ),
                    name: report.name,
                    version: report.version,
                })
            }
        },
    )
}

fn stdout_error(source: io::Error) -> PackError {
    PackError::Io {
        context: "could not write to stdout".to_owned(),
        source,
    }
}

fn run_pack_manifest(args: PackManifestArgs) -> ExitCode {
    let mut dests = Vec::new();
    for dest in &args.dest {
        match dest.split_once('=') {
            Some((component, dir)) if !component.is_empty() && !dir.is_empty() => {
                dests.push((component.to_owned(), dir.to_owned()));
            }
            _ => {
                let _ = writeln!(
                    io::stderr(),
                    "ashift: --dest expects COMPONENT=DIR, got '{dest}'"
                );
                return ExitCode::from(2);
            }
        }
    }
    let components = args
        .files
        .iter()
        .map(|file| match file.split_once('=') {
            Some((name, path))
                if !name.is_empty()
                    && name
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b)) =>
            {
                (Some(name.to_owned()), PathBuf::from(path))
            }
            _ => (None, PathBuf::from(file)),
        })
        .collect();
    let request = packs::ManifestRequest {
        name: args.name,
        version: args.version,
        target: args.target,
        base: args.base,
        entrypoint: args.entrypoint,
        url_prefix: args.url_prefix,
        dests,
        components,
    };
    let result = packs::generate_manifest(&request)
        .and_then(|manifest| manifest.to_toml())
        .and_then(|text| match &args.output {
            Some(path) => write_atomically(path, text.as_bytes()),
            None => io::stdout()
                .lock()
                .write_all(text.as_bytes())
                .map_err(stdout_error),
        });
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            let _ = writeln!(io::stderr(), "ashift: {err}");
            ExitCode::from(err.exit_code())
        }
    }
}

fn write_atomically(path: &std::path::Path, bytes: &[u8]) -> Result<(), PackError> {
    let directory = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| std::path::Path::new("."));
    let mut temporary =
        tempfile::NamedTempFile::new_in(directory).map_err(|source| PackError::Io {
            context: format!("could not create a file in {}", directory.display()),
            source,
        })?;
    temporary.write_all(bytes).map_err(|source| PackError::Io {
        context: format!("could not write {}", path.display()),
        source,
    })?;
    temporary.persist(path).map_err(|error| PackError::Io {
        context: format!("could not write {}", path.display()),
        source: error.error,
    })?;
    Ok(())
}

#[cfg(unix)]
async fn interrupt() -> io::Result<()> {
    use tokio::signal::unix::{SignalKind, signal};

    let mut sigterm = signal(SignalKind::terminate())?;
    let mut sighup = signal(SignalKind::hangup())?;

    tokio::select! {
        res = tokio::signal::ctrl_c() => res,
        _ = sigterm.recv() => Ok(()),
        _ = sighup.recv() => Ok(()),
    }
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
