//! MCP stdio server module for AriadShift.

pub mod resources;
pub mod tools;

use std::{
    path::PathBuf,
    pin::Pin,
    process::ExitCode,
    sync::{Arc, Mutex, RwLock},
    task::{Context, Poll},
    time::Duration,
};

use ariad_host::{
    confine::AllowedDirs,
    session::{PoolConfig, SessionPool},
};
use clap::Args;
use rmcp::{
    ErrorData, RoleServer, ServerHandler, ServiceExt,
    model::{Implementation, ReadResourceRequestParams, ServerCapabilities, ServerConfig},
    service::RequestContext,
    tool_handler,
};
use tokio::io::{AsyncRead, ReadBuf};

use resources::SessionResources;
pub use tools::McpServer;

struct EofWatcher<R> {
    inner: R,
    on_eof: tokio_util::sync::CancellationToken,
}

impl<R: AsyncRead + Unpin> AsyncRead for EofWatcher<R> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let prev_len = buf.filled().len();
        let res = Pin::new(&mut self.inner).poll_read(cx, buf);
        if let Poll::Ready(Ok(())) = &res
            && buf.remaining() > 0
            && buf.filled().len() == prev_len
        {
            self.on_eof.cancel();
        }
        res
    }
}

/// Command line arguments for `ashift mcp`.
#[derive(Args, Clone, Debug)]
pub struct McpArgs {
    /// Allowed filesystem directories for inputs and outputs.
    #[arg(long = "allow-dir", value_name = "DIR")]
    pub allow_dir: Vec<PathBuf>,
    /// Allow tools to overwrite existing files when explicitly requested in tool call.
    #[arg(long = "allow-overwrite")]
    pub allow_overwrite: bool,
}

const ROOTS_LIST_TIMEOUT: Duration = Duration::from_secs(10);

impl McpServer {
    #[allow(deprecated)]
    async fn refresh_roots_with_peer(
        &self,
        peer: &rmcp::service::Peer<RoleServer>,
        fail_closed: bool,
    ) {
        // Allocation and installation share this mutex, including on multithreaded runtimes.
        let generation = {
            let mut current = self
                .roots_generation
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            *current = current.wrapping_add(1);
            *current
        };
        let roots_uris = match tokio::time::timeout(ROOTS_LIST_TIMEOUT, peer.list_roots()).await {
            Ok(Ok(result)) => result.roots.into_iter().map(|root| root.uri).collect(),
            // Initialization can involve a client without roots support. A failed refresh
            // after list_changed must revoke the old client roots, retaining CLI grants.
            _ if fail_closed => Vec::new(),
            _ => return,
        };
        let existing_allow_dirs: Vec<PathBuf> = self
            .cli_allow_dirs
            .iter()
            .filter(|p| p.is_dir())
            .cloned()
            .collect();
        let new_allowed = AllowedDirs::new(&existing_allow_dirs, &roots_uris).unwrap_or_default();
        self.replace_allowed_dirs(generation, new_allowed);
    }

    fn replace_allowed_dirs(&self, generation: u64, new_allowed: AllowedDirs) {
        let current = self
            .roots_generation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if *current != generation {
            return;
        }
        let mut allowed = self
            .allowed_dirs
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *allowed = new_allowed;
        self.allowed_dirs.clear_poison();
    }
}

