//! The job queue: a [`JobQueue`] trait and its pgmq implementation.
//!
//! `send` takes the caller's connection so the message commits with the job
//! row. An adapter for a queue outside PostgreSQL would write an outbox row
//! there instead.

use std::{collections::BTreeMap, time::Duration};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use crate::packs::Pack;

/// Body of a queue message.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct JobMessage {
    pub job_id: Uuid,
}

/// A message handed to a worker. It stays invisible to other workers until
/// its visibility timeout passes.
#[derive(Clone, Debug)]
pub struct Delivery {
    pub pack: Pack,
    pub msg_id: i64,
    /// How many times the message was read, this delivery included.
    pub read_count: i32,
    pub enqueued_at: DateTime<Utc>,
    pub message: Option<JobMessage>,
    /// W3C trace context and similar metadata.
    pub headers: BTreeMap<String, String>,
}

#[async_trait]
pub trait JobQueue: Send + Sync + 'static {
    /// Fleet-wide queued plus running admission ceiling.
    fn active_limit(&self) -> u32 {
        crate::config::DEFAULT_GLOBAL_ACTIVE_JOBS
    }

    /// Enqueues a job inside the caller's transaction.
    async fn send(
        &self,
        conn: &mut PgConnection,
        pack: Pack,
        message: &JobMessage,
        headers: &BTreeMap<String, String>,
    ) -> Result<i64, sqlx::Error>;

    /// Waits up to `wait` for one message and hides it for `visibility`.
    async fn read(
        &self,
        pack: Pack,
        visibility: Duration,
        wait: Duration,
    ) -> Result<Option<Delivery>, sqlx::Error>;

    /// Hides a message for another `visibility` from now.
    async fn extend(
        &self,
        pack: Pack,
        msg_id: i64,
        visibility: Duration,
    ) -> Result<(), sqlx::Error>;

    /// Makes a message visible again at once.
    async fn release(&self, pack: Pack, msg_id: i64) -> Result<(), sqlx::Error>;

    /// Moves a handled message to the archive.
    async fn archive(&self, pack: Pack, msg_id: i64) -> Result<(), sqlx::Error>;

    /// Number of messages waiting in a pack's queue.
    async fn depth(&self, pack: Pack) -> Result<i64, sqlx::Error>;
}

/// [`JobQueue`] over the pgmq extension.
#[derive(Clone, Debug)]
pub struct PgmqQueue {
    pool: PgPool,
    active_limit: u32,
}

impl PgmqQueue {
    #[must_use]
    pub const fn new(pool: PgPool) -> Self {
        Self {
            pool,
            active_limit: crate::config::DEFAULT_GLOBAL_ACTIVE_JOBS,
        }
    }
    /// Configures the single fleet admission ceiling used by REST and MCP.
    #[must_use]
    pub fn with_active_limit(mut self, ceiling: u32) -> Self {
        assert!(ceiling > 0, "active job ceiling must be positive");
        self.active_limit = ceiling;
        self
    }

