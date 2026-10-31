//! Warm engine sessions.
//!
//! A [`SessionPool`] keeps engine processes that report `supports_session: true` alive between
//! jobs, keyed by their [`EngineCommand`]. Each session owns a directory that is both the
//! sandbox's read-write root and the parent of every job workspace in that session, so a
//! restricted engine can reach the workspaces of later jobs. Engines without session support
//! get a one-shot lease that behaves exactly like [`runner::run_command`].

use std::{
    collections::HashMap,
    sync::{Arc, Condvar, Mutex, MutexGuard, Weak},
    thread,
    time::{Duration, Instant},
};

use ariad_core::protocol::{ErrorCode, Event, Request};
use tempfile::TempDir;
use tokio::runtime::{Builder, Runtime};
use tokio_util::sync::CancellationToken;

use crate::{
    runner::{
        self, DESCRIBE_TIMEOUT, EngineCommand, EngineProcess, JobMode, JobSpec, RunError,
        RunOutcome,
    },
    sandbox::SandboxOutcome,
    workspace::{self, Workspace, WorkspaceError},
};

/// How long a closing session may take to exit after its stdin closes before it is killed.
pub const CLOSE_GRACE: Duration = Duration::from_secs(2);

const SESSION_PREFIX: &str = "ariadshift-session-";

/// Limits for warm engine processes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PoolConfig {
    /// An idle session is closed after this long without a job.
    pub idle_timeout: Duration,
    /// A process is retired after this many jobs.
    pub max_jobs_per_process: u32,
    /// Idle sessions kept per engine command; extra ones close when their job ends.
    pub max_idle_per_engine: usize,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            idle_timeout: Duration::from_secs(60),
            max_jobs_per_process: 100,
            max_idle_per_engine: 2,
        }
    }
}

/// A pool of warm engine processes shared by every conversion that uses it.
///
/// Clones share one pool. Dropping the last clone, or calling [`SessionPool::shutdown`], closes
/// every idle session.
#[derive(Clone)]
pub struct SessionPool {
    inner: Arc<PoolInner>,
}

struct PoolInner {
    config: PoolConfig,
    state: Mutex<PoolState>,
    reaper: Arc<ReaperSignal>,
}

#[derive(Default)]
struct PoolState {
    idle: HashMap<EngineCommand, Vec<Session>>,
    supports_session: HashMap<EngineCommand, bool>,
    shutdown: bool,
    reaper_started: bool,
}

#[derive(Default)]
struct ReaperSignal {
    stop: Mutex<bool>,
    wake: Condvar,
}

impl SessionPool {
    #[must_use]
    pub fn new(config: PoolConfig) -> Self {
        Self {
            inner: Arc::new(PoolInner {
                config,
                state: Mutex::new(PoolState::default()),
                reaper: Arc::new(ReaperSignal::default()),
            }),
        }
    }

    /// Leases an engine for one job: a warm session when the engine supports sessions (asked
    /// once per pool through `describe`), otherwise a one-shot process.
    pub fn lease(
        &self,
        command: &EngineCommand,
        cancel: CancellationToken,
    ) -> Result<Lease<'_>, RunError> {
        if !self.supports_session(command, cancel)? {
            return Ok(Lease::new(Some(self), command.clone(), None));
        }
        while let Some(mut session) = self.take_idle(command) {
            if session.is_reusable(&self.inner.config) {
                return Ok(Lease::new(Some(self), command.clone(), Some(session)));
            }
            session.close(false);
        }
        Ok(Lease::new(
            Some(self),
            command.clone(),
            Some(Session::new()?),
        ))
    }

    /// Closes every idle session now; sessions in use close when their job ends.
    pub fn shutdown(&self) {
        let idle = {
            let mut state = self.inner.lock();
            state.shutdown = true;
            std::mem::take(&mut state.idle)
        };
        self.inner.reaper.stop();
        close_all(idle.into_values().flatten());
    }

    /// The number of idle sessions across all engines.
    #[must_use]
    pub fn idle_sessions(&self) -> usize {
        self.inner.lock().idle.values().map(Vec::len).sum()
    }

    fn supports_session(
        &self,
        command: &EngineCommand,
        cancel: CancellationToken,
    ) -> Result<bool, RunError> {
        if let Some(known) = self.inner.lock().supports_session.get(command) {
            return Ok(*known);
        }
        let supported = match runner::describe_command(command, DESCRIBE_TIMEOUT, cancel)? {
            Event::Capabilities {
                supports_session, ..
            } => supports_session,
            _ => false,
        };
        self.inner
            .lock()
            .supports_session
            .insert(command.clone(), supported);
        Ok(supported)
    }

    fn take_idle(&self, command: &EngineCommand) -> Option<Session> {
        self.inner.lock().idle.get_mut(command)?.pop()
    }

    fn release(&self, command: &EngineCommand, mut session: Session) {
        session.idle_since = Instant::now();
        if session.jobs >= self.inner.config.max_jobs_per_process {
            session.close(true);
            return;
        }
        let mut state = self.inner.lock();
        let full = state
            .idle
            .get(command)
            .is_some_and(|list| list.len() >= self.inner.config.max_idle_per_engine);
        if state.shutdown || full {
            drop(state);
            session.close(true);
            return;
        }
        state.idle.entry(command.clone()).or_default().push(session);
        let start_reaper = !std::mem::replace(&mut state.reaper_started, true);
        drop(state);
        if start_reaper {
            start_reaper_thread(&self.inner);
        }
        self.inner.reaper.wake.notify_all();
    }
}

