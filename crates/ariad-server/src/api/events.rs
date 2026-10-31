//! `GET /v1/jobs/{id}/events`: job events over Server-Sent Events.
//!
//! One `LISTEN` connection per API process fans notifications out through a
//! broadcast channel. Each stream replays `job_events` after `Last-Event-ID`,
//! then re-reads the table when its job is notified (and every few seconds, so
//! a missed notification only delays an event). The stream ends after a final
//! event (`succeeded`, `failed`, `canceled` or `expired`).

use std::{
    collections::VecDeque,
    convert::Infallible,
    time::{Duration, Instant},
};

use axum::{
    extract::{Path, State},
    http::HeaderMap,
    response::sse::{Event, KeepAlive, Sse},
};
use futures_util::{Stream, stream};
use serde::Deserialize;
use sqlx::{PgPool, postgres::PgListener};
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::{AppState, Caller, jobs::visible_job};
use crate::{
    api::dto::JobEvent,
    db::{self, EventRow, JOB_EVENTS_CHANNEL},
    error::{ApiError, ApiResult, ErrorBody},
};

/// How often a stream re-reads events without a notification.
const POLL_INTERVAL: Duration = Duration::from_secs(5);

/// Event kinds after which nothing else happens to a job until it expires.
const FINAL_KINDS: [&str; 4] = ["succeeded", "failed", "canceled", "expired"];

/// Shared across all API replicas; one caller cannot accumulate unbounded
/// polling streams, including streams for shared anonymous jobs.
const STREAMS_PER_SUBJECT: i64 = 8;
const STREAMS_PER_JOB: i64 = 2;

struct StreamLease {
    pool: PgPool,
    id: Uuid,
    renewed: Instant,
}

impl StreamLease {
    async fn acquire(pool: &PgPool, subject: &str, job_id: Uuid) -> ApiResult<Self> {
        let mut tx = pool.begin().await?;
        db::lock_subject(&mut tx, &format!("event-job:{job_id}")).await?;
        db::lock_subject(&mut tx, &format!("event-subject:{subject}")).await?;
        let (subject_count, job_count): (i64, i64) = sqlx::query_as(
            "SELECT count(*) FILTER (WHERE subject = $1), count(*) FILTER (WHERE job_id = $2) \
             FROM event_stream_leases WHERE expires_at > now() AND (subject = $1 OR job_id = $2)",
        )
        .bind(subject)
        .bind(job_id)
        .fetch_one(&mut *tx)
        .await?;
        if subject_count >= STREAMS_PER_SUBJECT || job_count >= STREAMS_PER_JOB {
            return Err(ApiError::rate_limited(5));
        }
        let id = Uuid::new_v4();
        sqlx::query("INSERT INTO event_stream_leases (id, subject, job_id, expires_at) VALUES ($1, $2, $3, now() + interval '90 seconds')")
            .bind(id).bind(subject).bind(job_id).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(Self {
            pool: pool.clone(),
            id,
            renewed: Instant::now(),
        })
    }

    async fn live(&mut self) -> bool {
        if self.renewed.elapsed() < Duration::from_secs(30) {
            return true;
        }
        match sqlx::query("UPDATE event_stream_leases SET expires_at = now() + interval '90 seconds' WHERE id = $1 AND expires_at > now()")
            .bind(self.id).execute(&self.pool).await {
            Ok(result) if result.rows_affected() == 1 => {
                self.renewed = Instant::now();
                true
            }
            _ => false,
        }
    }
}

impl Drop for StreamLease {
    fn drop(&mut self) {
        let pool = self.pool.clone();
        let id = self.id;
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                let _ = sqlx::query("DELETE FROM event_stream_leases WHERE id = $1")
                    .bind(id)
                    .execute(&pool)
                    .await;
            });
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize)]
pub struct Signal {
    pub job_id: Uuid,
    pub id: i64,
}

/// Fans `job_events` notifications out to SSE streams.
#[derive(Clone)]
pub struct EventHub {
    tx: broadcast::Sender<Signal>,
    stop: CancellationToken,
}

impl Default for EventHub {
    fn default() -> Self {
        Self::new()
    }
}

impl EventHub {
    #[must_use]
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(1024);
        Self {
            tx,
            stop: CancellationToken::new(),
        }
    }

    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<Signal> {
        self.tx.subscribe()
    }

    /// Listens until `shutdown`, reconnecting after errors.
    pub async fn run(self, pool: PgPool, shutdown: CancellationToken) {
        let _stop = self.stop.clone().drop_guard();
        loop {
            if shutdown.is_cancelled() {
                return;
            }
            match self.listen(&pool, &shutdown).await {
                Ok(()) => return,
                Err(err) => {
                    tracing::warn!(error = %err, "event listener lost its connection; retrying");
                    tokio::select! {
                        () = shutdown.cancelled() => return,
                        () = tokio::time::sleep(Duration::from_secs(1)) => {}
                    }
                }
            }
        }
    }

    async fn listen(&self, pool: &PgPool, shutdown: &CancellationToken) -> Result<(), sqlx::Error> {
        let mut listener = PgListener::connect_with(pool).await?;
        listener.listen(JOB_EVENTS_CHANNEL).await?;
        loop {
            tokio::select! {
                () = shutdown.cancelled() => return Ok(()),
                notification = listener.recv() => {
                    let notification = notification?;
                    if let Ok(signal) = serde_json::from_str::<Signal>(notification.payload()) {
                        let _ = self.tx.send(signal);
                    }
                }
            }
        }
    }
}

