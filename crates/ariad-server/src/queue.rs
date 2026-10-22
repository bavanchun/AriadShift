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
}

impl PgmqQueue {
    #[must_use]
    pub const fn new(pool: PgPool) -> Self {
        Self { pool }
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

#[async_trait]
impl JobQueue for PgmqQueue {
    async fn send(
        &self,
        conn: &mut PgConnection,
        pack: Pack,
        message: &JobMessage,
        headers: &BTreeMap<String, String>,
    ) -> Result<i64, sqlx::Error> {
        sqlx::query_scalar("SELECT * FROM pgmq.send($1, $2, $3)")
            .bind(pack.queue())
            .bind(sqlx::types::Json(message))
            .bind(sqlx::types::Json(headers))
            .fetch_one(conn)
            .await
    }

    async fn read(
        &self,
        pack: Pack,
        visibility: Duration,
        wait: Duration,
    ) -> Result<Option<Delivery>, sqlx::Error> {
        let row: Option<MessageRow> = sqlx::query_as(
            "SELECT msg_id, read_ct, enqueued_at, message, headers \
             FROM pgmq.read_with_poll($1, $2, 1, $3, 250)",
        )
        .bind(pack.queue())
        .bind(secs(visibility).max(1))
        .bind(secs(wait).max(1))
        .fetch_optional(&self.pool)
        .await?;
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
                .and_then(|json| serde_json::from_value(json.0).ok())
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