    async fn lock_scheduler(conn: &mut PgConnection) -> Result<(), sqlx::Error> {
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('ariad-fair-queue', 0))")
            .execute(conn)
            .await?;
        Ok(())
    }

    /// The first queued event is an indexed pointer, not a history scan. It
    /// locates the pgmq row by primary key even behind a large stale backlog.
    /// All writes hold the short scheduler lock, including enqueue.
    async fn remember_message(
        conn: &mut PgConnection,
        pack: Pack,
        job_id: Uuid,
        msg_id: i64,
    ) -> Result<i64, sqlx::Error> {
        let event: Option<(i64, serde_json::Value)> = sqlx::query_as(
            "SELECT id, data FROM job_events WHERE job_id=$1 AND kind='queued' ORDER BY id LIMIT 1",
        )
        .bind(job_id)
        .fetch_optional(&mut *conn)
        .await?;
        if let Some((id, data)) = event {
            if let Some(existing) = data.get("_queue_id").and_then(serde_json::Value::as_i64) {
                let sql = format!(
                    "SELECT EXISTS(SELECT 1 FROM pgmq.q_{} WHERE msg_id=$1)",
                    pack.queue()
                );
                let exists: bool = sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
                    .bind(existing)
                    .fetch_one(&mut *conn)
                    .await?;
                if exists {
                    return Ok(existing);
                }
            }
            sqlx::query("UPDATE job_events SET data=data || jsonb_build_object('_queue_id',$2::bigint) WHERE id=$1")
                .bind(id).bind(msg_id).execute(&mut *conn).await?;
        } else {
            crate::db::add_event(
                conn,
                job_id,
                "queued",
                serde_json::json!({"pack": pack, "_queue_id": msg_id}),
            )
            .await?;
        }
        Ok(msg_id)
    }

    /// Only active jobs drive scheduling (bounded by fleet admission), never
    /// historical jobs or an unbounded scan of queue messages. Existing indexes
    /// cover the active set, each job's first queued event and queue primary key.
    async fn references(conn: &mut PgConnection) -> Result<Vec<FairRow>, sqlx::Error> {
        let mut rows = Vec::new();
        for pack in [Pack::Core, Pack::Office, Pack::Docling] {
            let sql = format!(
                "WITH active AS MATERIALIZED (SELECT id, subject FROM jobs \
                    WHERE status IN ('queued','running') AND pack=$1) \
                 SELECT q.msg_id, q.read_ct, q.enqueued_at, q.message, q.headers, \
                 j.subject, q.vt <= clock_timestamp() AS ready, $1::text AS pack \
                 FROM active j \
                 JOIN LATERAL (SELECT data FROM job_events WHERE job_id=j.id AND kind='queued' \
                    ORDER BY id LIMIT 1) e ON true \
                 JOIN LATERAL (SELECT * FROM pgmq.q_{} \
                    WHERE msg_id=(e.data->>'_queue_id')::bigint OFFSET 0) q ON true",
                pack.queue(),
            );
            rows.extend(
                sqlx::query_as::<_, FairRow>(sqlx::AssertSqlSafe(sql))
                    .bind(pack.as_str())
                    .fetch_all(&mut *conn)
                    .await?,
            );
        }
        Ok(rows)
    }

    /// Bounded maintenance also discovers messages queued by older binaries.
    /// Duplicates retain the existing delivery/session-lock semantics. Only
    /// visible rows handled here for archive consume a read; leased terminal
    /// rows can be archived without redelivery or another read-count debit.
    async fn maintain(conn: &mut PgConnection, pack: Pack) -> Result<Vec<FairRow>, sqlx::Error> {
        let sql = format!(
            "SELECT msg_id, read_ct, enqueued_at, message, headers, \
             vt <= clock_timestamp() AS ready FROM pgmq.q_{} ORDER BY vt LIMIT 32",
            pack.queue(),
        );
        let rows: Vec<MaintenanceRow> = sqlx::query_as(sqlx::AssertSqlSafe(sql))
            .fetch_all(&mut *conn)
            .await?;
        let mut duplicates = Vec::new();
        for MaintenanceRow { row, ready } in rows {
            let message = row
                .message
                .as_ref()
                .and_then(|value| serde_json::from_value::<JobMessage>(value.0.clone()).ok());
            if let Some(message) = message {
                let job: Option<(String, String, String)> =
                    sqlx::query_as("SELECT status, pack, subject FROM jobs WHERE id=$1")
                        .bind(message.job_id)
                        .fetch_optional(&mut *conn)
                        .await?;
                if let Some((status, job_pack, subject)) = job
                    && matches!(status.as_str(), "queued" | "running")
                    && job_pack == pack.as_str()
                {
                    let canonical =
                        Self::remember_message(conn, pack, message.job_id, row.msg_id).await?;
                    if canonical != row.msg_id {
                        duplicates.push(FairRow {
                            row,
                            subject,
                            ready,
                            pack: pack.as_str().to_owned(),
                        });
                    }
                    continue;
                }
            }
            let sql = format!(
                "UPDATE pgmq.q_{} SET read_ct=read_ct+ \
                 CASE WHEN vt <= clock_timestamp() THEN 1 ELSE 0 END WHERE msg_id=$1",
                pack.queue()
            );
            sqlx::query(sqlx::AssertSqlSafe(sql))
                .bind(row.msg_id)
                .execute(&mut *conn)
                .await?;
            sqlx::query("SELECT pgmq.archive($1,$2)")
                .bind(pack.queue())
                .bind(row.msg_id)
                .execute(&mut *conn)
                .await?;
        }
        Ok(duplicates)
    }

    async fn read_ready(
        &self,
        pack: Pack,
        visibility: Duration,
    ) -> Result<Option<MessageRow>, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        Self::lock_scheduler(&mut tx).await?;
        let duplicates = Self::maintain(&mut tx, pack).await?;
        let mut refs = Self::references(&mut tx).await?;
        refs.extend(duplicates);
        refs.sort_by_key(|r| (r.turn(), r.row.enqueued_at, r.row.msg_id));
        let Some(chosen) = refs.iter().find(|r| r.pack == pack.as_str() && r.ready) else {
            tx.commit().await?;
            return Ok(None);
        };
        let sql = format!(
            "UPDATE pgmq.q_{} SET read_ct=read_ct+1, vt=clock_timestamp()+make_interval(secs=>$2) \
             WHERE msg_id=$1 AND vt <= clock_timestamp() RETURNING msg_id, read_ct, enqueued_at, message, headers",
            pack.queue(),
        );
        let row = sqlx::query_as(sqlx::AssertSqlSafe(sql))
            .bind(chosen.row.msg_id)
            .bind(f64::from(secs(visibility).max(1)))
            .fetch_optional(&mut *tx)
            .await?;
        let Some(row) = row else {
            tx.commit().await?;
            return Ok(None);
        };
        // A database sequence gives a strict replica-wide order even when
        // several claims occur within one clock tick. Gaps in event ids are OK.
        let turn: i64 = sqlx::query_scalar("SELECT nextval('job_events_id_seq')")
            .fetch_one(&mut *tx)
            .await?;
        for pending in refs.iter().filter(|r| r.subject == chosen.subject) {
            let sql = format!(
                "UPDATE pgmq.q_{} SET headers=COALESCE(headers,'{{}}'::jsonb) || \
                 jsonb_build_object('__ariad_turn',$2::text) WHERE msg_id=$1",
                pending.pack.parse::<Pack>().expect("closed pack").queue(),
            );
            // pending.pack came from the closed Pack enum in references().
            sqlx::query(sqlx::AssertSqlSafe(sql))
                .bind(pending.row.msg_id)
                .bind(turn.to_string())
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(Some(row))
    }
}

