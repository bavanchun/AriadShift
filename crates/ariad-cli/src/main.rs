use clap::Parser;
use clap::Subcommand;
use std::io;
use std::process::ExitCode;

#[derive(Parser)]
#[command(name = "ashift", version)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    #[command(name = "__engine", hide = true)]
    Engine {
        #[command(subcommand)]
        command: EngineCommand,
    },
}

#[derive(Subcommand)]
enum EngineCommand {
    Pandoc,
}

fn main() -> ExitCode {
    match Cli::parse().command {
        Some(Command::Engine {
            command: EngineCommand::Pandoc,
        }) => ariad_host::engines::pandoc::serve(io::stdin().lock(), io::stdout().lock()),
        None => ExitCode::SUCCESS,
    }
}
