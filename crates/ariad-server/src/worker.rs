//! `ariad-server worker`: consumes pack queues and runs conversions through
//! `ariad_host`, exactly as the CLI does, under the job's plan limits.

use std::{
    path::{Path as FsPath, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

use ariad_core::limits::Limits;
use ariad_host::convert::{ConvertError, ConvertEvent, ConvertRequest};
use chrono::Utc;
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use tokio::sync::{Semaphore, mpsc};
use tokio_util::sync::CancellationToken;
use tracing::Instrument;
use uuid::Uuid;

use crate::{
    api::dto::{JobWarning, Profile},
    config::WorkerConfig,
    db::{self, JobOutcome, JobRow, JobStatus, NewArtifact, UploadStatus, UsageEvent},
    packs::Pack,
    queue::{Delivery, JobQueue},
    routing,
    storage::{self, Direction, Storage},
    telemetry::{self, Metrics},
};

/// Visibility timeout before the job's own timeout is known.
const CLAIM_VISIBILITY: Duration = Duration::from_secs(120);

/// Extra visibility beyond the job's timeout, for download and upload.
const VISIBILITY_MARGIN: Duration = Duration::from_secs(300);

/// Visibility for jobs whose plan sets no timeout.
const UNBOUNDED_VISIBILITY: Duration = Duration::from_secs(3600);

/// How often a running job checks for cancellation.
const CANCEL_POLL: Duration = Duration::from_secs(2);

/// Base delay before a failed attempt is retried.
const RETRY_BACKOFF: Duration = Duration::from_secs(5);

/// A worker process.
pub struct Worker {
    pub db: PgPool,
    pub queue: Arc<dyn JobQueue>,
    pub storage: Storage,
    pub config: WorkerConfig,
    pub metrics: Metrics,
}

/// How an attempt ended, for the queue.
#[derive(Debug, Eq, PartialEq)]
enum Disposition {
    /// Done for good: archive the message.
    Archive,
    /// Try again after a delay.
    Retry(Duration),
    /// Make the message visible again now (worker shutdown).
    Release,
}

/// A failure while running one attempt.
#[derive(Debug)]
struct Failure {
    code: &'static str,
    message: String,
    transient: bool,
}

impl Failure {
    fn permanent(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            transient: false,
        }
    }

    fn transient(code: &'static str, _cause: impl Into<String>) -> Self {
        let message = match code {
            "storage" => "object storage is unavailable",
            "database" => "job data is unavailable",
            "output_io" => "the output could not be written",
            "tool_missing" => "a required engine is missing on the worker",
            _ => "the conversion could not finish",
        };
        tracing::warn!(code, "conversion operation failed");
        Self {
            code,
            message: message.to_owned(),
            transient: true,
        }
    }

    fn from_convert(err: &ConvertError) -> Self {
        match err {
            ConvertError::ToolMissing
            | ConvertError::DoclingMissing
            | ConvertError::DoclingToolMissing => {
                Self::transient("tool_missing", "a required engine is missing on the worker")
            }
            ConvertError::Failed => Self::transient("failed", "the conversion engine failed"),
            ConvertError::OutputIo => {
                Self::transient("output_io", "the output could not be written")
            }
            ConvertError::LimitExceeded | ConvertError::MemoryLimitExceeded { .. } => {
                Self::permanent("limit_exceeded", "the document exceeds the plan's limits")
            }
            ConvertError::InputIo => Self::permanent("input_io", "the input could not be read"),
            other => Self::permanent(other.code(), "the conversion could not finish"),
        }
    }
}

impl Worker {
    /// Runs until `shutdown`. Jobs still running then are handed back to the queue.
    pub async fn run(self: Arc<Self>, shutdown: CancellationToken) {
        log_work_dir(&self.config.work_dir);
        let slots = Arc::new(Semaphore::new(self.config.concurrency));
        let mut loops = tokio::task::JoinSet::new();
        for &pack in &self.config.packs {
            let worker = Arc::clone(&self);
            let slots = Arc::clone(&slots);
            let shutdown = shutdown.clone();
            loops.spawn(async move { worker.consume(pack, slots, shutdown).await });
        }
        while loops.join_next().await.is_some() {}
    }

    async fn consume(
        self: Arc<Self>,
        pack: Pack,
        slots: Arc<Semaphore>,
        shutdown: CancellationToken,
    ) {
        let mut running = tokio::task::JoinSet::new();
        loop {
            let permit = tokio::select! {
                () = shutdown.cancelled() => break,
                permit = Arc::clone(&slots).acquire_owned() => match permit {
                    Ok(permit) => permit,
                    Err(_) => break,
                },
            };
            let delivery = tokio::select! {
                () = shutdown.cancelled() => break,
                // A permit must never be held while long-polling an empty
                // queue: another pack may have work ready for this slot.
                read = self.queue.read(pack, CLAIM_VISIBILITY, Duration::ZERO) => read,
            };
            match delivery {
                Ok(Some(delivery)) => {
                    let worker = Arc::clone(&self);
                    let cancel = shutdown.child_token();
                    running.spawn(async move {
                        worker.handle(delivery, cancel).await;
                        drop(permit);
                    });
                }
                Ok(None) => {
                    drop(permit);
                    tokio::select! {
                        () = shutdown.cancelled() => break,
                        () = tokio::time::sleep(self.config.poll) => {}
                    }
                }
                Err(err) => {
                    drop(permit);
                    tracing::warn!(error = %err, queue = pack.queue(), "queue read failed");
                    tokio::select! {
                        () = shutdown.cancelled() => break,
                        () = tokio::time::sleep(Duration::from_secs(2)) => {}
                    }
                }
            }
            while running.try_join_next().is_some() {}
        }
        while running.join_next().await.is_some() {}
    }

    /// Processes one delivery and settles its message.
    pub async fn handle(&self, delivery: Delivery, shutdown: CancellationToken) {
        let job_id = delivery.message.as_ref().map(|m| m.job_id);
        let span = tracing::info_span!(
            "job",
            job_id = %job_id.map(|id| id.to_string()).unwrap_or_default(),
            pack = delivery.pack.as_str(),
            attempt = delivery.read_count,
        );
        telemetry::set_parent_from(&span, &delivery.headers);
        async {
            if delivery.read_count == 1 {
                let waited = (Utc::now() - delivery.enqueued_at).num_milliseconds();
                #[allow(clippy::cast_precision_loss)]
                self.metrics
                    .queue_wait(delivery.pack.as_str(), waited as f64 / 1000.0);
            }
            let disposition = match job_id {
                Some(job_id) => match self.attempt(&delivery, job_id, &shutdown).await {
                    Ok(disposition) => disposition,
                    Err(err) => {
                        // The database is unreachable; the message reappears
                        // after its visibility timeout.
                        tracing::error!(error = %err, "job bookkeeping failed");
                        return;
                    }
                },
                None => {
                    tracing::warn!("discarding a message without a job id");
                    Disposition::Archive
                }
            };
            let pack = delivery.pack;
            let settled = match disposition {
                Disposition::Archive => self.queue.archive(pack, delivery.msg_id).await,
                Disposition::Retry(delay) => self.queue.extend(pack, delivery.msg_id, delay).await,
                Disposition::Release => self.queue.release(pack, delivery.msg_id).await,
            };
            if let Err(err) = settled {
                tracing::warn!(error = %err, "could not settle the queue message");
            }
        }
        .instrument(span)
        .await;
    }

    async fn attempt(
        &self,
        delivery: &Delivery,
        job_id: Uuid,
        shutdown: &CancellationToken,
    ) -> Result<Disposition, sqlx::Error> {
        // Own the session outside the pool so lock holders cannot exhaust
        // bookkeeping connections. Dropping it releases the advisory lock.
        let mut lock_conn = self.db.acquire().await?.detach();
        let locked: bool =
            sqlx::query_scalar("SELECT pg_try_advisory_lock(hashtextextended($1, 0))")
                .bind(format!("ariad-job:{job_id}"))
                .fetch_one(&mut lock_conn)
                .await?;
        if !locked {
            return Ok(Disposition::Retry(RETRY_BACKOFF));
        }
        let Some(job) = db::start_job(&self.db, job_id).await? else {
            tracing::info!("job is no longer runnable; dropping its message");
            return Ok(Disposition::Archive);
        };
        let attempts = u32::try_from(job.attempts).unwrap_or(u32::MAX);
        if attempts > self.config.max_attempts {
            let failure = Failure::permanent(
                "failed",
                format!("the job failed after {} attempts", self.config.max_attempts),
            );
            self.fail(&job, &failure, Vec::new()).await?;
            return Ok(Disposition::Archive);
        }
        let mut conn = self.db.acquire().await?;
        db::add_event(&mut conn, job.id, "started", json!({ "attempt": attempts })).await?;
        drop(conn);

        let limits: Limits = match serde_json::from_value(job.limits.0.clone()) {
            Ok(limits) => limits,
            Err(_) => {
                let failure = Failure::permanent("failed", "the job's limits are unreadable");
                self.fail(&job, &failure, Vec::new()).await?;
                return Ok(Disposition::Archive);
            }
        };
        let visibility = limits.timeout_s.map_or(UNBOUNDED_VISIBILITY, |s| {
            Duration::from_secs(s) + VISIBILITY_MARGIN
        });
        self.queue
            .extend(delivery.pack, delivery.msg_id, visibility)
            .await?;

        let cancel = shutdown.child_token();
        let heartbeat = tokio::spawn(heartbeat(
            self.db.clone(),
            Arc::clone(&self.queue),
            delivery.pack,
            delivery.msg_id,
            job.id,
            visibility,
            cancel.clone(),
        ));
        let dir = storage::job_dir(&self.config.work_dir, job.id);
        let started = Instant::now();
        let result = self.execute(&job, &limits, &dir, &cancel).await;
        heartbeat.abort();
        if let Err(err) = tokio::fs::remove_dir_all(&dir).await
            && err.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!(error = %err, "could not remove the job directory");
        }
        let elapsed = started.elapsed();
        let route = format!("{}->{}", job.source_format, job.target_format);

        match result {
            Ok(done) => {
                self.metrics
                    .conversion(&route, &job.profile, elapsed.as_secs_f64(), true);
                self.succeed(&job, done, elapsed).await?;
                Ok(Disposition::Archive)
            }
            Err(Outcome::Canceled) => {
                if shutdown.is_cancelled() {
                    let mut tx = self.db.begin().await?;
                    if db::requeue_job(&mut tx, job.id, true).await? {
                        db::add_event(
                            &mut tx,
                            job.id,
                            "retrying",
                            json!({ "reason": "worker_shutdown" }),
                        )
                        .await?;
                    }
                    tx.commit().await?;
                    tracing::info!("worker shutting down; job handed back to the queue");
                    Ok(Disposition::Release)
                } else {
                    tracing::info!("job canceled while running");
                    Ok(Disposition::Archive)
                }
            }
            Err(Outcome::Failed(failure, warnings)) => {
                self.metrics
                    .conversion(&route, &job.profile, elapsed.as_secs_f64(), false);
                if failure.code == "failed" {
                    self.metrics.engine_crash(&engine_label(&job));
                }
                if failure.transient && attempts < self.config.max_attempts {
                    let mut tx = self.db.begin().await?;
                    if db::requeue_job(&mut tx, job.id, false).await? {
                        db::add_event(
                            &mut tx,
                            job.id,
                            "retrying",
                            json!({ "code": failure.code, "message": failure.message }),
                        )
                        .await?;
                    }
                    tx.commit().await?;
                    tracing::warn!(code = failure.code, "attempt failed; retrying");
                    Ok(Disposition::Retry(RETRY_BACKOFF * attempts))
                } else {
                    self.fail(&job, &failure, warnings).await?;
                    Ok(Disposition::Archive)
                }
            }
        }
    }

    async fn execute(
        &self,
        job: &JobRow,
        limits: &Limits,
        dir: &FsPath,
        cancel: &CancellationToken,
    ) -> Result<Done, Outcome> {
        let upload = db::get_upload(&self.db, job.upload_id)
            .await
            .map_err(|err| Outcome::transient("database", err.to_string()))?
            .filter(|u| u.status() == UploadStatus::Completed)
            .ok_or_else(|| {
                Outcome::Failed(
                    Failure::permanent("input_io", "the input is no longer available"),
                    Vec::new(),
                )
            })?;
        let source = routing::resolve_format(&job.source_format)
            .ok_or_else(|| Outcome::permanent("unsupported_route", "unknown source format"))?;
        let target = routing::resolve_format(&job.target_format)
            .ok_or_else(|| Outcome::permanent("unsupported_route", "unknown target format"))?;

        if let Err(err) = tokio::fs::remove_dir_all(dir).await
            && err.kind() != std::io::ErrorKind::NotFound
        {
            return Err(Outcome::transient("output_io", err.to_string()));
        }
        let out_dir = dir.join("out");
        tokio::fs::create_dir_all(&out_dir)
            .await
            .map_err(|err| Outcome::transient("output_io", err.to_string()))?;
        let input = dir.join(format!("input.{}", routing::output_extension(source)));
        let declared = u64::try_from(upload.size_bytes).unwrap_or(0);
        let downloaded = tokio::select! {
            () = cancel.cancelled() => return Err(Outcome::Canceled),
            result = self.storage.download(&upload.storage_key, &input, declared) => result,
        };
        match downloaded {
            Ok((size, sha)) if size == declared && sha == upload.sha256 => {}
            Ok(_) => {
                return Err(Outcome::permanent(
                    "input_io",
                    "the stored input does not match its hash",
                ));
            }
            Err(err) if err.is_not_found() => {
                return Err(Outcome::permanent(
                    "input_io",
                    "the input is no longer available",
                ));
            }
            Err(storage::StorageError::TooLarge(_)) => {
                return Err(Outcome::permanent(
                    "input_io",
                    "the stored input exceeds its declared size",
                ));
            }
            Err(err) => return Err(Outcome::transient("storage", err.to_string())),
        }

        let stem = output_stem(&upload.filename);
        let extension = routing::output_extension(target);
        let output = out_dir.join(format!("output.{extension}"));
        let request = ConvertRequest {
            input,
            output: output.clone(),
            target_format: routing::target_arg(target).to_owned(),
            profile: Profile::parse(&job.profile).unwrap_or_default().into(),
            overwrite: false,
            engine_program: self.config.engine_program.clone(),
            title_fallback: Some(stem.clone()),
            asset_base_dir: None,
        };
        let (events_tx, events_rx) = mpsc::unbounded_channel::<String>();
        let progress = tokio::spawn(forward_progress(self.db.clone(), job.id, events_rx));
        let limits = limits.clone();
        let convert_cancel = cancel.clone();
        let conversion_program = self.config.conversion_program.clone();
        let sandbox_mode = self.config.sandbox.host_mode();
        let span = tracing::Span::current();
        let converted = tokio::task::spawn_blocking(move || {
            let _entered = span.enter();
            ariad_host::convert::isolated::convert(
                &conversion_program,
                &request,
                limits,
                sandbox_mode,
                convert_cancel,
                |event| {
                    let ConvertEvent::Progress { stage } = event;
                    let _ = events_tx.send(stage);
                },
            )
        })
        .await;
        let _ = progress.await;
        let report = match converted {
            Ok(Ok(report)) => report,
            Ok(Err(ariad_host::convert::isolated::Error::Convert(ConvertError::Interrupted))) => {
                return Err(Outcome::Canceled);
            }
            Ok(Err(err)) if cancel.is_cancelled() => {
                tracing::debug!(error = %err, "conversion stopped after cancellation");
                return Err(Outcome::Canceled);
            }
            Ok(Err(ariad_host::convert::isolated::Error::Convert(err))) => {
                return Err(Outcome::Failed(Failure::from_convert(&err), Vec::new()));
            }
            Ok(Err(ariad_host::convert::isolated::Error::Protocol)) => {
                return Err(Outcome::permanent(
                    "failed",
                    "the conversion helper returned invalid protocol data",
                ));
            }
            Ok(Err(ariad_host::convert::isolated::Error::Terminated)) => {
                return Err(Outcome::permanent(
                    "failed",
                    "the conversion helper was terminated",
                ));
            }
            Err(join) => return Err(Outcome::transient("failed", join.to_string())),
        };
        let warnings: Vec<JobWarning> =
            ariad_host::convert::isolated::bounded_warnings(report.warnings)
                .into_iter()
                .map(|w| JobWarning {
                    code: w.code,
                    message: w.message,
                })
                .collect();

        let (size, sha) = hash_file(output.clone()).await.map_err(|err| {
            Outcome::Failed(
                Failure::transient("output_io", err.to_string()),
                warnings.clone(),
            )
        })?;
        let key = self
            .storage
            .key(job.workspace_id, job.id, Direction::Out, &sha);
        if cancel.is_cancelled() {
            return Err(Outcome::Canceled);
        }
        self.storage
            .upload_file(&key, &output)
            .await
            .map_err(|err| {
                Outcome::Failed(
                    Failure::transient("storage", err.to_string()),
                    warnings.clone(),
                )
            })?;
        Ok(Done {
            filename: format!("{stem}.{extension}"),
            format: target.id().to_owned(),
            media_type: target.media_type().to_owned(),
            size,
            sha256: sha,
            key,
            input_bytes: upload.size_bytes,
            route: report
                .route
                .iter()
                .map(|s| {
                    (
                        s.from.id().to_owned(),
                        s.to.id().to_owned(),
                        s.engine.clone(),
                    )
                })
                .collect(),
            warnings,
        })
    }

    async fn succeed(
        &self,
        job: &JobRow,
        done: Done,
        elapsed: Duration,
    ) -> Result<(), sqlx::Error> {
        let warnings = serde_json::to_value(&done.warnings).unwrap_or_else(|_| json!([]));
        let mut tx = self.db.begin().await?;
        let finished = db::finish_job(
            &mut tx,
            job.id,
            &JobOutcome {
                status: JobStatus::Succeeded,
                error_code: None,
                error_message: None,
                warnings,
            },
        )
        .await?;
        if !finished {
            // Canceled or deleted meanwhile: keep nothing.
            tx.rollback().await?;
            let referenced = db::live_artifacts(&self.db, job.id)
                .await?
                .iter()
                .any(|artifact| artifact.storage_key == done.key);
            if !referenced && let Err(err) = self.storage.delete(&done.key).await {
                tracing::warn!(error = %err, "could not delete the output of a canceled job");
            }
            tracing::info!("job was canceled before it could finish; output discarded");
            return Ok(());
        }
        db::insert_artifact(
            &mut tx,
            &NewArtifact {
                id: Uuid::new_v4(),
                job_id: job.id,
                format: &done.format,
                filename: &done.filename,
                media_type: &done.media_type,
                size_bytes: i64::try_from(done.size).unwrap_or(i64::MAX),
                sha256: &done.sha256,
                storage_key: &done.key,
            },
        )
        .await?;
        if !done.route.is_empty() {
            db::set_job_steps(&mut tx, job.id, &done.route).await?;
        }
        let engine_ms = i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX);
        db::record_usage(
            &mut tx,
            &UsageEvent {
                event_key: format!("job:{}:completed", job.id),
                workspace_id: job.workspace_id,
                subject: &job.subject,
                job_id: Some(job.id),
                kind: "job_completed",
                input_bytes: done.input_bytes,
                output_bytes: i64::try_from(done.size).unwrap_or(i64::MAX),
                pages: 0,
                engine_ms,
            },
        )
        .await?;
        db::add_event(
            &mut tx,
            job.id,
            "succeeded",
            json!({ "outputs": 1, "bytes": done.size, "elapsed_ms": engine_ms }),
        )
        .await?;
        tx.commit().await?;
        tracing::info!(bytes = done.size, elapsed_ms = engine_ms, "job succeeded");
        Ok(())
    }

    async fn fail(
        &self,
        job: &JobRow,
        failure: &Failure,
        warnings: Vec<JobWarning>,
    ) -> Result<(), sqlx::Error> {
        let mut tx = self.db.begin().await?;
        let finished = db::finish_job(
            &mut tx,
            job.id,
            &JobOutcome {
                status: JobStatus::Failed,
                error_code: Some(failure.code),
                error_message: Some(&failure.message),
                warnings: serde_json::to_value(&warnings).unwrap_or_else(|_| json!([])),
            },
        )
        .await?;
        if finished {
            db::add_event(
                &mut tx,
                job.id,
                "failed",
                json!({ "code": failure.code, "message": failure.message }),
            )
            .await?;
        }
        tx.commit().await?;
        tracing::warn!(code = failure.code, "job failed");
        Ok(())
    }
}