impl PoolInner {
    fn lock(&self) -> MutexGuard<'_, PoolState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Removes idle sessions past the idle timeout and returns when the next one expires.
    fn take_expired(&self) -> (Vec<Session>, Option<Duration>) {
        let timeout = self.config.idle_timeout;
        let now = Instant::now();
        let mut state = self.lock();
        let mut expired = Vec::new();
        let mut next: Option<Duration> = None;
        for sessions in state.idle.values_mut() {
            let mut index = 0;
            while index < sessions.len() {
                let age = now.saturating_duration_since(sessions[index].idle_since);
                if age >= timeout {
                    expired.push(sessions.remove(index));
                } else {
                    let left = timeout - age;
                    next = Some(next.map_or(left, |current| current.min(left)));
                    index += 1;
                }
            }
        }
        state.idle.retain(|_, sessions| !sessions.is_empty());
        (expired, next)
    }
}

impl Drop for PoolInner {
    fn drop(&mut self) {
        self.reaper.stop();
        let idle = std::mem::take(&mut self.lock().idle);
        close_all(idle.into_values().flatten());
    }
}

impl ReaperSignal {
    fn stop(&self) {
        *self
            .stop
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = true;
        self.wake.notify_all();
    }

    /// Sleeps until `timeout` passes, a session is released, or the pool stops. Returns `false`
    /// once the pool has stopped.
    fn sleep(&self, timeout: Duration) -> bool {
        let stopped = self
            .stop
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if *stopped {
            return false;
        }
        let (stopped, _) = self
            .wake
            .wait_timeout(stopped, timeout)
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        !*stopped
    }
}

/// Closes idle sessions once they pass the idle timeout. The thread holds only a weak reference
/// between passes, so dropping the pool is never delayed by it.
fn start_reaper_thread(inner: &Arc<PoolInner>) {
    let pool = Arc::downgrade(inner);
    let signal = Arc::clone(&inner.reaper);
    let spawned = thread::Builder::new()
        .name("ashift-session-reaper".to_owned())
        .spawn(move || reap_idle_sessions(&pool, &signal));
    if spawned.is_err() {
        // Without the reaper, idle sessions still close at shutdown and on reuse checks.
        inner.lock().reaper_started = false;
    }
}

fn reap_idle_sessions(pool: &Weak<PoolInner>, signal: &ReaperSignal) {
    loop {
        let Some(inner) = pool.upgrade() else {
            return;
        };
        let (expired, next) = inner.take_expired();
        let idle_timeout = inner.config.idle_timeout;
        drop(inner);
        close_all(expired);
        let wait = next.unwrap_or(idle_timeout).max(Duration::from_millis(10));
        if !signal.sleep(wait) {
            return;
        }
    }
}

fn close_all(sessions: impl IntoIterator<Item = Session>) {
    for session in sessions {
        session.close(true);
    }
}

/// One warm engine process and the directory its jobs live in.
struct Session {
    id: String,
    directory: TempDir,
    runtime: Runtime,
    process: Option<EngineProcess>,
    sandbox: SandboxOutcome,
    jobs: u32,
    idle_since: Instant,
}

impl Session {
    fn new() -> Result<Self, RunError> {
        let directory = tempfile::Builder::new()
            .prefix(SESSION_PREFIX)
            .tempdir()
            .map_err(RunError::Io)?;
        std::fs::create_dir(directory.path().join("tmp")).map_err(RunError::Io)?;
        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(RunError::Io)?;
        Ok(Self {
            id: format!("session-{}", uuid::Uuid::new_v4().simple()),
            directory,
            runtime,
            process: None,
            sandbox: SandboxOutcome::Off,
            jobs: 0,
            idle_since: Instant::now(),
        })
    }

    fn is_reusable(&mut self, config: &PoolConfig) -> bool {
        if self.jobs >= config.max_jobs_per_process
            || self.idle_since.elapsed() >= config.idle_timeout
        {
            return false;
        }
        match self.process.as_mut() {
            Some(process) => !block_on(&self.runtime, process.is_stale()),
            None => true,
        }
    }

