//! `ariad-server`: the AriadShift cloud API and conversion workers.

use std::process::ExitCode;

use ariad_server::{
    app,
    config::{ApiConfig, CommonConfig, Env, TelemetryConfig, WorkerConfig},
    db, telemetry,
};
use clap::{Parser, Subcommand};
use tokio_util::sync::CancellationToken;

#[derive(Parser)]
#[command(
    name = "ariad-server",
    version,
    about = "AriadShift cloud API and conversion workers"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    #[command(name = "__convert", hide = true)]
    Convert,
    #[command(name = "__check-helper-network", hide = true)]
    CheckHelperNetwork,
    /// Serve the /v1 HTTP API.
    Api,
    /// Run conversion jobs from the queues of the given engine packs.
    Worker {
        /// Packs whose queues this worker reads (core, office, docling).
        #[arg(long, value_delimiter = ',', default_value = "core")]
        packs: Vec<String>,
    },
    /// Apply pending database migrations. Back up the database first.
    Migrate,
    /// Print the OpenAPI document.
    #[command(hide = true)]
    Openapi,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    if matches!(cli.command, Command::Worker { .. })
        && let Err(err) = ariad_server::sandbox::protect_worker_credentials()
    {
        eprintln!("ariad-server: cannot protect worker credentials: {err}");
        return ExitCode::FAILURE;
    }
    if matches!(cli.command, Command::Convert | Command::CheckHelperNetwork) {
        if let Err(err) = ariad_server::sandbox::deny_helper_network() {
            eprintln!("ariad-server: cannot deny helper network access: {err}");
            return ExitCode::FAILURE;
        }
        if matches!(cli.command, Command::CheckHelperNetwork) {
            return ExitCode::SUCCESS;
        }
        return match ariad_host::convert::isolated::serve(CancellationToken::new()) {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                eprintln!("ariad-server: {err}");
                ExitCode::FAILURE
            }
        };
    }
    if matches!(cli.command, Command::Worker { .. }) {
        // Probe in a disposable child so the worker keeps database/storage
        // network access. This helper boundary is mandatory in every mode.
        let result = std::env::current_exe().and_then(|program| {
            std::process::Command::new(program)
                .arg("__check-helper-network")
                .env_clear()
                .stdin(std::process::Stdio::null())
                .output()
        });
        match result {
            Ok(output) if output.status.success() => {}
            Ok(output) => {
                eprintln!(
                    "ariad-server: helper network preflight failed ({}): {}",
                    output.status,
                    String::from_utf8_lossy(&output.stderr).trim()
                );
                return ExitCode::FAILURE;
            }
            Err(err) => {
                eprintln!("ariad-server: cannot probe helper network access: {err}");
                return ExitCode::FAILURE;
            }
        }
    }
    if let Command::Openapi = cli.command {
        return match ariad_server::api::openapi().to_pretty_json() {
            Ok(json) => {
                println!("{json}");
                ExitCode::SUCCESS
            }
            Err(err) => {
                eprintln!("ariad-server: {err}");
                ExitCode::FAILURE
            }
        };
    }
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(err) => {
            eprintln!("ariad-server: could not start the runtime: {err}");
            return ExitCode::FAILURE;
        }
    };
    let code = runtime.block_on(run(cli.command));
    runtime.shutdown_timeout(std::time::Duration::from_secs(5));
    code
}

async fn run(command: Command) -> ExitCode {
    let env = Env::from_process();
    ariad_server::install_crypto_provider();
    let service = match &command {
        Command::Api => "ariad-server-api",
        Command::Worker { .. } => "ariad-server-worker",
        Command::Migrate | Command::Openapi | Command::Convert | Command::CheckHelperNetwork => {
            "ariad-server"
        }
    };
    let telemetry = match TelemetryConfig::from_env(&env)
        .map_err(|e| e.to_string())
        .and_then(|config| telemetry::init(&config, service))
    {
        Ok(telemetry) => telemetry,
        Err(err) => {
            eprintln!("ariad-server: {err}");
            return ExitCode::FAILURE;
        }
    };
    let result = execute(command, &env).await;
    telemetry.shutdown();
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            tracing::error!("{err}");
            eprintln!("ariad-server: {err}");
            ExitCode::FAILURE
        }
    }
}

async fn execute(command: Command, env: &Env) -> Result<(), String> {
    let common = CommonConfig::from_env(env).map_err(|e| e.to_string())?;
    let pool = db::connect(&common.database_url, common.database_max_connections)
        .await
        .map_err(|e| format!("cannot connect to the database: {e}"))?;
    match command {
        Command::Migrate => {
            db::migrate(&pool).await.map_err(|e| e.to_string())?;
            tracing::info!("migrations applied");
            Ok(())
        }
        Command::Api => {
            let config = ApiConfig::from_env(env).map_err(|e| e.to_string())?;
            db::check_schema(&pool).await.map_err(|e| e.to_string())?;
            let listener = tokio::net::TcpListener::bind(config.bind)
                .await
                .map_err(|e| format!("cannot listen on {}: {e}", config.bind))?;
            let shutdown = shutdown_token();
            app::serve_api(listener, config, pool, shutdown)
                .await
                .map_err(|e| e.to_string())
        }
        Command::Worker { packs } => {
            let config = WorkerConfig::from_env(env, &packs).map_err(|e| e.to_string())?;
            db::check_schema(&pool).await.map_err(|e| e.to_string())?;
            let shutdown = shutdown_token();
            app::run_worker(config, pool, shutdown)
                .await
                .map_err(|e| e.to_string())
        }
        Command::Openapi | Command::Convert | Command::CheckHelperNetwork => Ok(()),
    }
}

/// A token cancelled by Ctrl-C or, on Unix, SIGTERM.
fn shutdown_token() -> CancellationToken {
    let token = CancellationToken::new();
    let trigger = token.clone();
    tokio::spawn(async move {
        wait_for_signal().await;
        tracing::info!("shutting down");
        trigger.cancel();
    });
    token
}

#[cfg(unix)]
async fn wait_for_signal() {
    use tokio::signal::unix::{SignalKind, signal};
    match signal(SignalKind::terminate()) {
        Ok(mut term) => {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {}
                _ = term.recv() => {}
            }
        }
        Err(_) => {
            let _ = tokio::signal::ctrl_c().await;
        }
    }
}

#[cfg(not(unix))]
async fn wait_for_signal() {
    let _ = tokio::signal::ctrl_c().await;
}