fn to_dto(row: EventRow) -> JobEvent {
    JobEvent {
        id: row.id,
        job_id: row.job_id,
        kind: row.kind,
        data: row.data.0,
        created_at: row.created_at,
    }
}

fn to_sse(event: &JobEvent) -> Event {
    Event::default()
        .id(event.id.to_string())
        .event(event.kind.clone())
        .json_data(event)
        .unwrap_or_else(|_| Event::default().comment("unserializable event"))
}

struct StreamState {
    lease: StreamLease,
    pool: PgPool,
    job_id: Uuid,
    last_id: i64,
    rx: broadcast::Receiver<Signal>,
    pending: VecDeque<JobEvent>,
    done: bool,
    terminal: bool,
    reload: bool,
    stop: CancellationToken,
}

impl StreamState {
    async fn next_event(mut self) -> Option<(Result<Event, Infallible>, Self)> {
        loop {
            if self.stop.is_cancelled() || !self.lease.live().await {
                return None;
            }
            if let Some(event) = self.pending.pop_front() {
                self.last_id = event.id;
                if FINAL_KINDS.contains(&event.kind.as_str()) {
                    self.done = true;
                    self.pending.clear();
                }
                return Some((Ok(to_sse(&event)), self));
            }
            if self.done {
                return None;
            }
            if self.reload {
                self.reload = false;
                match db::events_after(&self.pool, self.job_id, self.last_id).await {
                    Ok(rows) if rows.is_empty() => {
                        if self.terminal {
                            return None;
                        }
                        match db::job_status(&self.pool, self.job_id).await {
                            Ok(Some(status)) if status.is_final() => {
                                // Read once more after observing the terminal
                                // status, so an event committed between these
                                // queries is still delivered.
                                self.terminal = true;
                                self.reload = true;
                            }
                            Ok(None) => return None,
                            Ok(Some(_)) => {}
                            Err(err) => {
                                tracing::warn!(error = %err, "event stream status query failed");
                                return None;
                            }
                        }
                    }
                    Ok(rows) => self.pending.extend(rows.into_iter().map(to_dto)),
                    Err(err) => {
                        tracing::warn!(error = %err, "event stream query failed");
                        return None;
                    }
                }
                continue;
            }
            tokio::select! {
                () = self.stop.cancelled() => return None,
                signal = self.rx.recv() => match signal {
                    Ok(signal) if signal.job_id == self.job_id && signal.id > self.last_id => {
                        self.reload = true;
                    }
                    Ok(_) => {}
                    Err(broadcast::error::RecvError::Lagged(_)) => self.reload = true,
                    Err(broadcast::error::RecvError::Closed) => return None,
                },
                () = tokio::time::sleep(POLL_INTERVAL) => self.reload = true,
            }
        }
    }
}

/// Follow a job's events.
///
/// A Server-Sent Events stream. Each message's `event` is the kind, `id` is the
/// event id, and `data` is a `JobEvent`. Reconnect with `Last-Event-ID` to
/// resume. The stream closes after the job's final event.
#[utoipa::path(
    get,
    path = "/v1/jobs/{id}/events",
    tag = "jobs",
    params(
        ("id" = Uuid, Path, description = "Job id"),
        ("Last-Event-ID" = Option<i64>, Header, description = "Resume after this event id"),
    ),
    responses(
        (status = 200, description = "Event stream", content_type = "text/event-stream", body = JobEvent),
        (status = 401, description = "Credentials rejected", body = ErrorBody),
        (status = 404, description = "No such job", body = ErrorBody),
        (status = 429, description = "Rate limited", body = ErrorBody,
         headers(("retry-after" = u64, description = "Seconds to wait before retrying; sent with the `rate_limited` code"))),
        (status = 500, description = "Internal service error", body = ErrorBody),
        (status = 503, description = "Identity verification is unavailable", body = ErrorBody),
    ),
    security((), ("bearer" = []), ("api_key" = [])),
)]
pub async fn job_events(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> ApiResult<Sse<impl Stream<Item = Result<Event, Infallible>>>> {
    let job = visible_job(&state, &caller, &id).await?;
    let lease = StreamLease::acquire(&state.db, &caller.principal.subject, job.id).await?;
    let last_id = headers
        .get("last-event-id")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<i64>().ok())
        .unwrap_or(0)
        .max(0);
    // Subscribe before the first read so no notification falls in between.
    let rx = state.events.subscribe();
    let initial = StreamState {
        lease,
        pool: state.db.clone(),
        job_id: job.id,
        last_id,
        rx,
        pending: VecDeque::new(),
        done: false,
        terminal: job.status().is_final(),
        reload: true,
        stop: state.events.stop.clone(),
    };
    let events = stream::unfold(initial, StreamState::next_event);
    Ok(Sse::new(events).keep_alive(KeepAlive::new().interval(Duration::from_secs(15))))
}
