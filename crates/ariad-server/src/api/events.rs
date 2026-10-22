//! `GET /v1/jobs/{id}/events`: job events over Server-Sent Events.
//!
//! One `LISTEN` connection per API process fans notifications out through a
//! broadcast channel. Each stream replays `job_events` after `Last-Event-ID`,
//! then re-reads the table when its job is notified (and every few seconds, so
//! a missed notification only delays an event). The stream ends after a final
//! event (`succeeded`, `failed`, `canceled` or `expired`).

use std::{collections::VecDeque, convert::Infallible, time::Duration};

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
    error::{ApiResult, ErrorBody},
};

/// How often a stream re-reads events without a notification.
const POLL_INTERVAL: Duration = Duration::from_secs(5);

/// Event kinds after which nothing else happens to a job until it expires.
const FINAL_KINDS: [&str; 4] = ["succeeded", "failed", "canceled", "expired"];

#[derive(Clone, Copy, Debug, Deserialize)]
pub struct Signal {
    pub job_id: Uuid,
    pub id: i64,
}

/// Fans `job_events` notifications out to SSE streams.
#[derive(Clone)]
pub struct EventHub {
    tx: broadcast::Sender<Signal>,
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
        Self { tx }
    }

    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<Signal> {
        self.tx.subscribe()
    }

    /// Listens until `shutdown`, reconnecting after errors.
    pub async fn run(self, pool: PgPool, shutdown: CancellationToken) {
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
    pool: PgPool,
    job_id: Uuid,
    last_id: i64,
    rx: broadcast::Receiver<Signal>,
    pending: VecDeque<JobEvent>,
    done: bool,
    reload: bool,
}

impl StreamState {
    async fn next_event(mut self) -> Option<(Result<Event, Infallible>, Self)> {
        loop {
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
                    Ok(rows) => self.pending.extend(rows.into_iter().map(to_dto)),
                    Err(err) => {
                        tracing::warn!(error = %err, "event stream query failed");
                        return None;
                    }
                }
                continue;
            }
            tokio::select! {
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
        (status = 429, description = "Rate limited", body = ErrorBody),
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
    let last_id = headers
        .get("last-event-id")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<i64>().ok())
        .unwrap_or(0)
        .max(0);
    // Subscribe before the first read so no notification falls in between.
    let rx = state.events.subscribe();
    let initial = StreamState {
        pool: state.db.clone(),
        job_id: job.id,
        last_id,
        rx,
        pending: VecDeque::new(),
        done: false,
        reload: true,
    };
    let events = stream::unfold(initial, StreamState::next_event);
    Ok(Sse::new(events).keep_alive(KeepAlive::new().interval(Duration::from_secs(15))))
}