fn secs(duration: Duration) -> i32 {
    i32::try_from(duration.as_secs()).unwrap_or(i32::MAX)
}

#[derive(sqlx::FromRow)]
struct MessageRow {
    msg_id: i64,
    read_ct: i32,
    enqueued_at: DateTime<Utc>,
    message: Option<sqlx::types::Json<serde_json::Value>>,
    headers: Option<sqlx::types::Json<serde_json::Value>>,
}

#[derive(sqlx::FromRow)]
struct MaintenanceRow {
    #[sqlx(flatten)]
    row: MessageRow,
    ready: bool,
}

#[derive(sqlx::FromRow)]
struct FairRow {
    #[sqlx(flatten)]
    row: MessageRow,
    subject: String,
    ready: bool,
    pack: String,
}

impl FairRow {
    fn turn(&self) -> i64 {
        self.row
            .headers
            .as_ref()
            .and_then(|h| h.0.get("__ariad_turn"))
            .and_then(serde_json::Value::as_str)
            .and_then(|turn| turn.parse().ok())
            .unwrap_or(0)
    }
}

#[async_trait]
impl JobQueue for PgmqQueue {
    fn active_limit(&self) -> u32 {
        self.active_limit
    }

    async fn send(
        &self,
        conn: &mut PgConnection,
        pack: Pack,
        message: &JobMessage,
        headers: &BTreeMap<String, String>,
    ) -> Result<i64, sqlx::Error> {
        Self::lock_scheduler(conn).await?;
        let subject: Option<String> = sqlx::query_scalar("SELECT subject FROM jobs WHERE id=$1")
            .bind(message.job_id)
            .fetch_optional(&mut *conn)
            .await?;
        let refs = Self::references(conn).await?;
        let turn = refs
            .iter()
            .filter(|r| Some(&r.subject) == subject.as_ref())
            .map(FairRow::turn)
            .max()
            // Join the current round's tail. Starting at zero would let a
            // subject repeatedly empty and refill its queue ahead of backlog.
            .unwrap_or_else(|| refs.iter().map(FairRow::turn).max().unwrap_or(0));
        let mut metadata = serde_json::to_value(headers).expect("string headers");
        metadata["__ariad_turn"] = turn.to_string().into();
        let msg_id = sqlx::query_scalar("SELECT * FROM pgmq.send($1, $2, $3)")
            .bind(pack.queue())
            .bind(sqlx::types::Json(message))
            .bind(sqlx::types::Json(metadata))
            .fetch_one(&mut *conn)
            .await?;
        if subject.is_some() {
            Self::remember_message(conn, pack, message.job_id, msg_id).await?;
        }
        Ok(msg_id)
    }