/// A converted, uploaded output.
struct Done {
    filename: String,
    format: String,
    media_type: String,
    size: u64,
    sha256: String,
    key: String,
    input_bytes: i64,
    route: Vec<(String, String, String)>,
    warnings: Vec<JobWarning>,
}

enum Outcome {
    Canceled,
    Failed(Failure, Vec<JobWarning>),
}

impl Outcome {
    fn permanent(code: &'static str, message: impl Into<String>) -> Self {
        Self::Failed(Failure::permanent(code, message), Vec::new())
    }

    fn transient(code: &'static str, message: impl Into<String>) -> Self {
        Self::Failed(Failure::transient(code, message), Vec::new())
    }
}

/// Keeps the message invisible while the job runs and stops the job when it
/// is canceled.
async fn heartbeat(
    pool: PgPool,
    queue: Arc<dyn JobQueue>,
    pack: Pack,
    msg_id: i64,
    job_id: Uuid,
    visibility: Duration,
    cancel: CancellationToken,
) {
    let extend_every = (visibility / 3).max(CANCEL_POLL);
    let mut last_extend = Instant::now();
    loop {
        tokio::select! {
            () = cancel.cancelled() => return,
            () = tokio::time::sleep(CANCEL_POLL) => {}
        }
        match db::job_status(&pool, job_id).await {
            Ok(Some(JobStatus::Running)) => {}
            Ok(_) => {
                cancel.cancel();
                return;
            }
            Err(err) => tracing::warn!(error = %err, "cancellation check failed"),
        }
        if last_extend.elapsed() >= extend_every {
            match queue.extend(pack, msg_id, visibility).await {
                Ok(()) => last_extend = Instant::now(),
                Err(err) => tracing::warn!(error = %err, "could not extend the message visibility"),
            }
        }
    }
}

