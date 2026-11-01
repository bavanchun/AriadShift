// On macOS, ld emits an informational warning when the unstripped binary's
// `__eh_frame` section exceeds the 16 MB compact-unwind encoding threshold,
// which otherwise trips `CARGO_BUILD_WARNINGS=deny` via `#[warn(linker_messages)]`.
#![cfg_attr(target_os = "macos", allow(linker_messages))]

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
    tools::{
        InputRef, OutputTarget, RecipeRequest, SecretBag, ToolError, ToolEvent, ToolRequest,
        run_recipe_with_events, run_tool_with_events,
    },
};
use clap::{Args, Parser, Subcommand};
use tokio_util::sync::CancellationToken;

mod batch;
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
    /// Convert a document between supported formats (Markdown, HTML, DOCX, EPUB, PDF, Office, IR JSON).
    Convert(ConvertArgs),
    /// Inspect document format, metrics, structure counts, and reachable routes.
    Inspect(InspectArgs),
    /// Plan and explain conversion route, metrics score, and alternatives without converting.
    Plan(PlanArgs),
    /// List engines and installed engine packs, or install, remove and verify packs.
    Engines(EnginesArgs),
    /// Check toolchain dependencies, workspace permissions, and engine health.
    Doctor(DoctorArgs),
    /// List registered document manipulation tools and their schemas.
    Tools(ToolsArgs),
    /// Execute a registered document manipulation tool.
    Tool(ToolArgs),
    /// Execute multi-step document manipulation and conversion recipes.
    Recipe {
        #[command(subcommand)]
        command: RecipeCommand,
    },
    /// Serve Model Context Protocol (MCP) over stdio.
    Mcp(mcp::McpArgs),
    #[command(name = "__ir", hide = true)]
    Ir(IrArgs),
    #[command(name = "__write", hide = true)]
    Write(WriteArgs),
    #[command(name = "__engine", hide = true)]
    Engine { engine: String },
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
    /// Input document file paths.
    #[arg(value_name = "INPUT", required = true, num_args = 1..)]
    inputs: Vec<PathBuf>,
    /// Target output format (md, html, docx, epub, pdf, json, xlsx, pptx); inferred from -o when omitted.
    #[arg(long = "to", value_name = "FORMAT")]
    target_format: Option<String>,
    /// Output file path (defaults to input stem with target extension); with several inputs,
    /// an existing directory for the outputs.
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
    /// Choose the engine that reads the input (for example pdf or docling).
    #[arg(long, value_name = "ENGINE")]
    engine: Option<String>,
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
    /// Target format to write (md, html, docx, epub, pdf).
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
    /// Choose the engine that reads the input (for example pdf or docling).
    #[arg(long, value_name = "ENGINE")]
    engine: Option<String>,
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

/// Arguments for listing registered document tools.
#[derive(Args)]
struct ToolsArgs {
    /// Filter tools by group (organize, convert, optimize, security, overlay, ocr, annotate, forms, sign, redact, compare, edit, ai, automation).
    #[arg(long, value_name = "GROUP")]
    group: Option<String>,
    /// Output tool descriptors as JSON to stdout.
    #[arg(long)]
    json: bool,
}

/// Arguments for running a single registered document tool.
#[derive(Args)]
struct ToolArgs {
    /// Canonical tool identifier (for example: pdf.info, pdf.merge, pdf.split).
    #[arg(value_name = "ID")]
    tool: String,
    /// Input document file paths.
    #[arg(value_name = "INPUT", required = true, num_args = 1..)]
    inputs: Vec<PathBuf>,
    /// Output file or directory path.
    #[arg(short = 'o', long = "output", value_name = "DIR_OR_FILE")]
    output: Option<PathBuf>,
    /// Tool parameter as key=value (may be repeated; values parse as JSON or string).
    #[arg(long = "param", value_name = "KEY=VALUE")]
    params: Vec<String>,
    /// Read tool parameters JSON object from a file or '-' for stdin.
    #[arg(long = "params-json", value_name = "FILE|-")]
    params_json: Option<String>,
    /// Supply a secret parameter from a file ('name=@file') or stdin ('name=-').
    #[arg(long = "secret", value_name = "NAME=@FILE|-")]
    secrets: Vec<String>,
    /// Overwrite existing destination file(s) if present.
    #[arg(long)]
    overwrite: bool,
    /// Output structured tool result as JSON to stdout.
    #[arg(long)]
    json: bool,
}