    async fn read(
        &self,
        pack: Pack,
        visibility: Duration,
        wait: Duration,
    ) -> Result<Option<Delivery>, sqlx::Error> {
        let deadline = tokio::time::Instant::now() + wait;
        let row = loop {
            let row = self.read_ready(pack, visibility).await?;
            if row.is_some() || tokio::time::Instant::now() >= deadline {
                break row;
            }
            // No transaction or pool connection is held while waiting.
            tokio::time::sleep(
                Duration::from_millis(250)
                    .min(deadline.saturating_duration_since(tokio::time::Instant::now())),
            )
            .await;
        };
        Ok(row.map(|row| Delivery {
            pack,
            msg_id: row.msg_id,
            read_count: row.read_ct,
            enqueued_at: row.enqueued_at,
            message: row
                .message
                .and_then(|json| serde_json::from_value(json.0).ok()),
            headers: row
                .headers
                .and_then(|mut json| {
                    if let Some(headers) = json.0.as_object_mut() {
                        headers.remove("__ariad_turn");
                    }
                    serde_json::from_value(json.0).ok()
                })
                .unwrap_or_default(),
        }))
    }

    async fn extend(
        &self,
        pack: Pack,
        msg_id: i64,
        visibility: Duration,
    ) -> Result<(), sqlx::Error> {
        sqlx::query("SELECT msg_id FROM pgmq.set_vt($1, $2, $3)")
            .bind(pack.queue())
            .bind(msg_id)
            .bind(secs(visibility))
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn release(&self, pack: Pack, msg_id: i64) -> Result<(), sqlx::Error> {
        self.extend(pack, msg_id, Duration::ZERO).await
    }

    async fn archive(&self, pack: Pack, msg_id: i64) -> Result<(), sqlx::Error> {
        sqlx::query("SELECT pgmq.archive($1, $2)")
            .bind(pack.queue())
            .bind(msg_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn depth(&self, pack: Pack) -> Result<i64, sqlx::Error> {
        sqlx::query_scalar("SELECT queue_length FROM pgmq.metrics($1)")
            .bind(pack.queue())
            .fetch_one(&self.pool)
            .await
    }
}