/// Stores progress events, dropping repeats of the same stage.
async fn forward_progress(pool: PgPool, job_id: Uuid, mut rx: mpsc::UnboundedReceiver<String>) {
    let mut last: Option<String> = None;
    while let Some(stage) = rx.recv().await {
        if last.as_deref() == Some(stage.as_str()) {
            continue;
        }
        if let Ok(mut conn) = pool.acquire().await
            && let Err(err) =
                db::add_event(&mut conn, job_id, "progress", json!({ "stage": stage })).await
        {
            tracing::warn!(error = %err, "could not store a progress event");
        }
        last = Some(stage);
    }
}

async fn hash_file(path: PathBuf) -> std::io::Result<(u64, String)> {
    tokio::task::spawn_blocking(move || {
        use std::io::Read;
        let mut file = std::fs::File::open(path)?;
        let mut hasher = Sha256::new();
        let mut buf = vec![0u8; 256 * 1024];
        let mut size = 0u64;
        loop {
            let n = file.read(&mut buf)?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
            size += n as u64;
        }
        Ok((size, hex::encode(hasher.finalize())))
    })
    .await
    .map_err(std::io::Error::other)?
}

/// The output's base name: the upload's file stem, or `document`.
fn output_stem(filename: &str) -> String {
    let stem = FsPath::new(filename)
        .file_stem()
        .and_then(|s| s.to_str())
        .map(str::trim)
        .unwrap_or_default();
    if stem.is_empty() {
        "document".to_owned()
    } else {
        stem.to_owned()
    }
}

