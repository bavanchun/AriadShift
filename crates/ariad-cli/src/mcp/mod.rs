//! MCP stdio server module for AriadShift.

pub mod resources;
pub mod tools;

use std::{
    path::PathBuf,
    pin::Pin,
    process::ExitCode,
    sync::{Arc, RwLock},
    task::{Context, Poll},
    time::Duration,
};

use ariad_host::confine::AllowedDirs;
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

impl McpServer {
    #[allow(deprecated)]
    async fn refresh_roots_with_peer(&self, peer: &rmcp::service::Peer<RoleServer>) {
        if let Ok(result) = peer.list_roots().await {
            let mut roots_uris = Vec::new();
            for root in result.roots {
                roots_uris.push(root.uri);
            }
            // Keep existing --allow-dir directories that still exist
            let existing_allow_dirs: Vec<PathBuf> = self
                .cli_allow_dirs
                .iter()
                .filter(|p| p.is_dir())
                .cloned()
                .collect();
            match AllowedDirs::new(&existing_allow_dirs, &roots_uris) {
                Ok(new_allowed) => {
                    if let Ok(mut allowed) = self.allowed_dirs.write() {
                        *allowed = new_allowed;
                    }
                }
                Err(_) => {
                    // Fail closed on error (empty allowed set) per M2
                    if let Ok(mut allowed) = self.allowed_dirs.write() {
                        *allowed = AllowedDirs::default();
                    }
                }
            }
        }
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
        self.refresh_roots_with_peer(&context.peer).await;
    }

    async fn on_roots_list_changed(&self, context: rmcp::service::NotificationContext<RoleServer>) {
        self.refresh_roots_with_peer(&context.peer).await;
    }
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

    let server = McpServer {
        allow_overwrite: args.allow_overwrite,
        allowed_dirs: Arc::new(RwLock::new(initial_allowed)),
        cli_allow_dirs: args.allow_dir,
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