    fn run<S>(
        &mut self,
        command: &EngineCommand,
        request: &Request,
        cancel: CancellationToken,
        sink: S,
    ) -> Result<RunOutcome, RunError>
    where
        S: FnMut(Event),
    {
        let Request::Convert { .. } = request else {
            return Err(RunError::ProtocolViolation(
                "a session only serves convert requests",
            ));
        };
        let mut request = request.clone();
        if let Request::Convert { session_id, .. } = &mut request {
            *session_id = Some(self.id.clone());
        }
        let line = runner::encode_request(&request)?;
        let job = JobSpec::for_request(&request, None, cancel);
        self.jobs += 1;
        let Self {
            directory,
            runtime,
            process,
            sandbox,
            ..
        } = self;
        runtime.block_on(async {
            if process.is_none() {
                let root = directory.path().to_path_buf();
                let (program, args, outcome) =
                    runner::prepare_launch(command, std::slice::from_ref(&root))?;
                let tmp = root.join("tmp");
                *process = Some(
                    EngineProcess::spawn(&program, &args, &command.env, Some(tmp.as_os_str()))
                        .await?,
                );
                *sandbox = outcome;
            }
            let Some(engine) = process.as_mut() else {
                return Err(RunError::Io(std::io::Error::other(
                    "session process is missing",
                )));
            };
            match engine.run_job(&line, &job, JobMode::Session, sink).await {
                Ok((artifacts, metrics)) => Ok(RunOutcome {
                    artifacts,
                    metrics,
                    sandbox: sandbox.clone(),
                }),
                Err(RunError::Crash { exit, .. }) => {
                    engine.reap(true).await;
                    Err(RunError::Crash {
                        exit,
                        stderr_tail: engine.stderr_tail(),
                    })
                }
                Err(error) => Err(error),
            }
        })
    }

    /// Ends the session: gracefully closes stdin and waits [`CLOSE_GRACE`] when `graceful`,
    /// otherwise kills the process tree at once. The directory is removed afterwards.
    fn close(mut self, graceful: bool) {
        self.stop(graceful);
    }

    /// Kills the process tree now and keeps the directory until the session is dropped.
    fn kill(&mut self) {
        if let Some(mut process) = self.process.take() {
            block_on(&self.runtime, process.reap(true));
        }
    }

    fn stop(&mut self, graceful: bool) {
        if let Some(mut process) = self.process.take() {
            block_on(&self.runtime, async {
                if graceful {
                    process.close(CLOSE_GRACE).await;
                } else {
                    process.reap(true).await;
                }
            });
        }
        let _ = workspace::remove_workspace(self.directory.path());
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.stop(false);
    }
}

/// Drives `future` on the session's own runtime. Inside another Tokio context `block_on` would
/// panic, so the work moves to a scoped thread there.
fn block_on<F>(runtime: &Runtime, future: F) -> F::Output
where
    F: std::future::Future + Send,
    F::Output: Send,
{
    if tokio::runtime::Handle::try_current().is_err() {
        return runtime.block_on(future);
    }
    thread::scope(|scope| {
        scope
            .spawn(|| runtime.block_on(future))
            .join()
            .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
    })
}

/// An engine reserved for one job.
///
/// Keep the lease until the job's workspace is no longer needed: a session lease returns its
/// process to the pool, or removes its directory, only when it drops.
pub struct Lease<'pool> {
    pool: Option<&'pool SessionPool>,
    command: EngineCommand,
    session: Option<Session>,
    healthy: bool,
}

impl Lease<'static> {
    /// A one-shot lease outside any pool.
    #[must_use]
    pub fn one_shot(command: EngineCommand) -> Self {
        Self::new(None, command, None)
    }
}

impl<'pool> Lease<'pool> {
    fn new(
        pool: Option<&'pool SessionPool>,
        command: EngineCommand,
        session: Option<Session>,
    ) -> Self {
        Self {
            pool,
            command,
            session,
            healthy: true,
        }
    }

    /// Whether this lease runs its job in a warm session.
    #[must_use]
    pub fn is_session(&self) -> bool {
        self.session.is_some()
    }

    /// Creates the job workspace where this lease's engine process can reach it.
    pub fn workspace(&self) -> Result<Workspace, WorkspaceError> {
        match &self.session {
            Some(session) => Workspace::new_in(session.directory.path()),
            None => Workspace::new(),
        }
    }

    /// Runs one job. After a cancellation, timeout, exceeded limit, crash or protocol violation
    /// the session's process tree is killed at once and the next lease starts a fresh process.
    pub fn run<S>(
        &mut self,
        request: &Request,
        cancel: CancellationToken,
        sink: S,
    ) -> Result<RunOutcome, RunError>
    where
        S: FnMut(Event),
    {
        let Some(session) = self.session.as_mut() else {
            return runner::run_command(&self.command, request, cancel, sink);
        };
        let result = session.run(&self.command, request, cancel, sink);
        let reusable = match &result {
            Ok(_) => true,
            Err(RunError::EngineFailed { code, .. }) => *code != ErrorCode::LimitExceeded,
            Err(_) => false,
        };
        if !reusable {
            self.healthy = false;
            session.kill();
        }
        result
    }
}

impl Drop for Lease<'_> {
    fn drop(&mut self) {
        let Some(session) = self.session.take() else {
            return;
        };
        match self.pool {
            Some(pool) if self.healthy => pool.release(&self.command, session),
            _ => session.close(self.healthy),
        }
    }
}