/// The non-native engine of a job's route, for crash metrics.
fn engine_label(job: &JobRow) -> String {
    let pack = job.pack();
    match pack {
        Pack::Core => "pandoc".to_owned(),
        Pack::Office => "libreoffice".to_owned(),
        Pack::Docling => "docling".to_owned(),
    }
}

/// Locks a worker-specific directory for its lifetime before reclaiming
/// abandoned job directories. Never follows a directory symlink.
pub fn prepare_work_dir(dir: &FsPath) -> std::io::Result<std::fs::File> {
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join(".ariad-worker.lock"))?;
    lock.try_lock().map_err(std::io::Error::other)?;
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name();
        if name
            .to_str()
            .and_then(|name| name.strip_prefix("ariad-job-"))
            .is_some_and(|id| Uuid::parse_str(id).is_ok())
            && entry.file_type()?.is_dir()
        {
            std::fs::remove_dir_all(entry.path())?;
        }
    }
    Ok(lock)
}

/// Logs whether the work directory is on tmpfs, as recommended for workers.
fn log_work_dir(dir: &FsPath) {
    match on_tmpfs(dir) {
        Some(true) => tracing::info!(dir = %dir.display(), "job workspaces are on tmpfs"),
        Some(false) => tracing::warn!(
            dir = %dir.display(),
            "job workspaces are not on tmpfs; mount one there to keep inputs off disk"
        ),
        None => tracing::info!(dir = %dir.display(), "job workspace directory"),
    }
}

