//! Process wiring for `api` and `worker`, callable from tests.

use std::{net::SocketAddr, sync::Arc};

use sqlx::PgPool;
use thiserror::Error;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

use crate::{
    api::{self, AppState, Inner, RouterOptions, events::EventHub},
    auth::Authenticator,
    config::{ApiConfig, WorkerConfig},
    queue::{JobQueue, PgmqQueue},
    sandbox,
    storage::{Storage, StorageError},
    sweeper,
    telemetry::Metrics,
    worker::Worker,
};

#[derive(Debug, Error)]
pub enum StartError {
    #[error(transparent)]
    Storage(#[from] StorageError),
    #[error(transparent)]
    Sandbox(#[from] sandbox::SandboxError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Builds the API state.
pub fn api_state(config: &ApiConfig, pool: PgPool) -> Result<AppState, StartError> {
    let storage = Storage::from_config(
        &config.storage,
        config.public_url.as_ref(),
        Some(&config.secret),
    )?;
    let queue: Arc<dyn JobQueue> =
        Arc::new(PgmqQueue::new(pool.clone()).with_active_limit(config.global_active_jobs));
    let auth = Authenticator::new(
        config.auth.clone(),
        config.secret.clone(),
        config.trusted_client_ip_header.clone(),
        crate::http_client(),
    );
    Ok(AppState::new(Inner {
        db: pool,
        storage,
        queue,
        auth,
        events: EventHub::new(),
        packs: config.packs.clone(),
        metrics: Metrics::new(),
    }))
}

/// Serves the API on `listener` until `shutdown`, with the event listener
/// and the retention sweeper running alongside.
pub async fn serve_api(
    listener: TcpListener,
    config: ApiConfig,
    pool: PgPool,
    shutdown: CancellationToken,
) -> Result<(), StartError> {
    let state = api_state(&config, pool.clone())?;
    let hub = tokio::spawn(state.events.clone().run(pool.clone(), shutdown.clone()));
    let sweep = tokio::spawn(sweeper::run(
        pool,
        state.storage.clone(),
        config.sweep_interval,
        config.sweep_batch,
        shutdown.clone(),
    ));
    let mcp = crate::mcp::router(state.clone(), &config, shutdown.clone());
    let router = api::router(
        state,
        &RouterOptions {
            cors_origins: config.cors_origins.clone(),
            request_timeout: config.request_timeout,
        },
    )
    .merge(mcp);
    tracing::info!(addr = %listener.local_addr()?, packs = ?config.packs, "API listening");
    axum::serve(
        listener,
        router.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown.clone().cancelled_owned())
    .await?;
    shutdown.cancel();
    let _ = hub.await;
    let _ = sweep.await;
    Ok(())
}

/// Runs a worker until `shutdown`.
pub async fn run_worker(
    config: WorkerConfig,
    pool: PgPool,
    shutdown: CancellationToken,
) -> Result<(), StartError> {
    sandbox::protect_worker_credentials()?;
    let status = sandbox::preflight_worker_with_support(
        config.sandbox,
        config.concurrency,
        &ariad_host::sandbox::support(),
    )?;
    if !status.missing.is_empty() {
        tracing::warn!(
            mode = %status.mode,
            missing = ?status.missing,
            "engine isolation is incomplete on this host"
        );
    }
    std::fs::create_dir_all(&config.work_dir)?;
    let _work_dir = crate::worker::prepare_work_dir(&config.work_dir)?;
    let storage = Storage::from_config(&config.storage, None, None)?;
    let queue: Arc<dyn JobQueue> = Arc::new(PgmqQueue::new(pool.clone()));
    tracing::info!(
        packs = ?config.packs,
        concurrency = config.concurrency,
        engine = %config.engine_program.display(),
        sandbox = %config.sandbox,
        "worker started"
    );
    let worker = Arc::new(Worker {
        db: pool,
        queue,
        storage,
        config,
        metrics: Metrics::new(),
    });
    worker.run(shutdown).await;
    Ok(())
}