#[derive(Subcommand)]
enum RecipeCommand {
    /// Run a pipeline recipe on one or more input files.
    Run(RecipeRunArgs),
}

/// Arguments for running a multi-step document recipe.
#[derive(Args)]
struct RecipeRunArgs {
    /// Recipe JSON file path (ariad-recipe/1).
    #[arg(value_name = "RECIPE.JSON")]
    recipe: PathBuf,
    /// Input document file paths.
    #[arg(value_name = "INPUT", required = true, num_args = 1..)]
    inputs: Vec<PathBuf>,
    /// Output directory for published artifacts.
    #[arg(short = 'o', long = "output", value_name = "DIR", required = true)]
    output: PathBuf,
    /// Confirm running an imported recipe without an interactive prompt.
    #[arg(long)]
    yes: bool,
    /// Overwrite existing destination file(s) if present.
    #[arg(long)]
    overwrite: bool,
    /// Output structured recipe outcome as JSON to stdout.
    #[arg(long)]
    json: bool,
}

fn main() -> ExitCode {
    // The sandbox launcher runs before anything else so it restricts and `exec`s the engine
    // without parsing the engine's own arguments.
    if std::env::args_os()
        .nth(1)
        .is_some_and(|arg| arg == "__sandbox")
    {
        return ariad_host::sandbox::launch(std::env::args_os().skip(2));
    }
    match Cli::try_parse() {
        Ok(cli) => match cli.command {
            Some(Command::Convert(args)) => run_convert(args),
            Some(Command::Inspect(args)) => run_inspect(args),
            Some(Command::Plan(args)) => run_plan(args),
            Some(Command::Engines(args)) => run_engines(args),
            Some(Command::Doctor(args)) => run_doctor(args),
            Some(Command::Tools(args)) => run_tools(args),
            Some(Command::Tool(args)) => run_tool_cmd(args),
            Some(Command::Recipe {
                command: RecipeCommand::Run(args),
            }) => run_recipe_run(args),
            Some(Command::Mcp(args)) => run_mcp_entry(args),
            Some(Command::Ir(args)) => run_ir(args),
            Some(Command::Write(args)) => run_write(args),
            Some(Command::Engine { engine }) => serve_engine(&engine),
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
    /// Diagnostic code shown alongside the message, when needed.
    fn diagnostic_code(&self) -> Option<&'static str> {
        None
    }
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
    fn diagnostic_code(&self) -> Option<&'static str> {
        matches!(self, Self::SandboxUnavailable(_)).then(|| self.code())
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

impl CommandError for ToolError {
    fn exit_code(&self) -> u8 {
        ToolError::exit_code(self)
    }
    fn is_interrupted(&self) -> bool {
        matches!(self, Self::Interrupted)
    }
    fn is_generic(&self) -> bool {
        false
    }
    fn diagnostic_code(&self) -> Option<&'static str> {
        Some(self.code())
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
        let _ = match err.diagnostic_code() {
            Some(code) => writeln!(io::stderr(), "ashift: error[{code}]: {err}"),
            None => writeln!(io::stderr(), "ashift: {err}"),
        };
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
    run_blocking_exit(command_name, f, move |value| match render(value) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => report_error(command_name, &err),
    })
}

/// Runs `f` on a blocking thread, cancels it on SIGINT, SIGTERM, SIGHUP, Ctrl-C or Ctrl-Break,
/// and lets `render` choose the exit code of a completed run.
fn run_blocking_exit<T, E, F, R>(command_name: &'static str, f: F, render: R) -> ExitCode
where
    T: Send + 'static,
    E: CommandError,
    F: FnOnce(CancellationToken) -> Result<T, E> + Send + 'static,
    R: FnOnce(T) -> ExitCode + Send + 'static,
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
                    Ok(Ok(val)) => render(val),
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
                    render(val)
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

fn target_extension(target_format: &str) -> &str {
    match ariad_core::format::Format::from_user_name(target_format) {
        Some(fmt) => fmt.user_name(),
        None => target_format,
    }
}

fn run_convert(mut args: ConvertArgs) -> ExitCode {
    if args.inputs.len() > 1 {
        return batch::run(args);
    }
    let input = args.inputs.remove(0);
    let is_json = args.json;
    let (target_format, output) = match (args.target_format, args.output) {
        (Some(target_format), Some(output)) => (target_format, output),
        (Some(target_format), None) => {
            let output = input.with_extension(target_extension(&target_format));
            (target_format, output)
        }
        (None, Some(output)) => {
            let Some(format) = output
                .extension()
                .and_then(|ext| ext.to_str())
                .and_then(ariad_core::format::Format::from_extension)
            else {
                let _ = writeln!(
                    io::stderr(),
                    "ashift: cannot infer target format from output path '{}'; pass --to <FORMAT>",
                    render::escape_control_chars(&output.display().to_string())
                );
                return ExitCode::from(2);
            };
            (format.user_name().to_owned(), output)
        }
        (None, None) => {
            let _ = writeln!(
                io::stderr(),
                "ashift: missing target format; pass --to <FORMAT> or -o <OUTPUT> with a known extension"
            );
            return ExitCode::from(2);
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
        input,
        output,
        target_format,
        profile: args.profile.into(),
        overwrite: args.overwrite,
        engine_program,
        title_fallback: None,
        asset_base_dir: None,
        reader_engine: args.engine.clone(),
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
    let reader_engine = args.engine;
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
            ariad_host::commands::plan::plan_with_engine(
                &input,
                &target,
                profile,
                &caps,
                Some(&engine_program),
                reader_engine.as_deref(),
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

fn run_tools(args: ToolsArgs) -> ExitCode {
    let specs: Vec<&'static ariad_core::tools::ToolSpec> = match args.group.as_deref() {
        Some(group_str) => match group_str.parse::<ariad_core::tools::ToolGroup>() {
            Ok(group) => ariad_core::tools::by_group(group).collect(),
            Err(err) => return report_error("tools", &ToolError::Usage(err.to_string())),
        },
        None => ariad_core::tools::all().iter().collect(),
    };
    let descriptors: Vec<ariad_core::tools::ToolDescriptor> = specs
        .into_iter()
        .map(ariad_core::tools::ToolSpec::to_descriptor)
        .collect();
    let render_res = if args.json {
        render::render_json(&descriptors)
    } else {
        render::render_tools_human(&descriptors)
    };
    match render_res {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => report_error("tools", &ToolError::OutputIo(err.to_string())),
    }
}

fn trim_single_trailing_newline(mut value: String) -> String {
    if value.ends_with('\n') {
        value.pop();
        if value.ends_with('\r') {
            value.pop();
        }
    }
    value
}

fn parse_tool_params_and_secrets(
    tool_id: &str,
    params_json_arg: Option<&str>,
    param_args: &[String],
    secret_args: &[String],
) -> Result<(serde_json::Value, SecretBag), ToolError> {
    let params_uses_stdin = params_json_arg == Some("-");
    let secret_stdin_count = secret_args
        .iter()
        .filter(|s| s.split_once('=').is_some_and(|(_, v)| v == "-"))
        .count();
    if secret_stdin_count > 1 || (params_uses_stdin && secret_stdin_count > 0) {
        return Err(ToolError::Usage(
            "stdin ('-') can only be read once across --params-json and --secret".to_owned(),
        ));
    }

    let mut params_map = serde_json::Map::new();
    if let Some(source) = params_json_arg {
        let raw_json = if source == "-" {
            io::read_to_string(io::stdin()).map_err(|err| ToolError::InputIo(err.to_string()))?
        } else {
            std::fs::read_to_string(source).map_err(|err| {
                ToolError::InvalidParams(format!("cannot read --params-json {source:?}: {err}"))
            })?
        };
        let parsed: serde_json::Value = serde_json::from_str(&raw_json).map_err(|err| {
            ToolError::InvalidParams(format!("invalid JSON in --params-json: {err}"))
        })?;
        match parsed {
            serde_json::Value::Object(map) => {
                params_map = map;
            }
            _ => {
                return Err(ToolError::InvalidParams(
                    "--params-json must contain a JSON object".to_owned(),
                ));
            }
        }
    }

    for raw in param_args {
        let Some((key, raw_val)) = raw.split_once('=') else {
            return Err(ToolError::Usage(format!(
                "invalid --param {raw:?}: expected KEY=VALUE"
            )));
        };
        let key = key.trim();
        if key.is_empty() {
            return Err(ToolError::Usage(format!(
                "invalid --param {raw:?}: parameter key must not be empty"
            )));
        }
        let value = serde_json::from_str::<serde_json::Value>(raw_val)
            .unwrap_or_else(|_| serde_json::Value::String(raw_val.to_owned()));
        params_map.insert(key.to_owned(), value);
    }

    let params_value = serde_json::Value::Object(params_map);

    // Reject inline secrets passed via --param or --params-json with exit code 9 and a hint.
    if let Some(spec) = ariad_core::tools::find(tool_id) {
        ariad_core::tools::validate_secret_params(
            spec,
            &params_value,
            ariad_core::tools::SecretSource::Argv,
        )
        .map_err(|err| ToolError::InvalidParams(err.to_string()))?;
    }
    ariad_core::tools::validate_secret_fields(
        ariad_core::tools::KNOWN_SECRET_FIELD_NAMES,
        &params_value,
        ariad_core::tools::SecretSource::Argv,
    )
    .map_err(|err| ToolError::InvalidParams(err.to_string()))?;

    let mut secrets = SecretBag::new();
    for raw in secret_args {
        let Some((name, target)) = raw.split_once('=') else {
            return Err(ToolError::Usage(format!(
                "invalid --secret {raw:?}: expected NAME=@FILE or NAME=-"
            )));
        };
        let name = name.trim();
        if name.is_empty() {
            return Err(ToolError::Usage(format!(
                "invalid --secret {raw:?}: secret parameter name must not be empty"
            )));
        }
        let secret_value = if target == "-" {
            let stdin_str = io::read_to_string(io::stdin())
                .map_err(|err| ToolError::InputIo(err.to_string()))?;
            trim_single_trailing_newline(stdin_str)
        } else if let Some(file_path) = target.strip_prefix('@') {
            if file_path.is_empty() {
                return Err(ToolError::InvalidParams(format!(
                    "empty file path in --secret {name}=@"
                )));
            }
            let file_str = std::fs::read_to_string(file_path).map_err(|err| {
                ToolError::InvalidParams(format!(
                    "cannot read secret file {file_path:?} for parameter '{name}': {err}"
                ))
            })?;
            trim_single_trailing_newline(file_str)
        } else {
            return Err(ToolError::InvalidParams(format!(
                "parameter '{name}' is marked x-ariad-secret and cannot be passed as an inline literal; use --secret {name}=@<file> or --secret {name}=-"
            )));
        };
        secrets.insert(name.to_owned(), secret_value);
    }

    Ok((params_value, secrets))
}

fn run_tool_cmd(args: ToolArgs) -> ExitCode {
    let (params, secrets) = match parse_tool_params_and_secrets(
        &args.tool,
        args.params_json.as_deref(),
        &args.params,
        &args.secrets,
    ) {
        Ok(pair) => pair,
        Err(err) => return report_error("tool", &err),
    };

    let engine_program = match std::env::current_exe() {
        Ok(path) => path,
        Err(err) => return report_error("tool", &ToolError::InputIo(err.to_string())),
    };
    let is_json = args.json;
    let renders_info_human = args.tool == "pdf.info";
    let stderr_is_terminal = io::stderr().is_terminal();
    let inputs: Vec<InputRef> = args.inputs.into_iter().map(InputRef::new).collect();
    let output = match args.output {
        Some(path) => OutputTarget::Path(path),
        None => OutputTarget::DefaultBesideInput,
    };

    let request = ToolRequest::new(args.tool, inputs, output)
        .with_params(params)
        .with_secrets(secrets)
        .with_secret_source(ariad_core::tools::SecretSource::Argv)
        .with_overwrite(args.overwrite)
        .with_engine_program(engine_program);

    run_blocking(
        "tool",
        move |cancel| {
            let req = request.with_cancel(cancel);
            run_tool_with_events(&req, |event| match event {
                ToolEvent::Progress { stage, .. } if stderr_is_terminal => {
                    let _ = writeln!(io::stderr(), "progress: {stage}");
                }
                ToolEvent::Progress { .. } => {}
            })
        },
        move |outcome| {
            for warning in &outcome.warnings {
                let _ = writeln!(
                    io::stderr(),
                    "warning[{}]: {}",
                    warning.code,
                    warning.message
                );
            }
            if is_json {
                if outcome.artifacts.is_empty()
                    && let Some(ref inline_json) = outcome.inline_json
                {
                    return render::render_json(inline_json)
                        .map_err(|err| ToolError::OutputIo(err.to_string()));
                }
                render::render_json(&outcome).map_err(|err| ToolError::OutputIo(err.to_string()))
            } else if outcome.artifacts.is_empty()
                && let Some(ref inline_json) = outcome.inline_json
            {
                if renders_info_human {
                    render::render_pdf_info_human(inline_json)
                } else {
                    render::render_json(inline_json)
                }
                .map_err(|err| ToolError::OutputIo(err.to_string()))
            } else {
                let mut stdout = io::stdout().lock();
                for artifact in &outcome.artifacts {
                    writeln!(
                        stdout,
                        "{}",
                        render::escape_control_chars(&artifact.path.display().to_string())
                    )
                    .map_err(|err| ToolError::OutputIo(err.to_string()))?;
                }
                stdout
                    .flush()
                    .map_err(|err| ToolError::OutputIo(err.to_string()))
            }
        },
    )
}

fn prompt_imported_recipe_confirmation() -> bool {
    let _ = write!(io::stderr(), "Execute imported recipe? [y/N]: ");
    let _ = io::stderr().flush();
    let mut line = String::new();
    if io::stdin().read_line(&mut line).is_err() {
        return false;
    }
    matches!(line.trim(), "y" | "Y" | "yes" | "YES")
}

fn run_recipe_run(args: RecipeRunArgs) -> ExitCode {
    let raw = match std::fs::read_to_string(&args.recipe) {
        Ok(text) => text,
        Err(err) => return report_error("recipe", &ToolError::InputIo(err.to_string())),
    };
    let recipe = match ariad_core::tools::Recipe::from_json_str(
        &raw,
        ariad_core::tools::RecipeOrigin::Imported,
    ) {
        Ok(recipe) => recipe,
        Err(err) => return report_error("recipe", &ToolError::InvalidParams(err.to_string())),
    };

    let _ = writeln!(
        io::stderr(),
        "recipe: {} ({} step(s))",
        render::escape_control_chars(&args.recipe.display().to_string()),
        recipe.steps.len()
    );
    for (idx, step) in recipe.steps.iter().enumerate() {
        match step {
            ariad_core::tools::Step::Tool { tool, .. } => {
                let _ = writeln!(io::stderr(), "  {}. tool {}", idx + 1, tool.as_str());
            }
            ariad_core::tools::Step::Convert { to, profile } => match profile {
                Some(p) => {
                    let _ = writeln!(
                        io::stderr(),
                        "  {}. convert -> {} (profile: {})",
                        idx + 1,
                        to.id(),
                        p.as_str()
                    );
                }
                None => {
                    let _ = writeln!(io::stderr(), "  {}. convert -> {}", idx + 1, to.id());
                }
            },
        }
    }

    let confirmed = if args.yes {
        true
    } else if io::stdin().is_terminal() && io::stderr().is_terminal() {
        prompt_imported_recipe_confirmation()
    } else {
        false
    };

    let engine_program = match std::env::current_exe() {
        Ok(path) => path,
        Err(err) => return report_error("recipe", &ToolError::InputIo(err.to_string())),
    };
    let is_json = args.json;
    let stderr_is_terminal = io::stderr().is_terminal();
    let inputs: Vec<InputRef> = args.inputs.into_iter().map(InputRef::new).collect();

    let request = RecipeRequest::new(recipe, inputs, args.output)
        .with_origin(ariad_core::tools::RecipeOrigin::Imported, confirmed)
        .with_overwrite(args.overwrite)
        .with_engine_program(engine_program);

    run_blocking(
        "recipe",
        move |cancel| {
            let req = request.with_cancel(cancel);
            run_recipe_with_events(&req, |event| match event {
                ToolEvent::Progress { stage, .. } if stderr_is_terminal => {
                    let _ = writeln!(io::stderr(), "progress: {stage}");
                }
                ToolEvent::Progress { .. } => {}
            })
        },
        move |outcome| {
            for warning in &outcome.warnings {
                let _ = writeln!(
                    io::stderr(),
                    "warning[{}]: {}",
                    warning.code,
                    warning.message
                );
            }
            if is_json {
                render::render_json(&outcome).map_err(|err| ToolError::OutputIo(err.to_string()))
            } else {
                let mut stdout = io::stdout().lock();
                for artifact in &outcome.artifacts {
                    writeln!(
                        stdout,
                        "{}",
                        render::escape_control_chars(&artifact.path.display().to_string())
                    )
                    .map_err(|err| ToolError::OutputIo(err.to_string()))?;
                }
                stdout
                    .flush()
                    .map_err(|err| ToolError::OutputIo(err.to_string()))
            }
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

fn serve_engine(engine: &str) -> ExitCode {
    #[cfg(debug_assertions)]
    if engine == "pandoc" && test_engine_override() {
        return serve_test_override(io::stdin().lock(), io::stdout().lock());
    }
    ariad_host::engines::serve_builtin(engine, io::stdin().lock(), io::stdout().lock())
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