#[tool_handler]
impl ServerHandler for McpServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .build(),
        )
        .with_server_info(Implementation::new("ashift", env!("CARGO_PKG_VERSION")))
        .with_instructions(
            "AriadShift document conversion server. Exposes tools: list_engines, inspect, plan, convert. \
             Path confinement: all file operations require explicit path authorization via --allow-dir or client roots."
        )
    }

    async fn call_tool(
        &self,
        request: rmcp::model::CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<rmcp::model::CallToolResponse, ErrorData> {
        let tcc = rmcp::handler::server::tool::ToolCallContext::new(self, request, context);
        Self::tool_router().call(tcc).await
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<rmcp::model::ReadResourceResponse, ErrorData> {
        resources::handle_read_resource(request, context, &self.resources, &self.allowed_dirs).await
    }

    async fn on_initialized(&self, context: rmcp::service::NotificationContext<RoleServer>) {
        self.refresh_roots_with_peer(&context.peer, false).await;
    }

    async fn on_roots_list_changed(&self, context: rmcp::service::NotificationContext<RoleServer>) {
        self.refresh_roots_with_peer(&context.peer, true).await;
    }
}

/// Idle warm engine sessions survive this long between tool calls.
const MCP_SESSION_IDLE_TIMEOUT: Duration = Duration::from_secs(300);

/// Closes the warm engine sessions on a blocking thread; closing waits for engines to exit.
async fn shutdown_pool(pool: SessionPool) {
    let _ = tokio::task::spawn_blocking(move || pool.shutdown()).await;
}

/// Runs the MCP stdio server.
pub async fn run_mcp(args: McpArgs) -> ExitCode {
    // 1. Startup stale workspace sweep (older than 24h)
    let _ = ariad_host::workspace::sweep_stale(Duration::from_secs(24 * 3600));

    let engine_program = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error[mcp_init]: failed to get current executable path: {e}");
            return ExitCode::from(1);
        }
    };

    let initial_allowed = match AllowedDirs::new(&args.allow_dir, &[]) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("error[mcp_init]: invalid --allow-dir paths: {e}");
            return ExitCode::from(1);
        }
    };

    let server_cancel = tokio_util::sync::CancellationToken::new();
    let serve_ct = tokio_util::sync::CancellationToken::new();
    let in_flight = Arc::new(std::sync::atomic::AtomicUsize::new(0));

    let eof_cancel = server_cancel.clone();
    let eof_serve_ct = serve_ct.clone();
    let eof_token = tokio_util::sync::CancellationToken::new();
    let eof_watcher_token = eof_token.clone();
    tokio::spawn(async move {
        eof_token.cancelled().await;
        eof_cancel.cancel();
        eof_serve_ct.cancel();
    });

    let pool = SessionPool::new(PoolConfig {
        idle_timeout: MCP_SESSION_IDLE_TIMEOUT,
        ..PoolConfig::default()
    });
    let server = McpServer {
        pool: pool.clone(),
        allow_overwrite: args.allow_overwrite,
        allowed_dirs: Arc::new(RwLock::new(initial_allowed)),
        cli_allow_dirs: args.allow_dir,
        roots_generation: Arc::new(Mutex::new(0)),
        resources: SessionResources::new(),
        engine_program,
        cancel_token: server_cancel.clone(),
        in_flight: in_flight.clone(),
    };

    let running = match server
        .serve_with_ct(
            (
                EofWatcher {
                    inner: tokio::io::stdin(),
                    on_eof: eof_watcher_token,
                },
                tokio::io::stdout(),
            ),
            serve_ct,
        )
        .await
    {
        Ok(r) => r,
        Err(e) => {
            eprintln!("ashift: mcp error: {e}");
            return ExitCode::from(1);
        }
    };

    let running_cancel = running.cancellation_token();
    let mut running_opt = Some(running);

    tokio::select! {
        _ = super::interrupt() => {
            server_cancel.cancel();
            running_cancel.cancel();
            if let Some(mut r) = running_opt.take() {
                let _ = r.close_with_timeout(Duration::from_secs(2)).await;
            }
            // Await in-flight conversion jobs with a bound (up to 3 seconds) (H2)
            let start = tokio::time::Instant::now();
            while in_flight.load(std::sync::atomic::Ordering::SeqCst) > 0
                && start.elapsed() < Duration::from_secs(3)
            {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            shutdown_pool(pool).await;
            ExitCode::from(130)
        }
        res = async {
            if let Some(r) = running_opt.take() {
                r.waiting().await
            } else {
                Ok(rmcp::service::QuitReason::Closed)
            }
        } => {
            server_cancel.cancel();
            running_cancel.cancel();
            let start = tokio::time::Instant::now();
            while in_flight.load(std::sync::atomic::Ordering::SeqCst) > 0
                && start.elapsed() < Duration::from_secs(3)
            {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            shutdown_pool(pool).await;
            match res {
                Ok(_) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("ashift: mcp session error: {e}");
                    ExitCode::from(1)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaced_poisoned_roots_support_revocation_and_recovery() {
        let explicit = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        let explicit_input = explicit.path().join("note.md");
        let root_input = root.path().join("note.md");
        std::fs::write(&explicit_input, "# Explicit").unwrap();
        std::fs::write(&root_input, "# Root").unwrap();
        let cli_allow_dirs = vec![explicit.path().to_path_buf()];
        let allowed_dirs = Arc::new(RwLock::new(
            AllowedDirs::new(
                &cli_allow_dirs,
                &[ariad_host::confine::path_to_file_uri(root.path())],
            )
            .unwrap(),
        ));
        let poisoned = allowed_dirs.clone();
        let _ = std::thread::spawn(move || {
            let _guard = poisoned.write().unwrap();
            panic!("poison roots lock");
        })
        .join();
        let server = McpServer {
            roots_generation: Arc::new(Mutex::new(0)),
            allowed_dirs,
            cli_allow_dirs: cli_allow_dirs.clone(),
            allow_overwrite: false,
            resources: SessionResources::new(),
            engine_program: std::env::current_exe().unwrap(),
            pool: SessionPool::new(PoolConfig::default()),
            cancel_token: tokio_util::sync::CancellationToken::new(),
            in_flight: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        };
        server.replace_allowed_dirs(0, AllowedDirs::new(&cli_allow_dirs, &[]).unwrap());
        assert!(
            !server.allowed_dirs.is_poisoned(),
            "replacement must clear poisoned state"
        );
        {
            let allowed = server.allowed_dirs.read().unwrap();
            assert!(ariad_host::confine::confine_input(&explicit_input, &allowed).is_ok());
            assert!(ariad_host::confine::confine_input(&root_input, &allowed).is_err());
        }
        server.replace_allowed_dirs(
            0,
            AllowedDirs::new(
                &cli_allow_dirs,
                &[ariad_host::confine::path_to_file_uri(root.path())],
            )
            .unwrap(),
        );
        assert!(
            ariad_host::confine::confine_input(&root_input, &server.allowed_dirs.read().unwrap())
                .is_ok()
        );
    }
}