/// Whether `dir` lives on a tmpfs mount (Linux only).
#[must_use]
pub fn on_tmpfs(dir: &FsPath) -> Option<bool> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    let canonical = std::fs::canonicalize(dir).ok()?;
    let mounts = std::fs::read_to_string("/proc/self/mounts").ok()?;
    let mut best: Option<(usize, bool)> = None;
    for line in mounts.lines() {
        let mut fields = line.split_whitespace();
        let (Some(_source), Some(target), Some(fstype)) =
            (fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        let target = target.replace("\\040", " ");
        if canonical.starts_with(&target) && best.is_none_or(|(len, _)| target.len() >= len) {
            best = Some((target.len(), fstype == "tmpfs"));
        }
    }
    best.map(|(_, tmpfs)| tmpfs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_names_use_the_upload_stem() {
        assert_eq!(output_stem("report.final.docx"), "report.final");
        assert_eq!(output_stem("notes"), "notes");
        assert_eq!(output_stem(".md"), ".md");
        assert_eq!(output_stem(""), "document");
    }

    #[test]
    fn convert_errors_split_into_transient_and_permanent() {
        assert!(Failure::from_convert(&ConvertError::Failed).transient);
        assert!(Failure::from_convert(&ConvertError::ToolMissing).transient);
        for error in [
            ConvertError::DoclingMissing,
            ConvertError::DoclingToolMissing,
        ] {
            let failure = Failure::from_convert(&error);
            assert!(failure.transient);
            assert_eq!(failure.code, "tool_missing");
            assert_eq!(
                failure.message,
                "a required engine is missing on the worker"
            );
        }
        assert!(!Failure::from_convert(&ConvertError::LimitExceeded).transient);
        let rss = Failure::from_convert(&ConvertError::MemoryLimitExceeded { limit_mb: 1 });
        assert!(!rss.transient);
        assert_eq!(rss.code, "limit_exceeded");
        let sandbox =
            Failure::from_convert(&ConvertError::SandboxUnavailable("unavailable".to_owned()));
        assert!(!sandbox.transient);
        assert_eq!(sandbox.code, "sandbox_unavailable");
        assert_eq!(sandbox.message, "the conversion could not finish");
        assert!(!Failure::from_convert(&ConvertError::UnsupportedRoute { detail: None }).transient);
        assert!(!Failure::from_convert(&ConvertError::InputIo).transient);
        assert_eq!(
            Failure::from_convert(&ConvertError::LimitExceeded).code,
            "limit_exceeded"
        );
    }

    #[test]
    fn tmpfs_detection_answers_on_linux() {
        let answer = on_tmpfs(&std::env::temp_dir());
        if cfg!(target_os = "linux") {
            assert!(answer.is_some());
        } else {
            assert!(answer.is_none());
        }
    }

    #[test]
    fn startup_reclaims_only_job_directories_with_exclusive_ownership() {
        let dir = tempfile::tempdir().unwrap();
        let stale = dir.path().join(format!("ariad-job-{}", Uuid::new_v4()));
        std::fs::create_dir(&stale).unwrap();
        std::fs::write(stale.join("input.md"), "abandoned").unwrap();
        let unrelated = dir.path().join("ariad-job-other-data");
        std::fs::create_dir(&unrelated).unwrap();
        let lock = prepare_work_dir(dir.path()).unwrap();
        assert!(!stale.exists());
        assert!(unrelated.exists());
        let active = dir.path().join(format!("ariad-job-{}", Uuid::new_v4()));
        std::fs::create_dir(&active).unwrap();
        assert!(prepare_work_dir(dir.path()).is_err());
        assert!(active.exists());
        drop(lock);
        let _lock = prepare_work_dir(dir.path()).unwrap();
        assert!(!active.exists());
    }
}
